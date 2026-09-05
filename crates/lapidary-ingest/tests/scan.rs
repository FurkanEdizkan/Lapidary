//! The scan route, which as of slice 5 task 6 enqueues a *job* rather than walking the
//! directory itself. What it must prove is now the smallest claim it has ever made: one
//! `scan_directory` row lands, and the ingest directory is not read at all.
//!
//! The walk those jobs later perform is `tests/handler.rs`'s subject — including the
//! candidate selection and the unreadable-mount failure, both of which used to be driven
//! through this route. They moved with the code they test; they were not dropped. See
//! `src/scan.rs`'s module doc for why the walk moved.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_core::ScanAccepted;
use lapidary_ingest::{AppState, router};
use std::path::Path;
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

/// POSTs `/api/libraries/{library}/scan` and returns the status alongside the raw body.
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

/// Every job row's `(kind, payload)`, in enqueue order.
async fn queued(pool: &sqlx::PgPool) -> Vec<(String, serde_json::Value)> {
    sqlx::query_as("SELECT kind, payload FROM job ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .expect("reads the queued jobs")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_enqueues_one_scan_directory_job_and_walks_nothing(pool: sqlx::PgPool) {
    // Three real candidates are staged and none of them may produce a job row here: the
    // request enqueues the walk, it does not perform it. A test against an *empty*
    // directory could not tell the two apart — it would pass just as well for a route
    // that walked and found nothing.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("stages a genuinely ingestable STL");
    std::fs::write(
        ingest_dir.path().join("carrier-lp-3480-02.3mf"),
        b"not a real OPC package, just bytes with the right extension",
    )
    .expect("stages a 3MF candidate");
    std::fs::write(
        ingest_dir.path().join("README.md"),
        b"Brackets for the LP-1042 mounting series. Not a part.\n",
    )
    .expect("stages a non-candidate");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(
        accepted.queued, 1,
        "one job, the walk itself — not one per candidate"
    );
    assert_eq!(
        queued(&pool).await,
        vec![("scan_directory".to_owned(), serde_json::json!({}))],
        "the library is the job row's own column and the directory is the worker's \
         mount, so the payload carries neither"
    );
}
