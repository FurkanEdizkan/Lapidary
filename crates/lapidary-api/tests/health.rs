use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use tower::ServiceExt;

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_reports_ok_and_the_postgres_major_version(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool }, Role::Api);

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
    let app = router(AppState { db: pool }, Role::Api);

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
    let app = router(AppState { db: pool }, Role::Api);
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
        let app = router(AppState { db: pool.clone() }, role);
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
async fn the_scan_route_is_unknown_to_this_crate_under_either_role(pool: sqlx::PgPool) {
    // Fix round 1 moved the scan handler into its own crate, lapidary-ingest (see its
    // lib.rs module doc for why a runtime Role check inside lapidary-api was not enough
    // on its own — it kept the open path from *invoking* the kernel, but not from
    // *linking* it, since lapidary-api depending on lapidary-cad at all made the api
    // image link it regardless of which routes Role::Api mounted). This crate now has no
    // route, dependency, or type that reaches ingest at all, under either role — unlike
    // the_worker_role_does_not_serve_the_grid below, this isn't "wrong role", it's "this
    // crate has never heard of /scan".
    for role in [Role::Api, Role::Worker] {
        let app = router(AppState { db: pool.clone() }, role);
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
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_the_grid(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool }, Role::Worker);
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

// the_worker_role_does_not_serve_the_grid proves the worker role does NOT serve the
// grid, but a 404 because the route is missing from *both* roles would satisfy that just
// as well as a 404 because it's missing from *this* role. This is the other half: it
// pins that the grid genuinely IS mounted in the role that owns it (returning a real
// 200 with a page body, not axum's fallback 404), so the pair together proves 404 means
// "wrong role", not "no such route anywhere". Task 10 replaced the 501 placeholder this
// test used to pin with the real handler — the seeded library has no parts in this
// freshly migrated pool, so an empty page is the correct response, not an error.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_api_role_serves_the_grid(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool }, Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/parts")
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
    assert_eq!(json["parts"], serde_json::json!([]));
    assert!(json["next"].is_null());
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
