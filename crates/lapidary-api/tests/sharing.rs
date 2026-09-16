//! Sharing (S1b) through the API: this installation's identity, and pairing with and removing people.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{DeviceId, PartId, ShareId};
use lapidary_db::{MirroredPartIn, OfferedRemote, PgMirror, PgSharing};
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
        // A path pasted with no scheme: only the refusal of `/` in a host catches this one.
        "nas.local/lapidary:8082",
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

/// Ayşe shares Terrain, and this installation has read its catalogue: two parts, one with a thumbnail and a
/// licence. Answers the mirrored share's id.
async fn mirrored_terrain(pool: &sqlx::PgPool) -> String {
    PgSharing(pool.clone())
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("pairs");
    PgSharing(pool.clone())
        .seen(ayse(), Some("Ayşe's workshop"))
        .await
        .expect("has said hello");
    let mirror = PgMirror(pool.clone());
    let remote = ShareId::from_uuid(
        "01a07c41-5d22-7b03-9014-7e2f6dab0001"
            .parse()
            .expect("uuid"),
    );
    let stale = mirror
        .take_offer(
            ayse(),
            &[OfferedRemote {
                remote,
                name: "Terrain",
                part_count: 2,
                digest: "2-100",
            }],
        )
        .await
        .expect("takes the offer");
    let licences = ["CC BY-NC 4.0".to_owned()];
    let part = |source_path, name, thumbnail| MirroredPartIn {
        source_path,
        remote_part: PartId::new(),
        name,
        part_number: None,
        tags: &[],
        licences: &licences,
        blake3: Some("5c0f8d3e9a1b2c4d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5"),
        size_bytes: Some(204_800),
        format: Some("stl"),
        thumbnail,
    };
    mirror
        .replace_catalogue(
            stale[0].id,
            "2-100",
            &[
                part(
                    "rocks/cliff-face.stl",
                    "Cliff face, LP-TR-0112",
                    Some(b"RIFF\x24\0\0\0WEBPVP8 cliff".as_slice()),
                ),
                part("standing-stone.stl", "Standing stone, LP-TR-0140", None),
            ],
        )
        .await
        .expect("reads the catalogue");
    stale[0].id.as_uuid().to_string()
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn what_a_paired_person_shares_is_listed_and_a_removed_persons_is_not(pool: sqlx::PgPool) {
    let id = mirrored_terrain(&pool).await;
    let shares = format!("/api/sharing/peers/{}/shares", ayse());

    let (status, listed) = send(&pool, "GET", &shares, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["id"], id);
    assert_eq!(listed[0]["name"], "Terrain");
    assert_eq!(listed[0]["partCount"], 2);
    assert_eq!(listed[0]["sharer"], "Ayşe's workshop");
    assert!(
        listed[0]["syncedAt"].is_string(),
        "its catalogue has been read"
    );

    PgSharing(pool.clone())
        .remove_peer(ayse())
        .await
        .expect("removes");
    assert_eq!(send(&pool, "GET", &shares, None).await.1, json!([]));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_shared_librarys_parts_page_by_path_with_their_licences(pool: sqlx::PgPool) {
    let id = mirrored_terrain(&pool).await;
    let (status, first) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{id}/parts?limit=1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["parts"][0]["sourcePath"], "rocks/cliff-face.stl");
    assert_eq!(first["parts"][0]["licences"], json!(["CC BY-NC 4.0"]));
    assert_eq!(first["parts"][0]["thumbnail"], true);
    let next = first["next"].as_str().expect("a full page has a next");

    let uri = format!(
        "/api/sharing/shares/{id}/parts?limit=1&after={}",
        next.replace('/', "%2F")
    );
    let (_, second) = send(&pool, "GET", &uri, None).await;
    assert_eq!(second["parts"][0]["sourcePath"], "standing-stone.stl");

    let (status, share) = send(&pool, "GET", &format!("/api/sharing/shares/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(share["deviceId"], ayse().to_string());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_mirrored_thumbnail_is_served_and_a_part_without_one_is_not_found(pool: sqlx::PgPool) {
    let id = mirrored_terrain(&pool).await;
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
    .oneshot(
        Request::builder()
            .uri(format!(
                "/api/sharing/shares/{id}/thumbnail?path=rocks%2Fcliff-face.stl"
            ))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await
    .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/webp");

    let (status, refusal) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{id}/thumbnail?path=standing-stone.stl"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noThumbnail");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_share_no_longer_offered_or_from_somebody_removed_is_not_found(pool: sqlx::PgPool) {
    let id = mirrored_terrain(&pool).await;
    let (status, refusal) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{}", unmirrored_share_id()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchShare");

    PgSharing(pool.clone())
        .remove_peer(ayse())
        .await
        .expect("removes");
    let (status, refusal) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{id}/parts"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["reason"], "noSuchShare");
}

/// A share id nobody mirrored.
fn unmirrored_share_id() -> String {
    lapidary_core::PeerShareId::new().as_uuid().to_string()
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_pull_is_recorded_for_the_peer_role_and_followed_from_its_share(pool: sqlx::PgPool) {
    let id = mirrored_terrain(&pool).await;
    let (status, none_yet) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{id}/pull"),
        None,
    )
    .await;
    assert_eq!((status, none_yet), (StatusCode::OK, Value::Null));

    let seeded = "01931b6e-0000-7000-8000-000000000001";
    let (status, pull) = send(
        &pool,
        "POST",
        &format!("/api/sharing/shares/{id}/pulls"),
        Some(json!({ "libraryId": seeded })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{pull}");
    assert_eq!(pull["state"], "queued");
    assert_eq!(pull["libraryId"], seeded);

    let (status, followed) = send(
        &pool,
        "GET",
        &format!("/api/sharing/shares/{id}/pull"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(followed["id"], pull["id"]);

    let (status, refusal) = send(
        &pool,
        "POST",
        &format!("/api/sharing/shares/{id}/pulls"),
        Some(json!({ "libraryId": "01a07c41-5d22-7b03-9014-7e2f6dab0999" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    let (status, refusal) = send(
        &pool,
        "POST",
        &format!("/api/sharing/shares/{}/pulls", unmirrored_share_id()),
        Some(json!({ "libraryId": seeded })),
    )
    .await;
    assert_eq!(
        (status, &refusal["reason"]),
        (StatusCode::NOT_FOUND, &json!("noSuchShare"))
    );
}
