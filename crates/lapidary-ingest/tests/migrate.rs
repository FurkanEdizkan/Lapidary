//! Moving an existing content-addressed store into model directories, the way the worker
//! moves it: through `JobHandler::handle` on a `migrate_storage` row.
//!
//! Every fixture here is a store as it existed before slice 7 — the source file at
//! `blobs/ab/cd/<hash>` under zstd, `file.storage_path` null, and the category tree
//! back-filled by migration `0008` with `slug = name`, unslugged. That is what
//! `seed_cas_part` reproduces, and nothing below is testing this job against a store this
//! job has already touched.

use lapidary_core::manifest::ModelManifest;
use lapidary_core::slug::disambiguate;
use lapidary_core::{BatchId, BlobHash, JobId, JobPayload, LibraryId, MeshMeasurements, Outcome};
use lapidary_db::{
    IngestRequest, JobRow, PgFolders, PgIngest, PgPool, PgStorageMigration, StoredBlobRow,
};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::JobHandler;
use lapidary_storage::{Compression, SourceReader, SourceStore, WorkerRole};
use std::path::Path;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
/// A rock, and a bracket. Two real meshes, so the two blobs in a shared-blob test are
/// genuinely different bytes rather than one fixture written twice.
const CLIFF: &[u8] = include_bytes!("../../../fixtures/spacer-lp-2001-00.stl");
const BRACKET: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

fn handler_over(pool: &PgPool, store: &Path) -> WorkerHandler {
    WorkerHandler {
        db: pool.clone(),
        // The migration never reads the ingest mount: it moves bytes the store already
        // holds. Pointing it at the store root proves that — a handler that reached for a
        // source file would find the blob tree there and nothing it could use.
        ingest_dir: store.to_path_buf(),
        blob_root: store.to_path_buf(),
    }
}

/// A `migrate_storage` row exactly as `PgJobs::enqueue` writes one: the kind is the
/// COLUMN, and the payload is empty.
fn migrate_job(library: LibraryId) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: library,
        kind: JobPayload::MigrateStorage.kind().to_owned(),
        payload: JobPayload::MigrateStorage.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// Where the content-addressed copy of `hash` sits, relative to the store root.
fn cas_rel(hash: &BlobHash) -> String {
    let hex = hash.to_hex();
    format!("blobs/{}/{}/{hex}", &hex[0..2], &hex[2..4])
}

/// Plausible numbers for a real part, so the manifest this migration writes carries
/// measurements a person could check rather than zeroes.
fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_684.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

/// A part as it existed before the store became a folder tree: bytes at
/// `blobs/ab/cd/<hash>`, `file.storage_path` null, and its categories back-filled the way
/// migration `0008` back-fills them — `slug = name`, straight off the ingest directory and
/// never slugified. Returns the hash so a test can name the old path.
async fn seed_cas_part(
    pool: &PgPool,
    store: &Path,
    library: LibraryId,
    source_path: &str,
    bytes: &[u8],
    compression: Compression,
) -> BlobHash {
    let stored = SourceStore::open(store, &WorkerRole::assume())
        .put(bytes, compression)
        .expect("writes the content-addressed copy");

    let folders = PgFolders(pool.clone());
    let mut parent = None;
    if let Some(dir) = Path::new(source_path)
        .parent()
        .and_then(|dir| dir.to_str())
        .filter(|dir| !dir.is_empty())
    {
        for segment in dir.split('/') {
            parent = Some(
                folders
                    // `segment` as the slug as well as the name: that is the back-fill's
                    // own behaviour, and the reason this job has to re-slug at all.
                    .get_or_create(library, parent, segment, segment)
                    .await
                    .expect("back-fills a category"),
            );
        }
    }

    let name = Path::new(source_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("the fixture has a stem");
    let blob = StoredBlobRow {
        hash: stored.hash,
        size_bytes: stored.size_bytes,
        stored_bytes: stored.stored_bytes,
        zstd_level: stored.zstd_level,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            library,
            name,
            source_path,
            folder: parent,
            // The whole point: a row written before model directories existed.
            storage_path: None,
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp: None,
            kernel_version: "lapidary-mesh 0.1.0",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds a part whose bytes are still content-addressed");
    stored.hash
}

async fn second_library(pool: &PgPool) -> LibraryId {
    let library = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, 'Terrain Packs')")
        .bind(library.as_uuid())
        .execute(pool)
        .await
        .expect("seeds a second library");
    library
}

async fn storage_path_of(pool: &PgPool, source_path: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT f.storage_path FROM file f \
         JOIN revision r ON r.id = f.revision_id \
         JOIN part p ON p.id = r.part_id WHERE p.source_path = $1",
    )
    .bind(source_path)
    .fetch_one(pool)
    .await
    .expect("reads the file row")
}

