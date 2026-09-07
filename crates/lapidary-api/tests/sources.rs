//! Where a part came from: what the routes accept, and what they refuse.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{LibraryId, PartId};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    }
}

async fn seed_part(pool: &sqlx::PgPool) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: Some("libraries/default/vee-block-lp-3072-02/vee-block-lp-3072-02.stl"),
            library: LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid")),
            name: "Vee block, LP-3072-02",
            source_path: "vee-block-lp-3072-02.stl",
            blob: &StoredBlobRow {
                hash: lapidary_core::BlobHash::from_bytes([0x5a; 32]),
                size_bytes: 82_144,
                stored_bytes: 82_144,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [60.0, 60.0, 40.0],
                triangle_count: 1_648,
                surface_area_mm2: 18_400.0,
                volume_mm3: Some(64_800.0),
                is_watertight: true,
            },
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("a part to record a source against")
}

async fn send(state: AppState, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = router(state, Role::Api)
        .oneshot(request)
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

fn post(part: PartId, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/parts/{part}/sources"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

fn list(part: PartId) -> Request<Body> {
    Request::builder()
        .uri(format!("/api/parts/{part}/sources"))
        .body(Body::empty())
        .expect("request builds")
}

/// The whole shape, round-tripped — including the licence, which is the field `DATA.md` is
/// most insistent about: somebody selling prints has to be able to see a model was
/// non-commercial before they print it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_source_is_recorded_and_read_back_whole(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;

    let (status, body) = send(
        state(pool.clone()),
        post(
            part,
            serde_json::json!({
                "url": "https://www.mcmaster.com/8975K51/",
                "vendor": "McMaster-Carr",
                "externalId": "8975K51",
                "title": "Multipurpose 6061 aluminium V-block",
                "license": "CC-BY-NC-SA 4.0",
                "priceMinor": 4_275,
                "currency": "USD",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(state(pool), list(part)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["vendor"], "McMaster-Carr");
    assert_eq!(body[0]["license"], "CC-BY-NC-SA 4.0");
    // Minor units, so 42.75 USD is 4275. Never a float: a price is money, and money in
    // binary floating point is a rounding error waiting for a total to be taken of it.
    assert_eq!(body[0]["priceMinor"], 4_275);
    assert_eq!(body[0]["currency"], "USD");
}

/// The same URL twice is somebody correcting what they typed. `0015`'s `unique (part_id,
/// url)` would refuse the second write; the `ON CONFLICT` turns it into an edit.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_url_twice_corrects_the_row_rather_than_refusing(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;
    let url = "https://www.printables.com/model/482910";

    send(
        state(pool.clone()),
        post(
            part,
            serde_json::json!({ "url": url, "vendor": "Printablse" }),
        ),
    )
    .await;
    let (status, _) = send(
        state(pool.clone()),
        post(
            part,
            serde_json::json!({ "url": url, "vendor": "Printables" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, body) = send(state(pool), list(part)).await;
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(1),
        "corrected, not added"
    );
    assert_eq!(body[0]["vendor"], "Printables");
}

/// A part with the model from one place and the hardware from another. Two rows, because
/// two different URLs are two different sources — which is exactly what `0015` declined to
/// put a `unique (part_id)` on.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_can_carry_more_than_one_source(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;
    for url in [
        "https://www.printables.com/model/482910",
        "https://www.mcmaster.com/91290A115/",
    ] {
        send(
            state(pool.clone()),
            post(part, serde_json::json!({ "url": url })),
        )
        .await;
    }
    let (_, body) = send(state(pool), list(part)).await;
    assert_eq!(body.as_array().map(Vec::len), Some(2));
}

/// A price without a currency is a number nobody can spend. The column has the same opinion
/// — `part_source_price_has_currency` — and checking first is what makes it a sentence
/// rather than a 500 with a constraint name in the log.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_price_without_a_currency_is_refused_with_a_sentence(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;
    let (status, body) = send(
        state(pool),
        post(
            part,
            serde_json::json!({ "url": "https://example.com/p", "priceMinor": 1_250 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("currency")),
        "{body}"
    );
}

/// A form posts `""` for every box nobody typed in. Empty is absent, and a source with
/// nothing in it at all is refused rather than stored as a row of nulls that reads like a
/// source somebody recorded and left blank.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_form_of_empty_boxes_records_nothing(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;
    let (status, body) = send(
        state(pool.clone()),
        post(
            part,
            serde_json::json!({ "url": "  ", "vendor": "", "title": "", "license": "" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (_, body) = send(state(pool), list(part)).await;
    assert_eq!(body.as_array().map(Vec::len), Some(0));
}

/// `0015`'s trade-show case: a part somebody bought at a stand, with a vendor and a price
/// and no link at all. A row like that is a real answer and has to be storable.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_source_with_no_url_is_still_a_source(pool: sqlx::PgPool) {
    let part = seed_part(&pool).await;
    let (status, body) = send(
        state(pool.clone()),
        post(
            part,
            serde_json::json!({
                "vendor": "Rutland Plastics, Hall 4",
                "priceMinor": 1_800,
                "currency": "GBP",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, body) = send(state(pool), list(part)).await;
    assert_eq!(body[0]["url"], serde_json::Value::Null);
    assert_eq!(body[0]["vendor"], "Rutland Plastics, Hall 4");
}
