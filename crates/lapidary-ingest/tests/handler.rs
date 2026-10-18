//! The handler, exercised the way the worker exercises it.

use lapidary_core::{
    BatchId, BlobHash, DerivativeKind, JobId, JobPayload, LibraryId, MeshMeasurements, Outcome,
    RevisionId,
};
use lapidary_db::{
    GridQuery, IngestRequest, JobRow, PartRepository, PgIngest, PgParts, Sort, StoredBlobRow,
};
use lapidary_ingest::Pause;
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
        inbox_dir: blob_root.to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
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
async fn a_real_stl_ingests_with_its_real_measurements(pool: PgPool) {
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
        inbox_dir: blob_root.path().to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
    };

    let outcome = handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(outcome, Outcome::Ingested);

    let (name, tri, x, y, z, watertight): (String, i32, f64, f64, f64, bool) = sqlx::query_as(
        "SELECT p.name, r.triangle_count, r.bbox_x, r.bbox_y, r.bbox_z, r.is_watertight \
         FROM part p \
         JOIN revision r ON r.part_id = p.id \
         WHERE p.library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("the part and its measurements landed");

    assert_eq!(name, "bracket-lp-1042-03");
    assert_eq!(tri, 20, "the fixture's real triangle count");
    assert_eq!(
        (x, y, z),
        (88.0, 40.0, 25.0),
        "the fixture's real bounding box"
    );
    assert!(watertight);
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
        inbox_dir: blob_root.path().to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
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
        inbox_dir: blob_root.path().to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
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
        inbox_dir: blob_root.path().to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
    };
    let handler_b = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
        inbox_dir: blob_root.path().to_path_buf().join("inbox"),
        pause: None,
        sites: lapidary_sites::Sites::public(),
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

    // Nothing was written by either: the winner's row names the file where it is.
    let (storage_path, root): (Option<String>, String) =
        sqlx::query_as("SELECT storage_path, root FROM file WHERE role = 'source'")
            .fetch_one(&pool)
            .await
            .expect("the winner's file row");
    assert_eq!(
        (storage_path.as_deref(), root.as_str()),
        (Some(BRACKET), "library")
    );
    assert!(all_files(blob_root.path()).is_empty());
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
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library: seeded(),
            name: "notes",
            // Must be the path the job below carries, not the part name: since slice 6a
            // the short-circuit is keyed on `source_path`, so seeding a different path
            // here would make this test assert a re-scan that never happened.
            source_path: "notes.stl",
            blob: &blob,
            measurements: Some(&measurements),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
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
        0,
        "and no content-addressed copy: ingest writes no derivative at all"
    );
    assert!(
        all_files(blob_root.path()).is_empty(),
        "and no copy for either: both libraries read the one file where it is"
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
    // One blob row, and nothing on disk of Lapidary's: each part reads its own file where
    // it is in the folder.
    assert!(all_files(blob_root.path()).is_empty());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failed_write_leaves_nothing_on_disk(pool: PgPool) {
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

    // Nothing was written before the transaction, so a failed one leaves nothing anywhere.
    let orphans = all_files(blob_root.path());
    assert!(orphans.is_empty(), "found {orphans:?}");
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
    assert!(
        after_first.is_empty(),
        "nothing is written for a part: {after_first:?}"
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

/// **Ingest writes no derivative and no derivative byte.** This test was "one tessellation blob
/// and row"; the ladder it checked is gone, and what is worth keeping is the other half of the
/// same claim — that nothing is written where a rung used to be, store included.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_stl_writes_no_derivative_blob_and_no_derivative_row(pool: PgPool) {
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
    assert!(kinds.is_empty(), "no derivative row at all: {kinds:?}");
    assert!(
        all_files(&blob_root.path().join("derivatives")).is_empty(),
        "and nothing in the derivative store"
    );
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(blobs, 1, "the source blob, and only it");
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

    assert!(
        derivatives(&pool).await.is_empty(),
        "an OBJ writes no derivative at ingest, exactly as an STL does: nothing renders"
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

/// Read off the exports, because ingest writes no derivative to read it off.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_kernel_version_differs_between_an_stl_and_an_obj_source(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write stl");
    std::fs::write(ingest_dir.path().join(IDLER_OBJ), IDLER_OBJ_FIXTURE).expect("write obj");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("stl");
    handler.handle(&job_for(IDLER_OBJ)).await.expect("obj");
    let revisions: Vec<sqlx::types::Uuid> = sqlx::query_scalar("SELECT id FROM revision")
        .fetch_all(&pool)
        .await
        .expect("both revisions");
    for revision in revisions {
        handler
            .handle(&derive_job(
                RevisionId::from_uuid(revision),
                DerivativeKind::Export3mf,
            ))
            .await
            .expect("derives a 3MF from each");
    }

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

/// A 3MF ingests through its own parser, and the format and the version its export records say
/// which parser ran. Nothing is derived at ingest, so the version is read off an export.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_3mf_is_parsed_and_its_export_records_the_3mf_parser(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("ingests"),
        Outcome::Ingested
    );

    assert!(
        derivatives(&pool).await.is_empty(),
        "nothing is derived at ingest"
    );
    let revision = only_revision(&pool).await;
    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::ExportStl))
            .await
            .expect("derives an STL from the 3MF"),
        Outcome::Rendered
    );

    let (format, version): (String, String) = sqlx::query_as(
        "SELECT f.format, d.kernel_version FROM file f \
         JOIN derivative d ON d.revision_id = f.revision_id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(format, "3mf");
    assert_eq!(
        version, "mesh 3mf-1+glb-3+cpu-1",
        "the version names the parser that ran, not the kind that was asked for"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_source_file_is_recorded_as_itself_whatever_its_format(pool: PgPool) {
    // Every source is the owner's own file, read where it is (M4): nothing compresses it, and
    // its row records no compression. A `zstd_level` other than 0 is only ever a pre-M4 store
    // row, which every reader still follows.
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
            std::fs::read(ingest_dir.path().join(&path)).expect("the file is at that path"),
            fixture,
            "the row names the file in the owner's folder, byte-identical ({path})"
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

/// **Ingest writes no derivative at all, and every part is still in the grid.**
///
/// This was the slice-3 exit criterion for a library with `auto_thumbnail = false`: zero
/// thumbnail rows, and the part still showing "no preview yet" rather than missing from the
/// grid, because a row missing from the grid is data the user cannot see. R3 made that state
/// the only state there is — nothing renders — so the assertion is the same one with no
/// setting to turn off.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_ingest_writes_no_derivative_and_the_part_is_still_in_the_grid(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );

    assert!(
        derivatives(&pool).await.is_empty(),
        "no thumbnail, no rung, no CAD read: the card shows the part's own picture now"
    );
    assert!(
        all_files(&blob_root.path().join("derivatives")).is_empty(),
        "and nothing was written to the derivative store either"
    );

    let page = PgParts(pool.clone())
        .page(&GridQuery::new(seeded(), 10), Sort::Newest)
        .await
        .expect("page");
    assert_eq!(page.len(), 1, "a part with no picture is still a part");
    assert_eq!(page[0].summary.name, "bracket-lp-1042-03");
    assert_eq!(
        page[0].summary.triangle_count,
        Some(20),
        "and it is measured, which is what ingest is for"
    );
}

/// A derive job re-reads the **source blob** for the revision it names, not the ingest mount —
/// which is the only copy still guaranteed to exist once the mount the file arrived on is gone.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_derive_job_reads_the_file_where_it_is_and_reports_rendered(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert!(derivatives(&pool).await.is_empty(), "nothing at ingest");

    let revision = only_revision(&pool).await;
    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::ExportStl))
            .await
            .expect("derives the export from the file where it is"),
        Outcome::Rendered
    );

    let rows = derivatives(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "export_stl");
    let hex = rows[0].1.clone().expect("an export is hash-addressed");
    let bytes = lapidary_storage::DerivativeStore::open(blob_root.path())
        .get(&BlobHash::parse_hex(&hex).expect("a stored hash parses"))
        .expect("the export's bytes are on disk");
    // Decoded rather than length-checked: a zeroed file has a length too, and only reading
    // the count back proves the row holds the mesh the kernel actually wrote.
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("four bytes"));
    assert_eq!(count, 20, "the bracket's own triangles, in a binary STL");
}

