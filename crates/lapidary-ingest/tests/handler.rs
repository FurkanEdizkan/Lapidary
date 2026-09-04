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
        2,
        "one copy of the source bytes, plus one rung: every rung of a 20-triangle bracket \
         clusters to the same mesh, so the ladder is one blob with three references"
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
    // Source bytes once, plus the ladder: both parts are the same mesh, so their rungs
    // are the same bytes too.
    assert_eq!(all_files(&blob_root.path().join("blobs")).len(), 2);
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

    // The mutation this pins: delete `source.remove(&hash)`, or the `reap` beside it, from
    // the record() error arm and a file survives on disk, failing the next assertion. The
    // returned error looks identical either way, which is why this checks the filesystem,
    // not the message.
    //
    // Slice 3 widened what "the blob" means here. Four writes now precede the failed
    // transaction -- the source and three rungs -- and the successful path above shows
    // they really are written, so an empty tree is the ladder being reaped as well.
    let orphans = all_files(&blob_root.path().join("blobs"));
    assert!(
        orphans.is_empty(),
        "expected no orphaned blob or rung under {}, found {orphans:?}",
        blob_root.path().display()
    );
    assert_eq!(part_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failed_link_to_existing_bytes_leaves_the_first_parts_blobs_alone(pool: PgPool) {
    // The other half of the reap, and the dangerous half. The link_existing branch writes
    // no source blob, so it must not reap one -- and its rungs are usually bytes some
    // earlier revision already stores, so reaping those would delete a part that ingested
    // perfectly well. A reap keyed on "this job wrote it" rather than "this job's
    // transaction failed" is what stops that.
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
    let after_first = all_files(&blob_root.path().join("blobs"));
    assert_eq!(
        after_first.len(),
        2,
        "the source blob and the ladder's one rung"
    );

    // Same bytes, so `blobs.exists` sends this down link_existing; a library that is not
    // a row fails the part insert after the rungs have been written.
    let nonexistent = LibraryId::from_uuid(
        Uuid::parse_str("01931b6e-0000-7000-8000-000000000099").expect("parses"),
    );
    handler
        .handle(&job_for_library(nonexistent, MIRRORED))
        .await
        .expect_err("a part row against a library that does not exist cannot be written");

    assert_eq!(
        all_files(&blob_root.path().join("blobs")),
        after_first,
        "the failed second ingest must leave the first part's bytes exactly as they were"
    );
    assert_eq!(part_count(&pool).await, 1, "the first part is still there");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_stl_writes_three_tessellation_blobs_and_rows(pool: PgPool) {
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
    assert_eq!(
        kinds,
        vec![
            "tessellation_l0",
            "tessellation_l1",
            "tessellation_l2",
            "thumbnail"
        ]
    );

    // Every rung row points at bytes that are really on disk, through a blob row that
    // really exists -- the whole chain migration 0004's foreign key exists to require.
    let hashes: Vec<String> = sqlx::query_scalar(
        "SELECT d.blake3 FROM derivative d JOIN blob b ON b.blake3 = d.blake3 \
         WHERE d.kind LIKE 'tessellation%'",
    )
    .fetch_all(&pool)
    .await
    .expect("hashes");
    assert_eq!(hashes.len(), 3, "three rungs, each with a blob row");
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
        vec![
            "tessellation_l0",
            "tessellation_l1",
            "tessellation_l2",
            "thumbnail"
        ],
        "an OBJ produces the same four derivatives an STL does"
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
        vec!["mesh obj-1+glb-1+cpu-1", "mesh stl-1+glb-1+cpu-1"]
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

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_3mf_yields_a_thumbnail_and_three_rungs(pool: PgPool) {
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
        vec![
            "tessellation_l0",
            "tessellation_l1",
            "tessellation_l2",
            "thumbnail"
        ],
        "a 3MF produces the same four derivatives an STL does"
    );

    let (format, version): (String, String) = sqlx::query_as(
        "SELECT f.format, d.kernel_version FROM file f \
         JOIN derivative d ON d.revision_id = f.revision_id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(format, "3mf");
    assert_eq!(version, "mesh 3mf-1+glb-1+cpu-1");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_3mf_source_blob_is_stored_uncompressed(pool: PgPool) {
    // DATA.md §1.2: 3MF is already a deflate ZIP. Re-compressing it spends CPU on every
    // ingest to make the file very slightly larger.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write stl");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(CARRIER)).await.expect("3mf");
    handler.handle(&job_for(BRACKET)).await.expect("stl");

    let rows: Vec<(String, i64, i64, Option<i16>)> = sqlx::query_as(
        "SELECT f.format, b.size_bytes, b.stored_bytes, b.zstd_level \
         FROM blob b JOIN file f ON f.blake3 = b.blake3 ORDER BY f.format",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    let three_mf = rows.iter().find(|r| r.0 == "3mf").expect("the 3mf row");
    assert_eq!(three_mf.1, three_mf.2, "a 3MF is stored at its own size");
    // And the STL beside it still compresses, so this proves a policy rather than a
    // pipeline that stopped compressing everything.
    let stl = rows.iter().find(|r| r.0 == "stl").expect("the stl row");
    assert!(
        stl.2 < stl.1,
        "an STL still compresses: {} vs {}",
        stl.2,
        stl.1
    );
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