async fn slug_of(pool: &PgPool, name: &str) -> String {
    sqlx::query_scalar("SELECT slug FROM folder WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("reads the folder row")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn migrate_storage_moves_a_cas_blob_into_a_model_directory(pool: PgPool) {
    let store = tempfile::tempdir().expect("store");
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;

    let handler = handler_over(&pool, store.path());
    assert_eq!(
        handler
            .handle(&migrate_job(seeded()))
            .await
            .expect("migrates"),
        Outcome::Migrated
    );

    let dir = store.path().join("libraries/default/Terrain/Rocks/cliff");
    assert!(
        dir.join("cliff.stl").exists(),
        "the file is under its own name"
    );
    assert!(
        dir.join("metadata.json").exists(),
        "and the directory says what it is"
    );
    assert_eq!(
        storage_path_of(&pool, "Terrain/Rocks/cliff.stl")
            .await
            .as_deref(),
        Some("libraries/default/Terrain/Rocks/cliff/cliff.stl")
    );
    assert!(
        !store.path().join(cas_rel(&hash)).exists(),
        "the content-addressed copy goes only after the row points at the new one"
    );

    // The manifest describes the part the row describes, measurements and all — a
    // migrated directory has to read exactly like an ingested one, or re-adoption treats
    // half the store as second class.
    let manifest: ModelManifest =
        serde_json::from_slice(&std::fs::read(dir.join("metadata.json")).expect("reads"))
            .expect("a manifest this build can read");
    assert_eq!(manifest.part.name, "cliff");
    assert_eq!(manifest.part.source_path, "Terrain/Rocks/cliff.stl");
    assert_eq!(manifest.revisions[0].bbox_mm, Some([61.0, 42.0, 18.5]));
    assert_eq!(manifest.revisions[0].volume_mm3, Some(21_478.5));
    assert_eq!(manifest.revisions[0].files[0].file_name, "cliff.stl");
    assert_eq!(manifest.revisions[0].files[0].blake3, hash);
}

/// Make the row update fail, and only for paths matching `pattern`.
///
/// The kill switch this suite needs sits at exactly one seam: the bytes are written and
/// the transaction that would record them has not committed. A `CHECK` on `storage_path`
/// fires there and nowhere earlier — every existing row holds NULL, and `NULL NOT LIKE …`
/// is NULL rather than false, so adding the constraint validates the table without
/// refusing anything already in it. Borrowed from `tests/handler.rs`'s `refuse_this_file`,
/// which is why this job needed no copy-only entry point of its own to be interruptible.
///
/// `pattern` is what lets one test refuse ONE library's rows while another library's row in
/// the same transaction would have been perfectly acceptable — the only way to observe
/// whether that transaction really is one transaction.
async fn refuse_the_row_update(pool: &PgPool, pattern: &str) {
    // `AssertSqlSafe` because `ALTER TABLE` takes no bind parameters; `pattern` is a literal
    // from the call sites below, never anything read back out of the database.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE file ADD CONSTRAINT file_storage_path_refused \
         CHECK (storage_path NOT LIKE '{pattern}')"
    )))
    .execute(pool)
    .await
    .expect("adds the refusing constraint");
}

/// Read a migrated file the way the download route reads it: through `SourceReader::get_at`
/// at the level the `blob` row records, then check the digest.
///
/// Not `fs::read`. A raw read bypasses both the decode and the hash — which is precisely
/// what a half-settled group corrupts: the row says level 0, the file on disk is still a
/// zstd frame, and only a reader that follows the recorded level can tell.
async fn downloads_as(pool: &PgPool, store: &Path, source_path: &str, expected: &[u8]) {
    let (rel, hex, level): (Option<String>, String, Option<i16>) = sqlx::query_as(
        "SELECT f.storage_path, f.blake3, b.zstd_level FROM file f \
         JOIN revision r ON r.id = f.revision_id \
         JOIN part p ON p.id = r.part_id \
         JOIN blob b ON b.blake3 = f.blake3 WHERE p.source_path = $1",
    )
    .bind(source_path)
    .fetch_one(pool)
    .await
    .expect("reads the file row");
    let rel = rel.unwrap_or_else(|| panic!("{source_path} has no storage_path to read"));

    let bytes = SourceReader::open(store)
        .get_at(&rel, level)
        .unwrap_or_else(|e| panic!("the download route reads {source_path} back: {e}"));
    assert_eq!(bytes, expected, "{source_path} reads back as itself");
    assert_eq!(
        BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes()),
        BlobHash::parse_hex(&hex).expect("a stored hash parses"),
        "{source_path} passes the digest check the download route makes"
    );
}

