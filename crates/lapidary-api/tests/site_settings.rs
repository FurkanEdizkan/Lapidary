//! F3's routes: the Settings page's keys, their check, and queueing an import from a model's page.
//! Nothing here reaches a real site — every check that would is refused before it asks — and
//! `site_settings_off.rs` holds the one test that turns importing off, in its own process, because
//! the switch is an environment variable.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use serde_json::{Value, json};
use tower::ServiceExt;

const LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

async fn send(
    pool: &sqlx::PgPool,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String) {
    let state = AppState {
        db: pool.clone(),
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router(state, Role::Api)
        .oneshot(
            request
                .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
                .expect("request"),
        )
        .await
        .expect("responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body).expect("json")
}

/// A41: the admin turns importing off and on again from Settings, after the wizard.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn importing_is_switched_off_and_on_from_settings(pool: sqlx::PgPool) {
    let (status, body) = send(&pool, "GET", "/api/settings/sites", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&body)["forcedOff"], false);
    for on in [false, true] {
        let (status, body) = send(
            &pool,
            "PUT",
            "/api/settings/sites",
            Some(json!({ "enabled": on })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
        let (_, body) = send(&pool, "GET", "/api/settings/sites", None).await;
        assert_eq!(json_of(&body)["enabled"], on);
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_saved_key_is_listed_by_its_last_four_characters_and_never_returned(pool: sqlx::PgPool) {
    let (status, body) = send(&pool, "GET", "/api/settings/sites", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = json_of(&body);
    assert_eq!(listed["enabled"], true);
    let sites: Vec<&str> = listed["sites"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|s| s["site"].as_str().expect("site"))
        .collect();
    assert_eq!(sites, ["printables", "thingiverse", "myminifactory"]);
    assert_eq!(listed["sites"][0]["needsKey"], false);
    assert!(listed["sites"][1]["hint"].is_null());

    let secret = "tv-0193a0c2-secret-token-9f2e";
    let (status, _) = send(
        &pool,
        "PUT",
        "/api/settings/sites/thingiverse",
        Some(json!({ "key": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(&pool, "GET", "/api/settings/sites", None).await;
    assert!(
        !body.contains(secret),
        "the key itself must never come back: {body}"
    );
    assert_eq!(json_of(&body)["sites"][1]["hint"], "9f2e");

    let (status, _) = send(&pool, "DELETE", "/api/settings/sites/thingiverse", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(&pool, "GET", "/api/settings/sites", None).await;
    assert!(json_of(&body)["sites"][1]["hint"].is_null());

    // Printables takes no key, an empty key is not a key, and an unknown site is not a site.
    assert_eq!(
        send(
            &pool,
            "PUT",
            "/api/settings/sites/printables",
            Some(json!({ "key": "x" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &pool,
            "PUT",
            "/api/settings/sites/thingiverse",
            Some(json!({ "key": "  " }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &pool,
            "PUT",
            "/api/settings/sites/cults3d",
            Some(json!({ "key": "x" }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

/// With no key saved and none offered, the check says what to do without asking the site anything.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn checking_a_site_with_no_key_says_to_add_one(pool: sqlx::PgPool) {
    let (status, body) = send(
        &pool,
        "POST",
        "/api/settings/sites/myminifactory/check",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let answer = json_of(&body);
    assert_eq!(answer["ok"], false);
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|m| m.contains("Settings")),
        "{answer}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_pasted_link_queues_one_import_or_is_refused_before_anything_is_queued(
    pool: sqlx::PgPool,
) {
    let uri = format!("/api/libraries/{LIBRARY}/imports/link");
    let printables = "https://www.printables.com/model/482910-motorcycle-throttle-stop";
    let (status, body) = send(&pool, "POST", &uri, Some(json!({ "url": printables }))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(json_of(&body)["queued"], 1);
    let queued: Vec<(String, Value)> = sqlx::query_as("SELECT kind, payload FROM job")
        .fetch_all(&pool)
        .await
        .expect("jobs");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, "import_link");
    assert_eq!(queued[0].1["url"], printables);

    for (url, says) in [
        ("https://www.thingiverse.com/thing:4049220", "Settings"),
        ("https://makerworld.com/en/models/1234567", "MakerWorld"),
        ("https://www.cults3d.com/en/3d-model/x", "cults3d.com"),
        ("https://www.printables.com/@someone", "model's page"),
    ] {
        let (status, body) = send(&pool, "POST", &uri, Some(json!({ "url": url }))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {body}");
        assert!(body.contains(says), "{url}: {body}");
    }
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(total, 1, "nothing queued for a refusal");

    let other = "/api/libraries/01931b6e-0000-7000-8000-00000000dead/imports/link";
    assert_eq!(
        send(&pool, "POST", other, Some(json!({ "url": printables })))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}