/// **An old STEP revision asked for an export is refused in the same words a scan refuses one.**
///
/// This is why `derive_one` checks the format at all, and the check is not belt-and-braces: a
/// STEP file cannot be ingested any more, but an install upgraded across the pivot still holds
/// the revisions one ingested before it — R2 kept `revision.face_count` and `edge_count` for
/// exactly those rows. `POST /api/parts/{id}/exports/{kind}` queues a `derive` job against
/// whatever revision it is given, so this path is reachable by one click on a part that has
/// been in the library for a year.
///
/// Seeded by ingesting an STL and then restating its format, which is what such a row looks
/// like from here: a readable source blob, a `file.format` no parser answers for.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_export_of_a_pre_pivot_step_revision_is_refused_in_the_same_words(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    let revision = only_revision(&pool).await;
    sqlx::query("UPDATE file SET format = 'step'")
        .execute(&pool)
        .await
        .expect("restates the format, as a revision ingested before the pivot holds it");

    match handler
        .handle(&derive_job(revision, DerivativeKind::ExportStl))
        .await
    {
        Err(HandlerError::Permanent { message }) => {
            assert_eq!(
                message,
                lapidary_core::cad_refusal("step"),
                "a person who meets this twice must read the same sentence both times"
            );
        }
        other => panic!("a STEP revision must be refused permanently, got {other:?}"),
    }
    assert!(
        derivatives(&pool).await.is_empty(),
        "and nothing is written for it"
    );
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
async fn a_derive_job_naming_another_librarys_revision_writes_nothing(pool: PgPool) {
    // A revision id is a uuid a caller might hold from anywhere, and the payload carries
    // one. Without the scope this job writes onto a revision its library does not own --
    // a cross-tenant WRITE, which is the direction that cannot be undone. CLAUDE.md:
    // content addressing is not authorization, and neither is knowing a revision id.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert!(derivatives(&pool).await.is_empty(), "nothing at ingest");
    let revision = only_revision(&pool).await;
    let other_library = second_library(&pool).await;

    let err = handler
        .handle(&derive_job_for(
            other_library,
            revision,
            DerivativeKind::ExportStl,
        ))
        .await
        .expect_err("a library that does not own this revision must not write onto it");

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
    assert!(
        derivatives(&pool).await.is_empty(),
        "the refusal has to happen before the write, or the row is already there"
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

/// **And it refuses before the parse**, which is the whole of what step 3a buys.
///
/// `library_slug` further down the ladder is a second guard with the same shape of message, so
/// removing step 3a leaves the case above passing — `mutate-r3.sh`'s `library-exists-always`
/// survived on exactly that. What tells them apart is a file that cannot parse: with the probe
/// where it is, the refusal names the library; without it, the mesh parser fails first and the
/// operator is told to re-export a file whose library does not exist.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_for_a_library_that_does_not_exist_is_refused_before_the_parse(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("notes.stl"),
        b"LP-1042-03 revision notes: chamfer the mounting face.\n",
    )
    .expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let absent = LibraryId::new();

    let err = handler
        .handle(&job_for_library(absent, "notes.stl"))
        .await
        .expect_err("a job naming a library that does not exist cannot ingest anything");

    let message = format!("{err:?}");
    assert!(
        message.contains(&absent.to_string()),
        "the library, not the file: nothing should have read a byte of it yet, got {message}"
    );
    assert!(
        !message.contains("Could not read this stl"),
        "the parse must not have run: step 3a sits above it so a re-scan costs one query, got \
         {message}"
    );
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

/// Design section 10's byte-identity criterion, asked of the thing it can still be asked of: an
/// export derived twice from the same source is the same bytes, at the same version, in one blob
/// with one reference.
///
/// It named L2 once, then L0 when ingest stopped building the finer rungs. Ingest builds nothing
/// now, so there is no eager derivative to compare a lazy one against at all — and the rule the
/// criterion exists for is still live: a second build of the same file must not store a second
/// copy or inflate `ref_count`, or eviction would leave bytes nothing points at.
///
/// The gear, not the bracket: a bigger mesh is a bigger export, so a writer that silently
/// truncated would show.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_export_derived_twice_is_one_blob_with_one_reference(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(GEAR), GEAR_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(GEAR)).await.expect("ingests");
    let revision = only_revision(&pool).await;
    let export = |pool: &PgPool| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (String, String)>(
                "SELECT blake3, kernel_version FROM derivative WHERE kind = 'export_stl'",
            )
            .fetch_one(&pool)
            .await
            .expect("exactly one export row, with bytes and a version on it")
        }
    };

    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::ExportStl))
            .await
            .expect("derives the export"),
        Outcome::Rendered
    );
    let (hash_before, version_before) = export(&pool).await;
    let blobs_before = all_blob_rows(&pool).await;
    let refs_before = refs_to(&pool, &hash_before).await;

    assert_eq!(
        handler
            .handle(&derive_job(revision, DerivativeKind::ExportStl))
            .await
            .expect("derives it again over the one already there"),
        Outcome::Rendered
    );

    let (hash_after, version_after) = export(&pool).await;
    assert_eq!(
        hash_after, hash_before,
        "the same bytes from the same source must hash the same however often they are asked for"
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
        "re-writing the same bytes moves no reference, so `ref_count` must not inflate"
    );
}

/// A `scan_directory` job row belonging to `batch`, shaped exactly as the api route's
/// `enqueue` writes one: the payload is empty, and the batch is the row's own column.
/// The seeded library holding the whole models folder, as a person who added it with Import has
/// (A2): what every scan test here walks.
async fn whole_folder(pool: &PgPool) {
    lapidary_db::PgSources(pool.clone())
        .add(seeded(), "", "folder", None, false)
        .await
        .expect("the whole folder, added");
}

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
    whole_folder(&pool).await;
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
    whole_folder(&pool).await;
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
    whole_folder(&pool).await;
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

/// A `.step` file is **found** by the walk and refused by the job, never skipped in silence.
///
/// This is the whole reason `CAD_FORMATS` stays chained into `is_model_file`. Drop it there and the
/// walk passes the file without a word: the owner sees a part that is simply not in their library
/// and nothing anywhere says why. Refused out loud is the behaviour; skipped is the bug.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_finds_a_b_rep_file_and_its_job_refuses_it_rather_than_skipping_it(pool: PgPool) {
    whole_folder(&pool).await;
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    stage(
        ingest_dir.path(),
        "fixtures/fixture-plate-lp-9000-00.step",
        BRACKET_FIXTURE,
    );

    assert_eq!(
        scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await,
        vec![
            BRACKET.to_owned(),
            "fixtures/fixture-plate-lp-9000-00.step".to_owned()
        ],
        "the walk reports the STEP file, so there is a job to carry the refusal"
    );

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let message = match handler
        .handle(&job_for("fixtures/fixture-plate-lp-9000-00.step"))
        .await
    {
        Err(HandlerError::Permanent { message }) => message,
        other => panic!("the file a scan found must be refused, got {other:?}"),
    };
    assert!(message.contains("STEP"), "{message}");
}

