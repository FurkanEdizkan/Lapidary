//! The handler, exercised the way the worker exercises it.

use lapidary_cad::{
    AssemblyNode, AssemblyTree, CadError, CadMetadata, Entity, Kernel, KernelOutput, KernelParams,
    KernelVersion, MeasurementProvenance, MeshKernel,
};
use lapidary_core::manifest::ModelManifest;
use lapidary_core::{
    BatchId, BlobHash, DerivativeKind, JobId, JobPayload, LibraryId, MeshMeasurements, Outcome,
    RevisionId,
};
use lapidary_db::{
    GridQuery, IngestRequest, JobRow, PartRepository, PgIngest, PgParts, Sort, StoredBlobRow,
};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &str = "bracket-lp-1042-03.stl";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
const CARRIER: &str = "planetary-carrier-lp-3480-02.3mf";
const CARRIER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

/// A `JobRow` shaped exactly like `PgJobs::enqueue_scan` would produce for `file`:
/// `payload` carries only `{"path": file}`, and `attempts`/`max_attempts` are irrelevant
/// here because the handler under test never consults the retry policy — that is
/// `lapidary-jobs`'s job, not this crate's.
fn job_for(file: &str) -> JobRow {
    job_for_library(seeded(), file)
}

/// `job_for`, against a library other than the seeded one. Two of the cases below turn
/// on *which* library a job names, which is the whole point of the short-circuit being
/// scoped to a library rather than to a hash.
fn job_for_library(library: LibraryId, file: &str) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: library,
        kind: "ingest_file".to_owned(),
        payload: serde_json::json!({ "path": file }),
        attempts: 1,
        max_attempts: 3,
    }
}

fn handler_over(pool: &PgPool, ingest_dir: &Path, blob_root: &Path) -> WorkerHandler {
    WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
        cad: None,
    }
}

/// A `derive` job against the seeded library, shaped exactly as task 8's enqueue routes
/// will write it: the kind is the COLUMN, and the payload names the revision rather than
/// the part, so nothing re-resolves "latest" between enqueue and run (design §3.7).
fn derive_job(revision: RevisionId, produce: DerivativeKind) -> JobRow {
    derive_job_for(seeded(), revision, produce)
}

/// `derive_job`, against a library other than the one that owns the revision. The library
/// is the job's own COLUMN, not part of the payload, which is what lets the query be
/// scoped by it.
fn derive_job_for(library: LibraryId, revision: RevisionId, produce: DerivativeKind) -> JobRow {
    let payload = JobPayload::Derive { revision, produce };
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: library,
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// The revision of the single part these tests ingested — what a `derive` payload names.
async fn only_revision(pool: &PgPool) -> RevisionId {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM revision")
        .fetch_one(pool)
        .await
        .expect("exactly one revision");
    RevisionId::from_uuid(id)
}

/// A library that declines to render (migration `0005`). The seeded row is updated rather
/// than a second library inserted, so every helper above keeps working unchanged.
async fn stop_rendering(pool: &PgPool, library: LibraryId) {
    sqlx::query("UPDATE library SET auto_thumbnail = false WHERE id = $1")
        .bind(library.as_uuid())
        .execute(pool)
        .await
        .expect("turns auto_thumbnail off");
}

async fn thumbnail_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM derivative WHERE kind = 'thumbnail'")
        .fetch_one(pool)
        .await
        .expect("count query")
}

async fn part_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(pool)
        .await
        .expect("count query")
}

async fn parts_in(pool: &PgPool, library: LibraryId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(library.as_uuid())
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// Blob rows for *source* bytes — the ones a `file` row points at. Slice 3's LOD ladder
/// writes blob rows too, and every assertion below is about source bytes being stored
/// once however many parts share them.
async fn blob_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM blob WHERE blake3 IN (SELECT blake3 FROM file)")
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// The `ref_count` on the source blob these tests store. Summed rather than fetched so
/// the query still says something if a second source blob row ever appears; scoped to
/// source bytes for the reason `blob_rows` gives.
async fn ref_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(ref_count), 0)::bigint FROM blob \
         WHERE blake3 IN (SELECT blake3 FROM file)",
    )
    .fetch_one(pool)
    .await
    .expect("sum query")
}

/// A second library to ingest the same file into. Migration `0002_parts.sql` seeds one,
/// and there is still no library-creation route, so the row is inserted directly.
async fn second_library(pool: &PgPool) -> LibraryId {
    const ID: &str = "01931b6e-0000-7000-8000-0000000000a2";
    sqlx::query(
        "INSERT INTO library (id, name, slug) VALUES ($1::uuid, 'Fixture jigs', 'fixture jigs')",
    )
    .bind(ID)
    .execute(pool)
    .await
    .expect("seeds a second library");
    LibraryId::from_uuid(Uuid::parse_str(ID).expect("second library id parses"))
}

