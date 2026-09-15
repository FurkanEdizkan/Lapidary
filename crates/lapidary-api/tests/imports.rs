//! `POST /api/libraries/{id}/imports`: a bundle this library uploaded, stored and queued for the
//! worker (Phase 4 slice 2 spec §7). The archive itself is checked by its job; `lapidary-ingest`'s
//! handler tests hold that half.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::BlobHash;
use lapidary_db::{PgBlobs, StoredBlobRow};
use std::path::Path;
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

async fn import(
    pool: sqlx::PgPool,
    uploads: &Path,
    store: &Path,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = router(
        AppState {
            db: pool,
            blob_root: store.to_path_buf(),
            upload_dir: uploads.to_path_buf(),
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

/// The bytes as the chunked upload leaves them: staged under this library, named by their hash.
fn staged(uploads: &Path, bytes: &[u8]) -> BlobHash {
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let folder = uploads.join(SEEDED_LIBRARY);
    std::fs::create_dir_all(&folder).expect("the library's staging folder");
    std::fs::write(folder.join(format!("{}.part", hash.to_hex())), bytes)
        .expect("the staged upload");
    hash
}

async fn jobs(pool: &sqlx::PgPool) -> Vec<(String, serde_json::Value)> {
    sqlx::query_as("SELECT kind, payload FROM job WHERE library_id::text = $1")
        .bind(SEEDED_LIBRARY)
        .fetch_all(pool)
        .await
        .expect("jobs")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_uploaded_bundle_is_stored_and_queued_as_one_import_job_named_for_its_file(
    pool: sqlx::PgPool,
) {
    let (uploads, store) = (
        tempfile::tempdir().expect("uploads"),
        tempfile::tempdir().expect("store"),
    );
    let bytes = b"PK\x03\x04 a bundle of flange revisions\n".repeat(40);
    let hash = staged(uploads.path(), &bytes);

    let (status, json) = import(
        pool.clone(),
        uploads.path(),
        store.path(),
        serde_json::json!({ "blake3": hash, "name": "workshop-bundle.lapidary.zip" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    let queued = jobs(&pool).await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, "import_bundle");
    assert_eq!(queued[0].1["path"], "workshop-bundle.lapidary.zip");
    assert_eq!(queued[0].1["blake3"], hash.to_hex());
}

/// Content addressing is not authorization: bytes the store holds, which this library never
/// uploaded, are not imported by naming their hash.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_this_library_never_uploaded_is_not_imported_by_its_hash(pool: sqlx::PgPool) {
    let (uploads, store) = (
        tempfile::tempdir().expect("uploads"),
        tempfile::tempdir().expect("store"),
    );
    let hash = BlobHash::from_bytes([0x5b; 32]);
    PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash,
            size_bytes: 612_480,
            stored_bytes: 598_112,
            zstd_level: 3,
        })
        .await
        .expect("another library's stored bundle");

    let (status, json) = import(
        pool.clone(),
        uploads.path(),
        store.path(),
        serde_json::json!({ "blake3": hash, "name": "workshop-bundle.lapidary.zip" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert!(jobs(&pool).await.is_empty(), "nothing queued");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_name_with_a_folder_in_it_is_refused(pool: sqlx::PgPool) {
    let (uploads, store) = (
        tempfile::tempdir().expect("uploads"),
        tempfile::tempdir().expect("store"),
    );
    let (status, json) = import(
        pool,
        uploads.path(),
        store.path(),
        serde_json::json!({
            "blake3": BlobHash::from_bytes([0x5b; 32]),
            "name": "../workshop-bundle.lapidary.zip",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}
