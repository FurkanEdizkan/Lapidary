//! Sharing (S1b) through the API: this installation's identity, and pairing with and removing people.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::DeviceId;
use lapidary_db::PgSharing;
use serde_json::{Value, json};
use tower::ServiceExt;

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

/// Ayşe's workshop PC, as its own sharing page shows its id.
fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn sharing_is_off_until_the_peer_role_has_claimed_an_id(pool: sqlx::PgPool) {
    let (status, identity) = send(&pool, "GET", "/api/sharing/identity", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity, json!({ "deviceId": null, "name": null }));

    let (status, refusal) = send(
        &pool,
        "PUT",
        "/api/sharing/identity",
        Some(json!({ "name": "Furkan's workbench" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["reason"], "sharingOff");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|message| message.contains("compose.sharing.yaml")),
        "says how to switch it on: {refusal}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_name_this_installation_goes_by_is_trimmed_cleared_and_kept_short(pool: sqlx::PgPool) {
    let here = DeviceId::from_public_key(b"the key this installation's peer role made");
    PgSharing(pool.clone())
        .claim_identity(here)
        .await
        .expect("claims");

    let (status, _) = send(
        &pool,
        "PUT",
        "/api/sharing/identity",
        Some(json!({ "name": "  Furkan's workbench " })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, identity) = send(&pool, "GET", "/api/sharing/identity", None).await;
    assert_eq!(
        identity,
        json!({ "deviceId": here.to_string(), "name": "Furkan's workbench" })
    );

    let (status, refusal) = send(
        &pool,
        "PUT",
        "/api/sharing/identity",
        Some(json!({ "name": "w".repeat(65) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "badName");

    let (status, _) = send(
        &pool,
        "PUT",
        "/api/sharing/identity",
        Some(json!({ "name": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, identity) = send(&pool, "GET", "/api/sharing/identity", None).await;
    assert_eq!(
        identity["name"],
        Value::Null,
        "nothing but spaces clears it"
    );
}

/// What two people paste to each other. The id may arrive as it was read aloud or retyped: lower case,
/// spaced out. It is stored and shown the one way `DeviceId` prints it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn pairing_by_pasting_an_id_and_an_address_lists_them(pool: sqlx::PgPool) {
    let pasted = ayse().to_string().to_lowercase().replace('-', " ");
    let (status, peer) = send(
        &pool,
        "POST",
        "/api/sharing/peers",
        Some(json!({ "deviceId": pasted, "address": " 192.168.1.24:8082 " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{peer}");
    assert_eq!(peer["deviceId"], ayse().to_string());
    assert_eq!(peer["address"], "192.168.1.24:8082");
    assert_eq!(peer["online"], false, "not said hello to yet");
    assert_eq!(peer["name"], Value::Null);

    let (status, listed) = send(&pool, "GET", "/api/sharing/peers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert_eq!(listed[0]["deviceId"], ayse().to_string());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_mistyped_id_or_an_address_nothing_could_reach_is_refused_in_words(pool: sqlx::PgPool) {
    let (status, refusal) = send(
        &pool,
        "POST",
        "/api/sharing/peers",
        Some(json!({ "deviceId": "0V5KQ-7M2JD", "address": "192.168.1.24:8082" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "badDeviceId");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|message| message.contains("Copy it from the sharing page")),
        "{refusal}"
    );

    for address in [
        "https://192.168.1.24:8082",
        "192.168.1.24",
        "192.168.1.24:0",
        "192.168.1.24:99999",
        "work shop:8082",
        "fd7a:115c:a1e0::3:8082",
    ] {
        let (status, refusal) = send(
            &pool,
            "POST",
            "/api/sharing/peers",
            Some(json!({ "deviceId": ayse().to_string(), "address": address })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address}");
        assert_eq!(refusal["reason"], "badAddress", "{address}");
    }

    let (_, listed) = send(&pool, "GET", "/api/sharing/peers", None).await;
    assert_eq!(listed, json!([]), "nothing was written for any of them");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn this_installations_own_id_is_refused(pool: sqlx::PgPool) {
    let here = DeviceId::from_public_key(b"the key this installation's peer role made");
    PgSharing(pool.clone())
        .claim_identity(here)
        .await
        .expect("claims");
    let (status, refusal) = send(
        &pool,
        "POST",
        "/api/sharing/peers",
        Some(json!({ "deviceId": here.to_string(), "address": "127.0.0.1:8082" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "ownDeviceId");
}

/// Removal is soft: gone from the list, and the row kept, which adding the same id again shows by
/// coming back as the row it was.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn removing_somebody_hides_them_and_keeps_their_row(pool: sqlx::PgPool) {
    let pair = json!({ "deviceId": ayse().to_string(), "address": "192.168.1.24:8082" });
    let (_, first) = send(&pool, "POST", "/api/sharing/peers", Some(pair.clone())).await;
    let removal = format!("/api/sharing/peers/{}", ayse());

    let (status, _) = send(&pool, "DELETE", &removal, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = send(&pool, "GET", "/api/sharing/peers", None).await;
    assert_eq!(listed, json!([]));

    let (status, refusal) = send(&pool, "DELETE", &removal, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "removed already");
    assert_eq!(refusal["reason"], "notPaired");

    let (_, again) = send(&pool, "POST", "/api/sharing/peers", Some(pair)).await;
    assert_eq!(again["addedAt"], first["addedAt"], "the same row came back");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn removing_by_an_id_that_is_not_one_is_refused(pool: sqlx::PgPool) {
    let (status, refusal) = send(&pool, "DELETE", "/api/sharing/peers/not-an-id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "badDeviceId");
}
