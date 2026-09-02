use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use std::path::PathBuf;
use tower::ServiceExt;

/// `AppState` for every test in this file that never dispatches to the scan handler
/// (everything except `the_worker_role_serves_the_scan_route`, which builds its own
/// `AppState` over a real `TempDir` so the handler has somewhere real to walk). These
/// paths are never read: a request either never reaches `scan::scan` at all (wrong role,
/// or a different route), or the test is about routing, not about ingest. Nonexistent,
/// deliberately — a real path here would be a silent, misleading hint that these tests
/// exercise the scan handler's filesystem behaviour, which is `tests/scan.rs`'s job.
fn placeholder_state(db: sqlx::PgPool) -> AppState {
    AppState {
        db,
        ingest_dir: PathBuf::from("/nonexistent-ingest-dir"),
        blob_root: PathBuf::from("/nonexistent-blob-root"),
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_reports_ok_and_the_postgres_major_version(pool: sqlx::PgPool) {
    let app = router(placeholder_state(pool), Role::Api);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");

    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"]["major"], 18);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_says_what_broke_and_what_to_do_when_the_database_is_gone(pool: sqlx::PgPool) {
    // Closing the pool is the cheapest reliable way to make server_version_num fail.
    // This test is also what gives the success test above its meaning: a handler that
    // hardcoded {"status":"ok","database":{"major":18}} and never touched the pool would
    // pass that one, and fail this one.
    pool.close().await;
    let app = router(placeholder_state(pool), Role::Api);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/healthz")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    assert_eq!(json["status"], "unavailable");

    // "Errors say what broke and what to do." Assert the remedy, not just the failure:
    // deleting the advice and leaving "Could not reach the database." must fail here.
    let message = json["message"].as_str().expect("message is a string");
    assert!(
        message.contains("`db` service"),
        "must name the service to check"
    );
    assert!(
        message.contains("DATABASE_URL"),
        "must name the setting to check"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn unknown_routes_are_not_found(pool: sqlx::PgPool) {
    let app = router(placeholder_state(pool), Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/nope")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn health_is_served_in_both_roles(pool: sqlx::PgPool) {
    for role in [Role::Api, Role::Worker] {
        let app = router(placeholder_state(pool.clone()), role);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/healthz")
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_api_role_does_not_serve_the_scan_route(pool: sqlx::PgPool) {
    // Ingest must not run in the process that serves the open path.
    let app = router(placeholder_state(pool), Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/scan")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_the_grid(pool: sqlx::PgPool) {
    let app = router(placeholder_state(pool), Role::Worker);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/parts")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// The two tests above prove a role does NOT serve the other role's route, but a 404
// because the route is missing from *both* roles would satisfy them just as well as a
// 404 because it's missing from *this* role. These two are the other half: they pin that
// each route genuinely IS mounted in the role that owns it, so the pair together proves
// 404 means "wrong role", not "no such route anywhere".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_serves_the_scan_route(pool: sqlx::PgPool) {
    // Unlike every other test in this file, this one exercises the real handler — it
    // replaced a 501 placeholder in Task 9 — so it needs somewhere real to walk. A real,
    // empty TempDir rather than `placeholder_state`'s nonexistent paths: pinning 200 with
    // an all-zero report is a stronger proof this route is mounted and actually runs
    // than pinning some fixed non-404 status would be — a nonexistent ingest_dir would
    // 500 (see tests/scan.rs's `an_unreadable_ingest_directory_...` test), which is not
    // distinguishable from a handler that panics, the exact ambiguity the old comment
    // here warned an `assert_ne!` would let through.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let app = router(
        AppState {
            db: pool,
            ingest_dir: ingest_dir.path().to_path_buf(),
            blob_root: blob_root.path().to_path_buf(),
        },
        Role::Worker,
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/scan")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    assert_eq!(
        json,
        serde_json::json!({ "ingested": 0, "skipped": 0, "failed": [] }),
        "an empty ingest directory scans cleanly with no candidates"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_api_role_serves_the_grid(pool: sqlx::PgPool) {
    let app = router(placeholder_state(pool), Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/parts")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    // Same reasoning the_worker_role_serves_the_scan_route above used before Task 9
    // replaced its placeholder: pin the exact placeholder status so a panicking handler
    // can't slip past as "not 404". Task 10 replaces this handler and must update this
    // assertion the same way — ideally to a real response, not just a different status.
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

#[test]
fn an_unknown_role_is_rejected_with_the_valid_values() {
    let err = Role::from_env_str("wroker").expect_err("must reject");
    let msg = err.to_string();
    assert!(msg.contains("wroker"), "names what was given: {msg}");
    assert!(
        msg.contains("api") && msg.contains("worker"),
        "names the valid values: {msg}"
    );
}
