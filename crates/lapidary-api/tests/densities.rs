//! A density per material, per library, through the API (goal 5, stage 2).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use serde_json::{Value, json};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

async fn send(
    pool: &sqlx::PgPool,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request builds");
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
    .oneshot(request)
    .await
    .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn densities() -> String {
    format!("/api/libraries/{SEEDED_LIBRARY}/densities")
}

/// A density is a finite number of kg/m³ above 0 and below 25,000. Anything else is refused in words,
/// and nothing is written for it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_density_out_of_bounds_or_not_a_number_is_refused_in_words(pool: sqlx::PgPool) {
    let osmium = format!("{}/Osmium", densities());
    for body in [
        json!({ "densityKgM3": 0 }),
        json!({ "densityKgM3": -7850 }),
        json!({ "densityKgM3": 25000 }),
        json!({ "densityKgM3": "heavy" }),
        json!({}),
    ] {
        let (status, refusal) = send(&pool, "PUT", &osmium, Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            refusal["message"]
                .as_str()
                .is_some_and(|message| message.contains("above 0 and below 25,000")),
            "{body} is refused in words: {refusal}"
        );
    }
    let (_, list) = send(&pool, "GET", &densities(), None).await;
    assert_eq!(list, json!([]), "nothing was written for any of them");

    let (status, _) = send(&pool, "PUT", &osmium, Some(json!({ "densityKgM3": 22590 }))).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the densest element is inside the bounds"
    );
}

/// Setting a density again replaces it; removing it takes it away, and a second removal finds none. A
/// material no part holds yet takes one too, and a name no part could hold is refused.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_density_is_replaced_by_setting_it_again_and_gone_once_removed(pool: sqlx::PgPool) {
    // No part in the library holds this material.
    let aluminium = format!("{}/EN%20AW-6082%20T6", densities());
    for density in [2700, 2710] {
        let (status, body) = send(
            &pool,
            "PUT",
            &aluminium,
            Some(json!({ "densityKgM3": density })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let (_, list) = send(&pool, "GET", &densities(), None).await;
    assert_eq!(
        list,
        json!([{ "material": "EN AW-6082 T6", "densityKgM3": 2710.0 }]),
        "one row, replaced"
    );

    let (status, _) = send(&pool, "DELETE", &aluminium, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = send(&pool, "GET", &densities(), None).await;
    assert_eq!(list, json!([]));
    let (status, refusal) = send(&pool, "DELETE", &aluminium, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");

    let (status, _) = send(
        &pool,
        "PUT",
        &format!("{}/%20Steel", densities()),
        Some(json!({ "densityKgM3": 7850 })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a padded name is no part's"
    );
    let (status, _) = send(
        &pool,
        "PUT",
        "/api/libraries/01931b6e-0000-7000-8000-0000000000ff/densities/Steel",
        Some(json!({ "densityKgM3": 7850 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no such library");
}