/// Every regular file under `dir`, recursively — used to prove the blob store holds
/// nothing after a reaped write. Blob storage is sharded two directories deep
/// (`blobs/ab/cd/<hash>`), so a shallow `read_dir` would miss a surviving blob.
fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            out.extend(all_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_stl_ingests_with_its_real_measurements_and_a_decodable_thumbnail(pool: PgPool) {
    // The direct descendant of slice 1's `scanning_one_real_stl_ingests_it_once`. Moving
    // ingest behind a queue puts a brand-new seam exactly where the untested one was, so
    // it gets its guard on day one instead of in a fix wave.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::copy(
        format!("{}/../../fixtures/{BRACKET}", env!("CARGO_MANIFEST_DIR")),
        ingest_dir.path().join(BRACKET),
    )
    .expect("stages the fixture");

    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let outcome = handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(outcome, Outcome::Ingested);

    let (name, tri, x, y, z, watertight, thumb): (
        String,
        i32,
        f64,
        f64,
        f64,
        bool,
        Option<Vec<u8>>,
    ) = sqlx::query_as(
        "SELECT p.name, r.triangle_count, r.bbox_x, r.bbox_y, r.bbox_z, r.is_watertight, \
                d.thumb_bytes \
         FROM part p \
         JOIN revision r ON r.part_id = p.id \
         JOIN derivative d ON d.revision_id = r.id AND d.kind = 'thumbnail' \
         WHERE p.library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("the part, its measurements and its thumbnail all landed");

    assert_eq!(name, "bracket-lp-1042-03");
    assert_eq!(tri, 20, "the fixture's real triangle count");
    assert_eq!(
        (x, y, z),
        (88.0, 40.0, 25.0),
        "the fixture's real bounding box"
    );
    assert!(watertight);

    let thumb = thumb.expect("a thumbnail was written");
    let decoded = image::load_from_memory(&thumb).expect("the thumbnail decodes as an image");
    assert_eq!(decoded.width(), 512, "the thumbnail is a real 512px render");
}

/// A `JobRow` shaped exactly as `lapidary-api`'s upload commit writes one: the kind is
/// the column, and the payload names bytes already in the store rather than a file on the
/// ingest mount.
fn blob_job(hash: BlobHash, source_path: &str) -> JobRow {
    let payload = JobPayload::IngestBlob {
        blake3: hash,
        source_path: source_path.to_owned(),
        lock: None,
    };
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: seeded(),
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// The api's half of an upload, without the api: verify and store the bytes, then record
/// the `blob` row with a zero count. Both halves, because the worker arm under test reads
/// the level off that row and would otherwise decode a zstd frame as an STL.
async fn upload_into(pool: &PgPool, blob_root: &Path, bytes: &[u8]) -> BlobHash {
    let staging = tempfile::tempdir().expect("temp dir");
    let staged = staging.path().join("upload.part");
    std::fs::write(&staged, bytes).expect("stages the upload");
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let stored = lapidary_storage::SourceWriter::open(blob_root)
        .put_file(&staged, &hash, lapidary_storage::Compression::Zstd)
        .expect("stores");
    lapidary_db::PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash: stored.hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        })
        .await
        .expect("records the blob");
    hash
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_blob_ingests_from_the_store_with_no_ingest_mount(pool: PgPool) {
    // The other end of the upload route. The api wrote these bytes and this job is all
    // that connects them to a part -- and `ingest_dir` deliberately points at nothing,
    // because an upload must not need the mount at all.
    let blob_root = tempfile::tempdir().expect("temp dir");
    let hash = upload_into(&pool, blob_root.path(), BRACKET_FIXTURE).await;
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: PathBuf::from("/nonexistent-ingest-dir"),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let outcome = handler
        .handle(&blob_job(hash, "brackets/steel/LP-1042-03.stl"))
        .await
        .expect("ingests");
    assert_eq!(outcome, Outcome::Ingested);

    // The path the browser reported is the part's identity, and the stem is what a
    // person reads -- the same two facts a scanned file lands with, which is what makes
    // an uploaded folder and a scanned folder the same thing here.
    let (name, source_path, tri): (String, String, i32) = sqlx::query_as(
        "SELECT p.name, p.source_path, r.triangle_count \
         FROM part p JOIN revision r ON r.part_id = p.id \
         WHERE p.library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("the part landed");
    assert_eq!(name, "LP-1042-03");
    assert_eq!(source_path, "brackets/steel/LP-1042-03.stl");
    assert!(tri > 0, "the kernel meshed the bytes it read back out");

    // The blob's row was the api's, and the ingest linked to it rather than writing a
    // second one -- `link_existing`, reached through the existing `exists` question.
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(blobs, 1);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_blob_that_is_no_longer_in_the_store_fails_permanently(pool: PgPool) {
    // Nothing wrote the bytes or the row, so the level needed to decode them does not
    // exist. Permanent: the row is written in the same request that writes the bytes, so
    // its absence is not a race that resolves, and three retries would say the same thing
    // three times.
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: PathBuf::from("/nonexistent-ingest-dir"),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET_FIXTURE).as_bytes());

    let err = handler
        .handle(&blob_job(hash, "brackets/LP-1042-03.stl"))
        .await
        .expect_err("there is nothing to ingest");
    assert!(
        matches!(err, HandlerError::Permanent { .. }),
        "expected a permanent failure, got: {err:?}"
    );
}

/// Where the api stages an upload's bytes: at their hash in `blobs/`. A scan never writes
/// there — its file's only copy is the one in the model directory.
fn staged_copy_of(blob_root: &Path, hash: &BlobHash) -> PathBuf {
    let hex = hash.to_hex();
    blob_root
        .join("blobs")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_upload_of_bytes_a_scan_already_filed_reads_them_from_that_part_directory(pool: PgPool) {
    // Re-uploading a file the library already has, at another path. The probe calls the
    // bytes known, so the browser sends none and the commit stages nothing — and since
    // the store became a folder tree a scanned file's only copy is the one in its model
    // directory. A live stack failed this job three times and gave up, with a message
    // blaming cache eviction, on the most ordinary re-upload there is.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), "a/LP-1042-03.stl", BRACKET_FIXTURE);
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };
    let scanned = handler
        .handle(&job_for("a/LP-1042-03.stl"))
        .await
        .expect("scans");
    assert_eq!(scanned, Outcome::Ingested);
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET_FIXTURE).as_bytes());
    assert!(
        !staged_copy_of(blob_root.path(), &hash).exists(),
        "the premise: a scan files its bytes by path only, so there is no hash-addressed copy to read"
    );

    let outcome = handler
        .handle(&blob_job(hash, "b/LP-1042-03.stl"))
        .await
        .expect("ingests from the copy the scan filed");
    assert_eq!(outcome, Outcome::Ingested);

    let filed: Vec<(String, String)> = sqlx::query_as(
        "SELECT p.source_path, f.storage_path FROM part p \
         JOIN revision r ON r.part_id = p.id \
         JOIN file f ON f.revision_id = r.id AND f.role = 'source' \
         WHERE p.library_id = $1 ORDER BY p.source_path",
    )
    .bind(seeded().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("query");
    assert_eq!(filed.len(), 2, "two paths are two parts: {filed:?}");
    assert_eq!(filed[1].0, "b/LP-1042-03.stl");
    assert_eq!(
        std::fs::read(blob_root.path().join(&filed[1].1)).expect("the upload has its own file"),
        BRACKET_FIXTURE,
        "the second directory holds the same bytes, not a reference to the first"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_upload_whose_only_filed_copy_was_edited_fails_permanently_and_indexes_nothing(
    pool: PgPool,
) {
    // The filed copy sits in a folder the owner opens in a file manager, so its name is
    // not proof of its contents. Indexing edited bytes under the hash the job names would
    // record a part whose file is not the file its hash says — the one thing content
    // addressing exists to rule out. Permanent: another attempt reads the same file.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), "a/LP-1042-03.stl", BRACKET_FIXTURE);
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };
    handler
        .handle(&job_for("a/LP-1042-03.stl"))
        .await
        .expect("scans");
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET_FIXTURE).as_bytes());
    let storage_path: String =
        sqlx::query_scalar("SELECT storage_path FROM file WHERE blake3 = $1 AND role = 'source'")
            .bind(hash.to_hex())
            .fetch_one(&pool)
            .await
            .expect("the scan filed it");
    std::fs::write(
        blob_root.path().join(&storage_path),
        b"solid LP-1042-03\nendsolid LP-1042-03\n",
    )
    .expect("the owner edits the file in place");

    let err = handler
        .handle(&blob_job(hash, "b/LP-1042-03.stl"))
        .await
        .expect_err("no stored copy still matches the hash");
    match &err {
        HandlerError::Permanent { message } => assert!(
            message.contains("b/LP-1042-03.stl"),
            "must name the file it could not add, got: {message}"
        ),
        other => panic!("expected a permanent failure, got: {other:?}"),
    }
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(seeded().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(parts, 1, "nothing was indexed from the edited bytes");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_blob_at_an_escaping_path_is_refused(pool: PgPool) {
    // The path never reaches a filesystem on this arm -- it becomes `part.source_path`,
    // and from there a Content-Disposition filename. The api refuses it first; this is
    // the guard on the door rather than on the caller, so a job enqueued any other way
    // meets it too.
    let blob_root = tempfile::tempdir().expect("temp dir");
    let hash = upload_into(&pool, blob_root.path(), BRACKET_FIXTURE).await;
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: PathBuf::from("/nonexistent-ingest-dir"),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let err = handler
        .handle(&blob_job(hash, "../../etc/passwd"))
        .await
        .expect_err("an escaping path is refused");
    assert!(
        matches!(err, HandlerError::Permanent { .. }),
        "expected a permanent failure, got: {err:?}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_file_twice_is_skipped_the_second_time(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::copy(
        format!("{}/../../fixtures/{BRACKET}", env!("CARGO_MANIFEST_DIR")),
        ingest_dir.path().join(BRACKET),
    )
    .expect("stages the fixture");

    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let first = handler.handle(&job_for(BRACKET)).await.expect("ingests");
    let second = handler.handle(&job_for(BRACKET)).await.expect("runs again");
    assert_eq!(first, Outcome::Ingested);
    assert_eq!(
        second,
        Outcome::Skipped,
        "slice 1's short-circuit, through the queue"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_truncated_stl_fails_permanently_so_it_is_never_retried(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    // Long enough to pass the "is this even a file" checks, far short of what the
    // header's declared triangle count needs -- the kernel reports this as a parse
    // failure. Slice 1 caught this inside the scan request; it is a job's failure now.
    std::fs::write(
        ingest_dir.path().join("spacer-lp-2001-00.stl"),
        &BRACKET_FIXTURE[..200],
    )
    .expect("write truncated fixture");

    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let error = handler
        .handle(&job_for("spacer-lp-2001-00.stl"))
        .await
        .expect_err("a truncated file must fail");

    assert!(
        matches!(error, HandlerError::Permanent { .. }),
        "the bytes are immutable, so retrying cannot help: {error:?}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn losing_the_race_for_a_file_is_a_skip_rather_than_a_failure(pool: PgPool) {
    // Two handlers over one staged file, run concurrently -- the lease-expiry race from
    // the design doc, section 3.5. One inserts; the other hits
    // part_name_unique_per_library and must report Skipped, not a failure the user sees.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::copy(
        format!("{}/../../fixtures/{BRACKET}", env!("CARGO_MANIFEST_DIR")),
        ingest_dir.path().join(BRACKET),
    )
    .expect("stages the fixture");

    let handler_a = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };
    let handler_b = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        cad: None,
    };

    let job_a = job_for(BRACKET);
    let job_b = job_for(BRACKET);
    let (first, second) = tokio::join!(handler_a.handle(&job_a), handler_b.handle(&job_b));
    let outcomes = [
        first.expect("one succeeds"),
        second.expect("the other does too"),
    ];
    assert!(outcomes.contains(&Outcome::Ingested), "{outcomes:?}");
    assert!(outcomes.contains(&Outcome::Skipped), "{outcomes:?}");

    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(seeded().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(parts, 1, "the race must not produce two parts");

    // The half this test did not check until a review restored the bug and watched it stay
    // green: the LOSER must not reap. The outcome and the row count cannot see a reap at
    // all, so this crosses to the filesystem.
    //
    // For the source file the two workers usually do NOT collide, and that is worth saying
    // plainly because it is easy to assume otherwise: `model_dir_for` disambiguates, so a
    // loser that resolves its directory after the winner has written one takes
    // `bracket-lp-1042-03_<hash6>/` and reaps only its own. Measured over 20 runs against a
    // restored bug, the source path never collided. What this assertion pins is therefore
    // the weaker but still real claim that the winner's file is intact and unaltered; the
    // rungs below are where the collision is deterministic.
    let (storage_path, blake3): (Option<String>, String) =
        sqlx::query_as("SELECT storage_path, blake3 FROM file WHERE role = 'source'")
            .fetch_one(&pool)
            .await
            .expect("the winner's file row");
    let storage_path = storage_path.expect("the winner recorded where its bytes went");
    let on_disk = std::fs::read(blob_root.path().join(&storage_path))
        .unwrap_or_else(|e| panic!("the winner's bytes must still be at {storage_path}: {e}"));
    assert_eq!(
        BlobHash::from_bytes(*blake3::hash(&on_disk).as_bytes()).to_hex(),
        blake3,
        "the bytes on disk must still be the bytes the winning row recorded"
    );

    // The rungs are where the loser's reap bites deterministically. Both workers meshed
    // the same file, so both produced the same rung bytes and both saw `blobs.exists`
    // answer false for them -- the loser therefore holds every one of the winner's rungs
    // in its own reapable list. Reaping them on the way to `Skipped` leaves the winner's
    // `derivative` rows pointing at bytes that are no longer there, and every assertion
    // above this one still passes while it happens.
    let rungs: Vec<String> =
        sqlx::query_scalar("SELECT blake3 FROM derivative WHERE blake3 IS NOT NULL")
            .fetch_all(&pool)
            .await
            .expect("the winner's hash-addressed derivatives");
    // Verified by restoring the bug: with the guard flipped to an unconditional reap this
    // goes red on the runs where the two handlers genuinely overlap, and stays green on the
    // runs where the scheduler serialises them -- in which case the second job
    // short-circuits at `library_holds` and never writes anything to reap. A review
    // measured that at 2 catches in 20 runs, so this is a corroborating check and not the
    // guard: `only_a_lost_race_for_the_same_path_is_a_skip_rather_than_an_error` in
    // `handler.rs` pins the branch condition itself, deterministically and without a
    // scheduler. Forcing the overlap here would mean a barrier in the pipeline, which is a
    // larger change than the thing it would pin.
    assert!(!rungs.is_empty(), "the ingest wrote at least one rung");
    for hash in rungs {
        let path = blob_root
            .path()
            .join(format!("blobs/{}/{}/{hash}", &hash[0..2], &hash[2..4]));
        assert!(
            path.exists(),
            "a derivative row points at {hash}, which is not on disk: the losing worker \
             reaped bytes the winning row still serves"
        );
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_known_hash_is_skipped_before_the_kernel_ever_sees_the_bytes(pool: PgPool) {
    // Moved from tests/scan.rs, which drove it through the HTTP route until the route
    // stopped ingesting. The claim is stronger than
    // `the_same_file_twice_is_skipped_the_second_time` above and is not implied by it:
    // these bytes are not an STL by any reading, so `kernel.ingest` on them is always an
    // error. Getting `Skipped` rather than an error is what proves step 3's short-circuit
    // ran BEFORE step 4 -- with a valid fixture, "skipped after parsing" and "skipped
    // without parsing" are indistinguishable.
    const NOT_AN_STL: &[u8] = b"LP-1042-03 revision notes: chamfer the mounting face.\n";
    let hash = BlobHash::from_bytes(*blake3::hash(NOT_AN_STL).as_bytes());

    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join("notes.stl"), NOT_AN_STL).expect("write fixture");

    // Seeded through PgIngest rather than hand-written INSERTs, so this stands in for a
    // part a previous scan really did record.
    let blob = StoredBlobRow {
        hash,
        size_bytes: NOT_AN_STL.len() as u64,
        stored_bytes: NOT_AN_STL.len() as u64,
        zstd_level: 3,
    };
    let measurements = MeshMeasurements {
        bbox_mm: [12.0, 8.0, 3.0],
        triangle_count: 4,
        surface_area_mm2: 240.0,
        volume_mm3: None,
        is_watertight: false,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: None,
            library: seeded(),
            name: "notes",
            // Must be the path the job below carries, not the part name: since slice 6a
            // the short-circuit is keyed on `source_path`, so seeding a different path
            // here would make this test assert a re-scan that never happened.
            source_path: "notes.stl",
            blob: &blob,
            measurements: &measurements,
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: Some(&[0x52, 0x49, 0x46, 0x46]),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeding the already-held part");

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let outcome = handler
        .handle(&job_for("notes.stl"))
        .await
        .expect("the short-circuit must answer before the kernel is ever handed the bytes");

    assert_eq!(outcome, Outcome::Skipped);
    assert_eq!(
        part_count(&pool).await,
        1,
        "no second part for a file this library already holds"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_library_gets_its_own_part_for_bytes_another_library_holds(pool: PgPool) {
    // Moved from tests/scan.rs. The bug it guards was live, and this module's doc writes
    // it up: keyed on the hash alone, ingesting six real STLs into a brand-new empty
    // library answered "skipped 6" and left the library empty.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let second = second_library(&pool).await;
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    assert_eq!(
        handler
            .handle(&job_for_library(second, BRACKET))
            .await
            .expect("ingests into the second library"),
        Outcome::Ingested,
        "a library that does not hold this part must get one, never Skipped"
    );

    // Both libraries show the part -- this is what the user sees, and what was empty.
    assert_eq!(parts_in(&pool, seeded()).await, 1);
    assert_eq!(parts_in(&pool, second).await, 1);

    // One blob ROW, still: `ref_count` counts `file` rows naming a hash, and that meaning
    // is unchanged. What it stopped implying is one copy on disk -- each library's model
    // directory holds its own file, which is the price the spec (§0) names for a store
    // the owner can open in a file manager.
    assert_eq!(blob_rows(&pool).await, 1, "one blob row, not two");
    assert_eq!(
        ref_count(&pool).await,
        2,
        "one reference per part row, and there are now two"
    );
    assert_eq!(
        all_files(&blob_root.path().join("blobs")).len(),
        1,
        "derivatives are still content-addressed and still deduplicated: every rung of a \
         20-triangle bracket clusters to the same mesh, so the ladder is one blob"
    );
    let copies: Vec<PathBuf> = all_files(&blob_root.path().join("libraries"))
        .into_iter()
        .filter(|f| f.ends_with(BRACKET))
        .collect();
    assert_eq!(
        copies.len(),
        2,
        "each library holds its own copy of the file, under its own name: {copies:?}"
    );

    // A third run against either library is a genuine re-scan and does nothing.
    assert_eq!(
        handler
            .handle(&job_for_library(second, BRACKET))
            .await
            .expect("runs again"),
        Outcome::Skipped
    );
    assert_eq!(part_count(&pool).await, 2);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_differently_named_files_with_identical_bytes_are_two_parts_sharing_one_blob(
    pool: PgPool,
) {
    // Moved from tests/scan.rs. A directory holding two differently-named copies of the
    // same geometry is a directory holding two files, and indexing only the first is the
    // same silent omission as the empty second library. They share the blob; they do not
    // share the part.
    const MIRRORED: &str = "bracket-lp-1042-03-mirrored.stl";
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(MIRRORED), BRACKET_FIXTURE)
        .expect("write second fixture");

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    assert_eq!(
        handler.handle(&job_for(MIRRORED)).await.expect("ingests"),
        Outcome::Ingested,
        "two files in the folder, two cards"
    );

    assert_eq!(part_count(&pool).await, 2);
    assert_eq!(blob_rows(&pool).await, 1);
    assert_eq!(ref_count(&pool).await, 2);
    // One blob row and one rung on disk -- both parts are the same mesh, so their rungs
    // are the same bytes -- but two source files, because two models are two directories.
    assert_eq!(all_files(&blob_root.path().join("blobs")).len(), 1);
    assert_eq!(all_files(&blob_root.path().join("libraries")).len(), 4);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failure_after_the_blob_write_leaves_no_orphan_blob_on_disk(pool: PgPool) {
    // Moved from tests/scan.rs. The Node prototype wrote its blob and then failed the
    // insert with no cleanup -- docs/prototype-notes.md records it -- so this guards a
    // bug that actually shipped rather than a hypothetical one.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");

    // The failure has to land AFTER the writes, which is the only way to exercise the reap
    // at all. It used to be injected by naming a library that is not a row -- but that has
    // been refused at step 3a (`auto_thumbnail` answers `None` for a missing library) since
    // slice 4, before a single byte is written, so the assertions below were passing over a
    // tree nothing had touched. A constraint the file insert violates puts the failure back
    // where the test says it is: inside the transaction, with the source file, its
    // directory and the rungs already on disk.
    refuse_this_file(&pool, "%bracket-lp-1042-03%").await;

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler
        .handle(&job_for(BRACKET))
        .await
        .expect_err("a file row the schema refuses cannot be written");

    // The mutation this pins: delete `reap_source`, or the `reap` beside it, from the
    // failure arm and a file survives on disk, failing one of the assertions below. The
    // returned error looks identical either way, which is why this checks the filesystem,
    // not the message.
    let orphans = all_files(blob_root.path());
    assert!(
        orphans.is_empty(),
        "expected no orphaned source file, rung or directory under {}, found {orphans:?}",
        blob_root.path().display()
    );
    // The empty directory goes too: `model_dir_for` reads "this directory exists" as "this
    // name is taken", so one left behind would make the retry of this same file store the
    // model under a disambiguated name it never earned.
    assert!(
        !blob_root
            .path()
            .join("libraries/default/bracket-lp-1042-03")
            .exists(),
        "the model directory must not survive the transaction that failed inside it"
    );
    assert_eq!(part_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failed_link_to_existing_bytes_leaves_the_first_parts_blobs_alone(pool: PgPool) {
    // The other half of the reap, and the dangerous half. The second file's rungs are
    // bytes the first part already stores, so reaping those would delete a part that
    // ingested perfectly well. A reap keyed on "this job wrote it" rather than "this job's
    // transaction failed" is what stops that -- and the source file is no longer shared at
    // all: two models are two files now, so the failing job owns its own copy and takes
    // exactly that one with it.
    const MIRRORED: &str = "bracket-lp-1042-03-mirrored.stl";
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(MIRRORED), BRACKET_FIXTURE).expect("write second");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let after_first = all_files(blob_root.path());
    assert_eq!(
        after_first.len(),
        3,
        "the ladder's one rung, the first model's file and its manifest: {after_first:?}"
    );

    // Same bytes, so `blobs.exists` sends this down link_existing; a constraint only the
    // second file violates fails its transaction after its own rungs and its own copy of
    // the source have been written.
    refuse_this_file(&pool, "%mirrored%").await;
    handler
        .handle(&job_for(MIRRORED))
        .await
        .expect_err("a file row the schema refuses cannot be written");

    assert_eq!(
        all_files(blob_root.path()),
        after_first,
        "the failed second ingest must leave the first part's bytes exactly as they were"
    );
    assert_eq!(part_count(&pool).await, 1, "the first part is still there");
}

/// Make the `file` insert fail for one path, and only for it.
///
/// A failure injected *inside* the transaction, which is where the reap's whole reason to
/// exist lives: the source file, its directory and the rungs are already on disk by then.
/// A `CHECK` on `storage_path` is the smallest thing that fires there and nowhere earlier
/// -- naming a library that does not exist is refused at step 3a, before anything is
/// written, which is what made two reap tests pass over a tree nothing had touched.
async fn refuse_this_file(pool: &PgPool, pattern: &str) {
    // `AssertSqlSafe` because `ALTER TABLE` cannot take a bind parameter and sqlx refuses
    // a non-static statement otherwise. `pattern` is a literal from the two call sites
    // below, never anything a test reads back out of the database.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE file ADD CONSTRAINT file_storage_path_refused \
         CHECK (storage_path NOT LIKE '{pattern}')"
    )))
    .execute(pool)
    .await
    .expect("adds the refusing constraint");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_stl_writes_one_tessellation_blob_and_row(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM derivative ORDER BY kind")
        .fetch_all(&pool)
        .await
        .expect("kinds");
    // Exactly one tessellation row, and it is L0. L1 and L2 are no longer ingest's to
    // write -- they arrive through a `derive` job, which is what this slice exists for.
    assert_eq!(kinds, vec!["tessellation_l0", "thumbnail"]);

    // The rung row points at bytes that are really on disk, through a blob row that
    // really exists -- the whole chain migration 0004's foreign key exists to require.
    let hashes: Vec<String> = sqlx::query_scalar(
        "SELECT d.blake3 FROM derivative d JOIN blob b ON b.blake3 = d.blake3 \
         WHERE d.kind LIKE 'tessellation%'",
    )
    .fetch_all(&pool)
    .await
    .expect("hashes");
    assert_eq!(hashes.len(), 1, "one rung, with a blob row");
    let store = lapidary_storage::DerivativeStore::open(blob_root.path());
    for hex in &hashes {
        let hash = BlobHash::parse_hex(hex).expect("a stored hash parses");
        let bytes = store.get(&hash).expect("the rung's bytes are on disk");
        assert_eq!(&bytes[0..4], b"glTF", "a rung is a glTF binary file");
    }
}

const IDLER_OBJ: &str = "idler-bracket-lp-2210-01.obj";
const IDLER_OBJ_FIXTURE: &[u8] = include_bytes!("../../../fixtures/idler-bracket-lp-2210-01.obj");
const GEAR: &str = "spur-gear-m2-20t-lp-5140-00.stl";
const GEAR_FIXTURE: &[u8] =
    include_bytes!("../../../example/parts/spur-gear-m2-20t-lp-5140-00.stl");

/// Kind and hash for every derivative in the database, ordered by kind.
async fn derivatives(pool: &PgPool) -> Vec<(String, Option<String>)> {
    sqlx::query_as("SELECT kind, blake3 FROM derivative ORDER BY kind")
        .fetch_all(pool)
        .await
        .expect("derivative rows")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_obj_yields_the_same_with_its_format_recorded(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(IDLER_OBJ), IDLER_OBJ_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(IDLER_OBJ)).await.expect("ingests"),
        Outcome::Ingested
    );

    let kinds: Vec<String> = derivatives(&pool)
        .await
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        kinds,
        vec!["tessellation_l0", "thumbnail"],
        "an OBJ produces the same two derivatives an STL does"
    );

    let format: String = sqlx::query_scalar("SELECT format FROM file")
        .fetch_one(&pool)
        .await
        .expect("format");
    assert_eq!(format, "obj", "the row must not still say 'stl'");

    // The part name is the stem, so the extension must not survive into it.
    let name: String = sqlx::query_scalar("SELECT name FROM part")
        .fetch_one(&pool)
        .await
        .expect("name");
    assert_eq!(name, "idler-bracket-lp-2210-01");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_kernel_version_differs_between_an_stl_and_an_obj_ingest(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write stl");
    std::fs::write(ingest_dir.path().join(IDLER_OBJ), IDLER_OBJ_FIXTURE).expect("write obj");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("stl");
    handler.handle(&job_for(IDLER_OBJ)).await.expect("obj");

    let versions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT kernel_version FROM derivative ORDER BY kernel_version",
    )
    .fetch_all(&pool)
    .await
    .expect("versions");
    // Two parsers, two versions. A single value here means an OBJ-derived derivative is
    // indistinguishable from an STL-derived one, which is the thing the column exists to
    // prevent -- and it would still pass every row-count assertion above.
    assert_eq!(
        versions,
        vec!["mesh obj-1+glb-3+cpu-1", "mesh stl-1+glb-3+cpu-1"]
    );
}

/// Triangle count read out of a stored `.glb`, by a reader that shares nothing with the
/// writer. Task 4 gives the reason: a self-consistent writer passes a reader built from
/// its own arithmetic every time.
fn triangles_in_glb(bytes: &[u8]) -> u64 {
    let u32_at = |at: usize| {
        let mut four = [0u8; 4];
        four.copy_from_slice(&bytes[at..at + 4]);
        u32::from_le_bytes(four)
    };
    assert_eq!(&bytes[0..4], b"glTF", "magic");
    assert_eq!(u32_at(4), 2, "glTF 2.0");
    assert_eq!(
        u32_at(8) as usize,
        bytes.len(),
        "the declared length is the real length"
    );
    let json_len = u32_at(12) as usize;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes[20..20 + json_len]).expect("the JSON chunk parses");
    // Accessor 1 is the index accessor; three indices per triangle.
    json["accessors"][1]["count"].as_u64().expect("count") / 3
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn each_rung_is_valid_gltf_and_l0_is_smaller_than_l2(pool: PgPool) {
    // Deliberately not the bracket: at 20 triangles it is coarser than L0's grid and
    // clusters to itself, so its rungs are identical by design (spec §3.6). Proving the
    // ladder actually ladders needs a mesh dense enough to decimate.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(GEAR), GEAR_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(GEAR)).await.expect("ingests"),
        Outcome::Ingested
    );

    // Ingest writes L0 alone now, so the two rungs this test compares against have to be
    // built the way anything else will build them: a `derive` job each. Trimming the
    // assertion to what ingest still writes would leave L0 compared with itself, which
    // passes whatever the clusterer does and proves nothing.
    let revision = only_revision(&pool).await;
    for rung in [
        DerivativeKind::TessellationL1,
        DerivativeKind::TessellationL2,
    ] {
        assert_eq!(
            handler
                .handle(&derive_job(revision, rung))
                .await
                .unwrap_or_else(|error| panic!("derives {}: {error:?}", rung.as_str())),
            Outcome::Rendered,
            "a derive job renders; it does not ingest"
        );
    }

    let store = lapidary_storage::DerivativeStore::open(blob_root.path());
    let mut counts = Vec::new();
    for (kind, blake3) in derivatives(&pool).await {
        let Some(hex) = blake3 else {
            assert_eq!(kind, "thumbnail", "only the thumbnail is stored inline");
            continue;
        };
        let hash = BlobHash::parse_hex(&hex).expect("a stored hash parses");
        let bytes = store.get(&hash).expect("the rung's bytes are on disk");
        counts.push((kind, triangles_in_glb(&bytes)));
    }

    assert_eq!(counts.len(), 3);
    let count = |k: &str| counts.iter().find(|(kind, _)| kind == k).expect("rung").1;
    assert!(
        count("tessellation_l0") < count("tessellation_l2"),
        "L0 {} must be coarser than L2 {} -- a ladder wired up but not laddered passes \
         every row count above and still ships three copies of the full mesh",
        count("tessellation_l0"),
        count("tessellation_l2")
    );
    assert!(
        count("tessellation_l1") <= count("tessellation_l2"),
        "L1 must never exceed L2"
    );
}

/// A slicer's file is written from the part's mesh when a derive job asks for it: a binary STL and
/// a 3MF, each holding every triangle the file was read with, counted by readers that share
/// nothing with the writers.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_derive_job_writes_a_slicer_its_stl_and_3mf_with_every_triangle(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(GEAR), GEAR_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(GEAR)).await.expect("ingests"),
        Outcome::Ingested
    );
    let revision = only_revision(&pool).await;
    for export in [DerivativeKind::ExportStl, DerivativeKind::Export3mf] {
        assert_eq!(
            handler
                .handle(&derive_job(revision, export))
                .await
                .unwrap_or_else(|error| panic!("derives {}: {error:?}", export.as_str())),
            Outcome::Rendered
        );
    }

    let triangles: i64 = sqlx::query_scalar("SELECT triangle_count::bigint FROM revision")
        .fetch_one(&pool)
        .await
        .expect("the count ingest read");
    let rows = derivatives(&pool).await;
    let store = lapidary_storage::DerivativeStore::open(blob_root.path());
    let stored = |kind: &str| {
        let hex = rows
            .iter()
            .find(|(row, _)| row == kind)
            .and_then(|(_, hash)| hash.clone())
            .unwrap_or_else(|| panic!("no stored {kind} row"));
        store
            .get(&BlobHash::parse_hex(&hex).expect("a stored hash parses"))
            .expect("the bytes are on disk")
    };

    let stl = stored("export_stl");
    let count = u32::from_le_bytes(stl[80..84].try_into().expect("four bytes"));
    assert_eq!(i64::from(count), triangles, "every triangle, in the STL");
    assert_eq!(
        stl.len(),
        84 + 50 * count as usize,
        "a binary STL is as long as its count says"
    );
    let three_mf = stored("export_3mf");
    assert!(three_mf.starts_with(b"PK\x03\x04"), "a 3MF is a ZIP");
    assert_eq!(
        String::from_utf8_lossy(&three_mf)
            .matches("<triangle ")
            .count() as i64,
        triangles,
        "every triangle, in the 3MF"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_3mf_yields_a_thumbnail_and_one_rung(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("ingests"),
        Outcome::Ingested
    );

    let kinds: Vec<String> = derivatives(&pool)
        .await
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        kinds,
        vec!["tessellation_l0", "thumbnail"],
        "a 3MF produces the same two derivatives an STL does"
    );

    let (format, version): (String, String) = sqlx::query_as(
        "SELECT f.format, d.kernel_version FROM file f \
         JOIN derivative d ON d.revision_id = f.revision_id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(format, "3mf");
    assert_eq!(version, "mesh 3mf-1+glb-3+cpu-1");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_source_file_is_stored_as_itself_whatever_its_format(pool: PgPool) {
    // This test used to assert DATA.md §1.2's table -- a 3MF stored as-is, an STL
    // compressed at zstd -3 -- against `blobs/ab/cd/<hash>`. Half of that is now the wrong
    // question. The store is a folder the owner opens in a file manager (spec §0), and a
    // zstd frame named `bracket-lp-1042-03.stl` is not a file they can open, so ingest
    // writes every source as itself. The compression policy is not deleted, it moved: the
    // recorded `zstd_level` is what every reader follows, so §1.3's opt-out and
    // sub-project 4's cold tiering can turn it back on per file without a reader changing.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write stl");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(CARRIER)).await.expect("3mf");
    handler.handle(&job_for(BRACKET)).await.expect("stl");

    /// One source file as the two tables record it: format, the file's two sizes and its
    /// recorded compression level, where the bytes were written, and what the blob row says its
    /// content-addressed copy holds.
    type StoredSource = (String, i64, Option<i64>, Option<i16>, Option<String>, i64);

    let rows: Vec<StoredSource> = sqlx::query_as(
        "SELECT f.format, f.size_bytes, f.stored_bytes, f.zstd_level, f.storage_path, b.stored_bytes \
         FROM blob b JOIN file f ON f.blake3 = b.blake3 ORDER BY f.format",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(rows.len(), 2, "one source blob row per file");

    for (format, size_bytes, stored_bytes, zstd_level, storage_path, blob_copy) in rows {
        assert_eq!(
            Some(size_bytes),
            stored_bytes,
            "a {format} is stored at its own size, on its file row"
        );
        assert_eq!(zstd_level, Some(0), "a {format} records no compression");
        assert_eq!(
            blob_copy, 0,
            "and a filed {format} has no content-addressed copy for its blob row to count (0026)"
        );
        let path = storage_path.expect("every ingested file records where it was written");
        let fixture: &[u8] = if format == "3mf" {
            CARRIER_FIXTURE
        } else {
            BRACKET_FIXTURE
        };
        assert_eq!(
            std::fs::read(blob_root.path().join(&path)).expect("the file is at that path"),
            fixture,
            "the file in the folder is byte-identical to the one ingested ({path})"
        );
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_refused_3mf_leaves_no_part_and_no_blob(pool: PgPool) {
    // A ZIP whose model entry expands far past the ratio cap. Built here rather than
    // committed: a fixture that is genuinely hostile is not something to keep in a repo.
    //
    // The relationships part is NOT optional padding. `parse_3mf` reads `_rels/.rels`
    // before it reads the model, so a bomb without one fails on the missing rels part and
    // never touches the cap — the test would still pass, still prove the reap, and
    // silently stop testing the thing it is named for.
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("_rels/.rels", opts).expect("start");
    std::io::Write::write_all(
        &mut w,
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/3dmodel.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#,
    )
    .expect("write");
    w.start_file("3D/3dmodel.model", opts).expect("start");
    std::io::Write::write_all(&mut w, &vec![0u8; 64 * 1024 * 1024]).expect("write");
    let bomb = w.finish().expect("finish").into_inner();

    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join("bomb.3mf"), &bomb).expect("write");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    let err = handler
        .handle(&job_for("bomb.3mf"))
        .await
        .expect_err("a refused archive is a permanent failure");
    // Assert on WHICH refusal. Without this the test passes on any error at all, which is
    // how it came to prove the reap while never reaching the cap.
    // `HandlerError` derives `Debug` but not `Display` -- `{err:?}` rather than the
    // brief's `err.to_string()`/`{err}`, same substring check against the same message.
    assert!(
        format!("{err:?}").contains("Refused this 3MF"),
        "expected the archive cap to refuse it, got: {err:?}"
    );
    assert_eq!(part_count(&pool).await, 0);
    assert!(
        all_files(&blob_root.path().join("blobs")).is_empty(),
        "a refused file must leave nothing behind"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_that_declines_to_render_gets_no_thumbnail_and_still_fills_the_grid(
    pool: PgPool,
) {
    // The slice's exit criterion, one part wide: a library with `auto_thumbnail = false`
    // ingests with zero thumbnail rows, and every part is still IN the grid showing "no
    // preview yet". A row missing from the grid would be data the user cannot see.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    stop_rendering(&pool, seeded()).await;
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );

    assert_eq!(
        thumbnail_rows(&pool).await,
        0,
        "a library that declines to render must write no thumbnail row at all: an empty \
         bytea is not NULL, so it would reach the grid as a broken image"
    );
    let kinds: Vec<String> = derivatives(&pool)
        .await
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        kinds,
        vec!["tessellation_l0"],
        "the rung is still written -- it is the grid's own LOD, not a preview"
    );

    let page = PgParts(pool.clone())
        .page(&GridQuery::new(seeded(), 10), Sort::Newest)
        .await
        .expect("page");
    assert_eq!(page.len(), 1, "a part with no preview is still a part");
    assert_eq!(page[0].summary.name, "bracket-lp-1042-03");
    assert_eq!(page[0].summary.triangle_count, Some(20));
    assert_eq!(page[0].thumbnail_webp, None);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_derive_job_fills_the_missing_thumbnail_and_reports_rendered(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    stop_rendering(&pool, seeded()).await;
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(thumbnail_rows(&pool).await, 0, "nothing rendered at ingest");

    // The ingest directory is irrelevant from here: a derive job re-reads the SOURCE BLOB
    // for the revision it names, which is the only copy still guaranteed to exist once
    // the mount the file arrived on is gone.
    std::fs::remove_file(ingest_dir.path().join(BRACKET)).expect("removes the ingested file");
    let revision = only_revision(&pool).await;
    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::Thumbnail))
            .await
            .expect("derives the thumbnail"),
        Outcome::Rendered
    );

    assert_eq!(thumbnail_rows(&pool).await, 1);
    let page = PgParts(pool.clone())
        .page(&GridQuery::new(seeded(), 10), Sort::Newest)
        .await
        .expect("page");
    let thumb = page[0]
        .thumbnail_webp
        .clone()
        .expect("the grid now has bytes to show");
    // Decoded rather than length-checked: a zeroed or empty WebP has a length too, and
    // only a decode proves the row holds the image the kernel actually rendered.
    let decoded = image::load_from_memory(&thumb).expect("the thumbnail decodes as an image");
    assert_eq!(decoded.width(), 512, "a real 512px render");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unknown_job_kind_fails_permanently_naming_the_kind(pool: PgPool) {
    // What an operator reads when something puts a job this build does not know into the
    // queue. It used to read "This job has no file path in its payload", which was false
    // for every one of them and sent the reader looking for a scan that never happened.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let mut job = job_for(BRACKET);
    job.kind = "transmute_to_step".to_owned();

    let err = handler
        .handle(&job)
        .await
        .expect_err("an unknown kind is a permanent failure");
    let message = format!("{err:?}");
    assert!(
        message.contains("transmute_to_step"),
        "the failure must name the kind it could not run, got: {message}"
    );
    assert!(
        !message.contains("no file path"),
        "the old message was false for every unknown kind, got: {message}"
    );
    assert!(
        matches!(err, HandlerError::Permanent { .. }),
        "an unknown kind will not become known on a retry, got: {err:?}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_derive_job_naming_another_librarys_revision_renders_nothing(pool: PgPool) {
    // A revision id is a uuid a caller might hold from anywhere, and the payload carries
    // one. Without the scope this job renders onto a revision its library does not own --
    // a cross-tenant WRITE, which is the direction that cannot be undone. CLAUDE.md:
    // content addressing is not authorization, and neither is knowing a revision id.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    stop_rendering(&pool, seeded()).await;
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(thumbnail_rows(&pool).await, 0, "nothing rendered at ingest");
    let revision = only_revision(&pool).await;
    let other_library = second_library(&pool).await;

    let err = handler
        .handle(&derive_job_for(
            other_library,
            revision,
            DerivativeKind::Thumbnail,
        ))
        .await
        .expect_err("a library that does not own this revision must not render onto it");

    let message = format!("{err:?}");
    assert!(
        message.contains(&revision.to_string()) && message.contains(&other_library.to_string()),
        "the failure must name the revision it refused and the library that asked, got: {message}"
    );
    assert!(
        !message.contains("bracket-lp-1042-03"),
        "and it must not leak what the owning library calls its own part, got: {message}"
    );
    assert!(
        matches!(err, HandlerError::Permanent { .. }),
        "a revision this library will never own is not worth three retries, got: {err:?}"
    );
    assert_eq!(
        thumbnail_rows(&pool).await,
        0,
        "the refusal has to happen before the render, or the row is already written"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_ingest_job_for_a_library_that_does_not_exist_fails_permanently_naming_it(pool: PgPool) {
    // Step 3a's `ok_or_else` is the whole guard, and nothing else in this suite can see
    // it: relaxing it to `unwrap_or(true)` leaves all 20 handler tests and the ingest and
    // jobs suites passing, and degrades this case to a Transient "violates foreign key
    // constraint \"part_library_id_fkey\" at line 2772" -- three retries, each paying a
    // full parse, render and blob write, to hand an operator a Postgres constraint name
    // and a line number.
    //
    // Only the classification and the message discriminate. The failed write reaps what it
    // wrote and rolls back its transaction, so "no part row" and "an empty blob store" are
    // true of the degraded path too.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let absent = LibraryId::new();

    let err = handler
        .handle(&job_for_library(absent, BRACKET))
        .await
        .expect_err("a job naming a library that does not exist cannot ingest anything");

    let message = format!("{err:?}");
    assert!(
        message.contains(&absent.to_string()),
        "the failure must name the library an operator has to go looking for, got: {message}"
    );
    assert!(
        !message.contains("foreign key") && !message.contains("part_library_id_fkey"),
        "a Postgres constraint name is not something an operator can act on, got: {message}"
    );
    assert!(
        matches!(err, HandlerError::Permanent { .. }),
        "a library row that is gone will not reappear on a retry, got: {err:?}"
    );
}

/// The L0 rung's blob hash and recorded kernel version — between them, the whole of what
/// "a lazily-built rung must not differ from an eager one" is a claim about.
async fn l0_row(pool: &PgPool) -> (String, String) {
    sqlx::query_as("SELECT blake3, kernel_version FROM derivative WHERE kind = 'tessellation_l0'")
        .fetch_one(pool)
        .await
        .expect("exactly one L0 row, with bytes and a version on it")
}

/// Every `blob` row, source and derivative alike — unlike `blob_rows` above, which is
/// scoped to source bytes. A derive that reproduces bytes already stored must add none.
async fn all_blob_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// How many rows point at these bytes. `ref_count` is what makes eviction safe, so a
/// derive that re-files bytes that are already stored must leave it exactly where it was.
async fn refs_to(pool: &PgPool, blake3: &str) -> i32 {
    sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(blake3)
        .fetch_one(pool)
        .await
        .expect("the rung's blob row")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_derived_l0_is_byte_identical_to_the_one_ingest_wrote(pool: PgPool) {
    // Design section 10's byte-identity criterion, asked of the rung it can actually be
    // asked of. It named L2, and that is unaskable now: ingest builds no L2 at all, which
    // is the point of this slice, so there is no eager rung to compare a lazy one against.
    // L0 is built both ways, so L0 is the one that can drift.
    //
    // The gear, not the bracket: at 20 triangles the bracket is coarser than L0's grid and
    // clusters to itself, so its rungs are identical whatever level produced them and this
    // test would pass under exactly the drift it exists to catch.
    //
    // `auto_thumbnail` stays on, so ingest asks the kernel for [Thumbnail, L0] while the
    // derive asks for [L0]. That difference is the point of the version assertion below:
    // `kernel.version()` names the build, not the run, and must not vary with `produce`.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(GEAR), GEAR_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(GEAR)).await.expect("ingests");

    let (hash_before, version_before) = l0_row(&pool).await;
    let blobs_before = all_blob_rows(&pool).await;
    let refs_before = refs_to(&pool, &hash_before).await;

    let revision = only_revision(&pool).await;
    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::TessellationL0))
            .await
            .expect("derives an L0 over the one ingest already built"),
        Outcome::Rendered
    );

    let (hash_after, version_after) = l0_row(&pool).await;
    assert_eq!(
        hash_after, hash_before,
        "the same bytes from the same source must hash the same however they were asked for"
    );
    assert_eq!(
        version_after, version_before,
        "the recorded kernel must name the build, not what this particular call produced"
    );
    assert_eq!(
        all_blob_rows(&pool).await,
        blobs_before,
        "identical bytes are one blob; a second row means the derive stored a second copy"
    );
    assert_eq!(
        refs_to(&pool, &hash_before).await,
        refs_before,
        "re-rendering the same bytes moves no reference, so `ref_count` must not inflate"
    );
}

/// A `scan_directory` job row belonging to `batch`, shaped exactly as the api route's
/// `enqueue` writes one: the payload is empty, and the batch is the row's own column.
fn scan_job(batch: BatchId, library: LibraryId) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: batch,
        library_id: library,
        kind: JobPayload::SCAN_DIRECTORY.to_owned(),
        payload: JobPayload::ScanDirectory.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// The `payload->>'path'` of every `ingest_file` job in one batch, in enqueue order.
async fn ingest_paths_in(pool: &PgPool, batch: BatchId) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT payload ->> 'path' FROM job \
         WHERE batch_id = $1 AND kind = 'ingest_file' ORDER BY created_at, id",
    )
    .bind(batch.as_uuid())
    .fetch_all(pool)
    .await
    .expect("reads the queued paths")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_job_enqueues_its_files_into_its_own_batch_and_grows_the_total(pool: PgPool) {
    // The reason `enqueue_into` exists. The api route enqueues the scan job into a fresh
    // batch and hands the browser that id; if the walk minted a second batch for the
    // files, the browser would poll a batch of one, watch it settle, and report a
    // finished scan while every file was still queued.
    //
    // Four files, each doing a different job. The two STLs and the 3MF are candidates;
    // the README is not and is counted nowhere. Nothing here is parsed — the walk decides
    // by extension, which is why arbitrary bytes under a mesh extension are fine.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    std::fs::write(
        ingest_dir.path().join("notes.stl"),
        b"LP-1042-03 revision notes: chamfer the mounting face.\n",
    )
    .expect("stages a file that is not an STL by any reading");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    std::fs::write(
        ingest_dir.path().join("README.md"),
        b"Brackets for the LP-1042 mounting series. Not a part.\n",
    )
    .expect("stages a non-candidate");

    let jobs = lapidary_db::PgJobs(pool.clone());
    let (batch, queued) = jobs
        .enqueue(seeded(), &[JobPayload::ScanDirectory])
        .await
        .expect("enqueues the scan itself, as the api route does");
    assert_eq!(queued, 1);
    let before = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads the batch")
        .expect("a batch with one job has a status");
    assert_eq!(before.total, 1, "the scan job alone, before it has run");

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler
            .handle(&scan_job(batch, seeded()))
            .await
            .expect("walks the directory"),
        Outcome::Scanned,
        "a scan ingests nothing, skips nothing and renders nothing, and says so"
    );

    // The growth assertion first, because it is the one the whole shape exists for: the
    // walk enqueued into the batch it was given, so the batch the browser is polling now
    // reports four jobs where it reported one.
    let after = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads the batch")
        .expect("the batch still has jobs");
    assert_eq!(
        after.total, 4,
        "batch_status counts rows by batch_id with no stored total, so a batch that \
         grows after the browser started polling it reports the grown number"
    );
    assert_eq!(after.batch_id, batch, "and it is still the same batch");
    assert_eq!(
        ingest_paths_in(&pool, batch).await,
        vec![
            BRACKET.to_owned(),
            "notes.stl".to_owned(),
            CARRIER.to_owned(),
        ],
        "one ingest_file per candidate, sorted, all in the batch the job was given"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unreadable_ingest_directory_fails_the_scan_job_and_names_the_mount(pool: PgPool) {
    // A path that was never created — the real-world case is a missing or unreadable
    // `/ingest` mount. It must not succeed with nothing queued, which is
    // indistinguishable from an empty directory that scanned perfectly well, and it must
    // be Permanent: `batch_status` reports `failures` for `state = 'failed'` rows only,
    // so a Transient classification would keep the operator's own diagnosis off the
    // screen until `max_attempts` ran out. See `src/scan.rs`'s module doc — the whole
    // reversal rests on this message reaching the browser on the first poll.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let missing = ingest_dir.path().join("does-not-exist");
    let blob_root = tempfile::tempdir().expect("temp dir");

    let handler = handler_over(&pool, &missing, blob_root.path());
    let error = handler
        .handle(&scan_job(BatchId::new(), seeded()))
        .await
        .expect_err("an unwalkable mount is a failure, not an empty scan");

    match error {
        HandlerError::Permanent { message } => {
            assert!(
                message.contains("does-not-exist"),
                "the message must name the directory it could not read: {message}"
            );
            assert!(
                message.contains("Check that the mount"),
                "and say what to check (CLAUDE.md): {message}"
            );
        }
        other => panic!("a bad mount must be reported at once, not retried, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// Slice 6a: the walk descends. See
// docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md §1.
// ---------------------------------------------------------------------------------------

/// Stage a file at a relative path, creating the folders above it.
fn stage(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("stages the folders above the file");
    }
    std::fs::write(&path, bytes).expect("stages the file");
}

/// Run a scan job over `ingest_dir` and return the relative paths it enqueued.
async fn scanned_paths(pool: &PgPool, ingest_dir: &Path, blob_root: &Path) -> Vec<String> {
    let jobs = lapidary_db::PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue(seeded(), &[JobPayload::ScanDirectory])
        .await
        .expect("enqueues the scan");
    let handler = handler_over(pool, ingest_dir, blob_root);
    handler
        .handle(&scan_job(batch, seeded()))
        .await
        .expect("walks the directory");
    ingest_paths_in(pool, batch).await
}

/// The slice's reason to exist. Before this, a nested corpus scanned as "0 files" with no
/// error anywhere, and the operator had to flatten their library to use Lapidary at all.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_descends_and_reports_paths_relative_to_the_mount(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    stage(
        ingest_dir.path(),
        &format!("mounting/{BRACKET}"),
        BRACKET_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        &format!("drives/planetary/{CARRIER}"),
        CARRIER_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        "drives/README.md",
        b"Planetary drive assemblies for the LP-3480 series. Not a part.\n",
    );

    assert_eq!(
        scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await,
        vec![
            BRACKET.to_owned(),
            format!("drives/planetary/{CARRIER}"),
            format!("mounting/{BRACKET}"),
        ],
        "every candidate at every depth, as a `/`-separated path relative to the mount, \
         sorted by the whole path so a folder's files stay together — and the README is \
         still counted nowhere"
    );
}

/// A symlinked directory is the only way a real filesystem can present a cycle, and
/// `DirEntry::file_type` not traversing it is what makes the cycle unreachable. A test,
/// because that guarantee is a property of the API rather than of anything visible in the
/// walk's own code.
#[cfg(unix)]
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_does_not_follow_a_symlinked_directory(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    stage(
        ingest_dir.path(),
        &format!("mounting/{BRACKET}"),
        BRACKET_FIXTURE,
    );
    // The loop: a folder inside `mounting` pointing back at the mount root. Following it
    // would recurse until the depth cap, reporting the same file many times over.
    std::os::unix::fs::symlink(ingest_dir.path(), ingest_dir.path().join("mounting/loop"))
        .expect("stages a symlink back to the root");

    assert_eq!(
        scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await,
        vec![format!("mounting/{BRACKET}")],
        "the file once, by the path it really has — the symlink is not descended, so the \
         cycle it would have created never exists"
    );
}

/// A `.git` inside someone's parts folder is not part of their library, and walking one on
/// a large corpus is pure waste. Nor is the rest of what `docs/DATA.md` §6.2's ignore list names, which
/// `lapidary watch` skips too.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_skips_what_the_ignore_list_names(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    // A dot-directory holding a real candidate, and a dot-file that is one. Both are
    // skipped: an STL inside `.git/objects` is a coincidence, not a part.
    stage(
        ingest_dir.path(),
        &format!(".git/objects/{BRACKET}"),
        BRACKET_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        ".hidden-draft.stl",
        b"An export the operator did not mean to publish.\n",
    );
    // An office lock file beside a model, and an editor's backup folder holding one.
    stage(ingest_dir.path(), &format!("~${BRACKET}"), BRACKET_FIXTURE);
    stage(
        ingest_dir.path(),
        &format!("drafts.bak/{BRACKET}"),
        BRACKET_FIXTURE,
    );

    assert_eq!(
        scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await,
        vec![BRACKET.to_owned()],
        "dot-directories, dot-files and the rest of the ignore list are skipped, at any depth"
    );
}

/// The cap bounds pathological nesting; it must not turn a deep tree into a failed scan,
/// because the files above the cap are still real work.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_stops_at_the_depth_cap_and_keeps_what_it_found_above_it(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    // Shallow enough to be found, at depth 3.
    stage(
        ingest_dir.path(),
        &format!("a/b/c/{BRACKET}"),
        BRACKET_FIXTURE,
    );
    // Past MAX_DEPTH = 16.
    let deep = (1..=20)
        .map(|n| format!("d{n}"))
        .collect::<Vec<_>>()
        .join("/");
    stage(
        ingest_dir.path(),
        &format!("{deep}/{BRACKET}"),
        BRACKET_FIXTURE,
    );

    assert_eq!(
        scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await,
        vec![format!("a/b/c/{BRACKET}")],
        "the shallow file is found and the scan succeeds; only the descent past the cap \
         stops, and it is logged rather than failing the job"
    );
}

/// The regression the source-path column exists to prevent, end to end through the real
/// handler rather than through the constraint alone.
///
/// Two folders, one filename, identical bytes. Before slice 6a these were one part name,
/// the second insert raised a unique violation, `classify_write` mapped it to `Skipped`,
/// and the file was reported as already here and never indexed.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn one_filename_in_two_folders_is_two_parts_sharing_one_blob(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    stage(
        ingest_dir.path(),
        &format!("mounting/{BRACKET}"),
        BRACKET_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        &format!("spares/{BRACKET}"),
        BRACKET_FIXTURE,
    );

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler
            .handle(&job_for(&format!("mounting/{BRACKET}")))
            .await
            .expect("the first ingests"),
        Outcome::Ingested
    );
    assert_eq!(
        handler
            .handle(&job_for(&format!("spares/{BRACKET}")))
            .await
            .expect("the second must ingest too"),
        Outcome::Ingested,
        "the same filename in a different folder is a second part, not a re-scan — this \
         returning Skipped is the silent data loss the slice exists to close"
    );

    let (parts, blobs, refs): (i64, i64, i32) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM part WHERE library_id = $1), \
                (SELECT count(DISTINCT f.blake3) FROM file f \
                   JOIN revision r ON r.id = f.revision_id \
                   JOIN part p ON p.id = r.part_id \
                  WHERE p.library_id = $1 AND f.role = 'source'), \
                (SELECT max(ref_count) FROM blob)",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");

    assert_eq!(parts, 2, "two parts");
    assert_eq!(blobs, 1, "one set of source bytes between them");
    assert!(
        refs >= 2,
        "and the blob is referenced by both, not written twice (ref_count {refs})"
    );

    let names: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, source_path FROM part WHERE library_id = $1 ORDER BY source_path",
    )
    .bind(seeded().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("reads the parts");
    assert_eq!(
        names,
        vec![
            (
                "bracket-lp-1042-03".to_owned(),
                format!("mounting/{BRACKET}")
            ),
            ("bracket-lp-1042-03".to_owned(), format!("spares/{BRACKET}")),
        ],
        "one name, two paths — the name is a label and the path is the identity"
    );
}

