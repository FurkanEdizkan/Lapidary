//! `POST /api/libraries` takes a search language (`docs/DATA.md` §3.3).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn create(pool: &sqlx::PgPool, body: Value) -> (StatusCode, Value) {
    let response = router(
        AppState {
            db: pool.clone(),
            blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            touches: Default::default(),
        },
        Role::Api,
    )
    .oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/libraries")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds"),
    )
    .await
    .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn language_of(pool: &sqlx::PgPool, name: &str) -> String {
    sqlx::query_scalar("SELECT language FROM library WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("the library")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_is_made_with_the_search_language_it_asks_for(pool: sqlx::PgPool) {
    let (status, body) = create(
        &pool,
        json!({ "name": "Atölye fikstürleri", "language": "turkish" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(language_of(&pool, "Atölye fikstürleri").await, "turkish");

    let (status, body) = create(&pool, json!({ "name": "Workshop fixtures" })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        language_of(&pool, "Workshop fixtures").await,
        "simple",
        "a library that asks for no language stems nothing, as every library did before"
    );

    let (status, body) = create(
        &pool,
        json!({ "name": "Stock parts", "language": "klingon" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["reason"], "badBody");
}
