//! M2: this install's display name.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(pool: &sqlx::PgPool, method: &str, body: Option<Value>) -> (StatusCode, Value) {
    let state = AppState {
        db: pool.clone(),
        blob_root: "/nonexistent-blob-root".into(),
        upload_dir: "/nonexistent-upload-dir".into(),
        host_storage_root: None,
        touches: Default::default(),
    };
    let mut request = Request::builder().method(method).uri("/api/profile");
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router(state, Role::Api)
        .oneshot(
            request
                .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
                .expect("request"),
        )
        .await
        .expect("responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_display_name_is_kept_trimmed_cleared_by_a_blank_and_refused_when_too_long(
    pool: sqlx::PgPool,
) {
    assert_eq!(
        send(&pool, "GET", None).await.1,
        json!({ "displayName": null })
    );
    let (status, saved) = send(&pool, "PUT", Some(json!({ "displayName": "  Furkan  " }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, json!({ "displayName": "Furkan" }));
    assert_eq!(send(&pool, "GET", None).await.1["displayName"], "Furkan");
    assert_eq!(
        send(&pool, "PUT", Some(json!({ "displayName": "   " })))
            .await
            .1,
        json!({ "displayName": null })
    );
    let (status, refused) =
        send(&pool, "PUT", Some(json!({ "displayName": "x".repeat(81) }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|m| m.contains("80"))
    );
}
