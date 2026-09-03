//! The handler, exercised the way the worker exercises it.

use lapidary_core::{BatchId, JobId, LibraryId, Outcome};
use lapidary_db::JobRow;
use lapidary_ingest::IngestHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
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
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: seeded(),
        kind: "ingest_file".to_owned(),
        payload: serde_json::json!({ "path": file }),
        attempts: 1,
        max_attempts: 3,
    }
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
    // failure, same as `tests/scan.rs`'s truncated-STL case.
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