/// `Path::join` resolves nothing and refuses nothing, so a payload path is a filesystem
/// reach out of the mount unless something stops it. Nothing produces such a payload
/// today; slice 6a's upload route will, and the guard belongs on the door.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_path_that_escapes_the_ingest_directory_is_refused_permanently(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    for escape in [
        "../bracket-lp-1042-03.stl",
        "a/../../b.stl",
        "/etc/passwd",
        "",
    ] {
        match handler.handle(&job_for(escape)).await {
            Err(HandlerError::Permanent { message }) => {
                assert!(
                    message.contains("outside the directory it belongs to"),
                    "the message must say what was wrong with {escape:?}: {message}"
                );
            }
            other => panic!(
                "{escape:?} must be refused permanently — a payload holds the same bytes \
                 on every attempt — got {other:?}"
            ),
        }
    }
}

/// Slice 7, and the first thing anyone will try: delete a part, leave the file on disk,
/// scan again.
///
/// Nothing may come back. A scan that resurrects a deleted part makes delete useless — the
/// next scan undoes every removal a person made — and un-deleting implicitly is the same
/// class of surprise as deleting implicitly. Restore is a button, not a side effect.
///
/// The two branches reach that answer by different routes and both are asserted, because
/// they are separately breakable:
///
/// - unchanged bytes take the hash short-circuit, which works only because
///   `PgBlobs::library_holds` deliberately does not filter `deleted_at`;
/// - changed bytes get past it, and `PgRevisions::current` finds the part deleted and settles
///   as `Skipped` before the kernel runs. Until Phase 4 slice 1 they reached
///   `part_source_path_unique_per_library` instead, which `classify_write` maps to `Skipped`.
///
/// Neither behaviour is new in slice 7 — both fall out of what slice 6a built — and that
/// is exactly why they are pinned here. Nothing else fails if either one silently stops
/// holding.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn re_scanning_a_deleted_part_does_not_bring_it_back(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let part = sqlx::query_scalar::<_, Uuid>("SELECT id FROM part")
        .fetch_one(&pool)
        .await
        .expect("the one part");
    assert!(
        PgParts(pool.clone())
            .soft_delete(lapidary_core::PartId::from_uuid(part))
            .await
            .expect("soft delete"),
    );

    // Branch one: the file on disk is untouched, so the hash matches and `library_holds`
    // answers yes for a path whose part is deleted.
    assert_eq!(
        handler
            .handle(&job_for(BRACKET))
            .await
            .expect("the re-scan succeeds"),
        Outcome::Skipped,
        "unchanged bytes must short-circuit, not re-ingest"
    );
    assert!(
        still_deleted(&pool, part).await,
        "the hash short-circuit must not clear deleted_at"
    );

    // Branch two: the file changed on disk, so the hash no longer matches and the part at
    // this path is looked up. `spacer` rather than a truncation of the bracket, so this is
    // a real mesh the pipeline gets all the way through — a file that fails to parse would
    // reach `Skipped` for the wrong reason entirely.
    stage(
        ingest_dir.path(),
        BRACKET,
        include_bytes!("../../../fixtures/spacer-lp-2001-00.stl"),
    );
    assert_eq!(
        handler
            .handle(&job_for(BRACKET))
            .await
            .expect("the re-scan succeeds"),
        Outcome::Skipped,
        "changed bytes at a deleted part's path must settle as Skipped"
    );
    assert!(
        still_deleted(&pool, part).await,
        "the changed-bytes path must not clear deleted_at either"
    );

    assert_eq!(
        part_count(&pool).await,
        1,
        "and neither branch may insert a second part at the same path"
    );

    // Branch two is the one that could leak, and did: it reached the constraint only after
    // writing the changed file's rung, and a `Skipped` is not reaped, so `blobs/` held two
    // files — this count said 2 and called that "no orphan". It now settles before the
    // kernel runs. The source lives in the model directory, so `blobs/` holds derivatives
    // only: the original part's one rung, and nothing else.
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("blob count");
    assert_eq!(
        blobs, 2,
        "the first part's source and its one rung, and no more"
    );
    assert_eq!(
        all_files(&blob_root.path().join("blobs")).len(),
        1,
        "a re-scan of a changed file at a deleted path must not orphan its bytes on disk"
    );
}

