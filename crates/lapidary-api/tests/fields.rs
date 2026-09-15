//! Custom fields through the API: definitions per library, one value per part, and the grid, its
//! facets and its saved filters narrowed by a field offered as a filter (`docs/DATA.md` §3.5).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, PartId};
use serde_json::{Value, json};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

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

/// A part in the seeded library, database only.
async fn part(pool: &sqlx::PgPool, name: &str, seed: u8) -> PartId {
    let source_path = format!("{name}.stl");
    let storage_path = format!("libraries/default/{name}/{name}.stl");
    lapidary_db::PgIngest(pool.clone())
        .record(lapidary_db::IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: &source_path,
            folder: None,
            storage_path: Some(&storage_path),
            blob: &lapidary_db::StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 48_112,
                stored_bytes: 0,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [80.0, 40.0, 12.0],
                triangle_count: 1_204,
                surface_area_mm2: 9_140.0,
                volume_mm3: Some(21_600.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("records a part")
}

async fn define(pool: &sqlx::PgPool, field: Value) {
    let (status, body) = send(
        pool,
        "POST",
        &format!("/api/libraries/{SEEDED_LIBRARY}/fields"),
        Some(field),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn custom(pool: &sqlx::PgPool, part: PartId) -> Value {
    let (status, detail) = send(pool, "GET", &format!("/api/parts/{part}"), None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    detail["custom"].clone()
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_number_field_refuses_words_and_keeps_numbers(pool: sqlx::PgPool) {
    define(
        &pool,
        json!({ "key": "stock_count", "label": "Stock count", "kind": "number" }),
    )
    .await;
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    let uri = format!("/api/parts/{bracket}/fields/stock_count");

    let (status, body) = send(&pool, "PUT", &uri, Some(json!({ "value": "twelve" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["reason"], "wrongType");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("Stock count")),
        "{body}"
    );
    assert_eq!(custom(&pool, bracket).await, json!({}));

    let (status, _) = send(&pool, "PUT", &uri, Some(json!({ "value": 12 }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(custom(&pool, bracket).await, json!({ "stock_count": 12 }));

    let (status, _) = send(&pool, "PUT", &uri, Some(json!({ "value": null }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(custom(&pool, bracket).await, json!({}));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_choice_takes_only_its_options_and_narrows_the_grid_its_facets_and_a_saved_filter(
    pool: sqlx::PgPool,
) {
    define(
        &pool,
        json!({ "key": "supplier", "label": "Supplier", "kind": "choice",
                "options": ["Hoffmann", "Misumi"], "indexed": true }),
    )
    .await;
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    let spacer = part(&pool, "spacer-lp-2001-00", 0x42).await;

    let (status, body) = send(
        &pool,
        "PUT",
        &format!("/api/parts/{bracket}/fields/supplier"),
        Some(json!({ "value": "Norelem" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    for (part, supplier) in [(bracket, "Misumi"), (spacer, "Hoffmann")] {
        let (status, body) = send(
            &pool,
            "PUT",
            &format!("/api/parts/{part}/fields/supplier"),
            Some(json!({ "value": supplier })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let (status, page) = send(
        &pool,
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?field=supplier&fieldValue=Misumi"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let ids: Vec<&str> = page["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .map(|card| card["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, [bracket.to_string()]);

    let (status, facets) = send(
        &pool,
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/facets?field=supplier&fieldValue=Misumi"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{facets}");
    assert_eq!(facets["formats"], json!([{ "value": "stl", "count": 1 }]));

    let (status, saved) = send(
        &pool,
        "POST",
        &format!("/api/libraries/{SEEDED_LIBRARY}/filters"),
        Some(json!({ "name": "From Misumi",
                     "search": { "field": "supplier", "fieldValue": "Misumi" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    let (_, listed) = send(
        &pool,
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/filters"),
        None,
    )
    .await;
    assert_eq!(
        listed[0]["search"],
        json!({ "field": "supplier", "fieldValue": "Misumi" })
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_field_not_offered_as_a_filter_filters_nothing(pool: sqlx::PgPool) {
    define(
        &pool,
        json!({ "key": "notes", "label": "Notes", "kind": "text" }),
    )
    .await;

    let (status, body) = send(
        &pool,
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?field=notes&fieldValue=deburred"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["reason"], "notAFilter");

    let (status, body) = send(
        &pool,
        "POST",
        &format!("/api/libraries/{SEEDED_LIBRARY}/filters"),
        Some(
            json!({ "name": "Deburred", "search": { "field": "notes", "fieldValue": "deburred" } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["reason"], "notAFilter");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_key_and_its_options_are_checked_before_anything_is_written(pool: sqlx::PgPool) {
    let uri = format!("/api/libraries/{SEEDED_LIBRARY}/fields");
    for (field, reason) in [
        (
            json!({ "key": "Stock Count", "label": "Stock count", "kind": "number" }),
            "badKey",
        ),
        (
            json!({ "key": "notes", "label": "Notes", "kind": "text", "options": ["a"] }),
            "badOptions",
        ),
        (
            json!({ "key": "supplier", "label": "Supplier", "kind": "choice",
                 "options": ["Misumi", " Misumi "] }),
            "badOptions",
        ),
        (
            json!({ "key": "supplier", "label": "Supplier", "kind": "choice" }),
            "badOptions",
        ),
    ] {
        let (status, body) = send(&pool, "POST", &uri, Some(field)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["reason"], reason, "{body}");
    }
    let (_, listed) = send(&pool, "GET", &uri, None).await;
    assert_eq!(listed, json!([]));

    define(
        &pool,
        json!({ "key": "notes", "label": "Notes", "kind": "text" }),
    )
    .await;
    let (status, body) = send(
        &pool,
        "POST",
        &uri,
        Some(json!({ "key": "notes", "label": "Remarks", "kind": "text" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["reason"], "keyTaken");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_removed_field_keeps_its_values_and_takes_no_new_ones(pool: sqlx::PgPool) {
    define(
        &pool,
        json!({ "key": "stock_count", "label": "Stock count", "kind": "number" }),
    )
    .await;
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    let uri = format!("/api/parts/{bracket}/fields/stock_count");
    send(&pool, "PUT", &uri, Some(json!({ "value": 12 }))).await;

    let (status, _) = send(
        &pool,
        "DELETE",
        &format!("/api/libraries/{SEEDED_LIBRARY}/fields/stock_count"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(custom(&pool, bracket).await, json!({ "stock_count": 12 }));

    let (status, body) = send(&pool, "PUT", &uri, Some(json!({ "value": 13 }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["reason"], "noSuchField");
    assert_eq!(custom(&pool, bracket).await, json!({ "stock_count": 12 }));
}
