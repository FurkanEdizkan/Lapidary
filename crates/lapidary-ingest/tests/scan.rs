//! Task 9: the scan endpoint, now living in its own crate (fix round 1 — see
//! crates/lapidary-ingest/src/lib.rs's module doc for why). Exercises the whole
//! pipeline end to end — walking a `TempDir` standing in for the read-only ingest
//! mount, and a `TempDir` blob root standing in for the real volume Task 12 wires up —
//! against a live, migrated Postgres (via `sqlx::test`), through this crate's router.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use image::ImageFormat;
use lapidary_core::{BlobHash, MeshMeasurements};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
use lapidary_ingest::{AppState, router};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

/// What `fixtures/bracket-lp-1042-03.stl` *is*, established by reading its bytes with a
/// from-scratch binary-STL reader rather than by asking this workspace's parser: the
/// 80-byte header is followed by a little-endian facet count of 20, and each of the 20
/// 50-byte records holds three vertices spanning 88 x 40 x 25 mm. Every one of its 30
/// undirected edges is shared by exactly two facets, so the mesh is closed.
///
/// These constants exist so the assertions below describe the fixture, not whatever the
/// pipeline happened to store. A zeroed `MeshMeasurements` — bbox 0x0x0, 0 triangles,
/// not watertight — is exactly what reaches the database if `IngestRequest.measurements`
/// stops carrying the kernel's output, and it fails every one of them.
const BRACKET_TRIANGLES: i32 = 20;
const BRACKET_BBOX_MM: [f64; 3] = [88.0, 40.0, 25.0];
/// `part.name` for that fixture: the file stem, not the file name. Pins `part_name` —
/// binding the raw `file_name` instead stores "bracket-lp-1042-03.stl" and fails here.
const BRACKET_PART_NAME: &str = "bracket-lp-1042-03";
/// `lapidary_cad::THUMB_PX`, restated rather than imported. This test asserts what the
/// *database* holds; taking the number from the crate that produced the image would make
/// the two sides of the seam agree by construction, which is the thing being checked.
const THUMB_PX: u32 = 512;
/// mm. The fixture's coordinates are `f32` in the file and `double precision` in the
/// column; a tolerance this wide passes any faithful round trip and fails a zeroed one.
const BBOX_TOLERANCE_MM: f64 = 1e-4;

fn state(pool: sqlx::PgPool, ingest_dir: &Path, blob_root: &Path) -> AppState {
    AppState {
        db: pool,
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
    }
}

