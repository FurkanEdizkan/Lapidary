//! The handler, exercised the way the worker exercises it.

use lapidary_core::{BatchId, BlobHash, JobId, LibraryId, MeshMeasurements, Outcome};
use lapidary_db::{IngestRequest, JobRow, PgIngest, StoredBlobRow};
use lapidary_ingest::IngestHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &str = "bracket-lp-1042-03.stl";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

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

fn handler_over(pool: &PgPool, ingest_dir: &Path, blob_root: &Path) -> IngestHandler {
    IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
    }
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

async fn blob_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// The `ref_count` on the one blob these tests store. Summed rather than fetched so the
/// query still says something if a second blob row ever appears.
async fn ref_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT coalesce(sum(ref_count), 0)::bigint FROM blob")
        .fetch_one(pool)
        .await
        .expect("sum query")
}

/// A second library to ingest the same file into. Migration `0002_parts.sql` seeds one,
/// and there is still no library-creation route, so the row is inserted directly.
async fn second_library(pool: &PgPool) -> LibraryId {
    const ID: &str = "01931b6e-0000-7000-8000-0000000000a2";
    sqlx::query("INSERT INTO library (id, name) VALUES ($1::uuid, 'Fixture jigs')")
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

    let handler = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
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

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_file_twice_is_skipped_the_second_time(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::copy(
        format!("{}/../../fixtures/{BRACKET}", env!("CARGO_MANIFEST_DIR")),
        ingest_dir.path().join(BRACKET),
    )
    .expect("stages the fixture");

    let handler = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
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

    let handler = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
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

    let handler_a = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
    };
    let handler_b = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
    };

    let job_a = job_for(BRACKET);
    let job_b = job_for(BRACKET);
    let (first, second) = tokio::join!(handler_a.handle(&job_a), handler_b.handle(&job_b));
    let outcomes = [
        first.expect("one succeeds"),
        second.expect("the other does too"),
    ];
    assert!(outcomes.contains(&Outcome::Ingested));
    assert!(outcomes.contains(&Outcome::Skipped));

    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(seeded().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(parts, 1, "the race must not produce two parts");
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
            library: seeded(),
            name: "notes",
            blob: &blob,
            measurements: &measurements,
            thumbnail_webp: &[0x52, 0x49, 0x46, 0x46],
            kernel_version: "mesh stl-1+cpu-1",
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

    // And the bytes are stored exactly once: reuse is the point of content addressing,
    // and it is what makes the second library cost a row rather than a copy.
    assert_eq!(blob_rows(&pool).await, 1, "one blob row, not two");
    assert_eq!(
        ref_count(&pool).await,
        2,
        "one reference per part row, and there are now two"
    );
    assert_eq!(
        all_files(&blob_root.path().join("blobs")).len(),
        1,
        "one copy of the bytes on disk"
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
    assert_eq!(all_files(&blob_root.path().join("blobs")).len(), 1);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failure_after_the_blob_write_leaves_no_orphan_blob_on_disk(pool: PgPool) {
    // Moved from tests/scan.rs. The Node prototype wrote its blob and then failed the
    // insert with no cleanup -- docs/prototype-notes.md records it -- so this guards a
    // bug that actually shipped rather than a hypothetical one.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write fixture");

    // Syntactically a library id, but not a row in `library` -- the part insert's foreign
    // key fails inside PgIngest::record, after step 5 (source.put) has already written
    // the blob to blob_root. That is what puts the failure after the write instead of
    // before it, which is the only way to exercise the reap at all.
    let nonexistent = LibraryId::from_uuid(
        Uuid::parse_str("01931b6e-0000-7000-8000-000000000099").expect("parses"),
    );

    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler
        .handle(&job_for_library(nonexistent, BRACKET))
        .await
        .expect_err("a part row against a library that does not exist cannot be written");

    // The mutation this pins: delete `source.remove(&hash)` from the record() error arm
    // and the file survives on disk, failing the next assertion. The returned error looks
    // identical either way, which is why this checks the filesystem, not the message.
    let orphans = all_files(&blob_root.path().join("blobs"));
    assert!(
        orphans.is_empty(),
        "expected no orphaned blob under {}, found {orphans:?}",
        blob_root.path().display()
    );
    assert_eq!(part_count(&pool).await, 0);
}
