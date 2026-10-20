//! A3: a person reaches their own models and nobody else's. Lena (the test account) owns the seeded
//! library and a bracket in it; Mira, a second account with her own session, tries every way in.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, UserId};
use lapidary_db::{IngestRequest, PgBlobs, PgIngest, PgParts, PgSiteKeys, StoredBlobRow};
use serde_json::{Value, json};
use tower::ServiceExt;

const LENAS: &str = "01931b6e-0000-7000-8000-000000000001";
const MIRA: uuid::Uuid = uuid::uuid!("01931b6e-0000-7000-8000-00000000b002");
const MIRAS_TOKEN: [u8; 32] = [0x6b; 32];

fn state(pool: &sqlx::PgPool) -> AppState {
    AppState {
        db: pool.clone(),
        blob_root: "/nonexistent-blob-root".into(),
        upload_dir: "/nonexistent-upload-dir".into(),
        host_storage_root: None,
        ingest_dir: "/nonexistent-ingest".into(),
        inbox_dir: "/nonexistent-inbox".into(),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    }
}

/// As Mira: the real router, with her cookie and nobody else's.
async fn as_mira(
    pool: &sqlx::PgPool,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let cookie = format!(
        "{}={}",
        lapidary_api::auth::COOKIE,
        MIRAS_TOKEN
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
    if body.is_some() {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    let response = router(state(pool), Role::Api)
        .oneshot(
            request
                .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
                .expect("request"),
        )
        .await
        .expect("responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn lenas_bracket(pool: &sqlx::PgPool) -> PartId {
    let bytes = b"solid bracket_lp_1042_03\nendsolid bracket_lp_1042_03\n";
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: LibraryId::from_uuid(uuid::Uuid::parse_str(LENAS).expect("uuid")),
            name: "Bracket, LP-1042-03",
            source_path: "brackets/bracket-lp-1042-03.stl",
            folder: None,
            storage_path: Some("brackets/bracket-lp-1042-03.stl"),
            root: lapidary_core::FileRoot::Library,
            mtime_ns: Some(1),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()),
                size_bytes: bytes.len() as u64,
                stored_bytes: bytes.len() as u64,
                zstd_level: 0,
            },
            measurements: Some(&MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            }),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("Lena's bracket")
}

async fn mira(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO app_user (id, username, display_name, password_hash) \
         VALUES ($1, 'mira.okafor', 'Mira Okafor', 'not-a-password-hash')",
    )
    .bind(MIRA)
    .execute(pool)
    .await
    .expect("Mira");
    sqlx::query(
        "INSERT INTO session (token_hash, user_id, expires_at) VALUES ($1, $2, now() + interval '1 day')",
    )
    .bind(&blake3::hash(&MIRAS_TOKEN).as_bytes()[..])
    .bind(MIRA)
    .execute(pool)
    .await
    .expect("Mira's session");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn another_persons_library_part_file_and_pictures_are_not_there(pool: sqlx::PgPool) {
    common::ensure_session(&pool).await;
    let part = lenas_bracket(&pool).await;
    // A picture of Lena's, stored by hash.
    let picture = BlobHash::from_bytes([0x3c; 32]);
    PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash: picture,
            size_bytes: 90_000,
            stored_bytes: 90_000,
            zstd_level: 0,
        })
        .await
        .expect("a stored picture");
    PgParts(pool.clone())
        .add_part_image(
            part,
            lapidary_db::NewPartImage {
                bytes: lapidary_db::ImageBytes::Blob(&picture),
                origin: "uploaded",
                source_url: None,
            },
        )
        .await
        .expect("Lena's picture");
    mira(&pool).await;

    // Her own list holds none of Lena's libraries.
    let (status, libraries) = as_mira(&pool, "GET", "/api/libraries", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(libraries, json!([]));

    // Every way in to Lena's library and part is the answer for an unknown one.
    for (method, uri) in [
        ("GET", format!("/api/libraries/{LENAS}/parts")),
        ("GET", format!("/api/libraries/{LENAS}/facets")),
        ("GET", format!("/api/libraries/{LENAS}/folders")),
        ("GET", format!("/api/libraries/{LENAS}/sources")),
        ("GET", format!("/api/libraries/{LENAS}/tags")),
        ("POST", format!("/api/libraries/{LENAS}/scan")),
        ("GET", format!("/api/parts/{part}")),
        ("GET", format!("/api/parts/{part}/file")),
        ("GET", format!("/api/parts/{part}/images")),
        ("PUT", format!("/api/parts/{part}/tags")),
        ("DELETE", format!("/api/parts/{part}")),
        ("GET", format!("/api/blob/{}", picture.to_hex())),
    ] {
        let body = (method == "PUT").then(|| json!({ "tags": ["mine"] }));
        let (status, _) = as_mira(&pool, method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
    // The picture's hash reaches Lena and not Mira: knowing a hash is not access.
    let parts = PgParts(pool.clone());
    assert!(
        parts
            .image_is_reachable(&picture, UserId::from_uuid(common::USER))
            .await
            .expect("asks")
    );
    assert!(
        !parts
            .image_is_reachable(&picture, UserId::from_uuid(MIRA))
            .await
            .expect("asks")
    );
    // And it is still Lena's, untouched.
    assert!(PgParts(pool.clone()).part_exists(part).await.expect("asks"));

    // A dashboard tile on Lena's library tells Mira nothing.
    let (status, resolved) = as_mira(
        &pool,
        "POST",
        "/api/dashboard/resolve",
        Some(json!({ "widgets": [{ "key": "theirs", "widget": { "kind": "recent", "library": LENAS, "limit": 6 } }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(
        resolved["results"][0]["result"]["status"], "failed",
        "{resolved}"
    );

    // Each person's site keys are their own.
    PgSiteKeys(pool.clone(), UserId::from_uuid(common::USER))
        .set("thingiverse", "lenas-app-token-4f2a")
        .await
        .expect("Lena's key");
    let (_, sites) = as_mira(&pool, "GET", "/api/settings/sites", None).await;
    assert!(
        sites["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .all(|site| site["hint"].is_null()),
        "{sites}"
    );

    // Mira may call her library what Lena calls hers.
    let (status, made) = as_mira(
        &pool,
        "POST",
        "/api/libraries",
        Some(json!({ "name": "Default" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let (_, libraries) = as_mira(&pool, "GET", "/api/libraries", None).await;
    assert_eq!(libraries.as_array().expect("a list").len(), 1);
}

/// A38: the router decodes a percent-encoded id into the same id, so an encoded id must not slip past the check.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_id_written_any_other_way_is_refused_not_let_through(pool: sqlx::PgPool) {
    common::ensure_session(&pool).await;
    let part = lenas_bracket(&pool).await;
    mira(&pool).await;
    // `0` is %30: the same id, written so a plain parse fails.
    let id = part.to_string();
    let encoded = format!("%3{}{}", &id[..1], &id[1..]);
    let library = format!("%3{}{}", &LENAS[..1], &LENAS[1..]);
    for (method, uri, body) in [
        ("GET", format!("/api/parts/{encoded}"), None),
        ("GET", format!("/api/parts/{encoded}/file"), None),
        (
            "PUT",
            format!("/api/parts/{encoded}/shares"),
            Some(json!({ "everyone": true, "people": [] })),
        ),
        ("DELETE", format!("/api/parts/{encoded}"), None),
        ("GET", format!("/api/libraries/{library}/parts"), None),
    ] {
        let (status, _) = as_mira(&pool, method, &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
    assert!(
        PgParts(pool.clone()).part_exists(part).await.expect("asks"),
        "Lena's bracket is still hers"
    );
}
