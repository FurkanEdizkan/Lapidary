//! A4: sharing a model, and the admin's accounts. Lena (the test account, an admin) owns the seeded
//! library and a bracket; Mira is a second, ordinary account with her own session.

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

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_shared_model_is_seen_and_downloadable_and_never_changed_by_its_reader(
    pool: sqlx::PgPool,
) {
    common::ensure_session(&pool).await;
    let part = lenas_bracket(&pool).await;
    mira(&pool).await;
    let shares = format!("/api/parts/{part}/shares");

    assert_eq!(
        send(&pool, true, "GET", &format!("/api/parts/{part}"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = send(
        &pool,
        false,
        "PUT",
        &shares,
        Some(json!({ "everyone": false, "people": [MIRA] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        send(&pool, false, "GET", &shares, None).await.1,
        json!({ "everyone": false, "people": [MIRA] })
    );

    // Mira sees it, under Shared with me, and is told it is not hers.
    let (status, detail) = send(&pool, true, "GET", &format!("/api/parts/{part}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["yours"], false);
    let (_, shared) = send(&pool, true, "GET", "/api/shared", None).await;
    assert_eq!(shared["parts"][0]["id"], json!(part));
    // Lena's own part is not "shared with" Lena.
    assert_eq!(
        send(&pool, false, "GET", "/api/shared", None).await.1["parts"],
        json!([])
    );
    // She cannot change it, delete it, see who else it is shared with, or reach the library.
    for (method, uri, body) in [
        (
            "PUT",
            format!("/api/parts/{part}/tags"),
            Some(json!({ "tags": ["mine"] })),
        ),
        ("DELETE", format!("/api/parts/{part}"), None),
        (
            "PUT",
            shares.clone(),
            Some(json!({ "everyone": true, "people": [] })),
        ),
    ] {
        assert_eq!(
            send(&pool, true, method, &uri, body).await.0,
            StatusCode::FORBIDDEN,
            "{method} {uri}"
        );
    }
    assert_eq!(
        send(&pool, true, "GET", &shares, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &pool,
            true,
            "GET",
            &format!("/api/libraries/{LENAS}/parts"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Unshared, it is gone for her; shared with everybody, it is back.
    send(
        &pool,
        false,
        "PUT",
        &shares,
        Some(json!({ "everyone": false, "people": [] })),
    )
    .await;
    assert_eq!(
        send(&pool, true, "GET", &format!("/api/parts/{part}"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    send(
        &pool,
        false,
        "PUT",
        &shares,
        Some(json!({ "everyone": true, "people": [] })),
    )
    .await;
    assert_eq!(
        send(&pool, true, "GET", &format!("/api/parts/{part}"), None)
            .await
            .0,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_admin_makes_resets_and_deletes_accounts_and_nothing_is_lost(pool: sqlx::PgPool) {
    common::ensure_session(&pool).await;
    mira(&pool).await;

    // Only an admin makes accounts.
    let theo = json!({ "username": "theo.brandt", "displayName": "Theo Brandt", "password": "a long first one" });
    assert_eq!(
        send(&pool, true, "POST", "/api/people", Some(theo.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, made) = send(&pool, false, "POST", "/api/people", Some(theo.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(
        send(&pool, false, "POST", "/api/people", Some(theo))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let id = made["id"].as_str().expect("an id").to_owned();

    // Theo starts with an empty library of his own, and signs in with what the admin set.
    let libraries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM library WHERE owner_id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("counts");
    assert_eq!(libraries, 1);
    let (status, _) = send(
        &pool,
        true,
        "POST",
        "/api/session",
        Some(json!({ "username": "theo.brandt", "password": "a long first one" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A reset is the admin's, not Mira's.
    let reset = format!("/api/people/{id}/password");
    assert_eq!(
        send(
            &pool,
            true,
            "PUT",
            &reset,
            Some(json!({ "password": "another long one" }))
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
            &reset,
            Some(json!({ "password": "another long one" }))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    // Deleting Theo gives his library to the admin, renamed beside hers when she has one called the same.
    sqlx::query("UPDATE library SET name = 'My models' WHERE id = $1::uuid")
        .bind(LENAS)
        .execute(&pool)
        .await
        .expect("Lena's library shares the name");
    assert_eq!(
        send(
            &pool,
            false,
            "DELETE",
            &format!("/api/people/{}", common::USER),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "not yourself"
    );
    assert_eq!(
        send(&pool, false, "DELETE", &format!("/api/people/{id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM library WHERE owner_id = $1 ORDER BY name")
            .bind(common::USER)
            .fetch_all(&pool)
            .await
            .expect("names");
    assert_eq!(names, ["My models", "My models (theo.brandt)"]);

    // Changing your own password needs the current one.
    let change = |current: &str| json!({ "current": current, "password": "mira's new password" });
    sqlx::query("UPDATE app_user SET password_hash = $2 WHERE id = $1")
        .bind(MIRA)
        .bind(
            lapidary_api::auth::hash_password("mira's old password".into())
                .await
                .expect("hashes"),
        )
        .execute(&pool)
        .await
        .expect("a known password");
    assert_eq!(
        send(
            &pool,
            true,
            "PUT",
            "/api/profile/password",
            Some(change("not it"))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &pool,
            true,
            "PUT",
            "/api/profile/password",
            Some(change("mira's old password"))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // This session survives its own change.
    assert_eq!(
        send(&pool, true, "GET", "/api/libraries", None).await.0,
        StatusCode::OK
    );
}
