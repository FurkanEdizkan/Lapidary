//! Check-outs over the api: take one, be refused one, hand one back, release one.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // Rows only: no route here reads the store.
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    }
}

async fn send(pool: sqlx::PgPool, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn post(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request builds")
}

async fn seed(pool: &sqlx::PgPool) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Flange DN40, LP-3310-02",
            source_path: "flange-dn40-lp-3310-02.stl",
            folder: None,
            storage_path: Some(
                "libraries/default/flange-dn40-lp-3310-02/flange-dn40-lp-3310-02.stl",
            ),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0x61; 32]),
                size_bytes: 184_342,
                stored_bytes: 184_342,
                zstd_level: 0,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [150.0, 150.0, 18.0],
                triangle_count: 36_868,
                surface_area_mm2: 41_210.5,
                volume_mm3: Some(214_780.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}

async fn controlled(pool: &sqlx::PgPool) -> PartId {
    assert!(
        PgParts(pool.clone())
            .make_controlled(library())
            .await
            .expect("switches the library")
    );
    seed(pool).await
}

/// The part's page says who has it, and a second check-out is refused naming them.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_checkout_names_its_holder_and_a_second_is_refused_naming_them(pool: sqlx::PgPool) {
    let part = controlled(&pool).await;
    let (status, taken) = send(
        pool.clone(),
        post(
            &format!("/api/parts/{part}/checkout"),
            serde_json::json!({ "holder": "mira@workshop-pc" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    assert_eq!(taken["lock"]["holder"], "mira@workshop-pc");
    assert!(taken["revision"].is_string(), "the revision the copy is of");

    let (_, detail) = send(pool.clone(), get(&format!("/api/parts/{part}"))).await;
    assert_eq!(detail["lock"]["holder"], "mira@workshop-pc");
    assert_eq!(detail["lock"]["id"], taken["lock"]["id"]);

    let (status, refused) = send(
        pool,
        post(
            &format!("/api/parts/{part}/checkout"),
            serde_json::json!({ "holder": "jonas@laptop" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["reason"], "checkedOut");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("mira@workshop-pc")),
        "{refused}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_hobby_library_refuses_a_checkout_and_a_nameless_one_is_a_400(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let (status, refused) = send(
        pool.clone(),
        post(
            &format!("/api/parts/{part}/checkout"),
            serde_json::json!({ "holder": "mira@workshop-pc" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["reason"], "hobbyLibrary");

    let (status, _) = send(
        pool,
        post(
            &format!("/api/parts/{part}/checkout"),
            serde_json::json!({ "holder": "   " }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Checking in and releasing each free the part once, and say so when there is nothing left.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn checking_in_and_releasing_each_free_the_part_once(pool: sqlx::PgPool) {
    let part = controlled(&pool).await;
    let checkout = || {
        post(
            &format!("/api/parts/{part}/checkout"),
            serde_json::json!({ "holder": "mira@workshop-pc" }),
        )
    };

    let (_, taken) = send(pool.clone(), checkout()).await;
    let lock = taken["lock"]["id"].clone();
    let checkin = || {
        post(
            &format!("/api/parts/{part}/checkin"),
            serde_json::json!({ "lock": lock }),
        )
    };
    let (status, _) = send(pool.clone(), checkin()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, again) = send(pool.clone(), checkin()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["reason"], "notHeld");

    let (status, _) = send(pool.clone(), checkout()).await;
    assert_eq!(status, StatusCode::CREATED, "free again after the check-in");
    let release = || {
        post(
            &format!("/api/parts/{part}/lock/release"),
            serde_json::json!({ "by": "jonas@laptop" }),
        )
    };
    let (status, _) = send(pool.clone(), release()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, again) = send(pool.clone(), release()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["reason"], "notCheckedOut");

    let (_, detail) = send(pool, get(&format!("/api/parts/{part}"))).await;
    assert!(detail["lock"].is_null(), "nobody holds it now: {detail}");
}
