//! A2: the Import browser over the models folder, and adding and removing what a library holds.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::router;
use lapidary_api::{AppState, Role};
use serde_json::{Value, json};
use tower::ServiceExt;

const LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn state(pool: &sqlx::PgPool, models: &std::path::Path) -> AppState {
    AppState {
        db: pool.clone(),
        blob_root: "/nonexistent-blob-root".into(),
        upload_dir: "/nonexistent-upload-dir".into(),
        host_storage_root: None,
        ingest_dir: models.to_path_buf(),
        inbox_dir: models.to_path_buf(),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    }
}

async fn send(
    pool: &sqlx::PgPool,
    models: &std::path::Path,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router(state(pool, models), Role::Api)
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

fn models() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a models folder");
    for (path, bytes) in [
        (
            "Printables/benchy/3DBenchy.stl",
            &b"solid benchy\nendsolid benchy\n"[..],
        ),
        ("Printables/benchy/notes.txt", b"print at 0.2 mm"),
        ("Printables/benchy/benchy.3mf", b"PK\x03\x04"),
        ("_removed/lena/old.stl", b"solid old\nendsolid old\n"),
        ("dragon.obj", b"v 0 0 0\n"),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().expect("parent")).expect("mkdir");
        std::fs::write(at, bytes).expect("write");
    }
    dir
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_browser_lists_folders_then_models_and_never_removed(pool: sqlx::PgPool) {
    let models = models();
    let (status, top) = send(&pool, models.path(), "GET", "/api/storage/browse", None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = top["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(
        names,
        ["Printables", "dragon.obj"],
        "folders first; no _removed"
    );
    let (_, benchy) = send(
        &pool,
        models.path(),
        "GET",
        "/api/storage/browse?path=Printables/benchy",
        None,
    )
    .await;
    let paths: Vec<&str> = benchy["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(
        paths,
        [
            "Printables/benchy/3DBenchy.stl",
            "Printables/benchy/benchy.3mf"
        ],
        "model files only"
    );
    for refused in [
        "/api/storage/browse?path=_removed",
        "/api/storage/browse?path=../../etc",
    ] {
        assert_eq!(
            send(&pool, models.path(), "GET", refused, None).await.0,
            StatusCode::BAD_REQUEST,
            "{refused}"
        );
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn adding_records_a_file_or_folder_and_removing_one_leaves_its_files(pool: sqlx::PgPool) {
    let models = models();
    let sources = format!("/api/libraries/{LIBRARY}/sources");
    let (status, folder) = send(
        &pool,
        models.path(),
        "POST",
        &sources,
        Some(json!({ "path": "Printables/benchy", "pageUrl": "https://www.printables.com/model/3161-3d-benchy", "fetchDetails": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{folder}");
    assert_eq!(folder["isFolder"], true);
    assert_eq!(folder["fetchDetails"], true);
    let (status, file) = send(
        &pool,
        models.path(),
        "POST",
        &sources,
        Some(json!({ "path": "dragon.obj" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(file["isFolder"], false);

    for (path, why) in [
        ("Printables/benchy/notes.txt", "not a model"),
        ("_removed/lena/old.stl", "in _removed"),
        ("../outside", "out of the folder"),
    ] {
        let (status, _) = send(
            &pool,
            models.path(),
            "POST",
            &sources,
            Some(json!({ "path": path })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}");
    }
    let (status, _) = send(
        &pool,
        models.path(),
        "POST",
        &sources,
        Some(json!({ "path": "dragon.obj", "pageUrl": "https://makerworld.com/en/models/1" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a page from a site Lapidary does not read"
    );

    let (_, listed) = send(&pool, models.path(), "GET", &sources, None).await;
    assert_eq!(listed.as_array().expect("a list").len(), 2);

    let id = folder["id"].as_str().expect("an id");
    let (status, removed) = send(
        &pool,
        models.path(),
        "DELETE",
        &format!("{sources}/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["partsRemoved"], 0);
    assert!(
        models
            .path()
            .join("Printables/benchy/3DBenchy.stl")
            .exists(),
        "the files stay"
    );
    assert_eq!(
        send(
            &pool,
            models.path(),
            "DELETE",
            &format!("{sources}/{id}"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
