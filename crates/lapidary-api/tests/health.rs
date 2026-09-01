use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, router};
use tower::ServiceExt;

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_reports_ok_and_the_postgres_major_version(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool });

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
    let app = router(AppState { db: pool });

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
    let app = router(AppState { db: pool });
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