async fn still_deleted(pool: &PgPool, part: Uuid) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM part WHERE id = $1 AND deleted_at IS NOT NULL")
        .bind(part)
        .fetch_optional(pool)
        .await
        .expect("deleted_at reads")
        .is_some()
}
// ---------------------------------------------------------------------------------------
// The model directory: one folder per model, holding its file and its metadata.
// ---------------------------------------------------------------------------------------

/// The seeded library is named `Default`, and its directory is that slugged and
/// lowercased. Every path below hangs off this, exactly as the layout in spec §1 does.
const LIBRARY_DIR: &str = "libraries/default";

/// The manifest beside a model's file, parsed back into the type that wrote it. Parsing
/// rather than reading strings out of the JSON is the point: `metadata.json` is what
/// re-adoption rebuilds rows from, so what matters is that it round-trips into a
/// `ModelManifest`, not that it contains some expected substrings.
fn manifest_in(dir: &Path) -> ModelManifest {
    let bytes = std::fs::read(dir.join("metadata.json"))
        .unwrap_or_else(|e| panic!("no metadata.json in {}: {e}", dir.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("metadata.json in {} does not parse: {e}", dir.display()))
}

/// A custom field's value lives on the part's row, and `metadata.json` mirrors the rows: `describe_part`
/// writes it again, so the model's directory holds the value re-adoption would otherwise lose.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn describing_a_part_writes_its_custom_values_into_metadata_json(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let part: Uuid = sqlx::query_scalar("SELECT id FROM part")
        .fetch_one(&pool)
        .await
        .expect("the part");
    sqlx::query(
        "UPDATE part SET metadata_json = jsonb_set(metadata_json, '{custom}', '{\"supplier\": \"Misumi\"}')",
    )
    .execute(&pool)
    .await
    .expect("a value set");
    let dir = blob_root
        .path()
        .join(LIBRARY_DIR)
        .join("bracket-lp-1042-03");
    assert_eq!(
        manifest_in(&dir).part.metadata.get("custom"),
        None,
        "not yet"
    );

    let payload = JobPayload::DescribePart {
        part: lapidary_core::PartId::from_uuid(part),
    };
    let job = JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: seeded(),
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    };
    assert_eq!(
        handler.handle(&job).await.expect("describes"),
        Outcome::Described
    );
    assert_eq!(
        manifest_in(&dir).part.metadata["custom"],
        serde_json::json!({ "supplier": "Misumi" })
    );

    let elsewhere = JobRow {
        library_id: LibraryId::from_uuid(Uuid::now_v7()),
        ..job
    };
    assert!(
        matches!(
            handler.handle(&elsewhere).await,
            Err(HandlerError::Permanent { .. })
        ),
        "another library's job describes nothing of this one's"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn ingesting_a_nested_file_writes_a_model_directory(pool: PgPool) {
    // The shape the owner asked for: one directory per model, holding its file under its
    // own name and its metadata beside it, reachable by opening the storage folder in a
    // file manager.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.stl",
        BRACKET_FIXTURE,
    );
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler
            .handle(&job_for("Terrain/Rocks/cliff.stl"))
            .await
            .expect("ingests"),
        Outcome::Ingested
    );

    let dir = blob_root
        .path()
        .join(LIBRARY_DIR)
        .join("Terrain/Rocks/cliff");
    assert_eq!(
        std::fs::read(dir.join("cliff.stl")).expect("the source sits under its own name"),
        BRACKET_FIXTURE,
        "byte-identical: nothing converts, compresses or renames what was ingested"
    );

    let manifest = manifest_in(&dir);
    assert_eq!(manifest.schema, ModelManifest::SCHEMA);
    assert_eq!(
        manifest.part.source_path, "Terrain/Rocks/cliff.stl",
        "the ingest identity key, not the storage path — spec §3 keeps the two distinct"
    );
    assert_eq!(manifest.part.name, "cliff");
    assert_eq!(manifest.revisions[0].files[0].file_name, "cliff.stl");
    assert_eq!(manifest.revisions[0].files[0].format, "stl");
    assert_eq!(
        manifest.revisions[0].triangle_count,
        Some(20),
        "the fixture's real triangle count, so the manifest carries measurements and not \
         a shell"
    );
    assert_eq!(manifest.revisions[0].units.as_deref(), Some("mm"));

    // The ids in the manifest are the ids in the database. This is the whole of
    // re-adoption: delete the rows and the directory still says which part it was.
    let (part_id, revision_id): (Uuid, Uuid) =
        sqlx::query_as("SELECT p.id, r.id FROM part p JOIN revision r ON r.part_id = p.id")
            .fetch_one(&pool)
            .await
            .expect("the part and its revision");
    assert_eq!(manifest.part.id.as_uuid(), part_id);
    assert_eq!(manifest.revisions[0].id.as_uuid(), revision_id);

    // And the category tree mirrors the directories the file sat in.
    let (folder, parent): (String, String) = sqlx::query_as(
        "SELECT child.name, parent.name FROM part p \
         JOIN folder child ON child.id = p.folder_id \
         JOIN folder parent ON parent.id = child.parent_id",
    )
    .fetch_one(&pool)
    .await
    .expect("the part sits in a folder that has a parent");
    assert_eq!((folder.as_str(), parent.as_str()), ("Rocks", "Terrain"));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_recorded_storage_path_is_where_the_bytes_actually_are(pool: PgPool) {
    // `file.storage_path` is what every later reader resolves bytes through — the download
    // route, the derive job, the move. A path that does not name the file on disk is a
    // grid full of parts nobody can open, and it looks exactly like a working ingest from
    // the database side, so the assertion has to cross to the filesystem.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.stl",
        BRACKET_FIXTURE,
    );
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler
        .handle(&job_for("Terrain/Rocks/cliff.stl"))
        .await
        .expect("ingests");

    let storage_path: Option<String> =
        sqlx::query_scalar("SELECT storage_path FROM file WHERE role = 'source'")
            .fetch_one(&pool)
            .await
            .expect("reads");
    let storage_path = storage_path.expect("an ingested file records where it was written");
    assert_eq!(
        storage_path,
        format!("{LIBRARY_DIR}/Terrain/Rocks/cliff/cliff.stl")
    );
    assert_eq!(
        std::fs::read(blob_root.path().join(&storage_path))
            .expect("the recorded path names a real file"),
        BRACKET_FIXTURE
    );

    // And nothing was left at the content-addressed path: source dedup is gone, and a
    // second copy under `blobs/` would be 23 GB of it on the owner's corpus.
    let hash: String = sqlx::query_scalar("SELECT blake3 FROM file WHERE role = 'source'")
        .fetch_one(&pool)
        .await
        .expect("reads");
    assert!(
        !blob_root
            .path()
            .join(format!("blobs/{}/{}/{hash}", &hash[0..2], &hash[2..4]))
            .exists(),
        "the source must not also be written to the content-addressed path"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_models_with_one_name_get_two_directories(pool: PgPool) {
    // 6a decided two parts called `cliff` are the truth. Two directories called `cliff/`
    // are impossible, so the second gets a deterministic suffix -- but only when they
    // land in the same category. In different ones there is no collision to resolve, and
    // suffixing anyway would put a hash in a name a person reads for no reason at all.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), "Terrain/cliff.stl", BRACKET_FIXTURE);
    stage(ingest_dir.path(), "Bases/cliff.stl", GEAR_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    handler
        .handle(&job_for("Terrain/cliff.stl"))
        .await
        .expect("a");
    handler
        .handle(&job_for("Bases/cliff.stl"))
        .await
        .expect("b");

    let terrain = blob_root.path().join(LIBRARY_DIR).join("Terrain/cliff");
    let bases = blob_root.path().join(LIBRARY_DIR).join("Bases/cliff");
    assert_eq!(
        std::fs::read(terrain.join("cliff.stl")).expect("the terrain cliff"),
        BRACKET_FIXTURE
    );
    assert_eq!(
        std::fs::read(bases.join("cliff.stl")).expect("the base cliff"),
        GEAR_FIXTURE,
        "each directory holds its own model's bytes, not the other's"
    );
    assert_eq!(manifest_in(&terrain).part.source_path, "Terrain/cliff.stl");
    assert_eq!(manifest_in(&bases).part.source_path, "Bases/cliff.stl");

    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM part ORDER BY source_path")
        .fetch_all(&pool)
        .await
        .expect("reads");
    assert_eq!(
        names,
        vec!["cliff", "cliff"],
        "one name, two parts — 6a's decision stands"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_model_of_the_same_name_in_one_category_takes_a_suffix(pool: PgPool) {
    // The collision that cannot be talked out of: one category, two models called `cliff`
    // -- the same model exported twice, which is ordinary in a parts library. The second
    // takes `_` plus six hex of its own hash (spec §2), and both directories are real,
    // each describing itself.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.stl",
        BRACKET_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.3mf",
        CARRIER_FIXTURE,
    );
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    handler
        .handle(&job_for("Terrain/Rocks/cliff.stl"))
        .await
        .expect("the stl");
    handler
        .handle(&job_for("Terrain/Rocks/cliff.3mf"))
        .await
        .expect("the 3mf");

    let category = blob_root.path().join(LIBRARY_DIR).join("Terrain/Rocks");
    let mut dirs: Vec<String> = std::fs::read_dir(&category)
        .expect("the category directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    assert_eq!(dirs.len(), 2, "two models, two directories: {dirs:?}");
    assert_eq!(dirs[0], "cliff", "the first keeps the plain name");

    let suffixed = &dirs[1];
    let hash: String = sqlx::query_scalar(
        "SELECT f.blake3 FROM file f JOIN revision r ON r.id = f.revision_id \
         JOIN part p ON p.id = r.part_id WHERE p.source_path = 'Terrain/Rocks/cliff.3mf'",
    )
    .fetch_one(&pool)
    .await
    .expect("reads");
    assert_eq!(
        *suffixed,
        format!("cliff_{}", &hash[..6]),
        "the suffix is six hex of the source hash, so a re-ingest lands on the same name"
    );

    // Both describe themselves, and each names its own file.
    assert_eq!(
        manifest_in(&category.join("cliff")).revisions[0].files[0].file_name,
        "cliff.stl"
    );
    let second = manifest_in(&category.join(suffixed));
    assert_eq!(second.revisions[0].files[0].file_name, "cliff.3mf");
    assert_eq!(second.part.source_path, "Terrain/Rocks/cliff.3mf");
    assert_eq!(
        second.part.name, "cliff",
        "the directory name is cosmetic; the part is still called what it is called"
    );
    assert!(
        category.join(suffixed).join("cliff.3mf").exists(),
        "the file itself sits in the suffixed directory"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_rescan_after_a_move_creates_no_folder_and_no_directory(pool: PgPool) {
    // Spec §6: categories are created only for files that actually ingest, which is why
    // `model_dir_for` runs after the `library_holds` short-circuit and not during the
    // walk. Deleting the category rows and the directory stands in for the move task 9
    // will perform: a re-scan must not put them back.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.stl",
        BRACKET_FIXTURE,
    );
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler
        .handle(&job_for("Terrain/Rocks/cliff.stl"))
        .await
        .expect("ingests");

    std::fs::remove_dir_all(blob_root.path().join(LIBRARY_DIR).join("Terrain"))
        .expect("the model moves away");

    assert_eq!(
        handler
            .handle(&job_for("Terrain/Rocks/cliff.stl"))
            .await
            .expect("re-scans"),
        Outcome::Skipped,
        "same library, same path, same bytes — the short-circuit settles it"
    );
    assert!(
        !blob_root.path().join(LIBRARY_DIR).join("Terrain").exists(),
        "a re-scan must not re-create a directory the user emptied"
    );
}

/// A binary STL of one degenerate triangle: three vertices at the same point.
///
/// Well formed — header, count and record are all valid — and with no extent, so no picture
/// can be made of it. This is the shape of the one file in 1,095 that failed the measured
/// exit-criterion run against the owner's own corpus. Built rather than committed, so it can
/// be read.
fn zero_extent_stl() -> Vec<u8> {
    let mut bytes = vec![0u8; 80];
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for _ in 0..12 {
        bytes.extend_from_slice(&0f32.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

/// **The Phase 1 exit criterion's fourth clause, at the level that decides it.**
///
/// *"every part appears — with a thumbnail where the library renders them automatically, and
/// with 'No preview yet' … where it does not"*. It did not: a mesh no picture could be made
/// of failed the ingest job, so no part row was written and the model was absent from the
/// library entirely. Measured, on a real corpus, at one file in 1,095.
///
/// A derivative is not the part. The mesh parses, the measurements are real, and losing the
/// model because its preview could not be drawn is the wrong way round.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_model_no_picture_can_be_made_of_is_still_ingested(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join("degenerate.stl"), zero_extent_stl())
        .expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler
            .handle(&job_for("degenerate.stl"))
            .await
            .expect("a file that parses is ingested, whatever can be drawn of it"),
        Outcome::Ingested,
        "not Failed: the criterion asks for the part to appear"
    );
    assert_eq!(
        parts_in(&pool, seeded()).await,
        1,
        "and it is in the library, which is the whole of what was wrong"
    );

    // No thumbnail row, which is exactly the state the grid renders as "No preview yet" —
    // the same state a library with `auto_thumbnail = false` produces, and which
    // `POST /api/libraries/{id}/thumbnails` exists to have another go at.
    let thumbnails: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative d JOIN revision r ON r.id = d.revision_id \
         JOIN part p ON p.id = r.part_id \
         WHERE p.library_id = $1 AND d.kind = 'thumbnail'",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(thumbnails, 0, "no preview yet, rather than no part");
}

/// The other half, so the change is not "nothing is fatal any more": a file that does not
/// parse still fails, because a mesh nobody can read has no measurements and no part to
/// hang them on.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_that_does_not_parse_still_fails_and_creates_no_part(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("notes.stl"),
        b"LP-1042-03 revision notes: chamfer the mounting face.\n",
    )
    .expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert!(
        handler.handle(&job_for("notes.stl")).await.is_err(),
        "an unreadable file is still an error"
    );
    assert_eq!(parts_in(&pool, seeded()).await, 0);
}

// ---- STEP and IGES -------------------------------------------------------------------

/// A CAD kernel with no OCCT behind it: it measures the bytes as an STL and reports every
/// figure as read off a B-rep, which is what `OcctKernel` does for a solid. Enough to prove
/// a STEP file reaches the CAD kernel and its provenance reaches the rows, without OCCT in
/// every test run; `cargo xtask verify occt` drives the real one.
struct FakeCad;

#[async_trait::async_trait]
impl Kernel for FakeCad {
    fn version(&self, _params: &KernelParams) -> KernelVersion {
        KernelVersion {
            implementation: "occt".to_owned(),
            version: "test".to_owned(),
        }
    }

    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        let as_mesh = KernelParams {
            format: "stl".to_owned(),
            ..params.clone()
        };
        let mut output = MeshKernel.process(bytes, &as_mesh).await?;
        output.provenance = MeasurementProvenance::ANALYTIC;
        output.structure = Some(fake_tree());
        output.metadata = Some(CadMetadata {
            file_name: Some("fixture-plate-lp-9000-00.step".to_owned()),
            authors: vec!["J. Okafor".to_owned()],
            organizations: vec!["Lapidary fixtures".to_owned()],
            originating_system: Some("SOLIDWORKS 2025".to_owned()),
            schemas: vec!["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF".to_owned()],
            materials: vec!["AISI 1045 steel".to_owned()],
            ..CadMetadata::default()
        });
        output.entities = vec![Entity::Cylinder {
            prototype: "0:1:1:1".to_owned(),
            face: 1,
            radius: 11.0,
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
        }];
        output.pmi = Some(fake_pmi());
        output.topology = Some(lapidary_core::Topology {
            faces: 38,
            edges: 96,
        });
        Ok(output)
    }
}

/// The one dimension `FakeCad` reports the file specifying: the cylinder's diameter.
fn fake_pmi() -> lapidary_core::Pmi {
    lapidary_core::Pmi {
        dimensions: vec![lapidary_core::PmiDimension {
            kind: "diameter".to_owned(),
            value: 22.0,
            upper: Some(0.05),
            lower: Some(0.0),
            faces: vec![lapidary_core::PmiFace {
                prototype: "0:1:1:1".to_owned(),
                face: Some(1),
            }],
        }],
        tolerances: Vec::new(),
        datums: Vec::new(),
    }
}

/// The one-part tree `FakeCad` reports, as the bridge writes one for a single solid.
fn fake_tree() -> AssemblyTree {
    AssemblyTree {
        roots: vec![AssemblyNode {
            name: "fixture-plate-lp-9000-00".to_owned(),
            prototype: "0:1:1:1".to_owned(),
            transform: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            children: Vec::new(),
        }],
        parts: 1,
        prototypes: 1,
    }
}

const FIXTURE_PLATE: &str = "fixture-plate-lp-9000-00.step";

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_step_files_exact_figures_are_stored_as_exact(pool: PgPool) {
    // Measurement must not lie in either direction. Before this, ingest wrote
    // 'tessellated' beside every figure whatever the kernel said, so an exact CAD volume
    // would have been labelled approximate forever.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };

    let outcome = handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("a STEP file ingests through the CAD kernel");
    assert_eq!(outcome, Outcome::Ingested);

    type Sources = (Option<String>, Option<String>, Option<String>, String);
    let (volume, area, bbox, storage_path): Sources = sqlx::query_as(
        "SELECT r.volume_source, r.surface_area_source, r.bbox_source, f.storage_path \
         FROM revision r JOIN file f ON f.revision_id = r.id AND f.role = 'source'",
    )
    .fetch_one(&pool)
    .await
    .expect("one revision with its source file");
    assert_eq!(
        (volume.as_deref(), area.as_deref(), bbox.as_deref()),
        (Some("analytic"), Some("analytic"), Some("analytic")),
        "each figure keeps the provenance the kernel reported"
    );

    let model_dir = blob_root.path().join(
        Path::new(&storage_path)
            .parent()
            .expect("a model directory"),
    );
    assert_eq!(
        manifest_in(&model_dir).revisions[0]
            .volume_source
            .as_deref(),
        Some("analytic"),
        "and metadata.json, which re-adoption rebuilds the rows from, says the same"
    );
}

/// The faces and edges a CAD kernel counts reach the revision a file's ingest committed, and a mesh's
/// revision has neither.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_step_files_faces_and_edges_are_recorded_on_its_revision(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };
    handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("the STEP file ingests");
    handler
        .handle(&job_for(BRACKET))
        .await
        .expect("the STL ingests");

    let counts: Vec<(String, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT p.name, r.face_count, r.edge_count FROM revision r \
         JOIN part p ON p.id = r.part_id ORDER BY p.name",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(
        counts,
        [
            ("bracket-lp-1042-03".to_owned(), None, None),
            ("fixture-plate-lp-9000-00".to_owned(), Some(38), Some(96)),
        ]
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_step_file_on_a_worker_without_a_cad_kernel_fails_and_says_why(pool: PgPool) {
    // Not the mesh parser's "no parser for step": the file is fine, the build is not, and
    // the message has to say which build reads it.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    match handler.handle(&job_for(FIXTURE_PLATE)).await {
        Err(HandlerError::Permanent { message }) => assert!(
            message.contains("STEP") && message.contains("occt-kernel"),
            "names the format and the build that reads it: {message}"
        ),
        other => panic!("a worker that cannot read STEP must say so, got {other:?}"),
    }
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(parts, 0, "and adds no part");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_step_files_tree_and_entities_are_stored_beside_its_rungs(pool: PgPool) {
    // The kernel reads both on every conversion. Dropped after measuring, as they were, the
    // detail page has no tree to show and Phase 3 has no faces to snap a measurement to.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };
    handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("the STEP file ingests");
    handler
        .handle(&job_for(BRACKET))
        .await
        .expect("the STL ingests");

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT p.name, d.kind, d.blake3 FROM derivative d \
         JOIN revision r ON r.id = d.revision_id JOIN part p ON p.id = r.part_id \
         WHERE d.kind IN ('structure', 'entities', 'pmi') ORDER BY d.kind",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    let kinds: Vec<(&str, &str)> = rows
        .iter()
        .map(|(name, kind, _)| (name.as_str(), kind.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("fixture-plate-lp-9000-00", "entities"),
            ("fixture-plate-lp-9000-00", "pmi"),
            ("fixture-plate-lp-9000-00", "structure"),
        ],
        "the STEP part has all three, and the mesh none"
    );

    let store = lapidary_storage::DerivativeStore::open(blob_root.path());
    let read = |hex: &str| {
        store
            .get(&BlobHash::parse_hex(hex).expect("a hash"))
            .expect("the bytes are in the store")
    };
    let pmi: lapidary_core::Pmi =
        serde_json::from_slice(&read(&rows[1].2)).expect("the PMI parses");
    assert_eq!(
        pmi,
        fake_pmi(),
        "what the file specified is stored as it was read"
    );
    let tree: AssemblyTree = serde_json::from_slice(&read(&rows[2].2)).expect("the tree parses");
    assert_eq!(tree, fake_tree());
    let entities: serde_json::Value =
        serde_json::from_slice(&read(&rows[0].2)).expect("the entities parse");
    assert_eq!(entities[0]["type"], "cylinder");
    assert_eq!(entities[0]["radius"], 11.0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_step_files_header_is_stored_on_its_part_and_in_its_manifest(pool: PgPool) {
    // Stage 4, semantic: what the file says about itself. Kept on the part row, where a
    // later filter can reach it, and in metadata.json, which re-adoption rebuilds rows from.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };
    handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("the STEP file ingests");

    let (metadata, storage_path): (serde_json::Value, String) = sqlx::query_as(
        "SELECT p.metadata_json, f.storage_path FROM part p \
         JOIN revision r ON r.part_id = p.id JOIN file f ON f.revision_id = r.id",
    )
    .fetch_one(&pool)
    .await
    .expect("the part");
    assert_eq!(metadata["cad"]["originating_system"], "SOLIDWORKS 2025");
    assert_eq!(metadata["cad"]["materials"][0], "AISI 1045 steel");
    let materials: Vec<String> = sqlx::query_scalar("SELECT materials FROM part")
        .fetch_one(&pool)
        .await
        .expect("the materials column");
    assert_eq!(
        materials,
        ["AISI 1045 steel"],
        "written to the typed column the facet reads, not only into the JSON"
    );

    let model_dir = blob_root.path().join(
        Path::new(&storage_path)
            .parent()
            .expect("a model directory"),
    );
    assert_eq!(
        manifest_in(&model_dir).part.metadata["cad"]["authors"][0],
        "J. Okafor"
    );
}

/// `FakeCad`, with an author Postgres will not store: `jsonb` refuses a NUL character.
struct NulHeaderCad;

#[async_trait::async_trait]
impl Kernel for NulHeaderCad {
    fn version(&self, params: &KernelParams) -> KernelVersion {
        FakeCad.version(params)
    }

    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        let mut output = FakeCad.process(bytes, params).await?;
        if let Some(metadata) = output.metadata.as_mut() {
            metadata.authors = vec!["J. Okafor\0".to_owned()];
        }
        Ok(output)
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_header_the_database_refuses_still_leaves_the_part_ingested(pool: PgPool) {
    // Stages commit independently (`docs/DATA.md` §3.1): the part is measured and searchable
    // before its header is written, so losing the header must not lose the part.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(NulHeaderCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };

    let outcome = handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("a refused header does not fail the file");
    assert_eq!(outcome, Outcome::Ingested);
    let metadata: serde_json::Value = sqlx::query_scalar("SELECT metadata_json FROM part")
        .fetch_one(&pool)
        .await
        .expect("the part is there");
    assert_eq!(metadata, serde_json::json!({}), "and carries no header");
}

/// A rung an older pipeline wrote is queued when the worker starts and rebuilt at the version
/// this worker's kernel reports, and nothing current is queued beside it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_rung_an_older_kernel_wrote_is_rebuilt_at_the_current_version(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    let (_, current) = l0_row(&pool).await;

    handler.enqueue_stale_derivatives().await;
    let jobs = lapidary_db::PgJobs(pool.clone());
    assert!(
        jobs.dequeue("worker-a", std::time::Duration::from_secs(60))
            .await
            .expect("dequeues")
            .is_none(),
        "a rung the current kernel wrote is not stale"
    );

    sqlx::query(
        "UPDATE derivative SET kernel_version = 'mesh stl-1+glb-1+cpu-1' \
         WHERE kind = 'tessellation_l0'",
    )
    .execute(&pool)
    .await
    .expect("ages the rung");
    handler.enqueue_stale_derivatives().await;
    let job = jobs
        .dequeue("worker-a", std::time::Duration::from_secs(60))
        .await
        .expect("dequeues")
        .expect("the old rung is queued");
    assert_eq!(
        handler.handle(&job).await.expect("rebuilds"),
        Outcome::Rendered
    );
    assert_eq!(
        l0_row(&pool).await.1,
        current,
        "written at the current version"
    );
}

/// A CAD file an older bridge read has its tree, entities and PMI read again when a worker starts,
/// which is how a part ingested before the bridge read PMI gets its PMI, and it is not queued again
/// once the tree is current.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_cad_read_an_older_kernel_wrote_is_read_again_with_its_pmi(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(FIXTURE_PLATE), BRACKET_FIXTURE).expect("write");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };
    handler
        .handle(&job_for(FIXTURE_PLATE))
        .await
        .expect("the STEP file ingests");
    // As a part ingested before the bridge read PMI stands: no PMI row, and a tree an older bridge
    // wrote.
    sqlx::query("DELETE FROM derivative WHERE kind = 'pmi'")
        .execute(&pool)
        .await
        .expect("drops the PMI");
    sqlx::query("UPDATE revision SET face_count = NULL, edge_count = NULL")
        .execute(&pool)
        .await
        .expect("and the counts, which that bridge did not read");
    sqlx::query(
        "UPDATE derivative SET kernel_version = 'occt bridge-5' WHERE kind IN ('structure', 'entities')",
    )
    .execute(&pool)
    .await
    .expect("ages the read");

    handler.enqueue_stale_derivatives().await;
    let jobs = lapidary_db::PgJobs(pool.clone());
    let job = jobs
        .dequeue("worker-a", std::time::Duration::from_secs(60))
        .await
        .expect("dequeues")
        .expect("the old read is queued");
    assert_eq!(
        handler.handle(&job).await.expect("reads the file again"),
        Outcome::Rendered
    );
    sqlx::query("UPDATE job SET state = 'done', outcome = 'rendered' WHERE id = $1")
        .bind(job.id.as_uuid())
        .execute(&pool)
        .await
        .expect("settles the job");

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT kind, kernel_version, blake3 FROM derivative \
         WHERE kind IN ('structure', 'entities', 'pmi') ORDER BY kind",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    let versions: Vec<(&str, &str)> = rows
        .iter()
        .map(|(kind, version, _)| (kind.as_str(), version.as_str()))
        .collect();
    assert_eq!(
        versions,
        [
            ("entities", "occt test"),
            ("pmi", "occt test"),
            ("structure", "occt test")
        ],
        "all three written again, at this kernel's version"
    );
    let store = lapidary_storage::DerivativeStore::open(blob_root.path());
    let pmi: lapidary_core::Pmi = serde_json::from_slice(
        &store
            .get(&BlobHash::parse_hex(&rows[1].2).expect("a hash"))
            .expect("the bytes are in the store"),
    )
    .expect("the PMI parses");
    assert_eq!(pmi, fake_pmi());
    let counts: (Option<i32>, Option<i32>) =
        sqlx::query_as("SELECT face_count, edge_count FROM revision")
            .fetch_one(&pool)
            .await
            .expect("the revision");
    assert_eq!(
        counts,
        (Some(38), Some(96)),
        "the faces and edges, read again"
    );

    handler.enqueue_stale_derivatives().await;
    assert!(
        jobs.dequeue("worker-a", std::time::Duration::from_secs(60))
            .await
            .expect("dequeues")
            .is_none(),
        "a read at this kernel's version is not queued again"
    );
}

// ---------------------------------------------------------------------------------------
// Phase 4 slice 1: a file whose bytes changed at a path the library already indexes.
// ---------------------------------------------------------------------------------------

const SPACER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/spacer-lp-2001-00.stl");

/// Governance is opt-in: the seeded library is hobby until somebody switches it.
async fn make_controlled(pool: &PgPool, library: LibraryId) {
    sqlx::query("UPDATE library SET mode = 'controlled' WHERE id = $1")
        .bind(library.as_uuid())
        .execute(pool)
        .await
        .expect("switches the library to controlled");
}

fn hex_of(bytes: &[u8]) -> String {
    BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()).to_hex()
}

/// Every revision of every part, oldest first: its label, its parent's label, its origin,
/// and its source file's hash and storage path.
async fn revision_rows(pool: &PgPool) -> Vec<(String, Option<String>, String, String, String)> {
    sqlx::query_as(
        "SELECT r.rev_label, parent.rev_label, r.origin, f.blake3, f.storage_path \
         FROM revision r \
         LEFT JOIN revision parent ON parent.id = r.parent_revision_id \
         JOIN file f ON f.revision_id = r.id AND f.role = 'source' \
         ORDER BY r.created_at, r.id",
    )
    .fetch_all(pool)
    .await
    .expect("revision rows")
}

/// A closed mesh's centre of mass is recorded on its revision, marked as the mesh's own: approximate.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_closed_meshs_centre_of_mass_is_recorded_on_its_revision(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let (centre, source): (serde_json::Value, String) = sqlx::query_as(
        "SELECT mass_props_json->'centre_mm', mass_props_json->>'source' FROM revision",
    )
    .fetch_one(&pool)
    .await
    .expect("the revision has a centre of mass");
    assert_eq!(source, "tessellated", "a mesh's centre is approximate");
    let axes = centre.as_array().expect("three axes");
    assert_eq!(axes.len(), 3, "{centre}");
    assert!(
        axes.iter()
            .all(|axis| axis.as_f64().is_some_and(f64::is_finite)),
        "{centre}"
    );
}

/// The one part's materials, and whether a person typed them.
async fn materials_of(pool: &PgPool) -> (Vec<String>, bool) {
    sqlx::query_as("SELECT materials, materials_typed FROM part")
        .fetch_one(pool)
        .await
        .expect("the part")
}

/// A material a person typed is kept when a revised CAD file states another, while what the revised
/// file says about itself is still recorded; clearing the typed list hands the part back to the file.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_typed_material_outlasts_a_revision_whose_file_states_another(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, ingest_dir.path(), blob_root.path())
    };
    make_controlled(&pool, seeded()).await;
    stage(ingest_dir.path(), FIXTURE_PLATE, BRACKET_FIXTURE);
    assert_eq!(
        handler
            .handle(&job_for(FIXTURE_PLATE))
            .await
            .expect("ingests"),
        Outcome::Ingested
    );
    assert_eq!(
        materials_of(&pool).await,
        (vec!["AISI 1045 steel".to_owned()], false),
        "what the file states, and nobody typed it"
    );
    let part = lapidary_core::PartId::from_uuid(
        sqlx::query_scalar("SELECT id FROM part")
            .fetch_one(&pool)
            .await
            .expect("the part"),
    );
    lapidary_db::PgParts(pool.clone())
        .set_materials(part, &["EN AW-6082 T6".to_owned()])
        .await
        .expect("a material is typed");
    // As an older reading left it, so the revision is seen to read the file again.
    sqlx::query(
        "UPDATE part SET metadata_json = jsonb_set(metadata_json, '{cad,originating_system}', '\"CATIA V5\"')",
    )
    .execute(&pool)
    .await
    .expect("ages the header");

    stage(ingest_dir.path(), FIXTURE_PLATE, SPACER_FIXTURE);
    assert_eq!(
        handler
            .handle(&job_for(FIXTURE_PLATE))
            .await
            .expect("revises"),
        Outcome::Revised
    );
    assert_eq!(
        materials_of(&pool).await,
        (vec!["EN AW-6082 T6".to_owned()], true),
        "the typed material, not the one the revised file states"
    );
    let system: serde_json::Value =
        sqlx::query_scalar("SELECT metadata_json->'cad'->'originating_system' FROM part")
            .fetch_one(&pool)
            .await
            .expect("the header");
    assert_eq!(
        system, "SOLIDWORKS 2025",
        "while what the revised file says about itself is recorded"
    );

    lapidary_db::PgParts(pool.clone())
        .set_materials(part, &[])
        .await
        .expect("the typed list is cleared");
    assert_eq!(
        materials_of(&pool).await,
        (vec!["AISI 1045 steel".to_owned()], false),
        "cleared, the part holds what its file states again"
    );
}