/// POSTs `/api/libraries/{library}/scan` and returns the response status alongside
/// the decoded `ScanReport` JSON.
async fn scan(app_state: AppState, library: &str) -> (StatusCode, serde_json::Value) {
    let app = router(app_state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/libraries/{library}/scan"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    (status, json)
}

async fn part_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// One part as the database holds it after a scan — the far side of the kernel -> DB
/// seam. Every field here is something the CAD kernel computed and `PgIngest::record`
/// bound; nothing in it can be produced by the counters in `ScanReport`.
struct StoredPart {
    name: String,
    bbox_mm: [f64; 3],
    triangle_count: i32,
    is_watertight: bool,
    thumb_bytes: Vec<u8>,
}

/// The single part in the database, with its revision's measurements and its thumbnail.
/// `fetch_one` deliberately: more than one row is as much a failure as none.
async fn stored_part(pool: &sqlx::PgPool) -> StoredPart {
    // The row's shape is the point — every nullable column is decoded as an Option so a
    // NULL is a named failure ("revision.bbox_x was written") rather than a silent zero.
    #[allow(clippy::type_complexity)]
    let row: (
        String,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<i32>,
        Option<bool>,
        Option<Vec<u8>>,
    ) = sqlx::query_as(
        "SELECT p.name, r.bbox_x, r.bbox_y, r.bbox_z, r.triangle_count, r.is_watertight, \
                d.thumb_bytes \
         FROM part p \
         JOIN revision r ON r.part_id = p.id \
         JOIN derivative d ON d.revision_id = r.id AND d.kind = 'thumbnail'",
    )
    .fetch_one(pool)
    .await
    .expect("exactly one part, with its revision and thumbnail");

    StoredPart {
        name: row.0,
        bbox_mm: [
            row.1.expect("revision.bbox_x was written"),
            row.2.expect("revision.bbox_y was written"),
            row.3.expect("revision.bbox_z was written"),
        ],
        triangle_count: row.4.expect("revision.triangle_count was written"),
        is_watertight: row.5.expect("revision.is_watertight was written"),
        thumb_bytes: row.6.expect("derivative.thumb_bytes was written"),
    }
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
async fn scanning_one_real_stl_ingests_it_once(pool: sqlx::PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("write fixture");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["ingested"], 1);
    assert_eq!(json["skipped"], 0);
    assert_eq!(json["failed"], serde_json::json!([]));
    assert_eq!(part_count(&pool).await, 1);

    // Everything above is a counter. Counters are identical whether the kernel's output
    // reached the database or a zeroed struct and an empty byte slice did — which is the
    // one seam in this pipeline that had no assertion at all. The rest of this test is
    // about the bytes and numbers that crossed it.
    let stored = stored_part(&pool).await;

    assert_eq!(
        stored.name, BRACKET_PART_NAME,
        "part.name must be the file stem, not the file name"
    );
    for (axis, (got, want)) in ["x", "y", "z"]
        .into_iter()
        .zip(stored.bbox_mm.into_iter().zip(BRACKET_BBOX_MM))
    {
        assert!(
            (got - want).abs() < BBOX_TOLERANCE_MM,
            "revision.bbox_{axis} is {got} mm, expected {want} mm from the fixture's own \
             vertices — a bounding box that does not describe the file means the \
             measurements never crossed the kernel -> DB seam"
        );
    }
    assert_eq!(
        stored.triangle_count, BRACKET_TRIANGLES,
        "revision.triangle_count must be the fixture's real facet count"
    );
    assert!(
        stored.is_watertight,
        "the fixture is closed — every one of its 30 edges is shared by exactly two \
         facets — so revision.is_watertight must say so"
    );

    // Decoded, not measured. A length assertion passes for any 2,844 bytes; only a
    // decode proves `derivative.thumb_bytes` holds the image the rasterizer rendered,
    // and only its dimensions prove it is the full-size one rather than a fallback.
    let thumbnail = image::load_from_memory_with_format(&stored.thumb_bytes, ImageFormat::WebP)
        .expect("derivative.thumb_bytes must decode as a WebP image");
    assert_eq!(
        (thumbnail.width(), thumbnail.height()),
        (THUMB_PX, THUMB_PX),
        "the stored thumbnail must be a {THUMB_PX}px square"
    );
}

/// Pins the hash-first short-circuit as a *mechanism*, not as a counter.
///
/// The re-scan test below proves the short-circuit's observable effect — nothing is
/// ingested twice — but it stays green under a mutation that moves `kernel.ingest`
/// *above* the `Skipped` return: the file still parses, the part is still not
/// duplicated, and the report still reads `skipped: 1`. Only the wall clock changes.
/// Slice 2 moves this handler behind a queue, and that reordering is exactly what would
/// regress it.
///
/// So the file here is bytes the kernel cannot parse. The library is seeded with a part
/// holding those same bytes first, through the production repository. If anything parses
/// or rasterizes before the short-circuit fires, this file lands in `failed` instead of
/// `skipped`.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_known_hash_is_skipped_before_the_kernel_ever_sees_the_bytes(pool: sqlx::PgPool) {
    // Plausible as a stray file in a parts folder, and not an STL by any reading:
    // `parse_stl` rejects it, so `kernel.ingest` on these bytes is always an error.
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
            library: SEEDED_LIBRARY.parse().expect("seeded library id parses"),
            name: "notes",
            blob: &blob,
            measurements: &measurements,
            thumbnail_webp: &[0x52, 0x49, 0x46, 0x46],
            kernel_version: "mesh stl-1+cpu-1",
        })
        .await
        .expect("seeding the already-held part");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["failed"],
        serde_json::json!([]),
        "the kernel must never have been handed these bytes — a `failed` entry here \
         means parse or raster ran before the hash short-circuit"
    );
    assert_eq!(json["ingested"], 0);
    assert_eq!(json["skipped"], 1);
    assert_eq!(part_count(&pool).await, 1);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn rescanning_an_unchanged_directory_short_circuits_without_duplicating_the_part(
    pool: sqlx::PgPool,
) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("write fixture");

    let (first_status, first) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(first["ingested"], 1, "first scan ingests the file");

    let (second_status, second) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(second_status, StatusCode::OK);
    // If the hash short-circuit (step 3) were removed, this second scan would re-parse,
    // re-rasterize and re-record the unchanged file: `ingested` would read 1 here, not
    // 0, and the part count assertion below would read 2, not 1. Both would have to stay
    // green for a mutation that deletes the short-circuit to slip past.
    assert_eq!(second["ingested"], 0, "the known hash is not re-ingested");
    assert_eq!(second["skipped"], 1, "the known hash is counted skipped");
    assert_eq!(second["failed"], serde_json::json!([]));
    assert_eq!(
        part_count(&pool).await,
        1,
        "a re-scan of an unchanged directory must not duplicate the part"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_readme_beside_an_stl_is_not_a_candidate_and_is_counted_nowhere(pool: sqlx::PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("write fixture");
    std::fs::write(
        ingest_dir.path().join("README.md"),
        b"Brackets for the LP-1042 mounting series. Not a part.\n",
    )
    .expect("write readme");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    // If the README were treated as a candidate, is_stl_candidate's extension filter
    // would have to be broken for this to hold — and the README would fail kernel.ingest
    // (it is not an STL), showing up in `failed` rather than nowhere.
    assert_eq!(json["ingested"], 1);
    assert_eq!(json["skipped"], 0);
    assert_eq!(json["failed"], serde_json::json!([]));
    assert_eq!(part_count(&pool).await, 1);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_truncated_stl_is_reported_failed_and_the_walk_still_returns_200(pool: sqlx::PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    // Long enough to pass the "is this even a file" checks, far short of what the
    // header's declared triangle count needs — parse_stl reports this as truncated.
    std::fs::write(
        ingest_dir.path().join("truncated.stl"),
        &BRACKET_FIXTURE[..200],
    )
    .expect("write truncated fixture");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "a partial success is the accurate description, not an error status"
    );
    assert_eq!(json["ingested"], 0);
    assert_eq!(json["skipped"], 0);
    let failed = json["failed"].as_array().expect("failed is an array");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["file"], "truncated.stl");
    assert!(
        !failed[0]["reason"]
            .as_str()
            .expect("reason is a string")
            .is_empty(),
        "the reason must say something, not just exist"
    );
    assert_eq!(
        part_count(&pool).await,
        0,
        "a failed ingest creates no part"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failure_after_the_blob_write_leaves_no_orphan_blob_on_disk(pool: sqlx::PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("write fixture");

    // Syntactically a library id, but not a row in `library` — the part insert's foreign
    // key fails inside PgIngest::record, after step 5 (source.put) has already written
    // the blob to blob_root. This is what makes the failure land after the write instead
    // of before it, which is the only way to exercise the reap at all.
    let nonexistent_library = "01931b6e-0000-7000-8000-000000000099";

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        nonexistent_library,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["ingested"], 0);
    let failed = json["failed"].as_array().expect("failed is an array");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["file"], "bracket-lp-1042-03.stl");

    // The mutation this pins: delete `source.remove(&hash)` from the record() error arm
    // in scan.rs and this file survives on disk, failing the next line — the report
    // above would look identical either way, which is why this test also checks the
    // filesystem, not just the JSON body.
    let orphans = all_files(&blob_root.path().join("blobs"));
    assert!(
        orphans.is_empty(),
        "expected no orphaned blob under {}, found {orphans:?}",
        blob_root.path().display()
    );
    assert_eq!(part_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unreadable_ingest_directory_fails_the_whole_request_not_silently(pool: sqlx::PgPool) {
    // A path that was never created — distinct from a per-file failure, this is the
    // directory itself being unwalkable (the real-world case is a missing /ingest mount).
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let missing = ingest_dir.path().join("does-not-exist");
    let blob_root = tempfile::tempdir().expect("temp dir");

    let (status, _json) = scan(state(pool, &missing, blob_root.path()), SEEDED_LIBRARY).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
