//! A28: the sign-in wall. Lena (the test account, an admin) owns a bracket with a picture; Mira is an ordinary
//! account; nobody at all is the signed-out visitor the wall is for.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
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

async fn send(
    pool: &sqlx::PgPool,
    as_mira: bool,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if as_mira {
        request = request.header(
            header::COOKIE,
            format!(
                "{}={}",
                lapidary_api::auth::COOKIE,
                MIRAS_TOKEN
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
        );
    }
    if body.is_some() {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    let app = if as_mira {
        router(state(pool), Role::Api)
    } else {
        common::router(state(pool), Role::Api)
    };
    let response = app
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

/// A request with no cookie at all: the visitor the sign-in page is for.
async fn signed_out(pool: &sqlx::PgPool, uri: &str) -> (StatusCode, Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
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

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_wall_shows_only_pictures_of_parts_shared_with_everyone_and_not_hidden(
    pool: sqlx::PgPool,
) {
    common::ensure_session(&pool).await;
    let part = lenas_bracket(&pool).await;
    mira(&pool).await;
    sqlx::query(
        "INSERT INTO part_image (id, part_id, image_webp, origin) VALUES (gen_random_uuid(), $1, $2, 'uploaded')",
    )
    .bind(part.as_uuid())
    .bind(&b"RIFF\0\0\0\0WEBPVP8 "[..])
    .execute(&pool)
    .await
    .expect("a picture");
    let wall = || signed_out(&pool, "/api/wall");

    // Private, then shared with Mira alone: on nobody's sign-in page.
    assert_eq!(wall().await, (StatusCode::OK, json!([])));
    let shares = format!("/api/parts/{part}/shares");
    send(
        &pool,
        false,
        "PUT",
        &shares,
        Some(json!({ "everyone": false, "people": [MIRA] })),
    )
    .await;
    assert_eq!(wall().await.1, json!([]));

    // Shared with everyone: its picture, and nothing else about it.
    send(
        &pool,
        false,
        "PUT",
        &shares,
        Some(json!({ "everyone": true, "people": [] })),
    )
    .await;
    let (status, pictures) = wall().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pictures.as_array().map(Vec::len), Some(1));
    let mut keys: Vec<_> = pictures[0]
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, ["fit", "focusX", "focusY", "src"]);
    assert!(
        pictures[0]["src"]
            .as_str()
            .is_some_and(|src| src.starts_with("data:image/webp;base64,"))
    );

    // Only its owner hides it; hidden, it is gone.
    let switch = format!("/api/parts/{part}/wall");
    assert_eq!(
        send(&pool, false, "GET", &switch, None).await.1,
        json!({ "on": true })
    );
    assert_eq!(
        send(&pool, true, "PUT", &switch, Some(json!({ "on": false })))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&pool, false, "PUT", &switch, Some(json!({ "on": false })))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(wall().await.1, json!([]));
    send(&pool, false, "PUT", &switch, Some(json!({ "on": true }))).await;
    assert_eq!(wall().await.1.as_array().map(Vec::len), Some(1));

    // The admin's switch takes the whole wall away, and only an admin's does.
    assert_eq!(
        send(
            &pool,
            true,
            "PUT",
            "/api/settings/wall",
            Some(json!({ "on": false }))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &pool,
            false,
            "PUT",
            "/api/settings/wall",
            Some(json!({ "on": false }))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&pool, false, "GET", "/api/settings/wall", None)
            .await
            .1,
        json!({ "on": false })
    );
    assert_eq!(wall().await.1, json!([]));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_picture_hash_is_served_signed_out_only_while_the_wall_shows_it(pool: sqlx::PgPool) {
    common::ensure_session(&pool).await;
    let part = lenas_bracket(&pool).await;
    let hash = BlobHash::from_bytes(*blake3::hash(b"a photograph of the bracket").as_bytes());
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level) VALUES ($1, 27, 27, 0)",
    )
    .bind(hash.to_hex())
    .execute(&pool)
    .await
    .expect("a blob");
    sqlx::query("INSERT INTO part_image (id, part_id, blake3, origin) VALUES (gen_random_uuid(), $1, $2, 'uploaded')")
        .bind(part.as_uuid())
        .bind(hash.to_hex())
        .execute(&pool)
        .await
        .expect("a picture");
    let wall = lapidary_db::PgWall(pool.clone());

    assert!(!wall.shows(&hash).await.expect("checks"));
    assert_eq!(
        signed_out(&pool, &format!("/api/wall/{}", hash.to_hex()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE part SET shared_with_everyone = true WHERE id = $1")
        .bind(part.as_uuid())
        .execute(&pool)
        .await
        .expect("shared");
    assert!(wall.shows(&hash).await.expect("checks"));
    assert_eq!(
        signed_out(&pool, "/api/wall").await.1[0]["src"],
        json!(format!("/api/wall/{}", hash.to_hex()))
    );
    wall.set_on_wall(part, false).await.expect("hidden");
    assert!(!wall.shows(&hash).await.expect("checks"));
}