/// A symlinked directory is the only way a real filesystem can present a cycle, and
/// `DirEntry::file_type` not traversing it is what makes the cycle unreachable. A test,
/// because that guarantee is a property of the API rather than of anything visible in the
/// walk's own code.
#[cfg(unix)]
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_does_not_follow_a_symlinked_directory(pool: PgPool) {
    whole_folder(&pool).await;
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
    whole_folder(&pool).await;
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
    whole_folder(&pool).await;
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
/// Delete is delete (owner, 2026-10-17), and a deleted part leaves no row for a scan to find. So
/// the file a deleted part came from, scanned again, is simply a file this library does not have:
/// it is ingested as a new part.
///
/// Since M4 the new part names the very file the deleted one did, where it is: no copy is made.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_re_scan_re_ingests_a_deleted_part_as_a_new_one(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    let (first, path): (Uuid, String) = sqlx::query_as(
        "SELECT p.id, f.storage_path FROM part p \
         JOIN revision r ON r.part_id = p.id JOIN file f ON f.revision_id = r.id",
    )
    .fetch_one(&pool)
    .await
    .expect("the one part");
    assert!(
        PgParts(pool.clone())
            .delete(lapidary_core::PartId::from_uuid(first))
            .await
            .expect("delete")
            .is_some(),
    );
    assert_eq!(
        std::fs::read(ingest_dir.path().join(&path)).expect("delete never touches the file"),
        BRACKET_FIXTURE
    );

    assert_eq!(
        handler
            .handle(&job_for(BRACKET))
            .await
            .expect("the re-scan succeeds"),
        Outcome::Ingested,
        "nothing is left to say this file is already here"
    );
    let (again, again_path): (Uuid, String) = sqlx::query_as(
        "SELECT p.id, f.storage_path FROM part p \
         JOIN revision r ON r.part_id = p.id JOIN file f ON f.revision_id = r.id",
    )
    .fetch_one(&pool)
    .await
    .expect("exactly one part again");
    assert_ne!(again, first, "a new part, not the deleted one brought back");
    assert_eq!(again_path, path, "the same file, where it is");
    assert!(
        all_files(blob_root.path()).is_empty(),
        "and no copy anywhere"
    );
    assert_eq!(part_count(&pool).await, 1);
}

// ---------------------------------------------------------------------------------------
// M4: the owner's folders, read where they are. Nothing under them is written.
// ---------------------------------------------------------------------------------------

/// Every file and directory under `root`, with its size, modification time and, for a file, its
/// bytes' BLAKE3 — what "nothing under the folder changed" is checked against. The shell form of
/// the same check is the rig's `find "$INGEST" -printf '%P %s %T@\n' | sort | sha256sum`.
fn snapshot(root: &Path) -> Vec<String> {
    let mut seen = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir).expect("reads a directory") {
            let path = entry.expect("an entry").path();
            let meta = std::fs::symlink_metadata(&path).expect("stats");
            let rel = path
                .strip_prefix(root)
                .expect("under root")
                .display()
                .to_string();
            let mtime = meta.modified().expect("an mtime");
            if meta.is_dir() {
                seen.push(format!("{rel}/ {mtime:?}"));
                queue.push(path);
            } else {
                let hash = blake3::hash(&std::fs::read(&path).expect("reads"));
                seen.push(format!("{rel} {} {mtime:?} {hash}", meta.len()));
            }
        }
    }
    seen.sort();
    seen
}

/// The source file row of the part at `source_path`: its root, path, modification time and
/// whether it is still on disk, newest revision first.
async fn file_rows(
    pool: &PgPool,
    source_path: &str,
) -> Vec<(String, Option<String>, Option<i64>, bool)> {
    sqlx::query_as(
        "SELECT f.root, f.storage_path, f.mtime_ns, f.on_disk FROM file f \
         JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id \
         WHERE p.source_path = $1 ORDER BY r.created_at DESC, r.id DESC",
    )
    .bind(source_path)
    .fetch_all(pool)
    .await
    .expect("the part's file rows")
}

fn mtime_ns(path: &Path) -> i64 {
    let since = std::fs::metadata(path)
        .expect("stats")
        .modified()
        .expect("an mtime")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch");
    i64::try_from(since.as_nanos()).expect("fits")
}

/// A nested file is recorded where it is — its path under the folder, with the time it was read
/// at — its category mirrors the folders it sits in, and nothing is written anywhere: not beside
/// it, not into the store.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_nested_file_is_recorded_where_it_is_and_nothing_is_written(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Terrain/Rocks/cliff.stl",
        BRACKET_FIXTURE,
    );
    let before = snapshot(ingest_dir.path());
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    assert_eq!(
        handler
            .handle(&job_for("Terrain/Rocks/cliff.stl"))
            .await
            .expect("ingests"),
        Outcome::Ingested
    );

    assert_eq!(
        file_rows(&pool, "Terrain/Rocks/cliff.stl").await,
        [(
            "library".to_owned(),
            Some("Terrain/Rocks/cliff.stl".to_owned()),
            Some(mtime_ns(&ingest_dir.path().join("Terrain/Rocks/cliff.stl"))),
            true
        )]
    );
    assert_eq!(
        snapshot(ingest_dir.path()),
        before,
        "nothing under the folder changed"
    );
    assert!(
        all_files(blob_root.path()).is_empty(),
        "and nothing went into the store: {:?}",
        all_files(blob_root.path())
    );
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

/// (a) The whole of a scan — the walk, every file's ingest, a re-scan, and the owner changing a
/// file so the next scan records revision 2 — writes nothing under the folder. The only change in
/// the snapshot is the one the owner made.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_its_ingests_and_a_revision_write_nothing_under_the_folders(pool: PgPool) {
    whole_folder(&pool).await;
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Brackets/bracket-lp-1042-03.stl",
        BRACKET_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        "Gears/spur-gear-m2-20t-lp-5140-00.stl",
        GEAR_FIXTURE,
    );
    stage(
        ingest_dir.path(),
        "Gears/README.txt",
        b"Spur gears, module 2.",
    );
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let before = snapshot(ingest_dir.path());

    let paths = scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await;
    assert_eq!(paths.len(), 2, "{paths:?}");
    for path in &paths {
        assert_eq!(
            handler.handle(&job_for(path)).await.expect("ingests"),
            Outcome::Ingested
        );
        assert_eq!(
            handler.handle(&job_for(path)).await.expect("re-scans"),
            Outcome::Skipped
        );
    }
    assert_eq!(snapshot(ingest_dir.path()), before, "a scan wrote nothing");

    // The owner saves a new version over the gear, in place.
    let gear = "Gears/spur-gear-m2-20t-lp-5140-00.stl";
    std::fs::write(ingest_dir.path().join(gear), BRACKET_FIXTURE).expect("the owner's edit");
    let edited = snapshot(ingest_dir.path());
    assert_eq!(
        handler.handle(&job_for(gear)).await.expect("revises"),
        Outcome::Revised
    );
    assert_eq!(
        snapshot(ingest_dir.path()),
        edited,
        "a revision wrote nothing either"
    );

    let rows = file_rows(&pool, gear).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].1.as_deref(), rows[0].3), (Some(gear), true));
    assert_eq!(
        (rows[1].1.as_deref(), rows[1].3),
        (Some(gear), false),
        "the first revision's bytes were overwritten by the owner and are no longer on disk"
    );
    assert!(
        all_files(blob_root.path()).is_empty(),
        "and nothing went into the store"
    );
}

/// A `touch`, or the same bytes copied over the file, is a re-scan — and the recorded time follows
/// the file, so its download is not refused as "changed" with no scan able to clear it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_touched_file_is_skipped_and_its_recorded_time_follows_it(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");

    let path = ingest_dir.path().join(BRACKET);
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3_600);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("opens")
        .set_modified(later)
        .expect("touches");
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("re-scans"),
        Outcome::Skipped
    );
    assert_eq!(file_rows(&pool, BRACKET).await[0].2, Some(mtime_ns(&path)));
}

