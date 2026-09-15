//! Saved filters over HTTP: `GET`/`POST /api/libraries/{id}/filters` and
//! `DELETE /api/libraries/{library}/filters/{filter}`, against a live migrated Postgres.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_db::{PgFolders, PgParts};
use serde_json::{Value, json};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn app(pool: sqlx::PgPool) -> axum::Router {
    router(
        AppState {
            db: pool,
            blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            touches: Default::default(),
        },
        Role::Api,
    )
}

async fn send(
    pool: &sqlx::PgPool,
    method: &str,
    uri: String,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("request builds");
    let response = app(pool.clone())
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn filters() -> String {
    format!("/api/libraries/{SEEDED_LIBRARY}/filters")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_saved_filter_is_listed_and_then_removed(pool: sqlx::PgPool) {
    let (status, saved) = send(
        &pool,
        "POST",
        filters(),
        Some(json!({ "name": " Stock STL ", "search": { "format": "stl", "tag": "stock", "q": "  " } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    assert_eq!(saved["name"], "Stock STL", "the name is kept trimmed");
    assert_eq!(
        saved["search"],
        json!({ "format": "stl", "tag": "stock" }),
        "an empty value is no filter, and is not kept"
    );

    let (status, listed) = send(&pool, "GET", filters(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!([saved]));

    let id = saved["id"].as_str().expect("an id");
    let (status, _) = send(&pool, "DELETE", format!("{}/{id}", filters()), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(&pool, "DELETE", format!("{}/{id}", filters()), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["reason"], "noSuchFilter");
    let (_, listed) = send(&pool, "GET", filters(), None).await;
    assert_eq!(listed, json!([]));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_filter_needs_a_name_and_something_to_keep(pool: sqlx::PgPool) {
    let cases = [
        (
            json!({ "name": "   ", "search": { "format": "stl" } }),
            StatusCode::BAD_REQUEST,
            "emptyName",
        ),
        (
            json!({ "name": "x".repeat(81), "search": { "format": "stl" } }),
            StatusCode::BAD_REQUEST,
            "nameTooLong",
        ),
        (
            json!({ "name": "Nothing", "search": {} }),
            StatusCode::BAD_REQUEST,
            "emptySearch",
        ),
        (
            json!({ "name": "Blank", "search": { "q": "   " } }),
            StatusCode::BAD_REQUEST,
            "emptySearch",
        ),
        (
            json!({ "name": "Long", "search": { "q": "x".repeat(513) } }),
            StatusCode::BAD_REQUEST,
            "valueTooLong",
        ),
    ];
    for (body, expected, reason) in cases {
        let (status, answer) = send(&pool, "POST", filters(), Some(body.clone())).await;
        assert_eq!(status, expected, "{body}: {answer}");
        assert_eq!(answer["reason"], reason, "{body}");
        assert!(
            answer["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{answer}"
        );
    }

    // The part a quick look is open on describes a moment, not a filter, and is not accepted.
    let (status, _) = send(
        &pool,
        "POST",
        filters(),
        Some(json!({ "name": "With a part", "search": { "format": "stl", "part": "01a09cc0-5257-7550-bf80-758fa917fb8c" } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, listed) = send(&pool, "GET", filters(), None).await;
    assert_eq!(listed, json!([]), "nothing refused was saved");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_taken_name_conflicts_and_an_unknown_library_is_not_found(pool: sqlx::PgPool) {
    let body = json!({ "name": "Stock STL", "search": { "format": "stl" } });
    let (status, _) = send(&pool, "POST", filters(), Some(body.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, answer) = send(&pool, "POST", filters(), Some(body.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert_eq!(answer["reason"], "nameTaken");

    let nowhere = "/api/libraries/01a09cc0-0000-7000-8000-00000000dead/filters".to_owned();
    let (status, answer) = send(&pool, "POST", nowhere.clone(), Some(body)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(answer["reason"], "noSuchLibrary");
    let (status, listed) = send(&pool, "GET", nowhere, None).await;
    assert_eq!((status, listed), (StatusCode::OK, json!([])));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_category_from_another_library_is_not_kept(pool: sqlx::PgPool) {
    let workshop = PgParts(pool.clone())
        .create_library("Workshop fixtures", "hobby")
        .await
        .expect("a second library");
    let jigs = PgFolders(pool.clone())
        .create(workshop, None, "Jigs", "Jigs")
        .await
        .expect("a category there");
    let (status, answer) = send(
        &pool,
        "POST",
        filters(),
        Some(json!({ "name": "Jigs", "search": { "folderId": jigs.to_string() } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["reason"], "crossLibraryFolder");

    let (status, answer) = send(
        &pool,
        "POST",
        format!("/api/libraries/{workshop}/filters"),
        Some(json!({ "name": "Jigs", "search": { "folderId": jigs.to_string() } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
}
