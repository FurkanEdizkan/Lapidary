//! `POST /api/libraries/{id}/imports`: an uploaded bundle stored and queued for the worker (Phase 4
//! slice 2 spec §7). The archive itself is checked by its job; `lapidary-ingest`'s handler tests
//! hold that half.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::BlobHash;
use lapidary_db::{PgBlobs, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

async fn import(pool: sqlx::PgPool, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let response = router(
        AppState {
            db: pool,
            blob_root: "/nonexistent-blob-root".into(),
            upload_dir: "/nonexistent-upload-dir".into(),
            host_storage_root: None,
            touches: Default::default(),
        },
        Role::Api,
    )
    .oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/libraries/{SEEDED_LIBRARY}/imports"))
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
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// Bytes already in the store, as the chunked upload leaves a bundle it was sent before.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_bundle_is_queued_as_one_import_job_named_for_its_file(pool: sqlx::PgPool) {
    let hash = BlobHash::from_bytes([0x5b; 32]);
    PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash,
            size_bytes: 612_480,
            stored_bytes: 598_112,
            zstd_level: 3,
        })
        .await
        .expect("the stored bundle");

    let (status, json) = import(
        pool.clone(),
        serde_json::json!({ "blake3": hash, "name": "workshop-bundle.lapidary.zip" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    let (kind, payload): (String, serde_json::Value) =
        sqlx::query_as("SELECT kind, payload FROM job WHERE library_id::text = $1")
            .bind(SEEDED_LIBRARY)
            .fetch_one(&pool)
            .await
            .expect("one job");
    assert_eq!(kind, "import_bundle");
    assert_eq!(payload["path"], "workshop-bundle.lapidary.zip");
    assert_eq!(payload["blake3"], hash.to_hex());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_name_with_a_folder_in_it_is_refused(pool: sqlx::PgPool) {
    let (status, json) = import(
        pool,
        serde_json::json!({
            "blake3": BlobHash::from_bytes([0x5b; 32]),
            "name": "../workshop-bundle.lapidary.zip",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}
