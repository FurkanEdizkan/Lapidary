//! `LAPIDARY_IMPORT_LINKS=false`, alone in its own test binary: the switch is an environment
//! variable, and setting it beside other tests would switch theirs off too.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use tower::ServiceExt;

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn switched_off_the_page_says_so_and_nothing_imports_or_checks(pool: sqlx::PgPool) {
    // SAFETY: the only test in this binary, so no other thread reads the environment.
    unsafe { std::env::set_var("LAPIDARY_IMPORT_LINKS", "false") };
    let app = || {
        router(
            AppState {
                db: pool.clone(),
                blob_root: "/nonexistent-blob-root".into(),
                upload_dir: "/nonexistent-upload-dir".into(),
                host_storage_root: None,
                ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
                inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
                host_ingest_root: None,
                host_inbox_root: None,
                touches: Default::default(),
            },
            Role::Api,
        )
    };
    let list = app()
        .oneshot(
            Request::get("/api/settings/sites")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("responds");
    let body = axum::body::to_bytes(list.into_body(), 64 * 1024)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("json")["enabled"],
        false
    );

    for uri in [
        "/api/libraries/01931b6e-0000-7000-8000-000000000001/imports/link",
        "/api/settings/sites/printables/check",
    ] {
        let response = app()
            .oneshot(
                Request::post(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"url":"https://www.printables.com/model/482910-x"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        assert!(
            String::from_utf8_lossy(&body).contains("LAPIDARY_IMPORT_LINKS"),
            "{uri}"
        );
    }
}