/// The whole round trip the slice exists for, as a scan sees it: the owner edits a file in
/// place, re-scans, and gets a second revision rather than "already here".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_changed_file_in_a_controlled_library_becomes_revision_two_beside_the_first(
    pool: PgPool,
) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("revises"),
        Outcome::Revised,
        "changed bytes in a controlled library are a revision, not a skip"
    );

    let rows = revision_rows(&pool).await;
    assert_eq!(rows.len(), 2, "two revisions of the one part: {rows:?}");
    assert_eq!(part_count(&pool).await, 1, "and still one part");
    let (top_dir, _) = rows[1].4.rsplit_once('/').expect("a model directory");
    assert_eq!(
        rows[0],
        (
            "1".to_owned(),
            None,
            "ingest".to_owned(),
            hex_of(BRACKET_FIXTURE),
            format!("{top_dir}/revisions/1/{BRACKET}"),
        ),
        "revision 1 keeps its bytes, set aside under revisions/1"
    );
    assert_eq!(
        rows[1],
        (
            "2".to_owned(),
            Some("1".to_owned()),
            "ingest".to_owned(),
            hex_of(SPACER_FIXTURE),
            format!("{top_dir}/{BRACKET}"),
        ),
        "revision 2 is on top, at the path the owner already knew"
    );

    for (_, _, _, hash, path) in &rows {
        let bytes = std::fs::read(blob_root.path().join(path))
            .unwrap_or_else(|e| panic!("{path} is not on disk: {e}"));
        assert_eq!(&hex_of(&bytes), hash, "{path} holds its row's bytes");
    }

    let manifest = manifest_in(&blob_root.path().join(top_dir));
    let labels: Vec<&str> = manifest
        .revisions
        .iter()
        .map(|revision| revision.rev_label.as_str())
        .collect();
    assert_eq!(labels, ["1", "2"], "metadata.json lists both, oldest first");
    assert_eq!(
        manifest.revisions[1].files[0].blake3.to_hex(),
        hex_of(SPACER_FIXTURE)
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_bytes_again_in_a_controlled_library_are_skipped_not_revised(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);

    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("re-scans"),
        Outcome::Skipped,
        "identical bytes are not a revision"
    );
    assert_eq!(revision_rows(&pool).await.len(), 1);
}

