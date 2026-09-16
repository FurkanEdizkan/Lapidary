//! Sharing a category (S2a), through the API.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, FolderId, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, NewPartSource, PgFolders, PgIngest, PgParts, StoredBlobRow};
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

async fn record(
    pool: &sqlx::PgPool,
    folder: FolderId,
    name: &str,
    seed: u8,
    license: Option<&str>,
) {
    let source_path = format!("{}.stl", name.to_lowercase().replace([' ', ','], "-"));
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: Some(folder),
            storage_path: None,
            library: library(),
            name,
            source_path: &source_path,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    if let Some(license) = license {
        PgParts(pool.clone())
            .add_part_source(
                part,
                NewPartSource {
                    license: Some(license),
                    ..Default::default()
                },
            )
            .await
            .expect("records its licence");
    }
}

/// Terrain holding Rocks, with three parts and two licences.
async fn terrain(pool: &sqlx::PgPool) -> FolderId {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    record(
        pool,
        terrain,
        "Standing stone, LP-TR-0140",
        1,
        Some("CC BY 4.0"),
    )
    .await;
    record(
        pool,
        rocks,
        "Cliff face, LP-TR-0112",
        2,
        Some("CC BY-NC 4.0"),
    )
    .await;
    record(pool, rocks, "Tor, LP-TR-0188", 3, None).await;
    terrain
}

fn shares() -> String {
    format!("/api/libraries/{SEEDED_LIBRARY}/shares")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_licence_warning_is_counted_before_anything_is_shared(pool: sqlx::PgPool) {
    let terrain = terrain(&pool).await;
    let (status, warning) = send(
        &pool,
        "GET",
        &format!("{}/preview?folderId={}", shares(), terrain.as_uuid()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        warning,
        json!({ "parts": 3, "unrecorded": 1, "nonCommercial": 1 })
    );

    let (_, listed) = send(&pool, "GET", &shares(), None).await;
    assert_eq!(listed, json!([]), "counting shares nothing");

    let (status, refusal) = send(
        &pool,
        "GET",
        &format!(
            "{}/preview?folderId={}",
            shares(),
            FolderId::new().as_uuid()
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchCategory");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn sharing_a_category_lists_it_once_however_often_it_is_shared(pool: sqlx::PgPool) {
    let terrain = terrain(&pool).await;
    let body = json!({ "folderId": terrain.as_uuid() });
    let (status, first) = send(&pool, "POST", &shares(), Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["name"], "Terrain");
    assert_eq!(first["folderId"], terrain.as_uuid().to_string());
    let (_, again) = send(&pool, "POST", &shares(), Some(body)).await;
    assert_eq!(again["id"], first["id"], "the same share");

    let (_, listed) = send(&pool, "GET", &shares(), None).await;
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let (status, everything) = send(&pool, "GET", "/api/shares", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        everything,
        json!([{ "id": first["id"], "name": "Terrain", "partCount": 3 }])
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_category_this_library_does_not_have_cannot_be_shared(pool: sqlx::PgPool) {
    terrain(&pool).await;
    let (status, refusal) = send(
        &pool,
        "POST",
        &shares(),
        Some(json!({ "folderId": FolderId::new().as_uuid() })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchCategory");
    let (status, refusal) = send(
        &pool,
        "POST",
        &shares(),
        Some(json!({ "folder": "Terrain" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "badShare");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn stopping_sharing_withdraws_it_everywhere(pool: sqlx::PgPool) {
    let terrain = terrain(&pool).await;
    let (_, shared) = send(
        &pool,
        "POST",
        &shares(),
        Some(json!({ "folderId": terrain.as_uuid() })),
    )
    .await;
    let stop = format!("/api/shares/{}", shared["id"].as_str().expect("an id"));

    let (status, _) = send(&pool, "DELETE", &stop, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(send(&pool, "GET", &shares(), None).await.1, json!([]));
    assert_eq!(send(&pool, "GET", "/api/shares", None).await.1, json!([]));

    let (status, refusal) = send(&pool, "DELETE", &stop, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "notShared");
}

/// A request under one library never previews or shares another library's category.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn another_librarys_category_is_neither_counted_nor_shared_here(pool: sqlx::PgPool) {
    terrain(&pool).await;
    let shop_floor = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, 'Shop floor', 'shop floor')")
        .bind(shop_floor.as_uuid())
        .execute(&pool)
        .await
        .expect("a second library");
    let fixtures = PgFolders(pool.clone())
        .get_or_create(shop_floor, None, "Fixtures", "Fixtures")
        .await
        .expect("a category of the shop floor");

    let (status, refusal) = send(
        &pool,
        "GET",
        &format!("{}/preview?folderId={}", shares(), fixtures.as_uuid()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchCategory");
    let (status, refusal) = send(
        &pool,
        "POST",
        &shares(),
        Some(json!({ "folderId": fixtures.as_uuid() })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchCategory");
    assert_eq!(
        send(&pool, "GET", "/api/shares", None).await.1,
        json!([]),
        "nothing shared anywhere"
    );
}
