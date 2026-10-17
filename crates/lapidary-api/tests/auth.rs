//! A1: setup, signing in and out, and the guard in front of everything else — through the real
//! router, with no test session in front of it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use serde_json::{Value, json};
use tower::ServiceExt;

fn state(pool: &sqlx::PgPool) -> AppState {
    AppState {
        db: pool.clone(),
        blob_root: "/nonexistent-blob-root".into(),
        upload_dir: "/nonexistent-upload-dir".into(),
        host_storage_root: None,
        ingest_dir: "/nonexistent-ingest".into(),
        inbox_dir: "/nonexistent-inbox".into(),
        host_ingest_root: Some("/srv/models".into()),
        host_inbox_root: None,
        touches: Default::default(),
    }
}

struct Answer {
    status: StatusCode,
    cookie: Option<String>,
    body: Value,
}

async fn send(
    pool: &sqlx::PgPool,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "nas.local:3000");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
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
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    Answer {
        status,
        cookie,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn setup_makes_the_admin_once_with_the_logged_code_and_claims_what_exists(
    pool: sqlx::PgPool,
) {
    // Nobody yet: every guarded route says so, and the wizard's status is open.
    let before = send(&pool, "GET", "/api/libraries", None, None).await;
    assert_eq!(before.status, StatusCode::CONFLICT);
    assert_eq!(before.body["code"], "setup_required");
    let status = send(&pool, "GET", "/api/setup", None, None).await;
    assert_eq!(status.body["required"], true);
    assert_eq!(status.body["hostIngestRoot"], "/srv/models");

    let code = lapidary_api::auth::prepare_setup(&pool)
        .await
        .expect("prepares")
        .expect("a code while nobody has an account");
    let wizard = |code: &str| {
        json!({
            "code": code,
            "username": "  Furkan ",
            "displayName": "Furkan Edizkan",
            "password": "correct horse battery",
            "importLinks": false
        })
    };
    let wrong = send(
        &pool,
        "POST",
        "/api/setup",
        None,
        Some(wizard("ABCD-EFGH-JKMN")),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    assert!(
        wrong.body["message"]
            .as_str()
            .is_some_and(|m| m.contains("docker compose logs"))
    );

    // Typed lowercase and without dashes, as a person might.
    let typed = code.replace('-', " ").to_lowercase();
    let made = send(&pool, "POST", "/api/setup", None, Some(wizard(&typed))).await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    assert_eq!(made.body["username"], "furkan");
    assert_eq!(made.body["isAdmin"], true);
    let cookie = made.cookie.expect("signed in by setup");

    // The Default library from before accounts is the admin's now.
    let unowned: i64 = sqlx::query_scalar("SELECT count(*) FROM library WHERE owner_id IS NULL")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(unowned, 0);
    // The wizard's switch was kept.
    let sites = send(&pool, "GET", "/api/settings/sites", Some(&cookie), None).await;
    assert_eq!(sites.status, StatusCode::OK);
    assert_eq!(sites.body["enabled"], false);

    // Once only, and the code is spent.
    let again = send(&pool, "POST", "/api/setup", None, Some(wizard(&code))).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.body["code"], "already_set_up");
    assert_eq!(
        send(&pool, "GET", "/api/setup", None, None).await.body["required"],
        false
    );
    assert!(
        lapidary_api::auth::prepare_setup(&pool)
            .await
            .expect("asks")
            .is_none()
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_session_opens_the_api_and_signing_out_or_a_new_password_closes_it(pool: sqlx::PgPool) {
    let code = lapidary_api::auth::prepare_setup(&pool)
        .await
        .expect("prepares")
        .expect("a code");
    let body = json!({ "code": code, "username": "furkan", "password": "correct horse battery" });
    let first = send(&pool, "POST", "/api/setup", None, Some(body))
        .await
        .cookie
        .expect("cookie");

    // Without a session, a person is told to sign in, not to set up.
    let nobody = send(&pool, "GET", "/api/libraries", None, None).await;
    assert_eq!(nobody.status, StatusCode::UNAUTHORIZED);
    assert_eq!(nobody.body["code"], "signed_out");
    assert_eq!(
        send(&pool, "GET", "/api/session", None, None).await.status,
        StatusCode::UNAUTHORIZED
    );

    let wrong = send(
        &pool,
        "POST",
        "/api/session",
        None,
        Some(json!({ "username": "furkan", "password": "wrong horse" })),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.cookie.is_none());

    let signed = send(
        &pool,
        "POST",
        "/api/session",
        None,
        Some(json!({ "username": "FURKAN", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(signed.status, StatusCode::OK);
    let second = signed.cookie.expect("cookie");
    assert_eq!(
        send(&pool, "GET", "/api/libraries", Some(&second), None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        send(&pool, "GET", "/api/session", Some(&second), None)
            .await
            .body["username"],
        "furkan"
    );

    // Signing out ends that session and no other.
    assert_eq!(
        send(&pool, "DELETE", "/api/session", Some(&second), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&pool, "GET", "/api/libraries", Some(&second), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&pool, "GET", "/api/libraries", Some(&first), None)
            .await
            .status,
        StatusCode::OK
    );

    // `reset-password` ends every session the person had.
    let hash = lapidary_api::auth::hash_password("another long one".into())
        .await
        .expect("hashes");
    assert!(
        lapidary_db::PgAccounts(pool.clone())
            .set_password("furkan", &hash)
            .await
            .expect("saves")
    );
    assert_eq!(
        send(&pool, "GET", "/api/libraries", Some(&first), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_change_sent_from_another_site_is_refused_before_it_is_looked_at(pool: sqlx::PgPool) {
    let response = router(state(&pool), Role::Api)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/session")
                .header(header::HOST, "nas.local:3000")
                .header(header::ORIGIN, "https://evil.example")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "username": "furkan", "password": "x" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The same request from the page itself is looked at (and refused for its password).
    let response = router(state(&pool), Role::Api)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/session")
                .header(header::HOST, "nas.local:3000")
                .header(header::ORIGIN, "http://nas.local:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "username": "furkan", "password": "x" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