/// A revert is history, not a skip: bytes an older revision held, arriving on top of a
/// newer one, are the third revision.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn going_back_to_an_older_revisions_bytes_is_a_new_revision(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;

    for (bytes, expected) in [
        (BRACKET_FIXTURE, Outcome::Ingested),
        (SPACER_FIXTURE, Outcome::Revised),
        (BRACKET_FIXTURE, Outcome::Revised),
    ] {
        stage(ingest_dir.path(), BRACKET, bytes);
        assert_eq!(
            handler.handle(&job_for(BRACKET)).await.expect("settles"),
            expected
        );
    }

    let rows = revision_rows(&pool).await;
    let labels: Vec<(&str, Option<&str>)> = rows
        .iter()
        .map(|(label, parent, ..)| (label.as_str(), parent.as_deref()))
        .collect();
    assert_eq!(labels, [("1", None), ("2", Some("1")), ("3", Some("2"))]);
    assert_eq!(rows[2].3, hex_of(BRACKET_FIXTURE));
    for (_, _, _, hash, path) in &rows {
        let bytes = std::fs::read(blob_root.path().join(path))
            .unwrap_or_else(|e| panic!("{path} is not on disk: {e}"));
        assert_eq!(&hex_of(&bytes), hash, "{path} holds its row's bytes");
    }
}