/// A library that was given one folder scans only there (A2: the folder is its one source).
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_with_its_own_folder_reads_only_there(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        ingest_dir.path(),
        "Workshop/Jigs/drill-guide-lp-6010-01.stl",
        BRACKET_FIXTURE,
    );
    stage(ingest_dir.path(), "Elsewhere/spur-gear.stl", GEAR_FIXTURE);
    let workshop = PgParts(pool.clone())
        .create_library("Workshop", "Workshop")
        .await
        .expect("a library with its own folder");
    let jobs = lapidary_db::PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue(workshop, &[JobPayload::ScanDirectory])
        .await
        .expect("enqueues the scan");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler
        .handle(&scan_job(batch, workshop))
        .await
        .expect("scans");
    // Since A2 a part is named by where it is in the models folder, whatever added it.
    assert_eq!(
        ingest_paths_in(&pool, batch).await,
        ["Workshop/Jigs/drill-guide-lp-6010-01.stl"]
    );
    handler
        .handle(&job_for_library(
            workshop,
            "Workshop/Jigs/drill-guide-lp-6010-01.stl",
        ))
        .await
        .expect("ingests");
    assert_eq!(
        file_rows(&pool, "Workshop/Jigs/drill-guide-lp-6010-01.stl").await[0]
            .1
            .as_deref(),
        Some("Workshop/Jigs/drill-guide-lp-6010-01.stl"),
        "where it is under the mount, which is also the part's name"
    );
}

fn inbox_job(path: &str, source_path: &str) -> JobRow {
    let payload = JobPayload::IngestInbox {
        path: path.to_owned(),
        source_path: source_path.to_owned(),
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

/// (e) An upload the api placed in the inbox is indexed from there, as the part the browser named,
/// and a later upload of the same part under another inbox name is its next revision — with the
/// earlier file still on disk, because nothing replaced it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_upload_in_the_inbox_is_indexed_from_there(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let inbox = blob_root.path().join("inbox");
    stage(
        &inbox,
        "uploads/brackets/bracket-lp-1042-03.stl",
        BRACKET_FIXTURE,
    );
    stage(
        &inbox,
        "uploads/brackets/bracket-lp-1042-03 (2).stl",
        GEAR_FIXTURE,
    );
    let handler = handler_over(
        &pool,
        Path::new("/nonexistent-ingest-dir"),
        blob_root.path(),
    );

    assert_eq!(
        handler
            .handle(&inbox_job(
                "uploads/brackets/bracket-lp-1042-03.stl",
                "brackets/bracket-lp-1042-03.stl"
            ))
            .await
            .expect("indexes"),
        Outcome::Ingested
    );
    assert_eq!(
        handler
            .handle(&inbox_job(
                "uploads/brackets/bracket-lp-1042-03 (2).stl",
                "brackets/bracket-lp-1042-03.stl"
            ))
            .await
            .expect("revises"),
        Outcome::Revised
    );
    let rows = file_rows(&pool, "brackets/bracket-lp-1042-03.stl").await;
    assert_eq!(
        rows.iter()
            .map(|(root, path, _, on_disk)| (root.as_str(), path.as_deref(), *on_disk))
            .collect::<Vec<_>>(),
        [
            (
                "inbox",
                Some("uploads/brackets/bracket-lp-1042-03 (2).stl"),
                true
            ),
            (
                "inbox",
                Some("uploads/brackets/bracket-lp-1042-03.stl"),
                true
            ),
        ]
    );
    let origin: String = sqlx::query_scalar(
        "SELECT r.origin FROM revision r JOIN part p ON p.id = r.part_id ORDER BY r.created_at LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("reads");
    assert_eq!(origin, "upload");

    match handler
        .handle(&inbox_job("../outside.stl", "outside.stl"))
        .await
    {
        Err(HandlerError::Permanent { .. }) => {}
        other => panic!("an inbox path that escapes is refused for good, got {other:?}"),
    }
}

/// A scan whose library holds a folder that escapes the mount, by a hand edit past the api's
/// refusal, is refused rather than walked.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_folder_that_escapes_the_mount_is_refused(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    sqlx::query(
        "INSERT INTO library_source (id, library_id, path, kind) VALUES (gen_random_uuid(), $1, '../etc', 'folder')",
    )
    .bind(seeded().as_uuid())
    .execute(&pool)
    .await
    .expect("a hand edit");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    match handler.handle(&scan_job(BatchId::new(), seeded())).await {
        Err(HandlerError::Permanent { .. }) => {}
        other => panic!("{other:?}"),
    }
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
/// *"every part appears"*. It did not: a mesh no picture could be made of failed the ingest job,
/// so no part row was written and the model was absent from the library entirely. Measured, on a
/// real corpus, at one file in 1,095. The clause about a thumbnail went with the renders (R3);
/// the clause that mattered is this one.
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

    // No derivative row, which since R3 is every part's state: the card shows the part's own
    // picture, or names its format where it has none.
    let derivatives: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative d JOIN revision r ON r.id = d.revision_id \
         JOIN part p ON p.id = r.part_id WHERE p.library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(derivatives, 0, "no derivative, rather than no part");
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

// ---- STEP and IGES, refused by name --------------------------------------------------

/// Real mesh bytes under a B-rep name, in every B-rep extension: the refusal is the
/// extension's and is decided before anything reads a byte, so a file that *would* have
/// parsed is refused all the same.
///
/// This case used to read the other way — "a worker built without `--features occt-kernel`
/// fails and says why" — and that is why the sentence matters more than the error kind here.
/// There is no feature to build in any more, so advice to rebuild would send somebody to a
/// flag that does not exist. `Permanent`, because the bytes are immutable and the answer is
/// the same every time.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn every_b_rep_format_is_refused_by_name_and_ingests_nothing(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    for format in lapidary_core::CAD_FORMATS {
        let path = format!("fixture-plate-lp-9000-00.{format}");
        std::fs::write(ingest_dir.path().join(&path), BRACKET_FIXTURE).expect("write");
        match handler.handle(&job_for(&path)).await {
            Err(HandlerError::Permanent { message }) => {
                assert!(
                    message.contains(&format.to_ascii_uppercase()),
                    "names the format it refused: {message}"
                );
                // The remedy, not only the refusal: deleting the advice and leaving "does not
                // read STEP files" must fail here.
                assert!(
                    message.contains("STL, OBJ or 3MF"),
                    "says what this library does take: {message}"
                );
                assert!(
                    !message.contains("occt") && !message.contains("--features"),
                    "and never sends somebody to a build flag that no longer exists: {message}"
                );
            }
            other => panic!("a {format} file must be refused permanently, got {other:?}"),
        }
    }
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(parts, 0, "and no part is added for any of them");
}

