//! Task 9: the scan endpoint. Exercises the whole pipeline end to end — walking a
//! `TempDir` standing in for the read-only ingest mount, and a `TempDir` blob root
//! standing in for the real volume Task 12 wires up — against a live, migrated Postgres
//! (via `sqlx::test`), through `router(..., Role::Worker)`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

fn state(pool: sqlx::PgPool, ingest_dir: &Path, blob_root: &Path) -> AppState {
    AppState {
        db: pool,
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
    }
}

/// POSTs `/api/libraries/{library}/scan` under `Role::Worker` and returns the response
/// status alongside the decoded `ScanReport` JSON.
async fn scan(app_state: AppState, library: &str) -> (StatusCode, serde_json::Value) {
    let app = router(app_state, Role::Worker);
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