/// A hobby library keeps no revisions, and still does not — but it no longer calls the
/// change "already here". Nothing is written: no row, no rung, no file.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_changed_file_in_a_hobby_library_is_unkept_and_writes_nothing(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    let files_before = all_files(blob_root.path());
    let blobs_before: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("blob count");

    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("settles"),
        Outcome::Unkept
    );

    let rows = revision_rows(&pool).await;
    assert_eq!(rows.len(), 1, "no second revision in a hobby library");
    assert_eq!(rows[0].3, hex_of(BRACKET_FIXTURE));
    assert_eq!(all_files(blob_root.path()), files_before, "no file written");
    let blobs_after: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("blob count");
    assert_eq!(blobs_after, blobs_before, "no blob row written");
}

/// The model directory a race gives the file: the plain name is taken, so it takes the
/// disambiguated one, and `bytes` are already written there.
fn taken_path(blob_root: &Path, bytes: &[u8]) -> PathBuf {
    let library = blob_root.join(LIBRARY_DIR);
    std::fs::create_dir_all(library.join("bracket-lp-1042-03")).expect("the plain name, taken");
    let taken = library
        .join(format!(
            "bracket-lp-1042-03_{}",
            &hex_of(BRACKET_FIXTURE)[..6]
        ))
        .join(BRACKET);
    stage(
        blob_root,
        taken
            .strip_prefix(blob_root)
            .expect("under the root")
            .to_str()
            .expect("utf-8"),
        bytes,
    );
    taken
}

/// A new part's file never lands on another's (Phase 4 slice 1 spec §3.3): other bytes already
/// at its path are left exactly as they are, and the job is decided again.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_new_file_never_replaces_other_bytes_already_at_its_path(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let taken = taken_path(blob_root.path(), GEAR_FIXTURE);

    let error = handler
        .handle(&job_for(BRACKET))
        .await
        .expect_err("another job's bytes are at this path");
    assert!(matches!(error, HandlerError::Transient { .. }), "{error:?}");
    assert_eq!(
        std::fs::read(&taken).expect("the other job's file"),
        GEAR_FIXTURE,
        "the bytes already there are left as they are"
    );
    assert_eq!(part_count(&pool).await, 0);
}

/// The same bytes already at that path are this file, left by an attempt that stopped before its
/// row. They ingest as written, rather than failing every attempt after the first.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_bytes_already_at_its_path_ingest_as_written(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let taken = taken_path(blob_root.path(), BRACKET_FIXTURE);

    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let rows = revision_rows(&pool).await;
    assert_eq!(
        blob_root.path().join(&rows[0].4),
        taken,
        "the row names the file already there"
    );
}

/// Holds a job inside the kernel until the test lets it go, so another job can finish first.
struct HeldCad {
    reached: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl Kernel for HeldCad {
    fn version(&self, params: &KernelParams) -> KernelVersion {
        FakeCad.version(params)
    }

    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        self.reached.notify_one();
        self.release.notified().await;
        FakeCad.process(bytes, params).await
    }
}

/// Two jobs, different bytes, one new path (slice 1 spec §3.3). The loser read no part there, and
/// the winner files one while the loser is in the kernel. The loser is decided again rather than
/// skipped, and its retry reads the winner's part: unkept in a hobby library, a revision in a
/// controlled one. Neither file replaces the other, and the loser leaves no copy behind.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_that_loses_a_new_path_to_other_bytes_is_decided_again(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let controlled = second_library(&pool).await;
    make_controlled(&pool, controlled).await;

    for (library, settled) in [(seeded(), Outcome::Unkept), (controlled, Outcome::Revised)] {
        let winner_dir = tempfile::tempdir().expect("temp dir");
        let loser_dir = tempfile::tempdir().expect("temp dir");
        stage(winner_dir.path(), FIXTURE_PLATE, BRACKET_FIXTURE);
        stage(loser_dir.path(), FIXTURE_PLATE, GEAR_FIXTURE);
        let reached = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let winner = WorkerHandler {
            cad: Some(Arc::new(FakeCad)),
            ..handler_over(&pool, winner_dir.path(), blob_root.path())
        };
        let loser = WorkerHandler {
            cad: Some(Arc::new(HeldCad {
                reached: reached.clone(),
                release: release.clone(),
            })),
            ..handler_over(&pool, loser_dir.path(), blob_root.path())
        };
        let job = job_for_library(library, FIXTURE_PLATE);

        let (lost, ()) = tokio::join!(loser.handle(&job), async {
            reached.notified().await;
            assert_eq!(
                winner.handle(&job).await.expect("the winner ingests"),
                Outcome::Ingested
            );
            release.notify_one();
        });
        let lost = lost.expect_err("the loser is not skipped");
        assert!(matches!(lost, HandlerError::Transient { .. }), "{lost:?}");
        let copies: Vec<PathBuf> = all_files(blob_root.path())
            .into_iter()
            .filter(|path| std::fs::read(path).is_ok_and(|bytes| bytes == GEAR_FIXTURE))
            .collect();
        assert!(copies.is_empty(), "the loser left its bytes at {copies:?}");

        let retry = WorkerHandler {
            cad: Some(Arc::new(FakeCad)),
            ..handler_over(&pool, loser_dir.path(), blob_root.path())
        };
        assert_eq!(
            retry.handle(&job).await.expect("the retry settles"),
            settled
        );
    }

    for (label, _, _, hash, path) in revision_rows(&pool).await {
        let bytes = std::fs::read(blob_root.path().join(&path))
            .unwrap_or_else(|e| panic!("revision {label} at {path} is not on disk: {e}"));
        assert_eq!(hex_of(&bytes), hash, "{path} holds its own row's bytes");
    }
}