/// **The test R3 exists for: a worker start queues nothing for work nothing can do.**
///
/// Four mechanisms used to queue a render for ever, and all four ran from this one call
/// (`derive.rs`'s `enqueue_stale_derivatives`, which sweeps stale derivatives, stale shape
/// profiles and centreless revisions in turn). Seeded here with exactly the shapes that
/// triggered them:
///
/// - a `thumbnail` row, which `revisions_missing` would report missing for ever once nothing
///   fills one;
/// - `tessellation_l0`/`l1`/`l2`, `structure`, `entities` and `pmi` rows at a kernel version
///   no build reports, which the per-format `produce` ladder would re-queue for ever;
/// - a mesh revision with a volume and no centre of mass, which the centre backfill would
///   queue an L0 rebuild for at every start;
/// - a part with no L0 at all, which the shape sweep's inner join made invisible — the
///   silent one.
///
/// **Twice**, because once proves only that the first run drained something. The job table
/// must gain zero rows both times.
///
/// The last leg is the positive control, and it is not decoration: `enqueue_stale_derivatives`
/// logs and swallows every database error, so a sweep whose SQL no longer parses also queues
/// nothing. An export row at a stale version must still queue exactly one rebuild — without
/// that leg this test passes on a sweep that does nothing at all.
///
/// Written against `derivative.kind` and `job.kind` as strings rather than through
/// `DerivativeKind`, because the point is rows this build can no longer name: the strings are
/// in the database on every row ever written, and R4 is what deletes them.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_worker_starts_over_a_library_of_dead_derivatives_queue_nothing(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    handler.handle(&job_for(CARRIER)).await.expect("ingests");

    // Every dead kind, on every revision, at a version no build reports. Inline rather than
    // hash-addressed so the seed needs no `blob` rows: `derivative_storage_is_exclusive`
    // (0004) asks for exactly one of the two, and says nothing about which kind uses which.
    for kind in [
        "thumbnail",
        "tessellation_l0",
        "tessellation_l1",
        "tessellation_l2",
        "structure",
        "entities",
        "pmi",
    ] {
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
             SELECT uuidv7(), r.id, $1, '\\x00'::bytea, 'mesh stl-1+glb-1+cpu-1', '{}'::jsonb \
               FROM revision r \
             ON CONFLICT (revision_id, kind) DO UPDATE \
                SET kernel_version = 'mesh stl-1+glb-1+cpu-1'",
        )
        .bind(kind)
        .execute(&pool)
        .await
        .expect("seeds a derivative of a kind nothing builds any more");
    }
    // A mesh revision that has a volume and no centre of mass: what every revision read
    // before L3 recorded centres looks like.
    sqlx::query("UPDATE revision SET mass_props_json = NULL")
        .execute(&pool)
        .await
        .expect("takes the centres away");
    // And the silent one: **one** part whose latest revision has no L0 at all. One, not both:
    // deleting every L0 row would leave the stale-derivative ladder nothing to find, and a
    // render kind put back into it would then be caught by nothing — which is exactly what the
    // first version of this seed did (`mutate-r3.sh`'s `ladder-sweeps-a-rung-again` survived).
    sqlx::query(
        "DELETE FROM derivative d USING revision r, part p \
         WHERE d.revision_id = r.id AND r.part_id = p.id \
           AND d.kind = 'tessellation_l0' AND p.source_path = $1",
    )
    .bind(CARRIER)
    .execute(&pool)
    .await
    .expect("leaves one part with no viewer mesh");

    let queued = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM job")
            .fetch_one(&pool)
            .await
            .expect("counts the queue")
    };
    assert_eq!(queued(pool.clone()).await, 0, "the queue starts empty");
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        queued(pool.clone()).await,
        0,
        "a worker start queues nothing for derivatives nothing produces"
    );
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        queued(pool.clone()).await,
        0,
        "and the second start queues nothing either — a sweep that re-queues for ever is \
         what this asserts against"
    );

    // The positive control. An export is still built on demand, so an export row an older
    // writer left is still swept — and this is what proves the sweep ran at all.
    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
         SELECT uuidv7(), r.id, 'export_stl', '\\x00'::bytea, 'mesh stl-1+glb-1+cpu-1', '{}'::jsonb \
           FROM revision r JOIN part p ON p.id = r.part_id WHERE p.source_path = $1",
    )
    .bind(BRACKET)
    .execute(&pool)
    .await
    .expect("seeds a stale export");
    handler.enqueue_stale_derivatives().await;
    let swept: Vec<(String, serde_json::Value)> =
        sqlx::query_as("SELECT kind, payload FROM job ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("reads the queue");
    assert_eq!(
        swept.len(),
        1,
        "the sweep is alive: a stale export is queued, and exactly once — {swept:?}"
    );
    assert_eq!(swept[0].0, "derive");
    assert_eq!(swept[0].1["produce"], "export_stl");
    assert_ne!(
        lapidary_cad::kernel_version("stl"),
        "mesh stl-1+glb-1+cpu-1",
        "and the seed is stale against what this build reports"
    );
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        queued(pool.clone()).await,
        1,
        "a rebuild already queued is left to the job that holds it"
    );
}

// ---------------------------------------------------------------------------------------
// Phase 4 slice 1: a file whose bytes changed at a path the library already indexes.
// ---------------------------------------------------------------------------------------

const SPACER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/spacer-lp-2001-00.stl");

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

/// The whole round trip the slice exists for, as a scan sees it: the owner edits a file in
/// place, re-scans, and gets a second revision rather than "already here".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_changed_file_becomes_revision_two_beside_the_first(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );
    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    assert_eq!(
        handler.handle(&job_for(BRACKET)).await.expect("revises"),
        Outcome::Revised,
        "changed bytes are a revision, not a skip"
    );

    let rows = revision_rows(&pool).await;
    assert_eq!(rows.len(), 2, "two revisions of the one part: {rows:?}");
    assert_eq!(part_count(&pool).await, 1, "and still one part");
    assert_eq!(
        rows[0],
        (
            "1".to_owned(),
            None,
            "ingest".to_owned(),
            hex_of(BRACKET_FIXTURE),
            BRACKET.to_owned(),
        ),
        "revision 1 keeps its figures and its hash, at the path it was read from"
    );
    assert_eq!(
        rows[1],
        (
            "2".to_owned(),
            Some("1".to_owned()),
            "ingest".to_owned(),
            hex_of(SPACER_FIXTURE),
            BRACKET.to_owned(),
        ),
        "revision 2 is on top, at the path the owner already knew"
    );
    let on_disk: Vec<bool> = file_rows(&pool, BRACKET)
        .await
        .into_iter()
        .map(|row| row.3)
        .collect();
    assert_eq!(
        on_disk,
        [true, false],
        "the owner overwrote revision 1's bytes, and Lapidary kept no copy"
    );
    assert!(
        all_files(blob_root.path()).is_empty(),
        "nothing was set aside anywhere"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_bytes_again_are_skipped_not_revised(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
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
}

/// The two handles [`Pause`] wants, and a release that cannot hang the suite.
///
/// **Awaiting `reached` bare is how this test file hangs rather than fails.** If the pause ever
/// stops firing — deleted, or moved above the read it has to sit below — a bare
/// `reached.notified().await` waits for ever, under the machine-wide compile lock, with no
/// message. Ten seconds is far longer than an ingest of a 48-triangle bracket and short enough
/// to read the panic.
fn held() -> (Pause, impl std::future::Future<Output = ()>) {
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let pause = Pause {
        reached: reached.clone(),
        release: release.clone(),
    };
    let wait = async move {
        tokio::time::timeout(std::time::Duration::from_secs(10), reached.notified())
            .await
            .expect("the held job must reach the parse: WorkerHandler::pause never fired");
    };
    (pause, wait)
}

/// Two jobs, different bytes, one new path (slice 1 spec §3.3). The loser read no part there, and
/// the winner records one while the loser is in the kernel. The loser is decided again rather than
/// skipped, and its retry reads the winner's part and becomes its next revision.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_that_loses_a_new_path_to_other_bytes_is_decided_again(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let library = seeded();
    let winner_dir = tempfile::tempdir().expect("temp dir");
    let loser_dir = tempfile::tempdir().expect("temp dir");
    stage(winner_dir.path(), BRACKET, BRACKET_FIXTURE);
    stage(loser_dir.path(), BRACKET, GEAR_FIXTURE);
    let (pause, reached) = held();
    let release = pause.release.clone();
    let winner = handler_over(&pool, winner_dir.path(), blob_root.path());
    let loser = WorkerHandler {
        pause: Some(pause),
        sites: lapidary_sites::Sites::public(),
        ..handler_over(&pool, loser_dir.path(), blob_root.path())
    };
    let job = job_for_library(library, BRACKET);

    let (lost, ()) = tokio::join!(loser.handle(&job), async {
        reached.await;
        assert_eq!(
            winner.handle(&job).await.expect("the winner ingests"),
            Outcome::Ingested
        );
        release.notify_one();
    });
    let lost = lost.expect_err("the loser is not skipped");
    assert!(matches!(lost, HandlerError::Transient { .. }), "{lost:?}");

    let retry = handler_over(&pool, loser_dir.path(), blob_root.path());
    assert_eq!(
        retry.handle(&job).await.expect("the retry settles"),
        Outcome::Revised
    );

    let hashes: Vec<String> = revision_rows(&pool)
        .await
        .into_iter()
        .map(|row| row.3)
        .collect();
    assert_eq!(hashes, [hex_of(BRACKET_FIXTURE), hex_of(GEAR_FIXTURE)]);
}

