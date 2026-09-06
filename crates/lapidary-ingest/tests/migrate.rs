//! Moving an existing content-addressed store into model directories, the way the worker
//! moves it: through `JobHandler::handle` on a `migrate_storage` row.
//!
//! Every fixture here is a store as it existed before slice 7 — the source file at
//! `blobs/ab/cd/<hash>` under zstd, `file.storage_path` null, and the category tree
//! back-filled by migration `0008` with `slug = name`, unslugged. That is what
//! `seed_cas_part` reproduces, and nothing below is testing this job against a store this
//! job has already touched.

use lapidary_core::manifest::ModelManifest;
use lapidary_core::{BatchId, BlobHash, JobId, JobPayload, LibraryId, MeshMeasurements, Outcome};
use lapidary_db::{
    IngestRequest, JobRow, PgFolders, PgIngest, PgPool, PgStorageMigration, StoredBlobRow,
};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::JobHandler;
use lapidary_storage::{Compression, SourceStore, WorkerRole};
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

/// Make the row update fail, and only it.
///
/// The kill switch this suite needs sits at exactly one seam: the bytes are written and
/// the transaction that would record them has not committed. A `CHECK` on `storage_path`
/// fires there and nowhere earlier — every existing row holds NULL, and `NULL NOT LIKE …`
/// is NULL rather than false, so adding the constraint validates the table without
/// refusing anything already in it. Borrowed from `tests/handler.rs`'s `refuse_this_file`,
/// which is why this job needed no copy-only entry point of its own to be interruptible.
async fn refuse_the_row_update(pool: &PgPool) {
    // `AssertSqlSafe` because `ALTER TABLE` takes no bind parameters; the statement is a
    // literal, never anything read back out of the database.
    sqlx::query(sqlx::AssertSqlSafe(
        "ALTER TABLE file ADD CONSTRAINT file_storage_path_refused \
         CHECK (storage_path NOT LIKE 'libraries/%')"
            .to_owned(),
    ))
    .execute(pool)
    .await
    .expect("adds the refusing constraint");
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

    refuse_the_row_update(&pool).await;
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