/// Two jobs, the same bytes, one new path. The loser's directory came second, so it wrote its own
/// copy under the disambiguated name before its insert lost. That copy is no part's, and it goes;
/// the winner's file stays.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_that_loses_a_new_path_to_the_same_bytes_leaves_no_copy(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let winner_dir = tempfile::tempdir().expect("temp dir");
    let loser_dir = tempfile::tempdir().expect("temp dir");
    stage(winner_dir.path(), FIXTURE_PLATE, BRACKET_FIXTURE);
    stage(loser_dir.path(), FIXTURE_PLATE, BRACKET_FIXTURE);
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let winner = WorkerHandler {
        cad: Some(Arc::new(FakeCad)),
        ..handler_over(&pool, winner_dir.path(), blob_root.path())
    };
    let loser = WorkerHandler {
        cad: Some(Arc::new(HeldCad {
            reached: reached.clone(),
            release: release.clone(),
        })),
        ..handler_over(&pool, loser_dir.path(), blob_root.path())
    };
    let job = job_for(FIXTURE_PLATE);

    let (lost, ()) = tokio::join!(loser.handle(&job), async {
        reached.notified().await;
        assert_eq!(
            winner.handle(&job).await.expect("the winner ingests"),
            Outcome::Ingested
        );
        release.notify_one();
    });
    assert_eq!(lost.expect("the loser settles"), Outcome::Skipped);

    let copy = blob_root.path().join(LIBRARY_DIR).join(format!(
        "fixture-plate-lp-9000-00_{}",
        &hex_of(BRACKET_FIXTURE)[..6]
    ));
    assert!(
        !copy.exists(),
        "the loser's copy at {} is left",
        copy.display()
    );
    let rows = revision_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    let bytes = std::fs::read(blob_root.path().join(&rows[0].4)).expect("the winner's file");
    assert_eq!(hex_of(&bytes), rows[0].3);
}

/// Purge collects every revision's file, and the sweep leaves no directory behind whichever
/// order it reaches them in: not `revisions/1`, not `revisions`, not the model directory.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn purging_a_revised_part_leaves_no_file_or_directory_behind(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("revises");
    let rows = revision_rows(&pool).await;
    let (model_dir, _) = rows[1].4.rsplit_once('/').expect("a model directory");
    let model_dir = blob_root.path().join(model_dir);
    assert!(model_dir.join("revisions").exists(), "the precondition");

    let part = sqlx::query_scalar::<_, Uuid>("SELECT id FROM part")
        .fetch_one(&pool)
        .await
        .expect("the one part");
    let parts = PgParts(pool.clone());
    let part = lapidary_core::PartId::from_uuid(part);
    assert!(parts.soft_delete(part).await.expect("soft delete"));
    parts.purge(part).await.expect("purge");
    lapidary_ingest::reap::sweep(&pool, blob_root.path(), std::time::Duration::ZERO)
        .await
        .expect("sweep");

    assert!(
        !model_dir.exists(),
        "the model directory must go with its last file; left behind: {:?}",
        all_files(&model_dir)
    );
}

/// A new part's first revision says how its bytes arrived: a scan is `ingest`, an upload is
/// `upload`. Until Phase 4 slice 2 every new part said `ingest`, whatever its route.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_new_parts_first_revision_says_whether_it_was_scanned_or_uploaded(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("scans");
    let hash = upload_into(&pool, blob_root.path(), BRACKET_FIXTURE).await;
    handler
        .handle(&blob_job(hash, "brackets/spare/LP-1042-03.stl"))
        .await
        .expect("uploads");

    let origins: Vec<(String, String)> = sqlx::query_as(
        "SELECT p.source_path, r.origin FROM revision r JOIN part p ON p.id = r.part_id \
         ORDER BY r.origin",
    )
    .fetch_all(&pool)
    .await
    .expect("origins");
    assert_eq!(
        origins,
        [
            (BRACKET.to_owned(), "ingest".to_owned()),
            (
                "brackets/spare/LP-1042-03.stl".to_owned(),
                "upload".to_owned()
            ),
        ]
    );
}

const CUBE_FIXTURE: &[u8] = include_bytes!("../../../fixtures/cube.stl");

fn hash_hex(bytes: &[u8]) -> String {
    BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()).to_hex()
}

/// A bundle as `download.rs` writes one: each part's revisions oldest first, the current one at its
/// source path and the earlier ones under `revisions/<label>/`, then the manifest.
fn bundle_of(parts: &[(&str, &[&[u8]])]) -> Vec<u8> {
    use lapidary_targets::bundle::{
        self, Manifest, ManifestLibrary, ManifestPart, ManifestRevision, StoreZip,
    };
    let mut zip = StoreZip::new(Vec::new());
    let mut manifest_parts = Vec::new();
    for (source_path, versions) in parts {
        let mut revisions = Vec::new();
        for (index, bytes) in versions.iter().enumerate() {
            let label = (index + 1).to_string();
            let path = bundle::entry_path(source_path, &label, index + 1 == versions.len());
            zip.add(&path, &mut &bytes[..]).expect("adds");
            revisions.push(ManifestRevision {
                rev_label: label,
                parent_label: (index > 0).then(|| index.to_string()),
                origin: if index == 0 { "ingest" } else { "agent" }.to_owned(),
                created_at: "2026-09-15T08:00:00Z".to_owned(),
                blake3: hash_hex(bytes),
                size_bytes: bytes.len() as u64,
                format: "stl".to_owned(),
                path,
            });
        }
        manifest_parts.push(ManifestPart {
            name: (*source_path).to_owned(),
            part_number: None,
            source_path: (*source_path).to_owned(),
            tags: vec![],
            sources: vec![],
            revisions,
        });
    }
    let manifest = serde_json::to_vec(&Manifest {
        format: bundle::FORMAT.to_owned(),
        version: bundle::VERSION,
        library: ManifestLibrary {
            name: "Workshop".to_owned(),
            mode: "controlled".to_owned(),
        },
        parts: manifest_parts,
    })
    .expect("serialises");
    zip.add(bundle::MANIFEST, &mut manifest.as_slice())
        .expect("adds");
    zip.finish().expect("finishes")
}

fn import_job(payload: JobPayload, batch: BatchId) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: batch,
        library_id: seeded(),
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// The bundle stored as the upload route stores one, its `ImportBundle` run, and every `ImportPart`
/// it queued run in turn. Each part's answer, in the bundle's order.
async fn import(
    handler: &WorkerHandler,
    pool: &PgPool,
    blob_root: &Path,
    bundle: &[u8],
) -> Result<Vec<lapidary_core::Outcome>, HandlerError> {
    let hash = upload_into(pool, blob_root, bundle).await;
    let batch = BatchId::new();
    let unpacked = handler
        .handle(&import_job(
            JobPayload::ImportBundle {
                blake3: hash,
                path: "workshop-bundle.lapidary.zip".to_owned(),
            },
            batch,
        ))
        .await?;
    assert_eq!(unpacked, lapidary_core::Outcome::Scanned);
    let queued: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM job WHERE batch_id = $1 AND kind = 'import_part' \
         ORDER BY (payload->>'part')::int",
    )
    .bind(batch.as_uuid())
    .fetch_all(pool)
    .await
    .expect("queued parts");
    let mut outcomes = Vec::new();
    for payload in queued {
        let part = JobPayload::from_row("import_part", &payload).expect("parses");
        outcomes.push(handler.handle(&import_job(part, batch)).await?);
    }
    Ok(outcomes)
}

/// Phase 4 slice 2 spec §7: a controlled library replays every revision, so labels, parents and
/// origins survive; importing the same bundle again finds every part already there.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_imported_into_a_controlled_library_keeps_each_parts_labels_parents_and_origins(
    pool: PgPool,
) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    let bundle = bundle_of(&[
        (
            "brackets/bracket-lp-1042-03.stl",
            &[BRACKET_FIXTURE, SPACER_FIXTURE],
        ),
        ("spacers/spacer-lp-2001-00.stl", &[SPACER_FIXTURE]),
    ]);

    let outcomes = import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports");
    assert_eq!(
        outcomes,
        [
            lapidary_core::Outcome::Ingested,
            lapidary_core::Outcome::Ingested
        ]
    );
    let lineage: Vec<(String, Option<String>, String, String)> = revision_rows(&pool)
        .await
        .into_iter()
        .map(|(label, parent, origin, hash, _)| (label, parent, origin, hash))
        .collect();
    assert_eq!(
        lineage,
        [
            (
                "1".to_owned(),
                None,
                "ingest".to_owned(),
                hash_hex(BRACKET_FIXTURE)
            ),
            (
                "2".to_owned(),
                Some("1".to_owned()),
                "agent".to_owned(),
                hash_hex(SPACER_FIXTURE)
            ),
            (
                "1".to_owned(),
                None,
                "ingest".to_owned(),
                hash_hex(SPACER_FIXTURE)
            ),
        ]
    );

    let again = import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports again");
    assert_eq!(
        again,
        [
            lapidary_core::Outcome::Skipped,
            lapidary_core::Outcome::Skipped
        ]
    );
    assert_eq!(revision_rows(&pool).await.len(), 3, "nothing twice");
}

/// A hobby library keeps no history, so a part arrives as its newest revision alone.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_hobby_library_imports_each_parts_newest_revision_only(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let bundle = bundle_of(&[(
        "brackets/bracket-lp-1042-03.stl",
        &[BRACKET_FIXTURE, SPACER_FIXTURE],
    )]);

    let outcomes = import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports");
    assert_eq!(outcomes, [lapidary_core::Outcome::Ingested]);
    let rows = revision_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "1");
    assert_eq!(rows[0].3, hash_hex(SPACER_FIXTURE));
}

/// A part already holding a file that is none of the bundle's revisions is another part's history.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_part_landing_on_another_parts_history_is_refused(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    stage(
        ingest_dir.path(),
        "brackets/bracket-lp-1042-03.stl",
        CUBE_FIXTURE,
    );
    handler
        .handle(&job_for("brackets/bracket-lp-1042-03.stl"))
        .await
        .expect("scans");

    let refused = import(
        &handler,
        &pool,
        blob_root.path(),
        &bundle_of(&[(
            "brackets/bracket-lp-1042-03.stl",
            &[BRACKET_FIXTURE, SPACER_FIXTURE],
        )]),
    )
    .await
    .expect_err("refused");
    assert!(
        matches!(&refused, HandlerError::Permanent { message } if message.contains("graft")),
        "{refused:?}"
    );
    assert_eq!(revision_rows(&pool).await.len(), 1, "only the scanned cube");
}

/// A bundle whose file was changed after export is refused whole, and not one part is queued.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_that_is_not_whole_queues_nothing(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let path = "brackets/bracket-lp-1042-03.stl";
    let mut tampered = bundle_of(&[(path, &[BRACKET_FIXTURE])]);
    tampered[30 + path.len() + 200] ^= 0xff;
    let hash = upload_into(&pool, blob_root.path(), &tampered).await;
    let batch = BatchId::new();

    let refused = handler
        .handle(&import_job(
            JobPayload::ImportBundle {
                blake3: hash,
                path: "workshop-bundle.lapidary.zip".to_owned(),
            },
            batch,
        ))
        .await
        .expect_err("refused");
    assert!(
        matches!(&refused, HandlerError::Permanent { message } if message.contains("not a bundle Lapidary can import")),
        "{refused:?}"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(queued, 0);
    assert!(revision_rows(&pool).await.is_empty());
}

async fn quarantined(pool: &PgPool, bytes: &[u8]) -> bool {
    sqlx::query_scalar("SELECT quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(hash_hex(bytes))
        .fetch_one(pool)
        .await
        .expect("the blob row")
}

/// Nothing ever points at a bundle's own bytes, since its parts are replayed into files of their
/// own: whether it imports or is refused, its blob is released into the 30-day quarantine.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_imported_bundles_own_bytes_are_released_whether_or_not_it_imports(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let path = "brackets/bracket-lp-1042-03.stl";
    let bundle = bundle_of(&[(path, &[BRACKET_FIXTURE])]);
    import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports");
    assert!(
        quarantined(&pool, &bundle).await,
        "released once its parts are queued"
    );

    let mut tampered = bundle_of(&[(path, &[SPACER_FIXTURE])]);
    tampered[30 + path.len() + 200] ^= 0xff;
    import(&handler, &pool, blob_root.path(), &tampered)
        .await
        .expect_err("refused");
    assert!(quarantined(&pool, &tampered).await, "and a refused one too");
}

/// A revert is a revision, so a history can hold the same bytes twice: an import resumes by the whole
/// sequence, and a second import of it finds every revision already there.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_history_with_a_revert_imports_once_and_a_second_import_skips_it(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    let bundle = bundle_of(&[(
        "brackets/bracket-lp-1042-03.stl",
        &[BRACKET_FIXTURE, SPACER_FIXTURE, BRACKET_FIXTURE],
    )]);

    assert_eq!(
        import(&handler, &pool, blob_root.path(), &bundle)
            .await
            .expect("imports"),
        [lapidary_core::Outcome::Ingested]
    );
    assert_eq!(revision_rows(&pool).await.len(), 3);
    assert_eq!(
        import(&handler, &pool, blob_root.path(), &bundle)
            .await
            .expect("imports again"),
        [lapidary_core::Outcome::Skipped]
    );
    assert_eq!(
        revision_rows(&pool).await.len(),
        3,
        "no revision recorded twice"
    );
}

/// A part whose history began with other bytes is another part's, even when its newest revision is
/// one the bundle holds.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_whose_history_began_elsewhere_is_refused_though_it_ends_on_a_bundle_revision(
    pool: PgPool,
) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    make_controlled(&pool, seeded()).await;
    let path = "brackets/bracket-lp-1042-03.stl";
    stage(ingest_dir.path(), path, CUBE_FIXTURE);
    handler.handle(&job_for(path)).await.expect("scans");
    stage(ingest_dir.path(), path, BRACKET_FIXTURE);
    handler.handle(&job_for(path)).await.expect("revises");
    assert_eq!(revision_rows(&pool).await.len(), 2);

    let refused = import(
        &handler,
        &pool,
        blob_root.path(),
        &bundle_of(&[(path, &[BRACKET_FIXTURE, SPACER_FIXTURE])]),
    )
    .await
    .expect_err("refused");
    assert!(
        matches!(&refused, HandlerError::Permanent { message } if message.contains("graft")),
        "{refused:?}"
    );
    assert_eq!(revision_rows(&pool).await.len(), 2, "nothing grafted");
}