/// Two jobs, the same bytes, one new path: the loser's insert loses to the winner's, and it is the
/// same file, so it settles as skipped.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_that_loses_a_new_path_to_the_same_bytes_is_skipped(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let winner_dir = tempfile::tempdir().expect("temp dir");
    let loser_dir = tempfile::tempdir().expect("temp dir");
    stage(winner_dir.path(), BRACKET, BRACKET_FIXTURE);
    stage(loser_dir.path(), BRACKET, BRACKET_FIXTURE);
    let (pause, reached) = held();
    let release = pause.release.clone();
    let winner = handler_over(&pool, winner_dir.path(), blob_root.path());
    let loser = WorkerHandler {
        pause: Some(pause),
        sites: lapidary_sites::Sites::public(),
        ..handler_over(&pool, loser_dir.path(), blob_root.path())
    };
    let job = job_for(BRACKET);

    let (lost, ()) = tokio::join!(loser.handle(&job), async {
        reached.await;
        assert_eq!(
            winner.handle(&job).await.expect("the winner ingests"),
            Outcome::Ingested
        );
        release.notify_one();
    });
    assert_eq!(lost.expect("the loser settles"), Outcome::Skipped);

    assert_eq!(revision_rows(&pool).await.len(), 1);
    assert!(
        all_files(blob_root.path()).is_empty(),
        "nothing was written by either"
    );
}

/// (b) Delete never touches a file: the owner's folder is exactly as it was after the delete and
/// after a sweep with no retention at all.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_revised_part_leaves_every_file_and_directory_in_place(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    handler.handle(&job_for(BRACKET)).await.expect("revises");
    let before = snapshot(ingest_dir.path());

    let part = sqlx::query_scalar::<_, Uuid>("SELECT id FROM part")
        .fetch_one(&pool)
        .await
        .expect("the one part");
    let parts = PgParts(pool.clone());
    let part = lapidary_core::PartId::from_uuid(part);
    assert!(parts.delete(part).await.expect("delete").is_some());
    lapidary_ingest::reap::sweep(&pool, blob_root.path(), std::time::Duration::ZERO)
        .await
        .expect("sweep");

    assert_eq!(
        snapshot(ingest_dir.path()),
        before,
        "the owner's folder is as it was"
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
    stage(
        &blob_root.path().join("inbox"),
        "uploads/brackets/spare/LP-1042-03.stl",
        BRACKET_FIXTURE,
    );
    handler
        .handle(&inbox_job(
            "uploads/brackets/spare/LP-1042-03.stl",
            "brackets/spare/LP-1042-03.stl",
        ))
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

/// What a manifest says about a part besides its files. `download.rs` fills these from the part's
/// own row and its `part_source` rows, so a bundle carrying them is what an export really looks
/// like.
#[derive(Default, Clone)]
struct PartMeta {
    part_number: Option<String>,
    tags: Vec<String>,
    sources: Vec<lapidary_targets::bundle::ManifestSource>,
}

/// A bundle as `download.rs` writes one: each part's revisions oldest first, the current one at its
/// source path and the earlier ones under `revisions/<label>/`, then the manifest.
fn bundle_of(parts: &[(&str, &[&[u8]])]) -> Vec<u8> {
    let described: Vec<(&str, &[&[u8]], PartMeta)> = parts
        .iter()
        .map(|(source_path, versions)| (*source_path, *versions, PartMeta::default()))
        .collect();
    bundle_describing(&described)
}

/// The same bundle, carrying what a person typed about each part as well as its bytes.
fn bundle_describing(parts: &[(&str, &[&[u8]], PartMeta)]) -> Vec<u8> {
    use lapidary_targets::bundle::{
        self, Manifest, ManifestLibrary, ManifestPart, ManifestRevision, StoreZip,
    };
    let mut zip = StoreZip::new(Vec::new());
    let mut manifest_parts = Vec::new();
    for (source_path, versions, meta) in parts {
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
            part_number: meta.part_number.clone(),
            source_path: (*source_path).to_owned(),
            tags: meta.tags.clone(),
            sources: meta.sources.clone(),
            revisions,
        });
    }
    let manifest = serde_json::to_vec(&Manifest {
        format: bundle::FORMAT.to_owned(),
        version: bundle::VERSION,
        library: ManifestLibrary {
            name: "Workshop".to_owned(),
        },
        parts: manifest_parts,
    })
    .expect("serialises");
    zip.add(bundle::MANIFEST, &mut manifest.as_slice())
        .expect("adds");
    zip.finish().expect("finishes")
}

/// A bundle stored as the api's import route stores one: in Lapidary's own store under its hash,
/// with a `blob` row that nothing references yet.
async fn upload_into(pool: &PgPool, blob_root: &Path, bytes: &[u8]) -> BlobHash {
    let stored = lapidary_storage::DerivativeStore::open(blob_root)
        .put(bytes)
        .expect("stores the bundle");
    lapidary_db::PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash: stored.hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        })
        .await
        .expect("records the bundle");
    stored.hash
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

/// Phase 4 slice 2 spec §7: a library replays every revision, so labels, parents and
/// origins survive; importing the same bundle again finds every part already there.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_import_keeps_each_parts_labels_parents_and_origins(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
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

/// A bundle carries what a person typed as well as the bytes: the part number, the tags, and each
/// source's licence. Replaying only the revisions loses all three, and the licence is exactly what a
/// part has to arrive with when it came from somebody else's library.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_imported_part_arrives_with_its_number_tags_and_source_licence(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let path = "brackets/bracket-lp-1042-03.stl";
    let bundle = bundle_describing(&[(
        path,
        &[BRACKET_FIXTURE],
        PartMeta {
            part_number: Some("LP-1042-03".to_owned()),
            tags: vec!["bracket".to_owned(), "corner".to_owned()],
            sources: vec![lapidary_targets::bundle::ManifestSource {
                url: Some("https://www.printables.com/model/412903-corner-bracket".to_owned()),
                vendor: Some("Printables".to_owned()),
                external_id: Some("412903".to_owned()),
                title: Some("Corner bracket, 40 mm".to_owned()),
                license: Some("CC-BY-4.0".to_owned()),
            }],
        },
    )]);

    let outcomes = import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports");
    assert_eq!(outcomes, [lapidary_core::Outcome::Ingested]);

    let part = lapidary_db::PgRevisions(pool.clone())
        .current(seeded(), path)
        .await
        .expect("the imported part reads back")
        .expect("a part at the bundle's path")
        .part;
    let (number, tags): (Option<String>, Vec<String>) =
        sqlx::query_as("SELECT part_number, tags FROM part WHERE id = $1")
            .bind(part.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("the part row");
    assert_eq!(number.as_deref(), Some("LP-1042-03"), "the part number");
    assert_eq!(tags, ["bracket", "corner"], "the tags");

    let sources = PgParts(pool.clone())
        .part_sources(part)
        .await
        .expect("its sources");
    assert_eq!(sources.len(), 1, "the one source the manifest holds");
    assert_eq!(
        sources[0].license.as_deref(),
        Some("CC-BY-4.0"),
        "the licence it arrived under"
    );
    assert_eq!(sources[0].vendor.as_deref(), Some("Printables"));
    assert_eq!(
        sources[0].url.as_deref(),
        Some("https://www.printables.com/model/412903-corner-bracket")
    );
}

/// A second import must not quietly overwrite what was typed here since. The bundle's values are
/// older than this library's own, so a part it already holds keeps the tags it was given here.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_import_leaves_a_parts_own_tags_alone(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let path = "brackets/bracket-lp-1042-03.stl";
    let bundle = bundle_describing(&[(
        path,
        &[BRACKET_FIXTURE],
        PartMeta {
            part_number: Some("LP-1042-03".to_owned()),
            tags: vec!["bracket".to_owned()],
            sources: vec![],
        },
    )]);
    import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports");
    let part = lapidary_db::PgRevisions(pool.clone())
        .current(seeded(), path)
        .await
        .expect("the imported part reads back")
        .expect("a part at the bundle's path")
        .part;
    PgParts(pool.clone())
        .set_tags(part, &["corner".to_owned(), "printed".to_owned()])
        .await
        .expect("the tags typed here");

    let again = import(&handler, &pool, blob_root.path(), &bundle)
        .await
        .expect("imports again");
    assert_eq!(again, [lapidary_core::Outcome::Skipped]);
    let (_, tags): (Option<String>, Vec<String>) =
        sqlx::query_as("SELECT part_number, tags FROM part WHERE id = $1")
            .bind(part.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("the part row");
    assert_eq!(
        tags,
        ["corner", "printed"],
        "what was typed in this library stays"
    );
}

