//! `POST /api/libraries/{id}/scan` — the route a browser can actually reach.
//!
//! Driven through this crate's router against a live, migrated Postgres, and read back
//! off the `job` table: what this route promises is that a `scan_directory` row lands
//! for the worker to pick up, and the row is the promise. The walk itself belongs to
//! `lapidary-ingest` and is tested there.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{LibraryId, ScanAccepted};
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// This route never reads a blob; the field is required to build the state, and a path
/// that does not exist is the honest value for a test that must not reach the store.
fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
    }
}

/// POSTs the scan route through `Role::Api`, which is the role under test: `Role::Worker`
/// does not mount it, and `deploy/web/Caddyfile` proxies `/api/*` to `api:8080` only, so
/// a scan route anywhere else is one no browser can call.
async fn scan(pool: sqlx::PgPool, library: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
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
    // An empty body decodes to `Null` rather than panicking, so a route that is not
    // mounted at all fails these tests on the status code — which says what happened —
    // instead of on a JSON parse error, which does not.
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body is JSON")
    };
    (status, json)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_from_the_browser_enqueues_a_scan_directory_job(pool: sqlx::PgPool) {
    let (status, json) = scan(pool.clone(), SEEDED_LIBRARY).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(
        accepted.queued, 1,
        "the walk is the one job this route enqueues; the files it finds join this same \
         batch later, which is why `total` and not `queued` is what the browser watches"
    );

    let rows: Vec<(String, serde_json::Value, String)> =
        sqlx::query_as("SELECT kind, payload, batch_id::text FROM job")
            .fetch_all(&pool)
            .await
            .expect("reads the queued jobs");
    assert_eq!(
        rows,
        vec![(
            "scan_directory".to_owned(),
            serde_json::json!({}),
            accepted.batch_id.to_string()
        )],
        "one job, in the batch the caller was handed to poll — the api container mounts \
         no ingest directory, so the row is the whole of what it can enqueue"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_a_library_that_does_not_exist_is_a_404_not_an_accepted_scan(pool: sqlx::PgPool) {
    // A 202 here would run the walk against an id that names nothing: every file it found
    // would be enqueued and then fail, one identical failure per file, for one mistyped
    // id. The same reason `POST /thumbnails` next door stopped answering 202 for a
    // phantom library.
    let phantom = LibraryId::new();

    let (status, json) = scan(pool.clone(), &phantom.to_string()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .expect("message is a string")
            .contains("No library with that id"),
        "the message must say the id names nothing: {json}"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts the jobs");
    assert_eq!(queued, 0, "and nothing may have been queued");
}
