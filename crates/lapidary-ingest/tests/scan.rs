//! The scan route, which as of slice 2 task 10 enqueues rather than ingests. What it
//! must prove is now a much smaller claim than slice 1's: the walk finds the right
//! candidates, turns each into a job row, and touches neither the bytes nor the kernel.
//!
//! The pipeline those jobs later run is `tests/handler.rs`'s subject — including the
//! four cases that used to be driven through this route (the pre-kernel short-circuit,
//! per-library parts, blob sharing, and the orphan-blob reap). They moved with the code
//! they test; they were not dropped.

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

async fn part_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(pool)
        .await
        .expect("count query")
}

/// The `payload -> 'path'` of every job row, in enqueue order.
async fn queued_paths(pool: &sqlx::PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT payload ->> 'path' FROM job ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .expect("reads the queued paths")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_enqueues_one_job_per_stl_and_parses_nothing(pool: sqlx::PgPool) {
    // The whole point of the slice: the request must not touch the CAD kernel. Three
    // files, each doing a different job here.
    //
    // The valid fixture is what makes the mutation check bite. Restore a synchronous
    // `ingest_one` inside the walk and this file becomes a part, so `part_count == 0`
    // fails. An unparseable file alone could not catch that -- it produces no part either
    // way, which is why this test does not rely on one.
    //
    // The unparseable file is what proves the bytes were never read. Under slice 1 it
    // came back in a `failed` list, because the walk parsed it inside the request. Now it
    // is simply accepted alongside the other, with nothing anywhere reporting on its
    // contents, because nothing has looked at them.
    //
    // The README is not a candidate and is counted nowhere.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        ingest_dir.path().join("bracket-lp-1042-03.stl"),
        BRACKET_FIXTURE,
    )
    .expect("stages a genuinely ingestable STL");
    std::fs::write(
        ingest_dir.path().join("notes.stl"),
        b"LP-1042-03 revision notes: chamfer the mounting face.\n",
    )
    .expect("stages a file that is not an STL by any reading");
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
        accepted.queued, 2,
        "both .stl files are candidates; the README is not"
    );

    assert_eq!(
        queued_paths(&pool).await,
        vec!["bracket-lp-1042-03.stl".to_owned(), "notes.stl".to_owned()],
        "one job per candidate, each carrying the file name the worker will read"
    );
    assert_eq!(
        part_count(&pool).await,
        0,
        "the request must enqueue, not ingest -- not even the file it could have ingested"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_an_empty_directory_is_accepted_with_nothing_queued(pool: sqlx::PgPool) {
    // Zero is a success, not an error. It is also the one response the client must not
    // poll on: `batch_status` has no rows to aggregate, so it answers 404 — see
    // `ScanAccepted`'s doc.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");

    let (status, json) = scan(
        state(pool.clone(), ingest_dir.path(), blob_root.path()),
        SEEDED_LIBRARY,
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(accepted.queued, 0, "an empty folder scanned successfully");
    assert!(queued_paths(&pool).await.is_empty());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unreadable_ingest_directory_fails_the_whole_request_not_silently(pool: sqlx::PgPool) {
    // A path that was never created — distinct from a per-file failure, this is the
    // directory itself being unwalkable (the real-world case is a missing /ingest mount).
    // It must not answer `202 { queued: 0 }`, which is indistinguishable from an empty
    // directory that scanned perfectly well. This is why the walk stays in the request.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let missing = ingest_dir.path().join("does-not-exist");
    let blob_root = tempfile::tempdir().expect("temp dir");

    let (status, json) = scan(state(pool, &missing, blob_root.path()), SEEDED_LIBRARY).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        json["message"]
            .as_str()
            .expect("message is a string")
            .contains("does-not-exist"),
        "the message must name the directory it could not read: {json}"
    );
}