/// A part already holding a file that is none of the bundle's revisions is another part's history.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_part_landing_on_another_parts_history_is_refused(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
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

// ---------------------------------------------------------------------------------------
// F1: a 3MF's own thumbnail becomes the part's picture, when the part has none.
// ---------------------------------------------------------------------------------------

/// A real picture of a real part, from the fixtures: a WebP, which `normalize` reads like a PNG.
const THUMB_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.thumb.webp");

/// The carrier fixture, with a thumbnail relationship and the picture it names added, the way a
/// slicer saves one. `None` leaves the package exactly as it was.
fn carrier_with_thumbnail(thumbnail: Option<&[u8]>) -> Vec<u8> {
    use std::io::{Read as _, Write as _};
    let Some(thumbnail) = thumbnail else {
        return CARRIER_FIXTURE.to_vec();
    };
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(CARRIER_FIXTURE)).expect("a zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).expect("an entry");
        let name = entry.name().expect("a name").into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("reads");
        if name == "_rels/.rels" {
            let rels = String::from_utf8(bytes).expect("utf-8");
            bytes = rels
                .replace(
                    "</Relationships>",
                    r#"<Relationship Id="thumb" Target="/Metadata/thumbnail.png" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"/></Relationships>"#,
                )
                .into_bytes();
        }
        out.start_file(name, opts).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.start_file("Metadata/thumbnail.png", opts)
        .expect("start");
    out.write_all(thumbnail).expect("write");
    out.finish().expect("finish").into_inner()
}

async fn pictures_of(pool: &PgPool, source_path: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT i.origin FROM part_image i JOIN part p ON p.id = i.part_id \
         WHERE p.source_path = $1 ORDER BY i.position",
    )
    .bind(source_path)
    .fetch_all(pool)
    .await
    .expect("reads the gallery")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_3mf_carrying_a_thumbnail_arrives_with_it_as_its_picture(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(
        ingest_dir.path(),
        CARRIER,
        &carrier_with_thumbnail(Some(THUMB_FIXTURE)),
    );
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("ingests"),
        Outcome::Ingested
    );
    assert_eq!(pictures_of(&pool, CARRIER).await, ["embedded"]);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_3mf_without_a_thumbnail_or_with_a_broken_one_ingests_with_no_picture(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), CARRIER, &carrier_with_thumbnail(None));
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("ingests"),
        Outcome::Ingested
    );
    assert!(pictures_of(&pool, CARRIER).await.is_empty());

    // Half a picture: the file still ingests, and a broken thumbnail is no reason to fail it.
    let other = "Terrain/planetary-carrier-truncated.3mf";
    stage(
        ingest_dir.path(),
        other,
        &carrier_with_thumbnail(Some(&THUMB_FIXTURE[..THUMB_FIXTURE.len() / 2])),
    );
    assert_eq!(
        handler.handle(&job_for(other)).await.expect("ingests"),
        Outcome::Ingested
    );
    assert!(pictures_of(&pool, other).await.is_empty());
}

/// A person's picture always wins: a revised 3MF that now carries a thumbnail adds nothing to a
/// part somebody gave a picture.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_thumbnail_never_replaces_a_picture_somebody_chose(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), CARRIER, &carrier_with_thumbnail(None));
    handler.handle(&job_for(CARRIER)).await.expect("ingests");
    let part: uuid::Uuid = sqlx::query_scalar("SELECT id FROM part WHERE source_path = $1")
        .bind(CARRIER)
        .fetch_one(&pool)
        .await
        .expect("the part");
    lapidary_db::PgParts(pool.clone())
        .add_part_image(
            lapidary_core::PartId::from_uuid(part),
            lapidary_db::NewPartImage {
                bytes: lapidary_db::ImageBytes::Inline(THUMB_FIXTURE),
                origin: "uploaded",
                source_url: None,
            },
        )
        .await
        .expect("a person's picture");

    stage(
        ingest_dir.path(),
        CARRIER,
        &carrier_with_thumbnail(Some(THUMB_FIXTURE)),
    );
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("revises"),
        Outcome::Revised
    );
    assert_eq!(pictures_of(&pool, CARRIER).await, ["uploaded"]);
}

/// An export is never built from a file changed in its folder since it was indexed: the bytes no
/// longer hash to the revision's, and the job says to scan again rather than export something else.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_export_of_a_file_changed_in_its_folder_is_refused(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(BRACKET)).await.expect("ingests");
    stage(ingest_dir.path(), BRACKET, GEAR_FIXTURE);
    let revision = only_revision(&pool).await;
    match handler
        .handle(&derive_job(revision, DerivativeKind::ExportStl))
        .await
    {
        Err(HandlerError::Permanent { message }) => {
            assert!(message.contains("Scan again"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    assert!(derivatives(&pool).await.is_empty());
}

/// (A2) A scan walks what the library was given — a folder, a single file — and never `_removed/`,
/// where a removed model's file is moved, so a removed model does not come back.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_walks_what_was_added_and_never_what_was_removed(pool: PgPool) {
    let models = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        models.path(),
        "prints/bracket-lp-1042-03.stl",
        BRACKET_FIXTURE,
    );
    stage(
        models.path(),
        "gears/spur-gear-m2-20t-lp-5140-00.stl",
        GEAR_FIXTURE,
    );
    stage(
        models.path(),
        "gears/spare/spur-gear-copy.stl",
        GEAR_FIXTURE,
    );
    stage(
        models.path(),
        "_removed/lena/prints/old-bracket.stl",
        BRACKET_FIXTURE,
    );
    let sources = lapidary_db::PgSources(pool.clone());
    sources
        .add(seeded(), "prints", "folder", None, false)
        .await
        .expect("a folder");
    sources
        .add(
            seeded(),
            "gears/spur-gear-m2-20t-lp-5140-00.stl",
            "file",
            None,
            false,
        )
        .await
        .expect("one file of another folder");
    let handler = handler_over(&pool, models.path(), blob_root.path());
    let (batch, _) = lapidary_db::PgJobs(pool.clone())
        .enqueue(seeded(), &[JobPayload::ScanDirectory])
        .await
        .expect("enqueues");
    handler
        .handle(&scan_job(batch, seeded()))
        .await
        .expect("scans");
    assert_eq!(
        ingest_paths_in(&pool, batch).await,
        [
            "gears/spur-gear-m2-20t-lp-5140-00.stl",
            "prints/bracket-lp-1042-03.stl"
        ]
    );

    // The whole folder: everything, still never `_removed/`.
    sources
        .add(seeded(), "", "folder", None, false)
        .await
        .expect("the whole folder");
    let (batch, _) = lapidary_db::PgJobs(pool.clone())
        .enqueue(seeded(), &[JobPayload::ScanDirectory])
        .await
        .expect("enqueues");
    handler
        .handle(&scan_job(batch, seeded()))
        .await
        .expect("scans");
    let found = ingest_paths_in(&pool, batch).await;
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found.iter().all(|path| !path.starts_with("_removed")),
        "{found:?}"
    );
}