async fn allow_the_row_update(pool: &PgPool) {
    sqlx::query("ALTER TABLE file DROP CONSTRAINT file_storage_path_refused")
        .execute(pool)
        .await
        .expect("drops the refusing constraint");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_interrupted_migration_loses_no_file(pool: PgPool) {
    // Copy before delete: at every instant the bytes are readable at the old path, the new
    // path, or both — never at neither.
    let store = tempfile::tempdir().expect("store");
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    let handler = handler_over(&pool, store.path());

    refuse_the_row_update(&pool, "libraries/%").await;
    handler
        .handle(&migrate_job(seeded()))
        .await
        .expect_err("the row update is refused, so the move does not settle");

    // The instant the migration was interrupted at. Both paths hold the file, and both
    // hold the same bytes. This is the assertion an unlink-before-commit fails.
    let old = store.path().join(cas_rel(&hash));
    let new = store
        .path()
        .join("libraries/default/Terrain/cliff/cliff.stl");
    assert!(old.exists(), "the content-addressed copy is still there");
    assert_eq!(
        std::fs::read(&new).expect("the new copy is already on disk"),
        CLIFF,
        "and it is complete, not half-written"
    );
    assert_eq!(
        storage_path_of(&pool, "Terrain/cliff.stl").await,
        None,
        "nothing was recorded, so every reader is still pointed at the old path"
    );

    // Resuming completes, and the old path goes.
    allow_the_row_update(&pool).await;
    assert_eq!(
        handler
            .handle(&migrate_job(seeded()))
            .await
            .expect("resumes"),
        Outcome::Migrated
    );
    assert!(
        !old.exists(),
        "the CAS copy is removed only after the new one is durable"
    );
    let settled = storage_path_of(&pool, "Terrain/cliff.stl")
        .await
        .expect("the resumed run recorded a path");
    assert_eq!(
        std::fs::read(store.path().join(&settled)).expect("the recorded path holds the file"),
        CLIFF
    );

    // What an interruption costs, stated rather than discovered. `model_dir_for` decides a
    // model's directory by asking whether that name is already taken, and the copy the
    // interrupted run left behind takes it — so the resumed run disambiguates and the
    // interrupted copy stays as a duplicate. It is bounded at one per file that was in
    // flight when the worker died, and the name is deterministic (it keys on the source
    // hash), so a second interruption lands on the same directory rather than a third.
    // Never a loss: both directories hold the whole file.
    assert!(
        settled.starts_with("libraries/default/Terrain/cliff_"),
        "the resumed run does not overwrite the interrupted attempt: {settled}"
    );
    assert_eq!(
        std::fs::read(
            store
                .path()
                .join("libraries/default/Terrain/cliff/cliff.stl")
        )
        .expect("the interrupted attempt is left behind"),
        CLIFF,
        "a duplicate a re-adoption walk will report, not a truncated file"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_migrated_file_is_really_uncompressed_and_its_row_says_so(pool: PgPool) {
    let store = tempfile::tempdir().expect("store");
    // Zstd, not AsIs. A `.3mf` fixture would be stored at level 0 and this test would pass
    // over code that never decompresses anything — the level is what has to discriminate.
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;

    let (level, stored_bytes, size_bytes): (Option<i16>, i64, i64) =
        sqlx::query_as("SELECT zstd_level, stored_bytes, size_bytes FROM blob WHERE blake3 = $1")
            .bind(hash.to_hex())
            .fetch_one(&pool)
            .await
            .expect("reads the blob row");
    assert_eq!(level, Some(3), "the fixture really is a zstd-3 blob");
    assert!(
        stored_bytes < size_bytes,
        "and it really is smaller on disk than the file it holds: {stored_bytes} vs {size_bytes}"
    );

    handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates");

    // On the bytes, not on the flag: a plain read, with nothing given the chance to decode
    // on the way past.
    let moved = store
        .path()
        .join("libraries/default/Terrain/Rocks/cliff/cliff.stl");
    assert_eq!(
        std::fs::read(&moved).expect("reads the moved file"),
        CLIFF,
        "the file in the model directory is the file, not a zstd frame named .stl"
    );

    let (level, stored_bytes, size_bytes): (Option<i16>, i64, i64) =
        sqlx::query_as("SELECT zstd_level, stored_bytes, size_bytes FROM blob WHERE blake3 = $1")
            .bind(hash.to_hex())
            .fetch_one(&pool)
            .await
            .expect("reads the blob row");
    assert_eq!(
        level,
        Some(0),
        "every reader follows the recorded level, so a moved blob has to say 0"
    );
    assert_eq!(
        stored_bytes, size_bytes,
        "and a row claiming level 0 cannot go on reporting a compressed size"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_back_filled_category_ends_up_at_a_windows_safe_path(pool: PgPool) {
    let store = tempfile::tempdir().expect("store");
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks?/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    assert_eq!(
        slug_of(&pool, "Rocks?").await,
        "Rocks?",
        "migration 0008 back-fills the slug unslugged; that is what this fixes"
    );

    handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates");

    assert_eq!(slug_of(&pool, "Rocks?").await, "Rocks-");
    assert_eq!(
        storage_path_of(&pool, "Terrain/Rocks?/cliff.stl")
            .await
            .as_deref(),
        Some("libraries/default/Terrain/Rocks-/cliff/cliff.stl"),
        "the store path is one no Windows client would refuse"
    );
    assert!(
        store
            .path()
            .join("libraries/default/Terrain/Rocks-/cliff/cliff.stl")
            .exists()
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_categories_that_slug_alike_get_two_directories(pool: PgPool) {
    // `Rocks?` and `Rocks*` are distinct names that slug to the same directory. Without a
    // disambiguating suffix the second re-slug violates `folder_slug_unique_per_parent`,
    // and the tree is legal while the disk is not.
    let store = tempfile::tempdir().expect("store");
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks?/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks*/bracket-lp-1042-03.stl",
        BRACKET,
        Compression::Zstd,
    )
    .await;

    handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates both");

    let question = slug_of(&pool, "Rocks?").await;
    let star = slug_of(&pool, "Rocks*").await;
    assert_ne!(question, star, "two categories, two directories");
    assert!(
        question.starts_with("Rocks-") && star.starts_with("Rocks-"),
        "both still read as the category they came from: {question} and {star}"
    );
    for (source_path, file, bytes) in [
        ("Terrain/Rocks?/cliff.stl", "cliff/cliff.stl", CLIFF),
        (
            "Terrain/Rocks*/bracket-lp-1042-03.stl",
            "bracket-lp-1042-03/bracket-lp-1042-03.stl",
            BRACKET,
        ),
    ] {
        let path = storage_path_of(&pool, source_path).await.expect("migrated");
        assert!(path.ends_with(file), "{path} should end with {file}");
        assert_eq!(
            std::fs::read(store.path().join(&path)).expect("reads the moved file"),
            bytes
        );
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_libraries_sharing_one_blob_settle_together(pool: PgPool) {
    // Before the store became a folder tree, ingest deduplicated source bytes: two parts
    // in two libraries share one `blob` row, one file on disk, and therefore one
    // `zstd_level`. Migrating one of them and not the other would leave the survivor
    // reading a compressed file at level 0 — or reading a file that has been unlinked.
    let store = tempfile::tempdir().expect("store");
    let other = second_library(&pool).await;
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    let shared = seed_cas_part(
        &pool,
        store.path(),
        other,
        "Cliffs/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    assert_eq!(hash, shared, "one blob, two file rows");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM file WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(rows, 2);

    // The job runs for the seeded library only.
    handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates");

    for (source_path, expected) in [
        (
            "Terrain/Rocks/cliff.stl",
            "libraries/default/Terrain/Rocks/cliff/cliff.stl",
        ),
        (
            "Cliffs/cliff.stl",
            // `library_slug` lowercases the library's name and slugifies it; a space is
            // not a hostile character, so it survives.
            "libraries/terrain packs/Cliffs/cliff/cliff.stl",
        ),
    ] {
        let path = storage_path_of(&pool, source_path)
            .await
            .unwrap_or_else(|| panic!("{source_path} must move with the blob it shares"));
        assert_eq!(path, expected);
        assert_eq!(
            std::fs::read(store.path().join(&path)).expect("reads the moved file"),
            CLIFF,
            "both copies are the file, at full size, uncompressed"
        );
    }
    let level: Option<i16> = sqlx::query_scalar("SELECT zstd_level FROM blob WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("reads the blob row");
    assert_eq!(level, Some(0), "one row, and it is true of both readers");
    assert!(
        !store.path().join(cas_rel(&hash)).exists(),
        "nothing reads the content-addressed copy any more, so it goes"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn re_running_a_drained_migration_is_a_no_op(pool: PgPool) {
    let store = tempfile::tempdir().expect("store");
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    let handler = handler_over(&pool, store.path());
    handler
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates");
    let after_first = storage_path_of(&pool, "Terrain/Rocks/cliff.stl").await;

    assert_eq!(
        handler
            .handle(&migrate_job(seeded()))
            .await
            .expect("a drained store is a success, not an error"),
        Outcome::Migrated
    );
    assert_eq!(
        storage_path_of(&pool, "Terrain/Rocks/cliff.stl").await,
        after_first,
        "and it moved nothing a second time"
    );
    let dirs = std::fs::read_dir(store.path().join("libraries/default/Terrain/Rocks"))
        .expect("reads the category directory")
        .count();
    assert_eq!(dirs, 1, "no second copy under a disambiguated name");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn another_librarys_backlog_does_not_keep_this_job_running(pool: PgPool) {
    // The re-enqueue asks whether THIS library has anything left. Asked globally, one
    // library's job re-enqueues itself forever over rows nothing in its batch will move.
    let store = tempfile::tempdir().expect("store");
    let other = second_library(&pool).await;
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    // Different bytes, so this row is not swept along by the hash it shares — it shares
    // none.
    seed_cas_part(
        &pool,
        store.path(),
        other,
        "Brackets/bracket-lp-1042-03.stl",
        BRACKET,
        Compression::Zstd,
    )
    .await;

    handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect("migrates its own library");

    assert!(
        PgStorageMigration(pool.clone())
            .any_pending(other)
            .await
            .expect("asks"),
        "the other library really is still waiting"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(
        queued, 0,
        "this library drained, so its job queued no successor"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_that_does_not_match_its_hash_is_refused_rather_than_copied(pool: PgPool) {
    // The guard between "the recorded level was wrong, or something rewrote the blob" and
    // "wrote the wrong bytes into a model directory, then deleted the original". A store
    // where the file under `blobs/ab/cd/<hash>` is not the file that hash names is exactly
    // the case where copying it somewhere else launders the corruption into a file the
    // user opens.
    let store = tempfile::tempdir().expect("store");
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        // Uncompressed, so the bytes below are read back verbatim and the hash — not a
        // failed zstd decode — is what refuses them.
        Compression::AsIs,
    )
    .await;
    let cas = store.path().join(cas_rel(&hash));
    std::fs::write(&cas, BRACKET).expect("something rewrote the blob");

    let error = handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await
        .expect_err("bytes that do not hash to the row's own key are not that row's bytes");
    let message = format!("{error:?}");
    assert!(
        message.contains("re-scan this part"),
        "the refusal has to say what to do about it, got: {message}"
    );

    assert_eq!(
        storage_path_of(&pool, "Terrain/Rocks/cliff.stl").await,
        None,
        "nothing was recorded"
    );
    assert!(
        !store
            .path()
            .join("libraries/default/Terrain/Rocks/cliff")
            .exists(),
        "and nothing was written into a model directory"
    );
    assert_eq!(
        std::fs::read(&cas).expect("the old copy is untouched"),
        BRACKET
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn settling_one_shared_blob_is_all_or_nothing(pool: PgPool) {
    // THE test for why this job batches on a hash. `two_libraries_sharing_one_blob_settle_together`
    // proves both rows are SELECTED; this proves they are RECORDED together, which is a
    // different claim and the one the design rests on. A per-file settle passes that test
    // and fails this one.
    //
    // What a per-file settle does when the second row's write fails: the first row commits,
    // `blob.zstd_level` goes to 0, and the second row is left NULL pointing at a
    // content-addressed copy that is still a zstd frame. Downloading it decodes nothing and
    // fails on the digest — and it is *permanently* unmigratable, because a resumed run
    // reads that same blob at the level 0 the first row installed, gets the compressed
    // bytes raw, and the BLAKE3 guard refuses them `Permanent` on every future attempt.
    let store = tempfile::tempdir().expect("store");
    let other = second_library(&pool).await;
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    seed_cas_part(
        &pool,
        store.path(),
        other,
        "Cliffs/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;

    // Refuses ONLY the second library's row. The seeded library's row in the same
    // transaction would have been accepted, so anything that survives is something that
    // committed on its own.
    refuse_the_row_update(&pool, "libraries/terrain packs/%").await;
    let handler = handler_over(&pool, store.path());
    handler
        .handle(&migrate_job(seeded()))
        .await
        .expect_err("one row of the group was refused, so the group was refused");

    let (level, storage_paths): (Option<i16>, i64) = sqlx::query_as(
        "SELECT (SELECT zstd_level FROM blob WHERE blake3 = $1), \
                (SELECT count(*) FROM file WHERE blake3 = $1 AND storage_path IS NOT NULL)",
    )
    .bind(hash.to_hex())
    .fetch_one(&pool)
    .await
    .expect("reads the blob and its file rows");
    assert_eq!(
        level,
        Some(3),
        "the level describes bytes both rows still read, so it cannot move while one of \
         them has not"
    );
    assert_eq!(
        storage_paths, 0,
        "neither row may be recorded when the other could not be"
    );
    assert!(
        store.path().join(cas_rel(&hash)).exists(),
        "and the copy they both still read is untouched"
    );

    // Resuming converges. Both rows land, and both read back through the level the row
    // records — which is the check a half-settled group fails and a raw read cannot make.
    allow_the_row_update(&pool).await;
    assert_eq!(
        handler
            .handle(&migrate_job(seeded()))
            .await
            .expect("resumes"),
        Outcome::Migrated
    );
    downloads_as(&pool, store.path(), "Terrain/Rocks/cliff.stl", CLIFF).await;
    downloads_as(&pool, store.path(), "Cliffs/cliff.stl", CLIFF).await;
    let level: Option<i16> = sqlx::query_scalar("SELECT zstd_level FROM blob WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("reads the blob row");
    assert_eq!(
        level,
        Some(0),
        "one row, and now it is true of both readers"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_category_whose_disambiguated_slug_is_taken_too_does_not_stall_the_library(pool: PgPool) {
    // Adversarial, and the only reason it is worth a test is the size of the failure: a
    // re-slug that insisted here would raise a bare `folder_slug_unique_per_parent`
    // violation out of `migrate_storage` BEFORE a single file moved, stalling the whole
    // library's migration behind a message naming a constraint.
    let store = tempfile::tempdir().expect("store");
    seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks?/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;

    let folders = PgFolders(pool.clone());
    let terrain: Uuid = sqlx::query_scalar("SELECT id FROM folder WHERE name = 'Terrain'")
        .fetch_one(&pool)
        .await
        .expect("the back-filled parent");
    let terrain = Some(lapidary_core::FolderId::from_uuid(terrain));
    let hostile: Uuid = sqlx::query_scalar("SELECT id FROM folder WHERE name = 'Rocks?'")
        .fetch_one(&pool)
        .await
        .expect("the category that wants re-slugging");

    // A sibling literally named `Rocks-` takes the target slug, and one named after the
    // disambiguated form takes the fallback. Both slug to themselves, so both are already
    // correct and neither moves.
    let id = hostile.simple().to_string();
    let taken = format!("Rocks-_{}", &id[id.len() - 6..]);
    for name in ["Rocks-".to_owned(), taken.clone()] {
        folders
            .get_or_create(seeded(), terrain, &name, &name)
            .await
            .expect("seeds a sibling that already holds the slug");
    }

    assert_eq!(
        handler_over(&pool, store.path())
            .handle(&migrate_job(seeded()))
            .await
            .expect("the files still move"),
        Outcome::Migrated
    );
    assert_eq!(
        slug_of(&pool, "Rocks?").await,
        "Rocks?",
        "the category keeps the slug it had rather than colliding with a sibling"
    );
    downloads_as(&pool, store.path(), "Terrain/Rocks?/cliff.stl", CLIFF).await;
}

// Everything below reconstructs ONE interleaving: a second `migrate_storage` runner acting
// on rows a first runner has written but not yet committed.
//
// It is built out of blocking, never out of timing. The winner is stopped inside `settle`
// by a row lock this test holds on the `blob` row that `settle` updates — so its `file`
// UPDATE is applied and uncommitted, and under MVCC the loser's own `pending_sources`
// legitimately reads those rows as still NULL. That is the stale row set the defect turns
// on, produced by Postgres rather than by the test. `tokio::join!` reproduces this
// interleaving only by luck, which is why nothing here waits on a clock.

/// The store-relative directory `model_dir_for` gives a part called `cliff` under `category`
/// when a directory of that name is already taken.
fn disambiguated(category: &str, hash: &BlobHash) -> String {
    format!(
        "libraries/default/{category}/{}",
        disambiguate("cliff", hash)
    )
}

/// Record a `storage_path` on a seeded part, the way an earlier migration run would have.
/// A row that carries one is not in anybody's page.
async fn mark_migrated(pool: &PgPool, source_path: &str, storage_path: &str) {
    let updated = sqlx::query(
        "UPDATE file SET storage_path = $2 WHERE revision_id IN ( \
           SELECT r.id FROM revision r JOIN part p ON p.id = r.part_id \
            WHERE p.source_path = $1)",
    )
    .bind(source_path)
    .bind(storage_path)
    .execute(pool)
    .await
    .expect("records the path an earlier run wrote");
    assert_eq!(updated.rows_affected(), 1, "{source_path} is one file row");
}

/// How many backends in THIS test's database are waiting on a lock, right now.
///
/// `pg_stat_clear_snapshot()` first, and it is not optional: Postgres freezes the activity
/// snapshot at the first `pg_stat_activity` read in a transaction, and the connection asking
/// here is holding one open for the length of the test. Without the clear, every later poll
/// re-reads the answer the first one got and a loop waiting for the number to rise never
/// ends. `datname = current_database()` because `#[sqlx::test]` runs the suite in parallel
/// against one cluster, and a wait belonging to another test's database is not ours to see.
async fn lock_waiters(holder: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> i64 {
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut **holder)
        .await
        .expect("drops this transaction's cached view of who is doing what");
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity \
          WHERE datname = current_database() AND wait_event_type = 'Lock'",
    )
    .fetch_one(&mut **holder)
    .await
    .expect("asks what is waiting")
}

/// Block until `waiters` backends are waiting on a lock.
///
/// A state poll, not a delay: the condition is a fact about the server, and the loop exists
/// only because there is no way to be notified of it.
async fn wait_for_lock_waiters(holder: &mut sqlx::Transaction<'_, sqlx::Postgres>, waiters: i64) {
    for _ in 0..600 {
        if lock_waiters(holder).await >= waiters {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("no runner ever blocked on the lock this test holds");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_runner_may_not_reap_a_move_the_first_committed(pool: PgPool) {
    // The data-loss case, end to end. Two runners resolve the SAME model directory —
    // `model_dir_for` disambiguates on the shared blob hash, so both land on
    // `cliff_<hash6>` whenever plain `cliff` is already taken — and the second one fails
    // partway through the group. Its reap then removes the file, the manifest and the
    // directory that the first runner's committed row names, and the first runner has
    // already unlinked the content-addressed copy. The bytes are at neither path: the
    // module doc calls that forbidden.
    let store = tempfile::tempdir().expect("store");

    // Two parts in one category both called `cliff` is what `disambiguate` exists for. This
    // one is a bracket exported alongside the rock; an earlier run already moved it, so its
    // row names a path and no page contains it — it is here only to own `Terrain/Rocks/cliff`.
    let taken = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.step",
        BRACKET,
        Compression::AsIs,
    )
    .await;
    let taken_dir = store.path().join("libraries/default/Terrain/Rocks/cliff");
    std::fs::create_dir_all(&taken_dir).expect("the directory an earlier run wrote");
    std::fs::write(taken_dir.join("cliff.step"), BRACKET).expect("with its file in it");
    std::fs::remove_file(store.path().join(cas_rel(&taken))).expect("and its old copy gone");
    mark_migrated(
        &pool,
        "Terrain/Rocks/cliff.step",
        "libraries/default/Terrain/Rocks/cliff/cliff.step",
    )
    .await;

    // The group: one blob, two parts, deduplicated by ingest before the store became a
    // folder tree.
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    let shared = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Boulders/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    assert_eq!(hash, shared, "one blob, two file rows");

    // The reap only reaches a committed file if the converging row is written BEFORE the
    // one that fails, and the group is ordered by `file.id` — uuidv7, so by creation.
    // Asserted rather than assumed, because a silent flip would make this test pass by
    // testing nothing.
    let page = PgStorageMigration(pool.clone())
        .pending_sources(seeded(), 10)
        .await
        .expect("reads the page");
    assert_eq!(
        page.iter()
            .map(|row| row.source_path.as_str())
            .collect::<Vec<_>>(),
        ["Terrain/Rocks/cliff.stl", "Terrain/Boulders/cliff.stl"]
    );

    // Stop the winner inside `settle`, between its `file` UPDATE and its commit.
    let mut holder = pool.begin().await.expect("a connection of its own");
    sqlx::query("SELECT blake3 FROM blob WHERE blake3 = $1 FOR NO KEY UPDATE")
        .bind(hash.to_hex())
        .fetch_one(&mut *holder)
        .await
        .expect("locks the blob row settle updates");
    let winner = tokio::spawn({
        let handler = handler_over(&pool, store.path());
        async move { handler.handle(&migrate_job(seeded())).await }
    });
    wait_for_lock_waiters(&mut holder, 1).await;

    let committed = format!("{}/cliff.stl", disambiguated("Terrain/Rocks", &hash));
    assert!(
        store.path().join(&committed).exists(),
        "the winner has written both rows and is parked in its settle"
    );

    // The loser's second row has to fail. Any error at all takes the reap arm — a full
    // volume, a permission — and a plain file where its directory must go is the one that
    // is the same on every machine. The winner already wrote `Boulders/cliff`, so the loser
    // disambiguates to `cliff_<hash6>` there and finds this.
    std::fs::write(
        store.path().join(disambiguated("Terrain/Boulders", &hash)),
        b"",
    )
    .expect("puts something in the way of the loser's second row");

    // The loser: a real run, reading rows the winner has not committed.
    let _ = handler_over(&pool, store.path())
        .handle(&migrate_job(seeded()))
        .await;

    holder.rollback().await.expect("lets the winner commit");
    assert_eq!(
        winner
            .await
            .expect("the winner's task")
            .expect("the winner migrates"),
        Outcome::Migrated
    );

    assert_eq!(
        storage_path_of(&pool, "Terrain/Rocks/cliff.stl")
            .await
            .as_deref(),
        Some(committed.as_str())
    );
    assert!(
        store.path().join(&committed).exists(),
        "the file a committed row names was reaped by a second runner"
    );
    assert!(
        store
            .path()
            .join(disambiguated("Terrain/Rocks", &hash))
            .join("metadata.json")
            .exists(),
        "and its manifest with it"
    );
    assert!(
        !store.path().join(cas_rel(&hash)).exists(),
        "the old copy went when the winner settled, which is what makes the reap permanent"
    );
    // Through the download route, at the level the row records: the check a half-migrated
    // store fails and a raw read cannot make.
    downloads_as(&pool, store.path(), "Terrain/Rocks/cliff.stl", CLIFF).await;
    downloads_as(&pool, store.path(), "Terrain/Boulders/cliff.stl", CLIFF).await;
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_runner_leaves_no_duplicate_model_directory(pool: PgPool) {
    // The same stale view, one step earlier. A second runner that re-processes a row the
    // first has committed sees the first's directory, disambiguates around it without asking
    // whose it is, writes a complete second copy of the file and the manifest — and then
    // settles nothing, because the row already holds a path. A whole model directory on the
    // volume that no database row names.
    let store = tempfile::tempdir().expect("store");
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;

    let mut holder = pool.begin().await.expect("a connection of its own");
    sqlx::query("SELECT blake3 FROM blob WHERE blake3 = $1 FOR NO KEY UPDATE")
        .bind(hash.to_hex())
        .fetch_one(&mut *holder)
        .await
        .expect("locks the blob row settle updates");
    let winner = tokio::spawn({
        let handler = handler_over(&pool, store.path());
        async move { handler.handle(&migrate_job(seeded())).await }
    });
    wait_for_lock_waiters(&mut holder, 1).await;

    // Spawned, not awaited: a loser that gets as far as its own `settle` parks on the same
    // row lock this test is holding, so waiting for it here would wait forever. Either it
    // skipped the hash and finished, or it is queued behind the winner — both are terminal.
    let loser = tokio::spawn({
        let handler = handler_over(&pool, store.path());
        async move { handler.handle(&migrate_job(seeded())).await }
    });
    while !loser.is_finished() && lock_waiters(&mut holder).await < 2 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    holder.rollback().await.expect("lets them both finish");
    winner
        .await
        .expect("the winner's task")
        .expect("the winner migrates");
    let _ = loser.await.expect("the loser's task");

    let mut in_category: Vec<String> =
        std::fs::read_dir(store.path().join("libraries/default/Terrain/Rocks"))
            .expect("reads the category")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
    in_category.sort();
    assert_eq!(
        in_category,
        ["cliff"],
        "a second runner wrote a duplicate model directory that no row names"
    );
    downloads_as(&pool, store.path(), "Terrain/Rocks/cliff.stl", CLIFF).await;
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn one_runner_holds_a_hash_and_the_next_one_is_told_so(pool: PgPool) {
    // The claim on its own, without a file in sight: what `Ok(None)` means, and that the
    // hash comes back when the claim ends — by settling, and by being dropped, which is the
    // path a failed copy loop takes and the reason its reap is safe.
    let store = tempfile::tempdir().expect("store");
    let hash = seed_cas_part(
        &pool,
        store.path(),
        seeded(),
        "Terrain/Rocks/cliff.stl",
        CLIFF,
        Compression::Zstd,
    )
    .await;
    let migrations = PgStorageMigration(pool.clone());

    let held = migrations
        .claim_hash(&hash)
        .await
        .expect("asks")
        .expect("nothing else holds this hash");
    assert_eq!(held.rows().len(), 1, "the row that still says NULL");
    assert!(
        migrations.claim_hash(&hash).await.expect("asks").is_none(),
        "a second runner is told the hash is taken rather than queueing behind a file copy"
    );

    // A row ingested WHILE a claim is open cannot widen it. Today's ingest writes the file
    // into its model directory and records the path in one request (`handler.rs` builds a
    // single `IngestRequest` with `storage_path: Some(..)` for both `record` and
    // `link_existing`), so no path in this build creates a null-`storage_path` row for a
    // blob a migration is holding. That is what lets `migrate_storage` re-slug the libraries
    // its PAGE names and still trust the claim's re-read: the claim can only ever see fewer
    // rows than the page, never one from a library the re-slug pre-pass did not cover.
    let other = second_library(&pool).await;
    let blob = StoredBlobRow {
        hash,
        size_bytes: CLIFF.len() as u64,
        stored_bytes: CLIFF.len() as u64,
        zstd_level: 0,
    };
    PgIngest(pool.clone())
        .link_existing(IngestRequest {
            library: other,
            name: "cliff",
            source_path: "Cliffs/cliff.stl",
            folder: None,
            storage_path: Some("libraries/terrain packs/cliff/cliff.stl"),
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp: None,
            kernel_version: "lapidary-mesh 0.1.0",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("ingests a part that shares the blob being migrated");

    let settled = held.settle(&[]).await.expect("settles nothing and commits");
    assert!(
        !settled,
        "a row that shares this blob still reads it, so the old copy stays"
    );
    let after = migrations
        .claim_hash(&hash)
        .await
        .expect("asks")
        .expect("the hash came back with the transaction that held it");
    assert_eq!(
        after
            .rows()
            .iter()
            .map(|row| row.source_path.as_str())
            .collect::<Vec<_>>(),
        ["Terrain/Rocks/cliff.stl"],
        "the claim re-reads un-migrated rows only, so a row ingested since is not in it"
    );

    // Dropped rather than settled: the transaction rolls back and the lock goes with it.
    // sqlx rolls a dropped transaction back on the connection as it returns to the pool, so
    // this asks until it has, rather than assuming the drop finished the round trip.
    drop(after);
    for attempt in 0.. {
        if migrations.claim_hash(&hash).await.expect("asks").is_some() {
            break;
        }
        assert!(attempt < 200, "a dropped claim never released its hash");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