/// (A2) The pictures a person chose while adding a folder go to each part found in it, once: filing
/// the same file again gives it nothing twice.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_gets_the_pictures_its_folder_was_added_with_once(pool: PgPool) {
    let models = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    stage(
        models.path(),
        "prints/bracket-lp-1042-03.stl",
        BRACKET_FIXTURE,
    );
    let sources = lapidary_db::PgSources(pool.clone());
    let source = sources
        .add(seeded(), "prints", "folder", None, false)
        .await
        .expect("a folder");
    let webp = lapidary_storage::images::normalize(include_bytes!(
        "../../../fixtures/bracket-lp-1042-03.thumb.webp"
    ))
    .expect("normalizes")
    .webp;
    sources
        .add_image(source.id, &webp)
        .await
        .expect("a picture");
    let handler = handler_over(&pool, models.path(), blob_root.path());
    let job = job_for_library(seeded(), "prints/bracket-lp-1042-03.stl");
    assert_eq!(
        handler.handle(&job).await.expect("files"),
        Outcome::Ingested
    );
    let gallery = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT i.origin FROM part_image i JOIN part p ON p.id = i.part_id WHERE p.source_path = $1",
        )
        .bind("prints/bracket-lp-1042-03.stl")
        .fetch_all(&pool)
        .await
        .expect("gallery")
    };
    assert_eq!(gallery().await, ["uploaded"]);
    // Changed bytes at the same path: a revision, and still one picture.
    stage(models.path(), "prints/bracket-lp-1042-03.stl", GEAR_FIXTURE);
    assert_eq!(handler.handle(&job).await.expect("files"), Outcome::Revised);
    assert_eq!(gallery().await, ["uploaded"]);
}

/// (A10) An uploaded zip is kept, its models are unpacked beside it and queued into the upload's
/// batch, and each is indexed as the part `<archive>/<path inside>`; a RAR is kept as a pack (A13).
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_zip_unpacks_its_models_into_the_batch_and_a_rar_is_kept_as_a_pack(
    pool: PgPool,
) {
    use std::io::Write;
    let blob_root = tempfile::tempdir().expect("temp dir");
    let inbox = blob_root.path().join("inbox");
    std::fs::create_dir_all(inbox.join("uploads/ayse")).expect("uploads");
    let mut zip = zip::ZipWriter::new(
        std::fs::File::create(inbox.join("uploads/ayse/brackets.zip")).expect("zip"),
    );
    for (name, bytes) in [
        (format!("lp-1042/{BRACKET}"), BRACKET_FIXTURE),
        (CARRIER.to_owned(), CARRIER_FIXTURE),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(bytes).expect("bytes");
    }
    zip.finish().expect("zip written");
    let upload = JobPayload::IngestInbox {
        path: "uploads/ayse/brackets.zip".to_owned(),
        source_path: "brackets.zip".to_owned(),
    };
    let (batch, _) = lapidary_db::PgJobs(pool.clone())
        .enqueue(seeded(), std::slice::from_ref(&upload))
        .await
        .expect("enqueues the upload");
    let row = |payload: &JobPayload| JobRow {
        id: JobId::new(),
        batch_id: batch,
        library_id: seeded(),
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    };
    let handler = handler_over(
        &pool,
        Path::new("/nonexistent-ingest-dir"),
        blob_root.path(),
    );
    assert_eq!(
        handler.handle(&row(&upload)).await.expect("unpacks"),
        Outcome::Scanned
    );
    assert!(
        inbox.join("uploads/ayse/brackets.zip").exists(),
        "the archive is kept"
    );

    let queued: Vec<(String, String)> = sqlx::query_as(
        "SELECT payload ->> 'inbox', payload ->> 'path' FROM job \
         WHERE batch_id = $1 AND kind = 'ingest_inbox' AND payload ->> 'inbox' <> 'uploads/ayse/brackets.zip' \
         ORDER BY created_at, id",
    )
    .bind(batch.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("reads the queued jobs");
    assert_eq!(
        queued,
        [
            (
                format!("uploads/ayse/brackets/lp-1042/{BRACKET}"),
                format!("brackets/lp-1042/{BRACKET}")
            ),
            (
                format!("uploads/ayse/brackets/{CARRIER}"),
                format!("brackets/{CARRIER}")
            ),
        ]
    );
    for (path, source_path) in &queued {
        let child = JobPayload::IngestInbox {
            path: path.clone(),
            source_path: source_path.clone(),
        };
        assert_eq!(
            handler.handle(&row(&child)).await.expect("indexes"),
            Outcome::Ingested
        );
    }
    assert_eq!(part_count(&pool).await, 2);

    stage(&inbox, "uploads/ayse/spares.rar", b"Rar!");
    let rar = JobPayload::IngestInbox {
        path: "uploads/ayse/spares.rar".to_owned(),
        source_path: "spares.rar".to_owned(),
    };
    // A13: a RAR can't be opened, so it is kept as a pack rather than refused.
    assert_eq!(
        handler.handle(&row(&rar)).await.expect("kept as a pack"),
        Outcome::Ingested
    );
    assert_eq!(part_count(&pool).await, 3);
}

/// (A13) A scan takes a creator's archives as packs: one part each, filed under their folder, with no
/// figures and the contents its index lists. A rescan of an unchanged pack reads nothing; an archive under
/// `uploads/` is not a pack, because its upload already unpacked it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_keeps_archives_packed_as_one_part_each(pool: PgPool) {
    use std::io::Write;
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let folder = "Creators/Bite The Bullet/Bleeding Procession";
    std::fs::create_dir_all(ingest_dir.path().join(folder)).expect("folder");
    let mut zip = zip::ZipWriter::new(
        std::fs::File::create(
            ingest_dir
                .path()
                .join(format!("{folder}/TCAA3002 Castigator.zip")),
        )
        .expect("zip"),
    );
    for (name, bytes) in [
        ("Supported/castigator-body.stl", BRACKET_FIXTURE),
        ("Unsupported/castigator-head.stl", BRACKET_FIXTURE),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(bytes).expect("bytes");
    }
    zip.finish().expect("zip written");
    stage(
        ingest_dir.path(),
        &format!("{folder}/TCAA3002 Trench Pilgrims.rar"),
        b"Rar!\x1a\x07\x00",
    );
    stage(ingest_dir.path(), "uploads/ayse/spares.zip", b"PK");
    whole_folder(&pool).await;
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    let scanned = scanned_paths(&pool, ingest_dir.path(), blob_root.path()).await;
    assert_eq!(
        scanned,
        [
            format!("{folder}/TCAA3002 Castigator.zip"),
            format!("{folder}/TCAA3002 Trench Pilgrims.rar")
        ]
    );
    for path in &scanned {
        assert_eq!(
            handler.handle(&job_for(path)).await.expect("indexed"),
            Outcome::Ingested
        );
    }
    let (format, bbox, triangles, contents): (
        String,
        Option<f64>,
        Option<i32>,
        Option<serde_json::Value>,
    ) = sqlx::query_as(
        "SELECT f.format, r.bbox_x, r.triangle_count, r.contents FROM part p \
             JOIN revision r ON r.part_id = p.id JOIN file f ON f.revision_id = r.id \
             WHERE p.source_path LIKE '%Castigator.zip'",
    )
    .fetch_one(&pool)
    .await
    .expect("the pack's rows");
    assert_eq!(format, "zip");
    assert_eq!((bbox, triangles), (None, None), "a pack has no figures");
    let contents = contents.expect("its contents");
    assert_eq!(contents["total"], 2);
    assert_eq!(
        contents["entries"][0]["path"],
        "Supported/castigator-body.stl"
    );
    let rar: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT r.contents FROM part p JOIN revision r ON r.part_id = p.id WHERE p.source_path LIKE '%.rar'",
    )
    .fetch_one(&pool)
    .await
    .expect("the rar's row");
    assert!(
        rar.expect("a sentence")["unread"]
            .as_str()
            .is_some_and(|s| s.contains("RAR"))
    );
    // Filed under its folder, as a model is.
    assert!(
        file_rows(&pool, &format!("{folder}/TCAA3002 Castigator.zip")).await[0].3,
        "on disk"
    );
    // Unchanged, a rescan reads nothing and settles as already here.
    assert_eq!(
        handler.handle(&job_for(&scanned[0])).await.expect("again"),
        Outcome::Skipped
    );
}
