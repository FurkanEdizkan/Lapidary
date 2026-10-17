//! `import_link` (F3): a model from the page a person pasted, through ordinary ingest. The sites are
//! a fixture server in the shapes `lapidary-sites`' own tests use; the bytes are real fixtures, so the
//! parse, the measurement and the picture's normalize all run for real.

use axum::Json;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use lapidary_core::{BatchId, JobId, LibraryId, Outcome};
use lapidary_db::{JobRow, PgParts, PgSiteKeys};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_sites::Sites;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::Ipv4Addr;

const STL: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
const PICTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.thumb.webp");
const PRINTABLES: &str = "https://www.printables.com/model/482910-motorcycle-throttle-stop";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(
        "01931b6e-0000-7000-8000-000000000001"
            .parse()
            .expect("uuid"),
    )
}

fn job(url: &str) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: seeded(),
        kind: "import_link".to_owned(),
        payload: json!({ "url": url, "path": url }),
        attempts: 1,
        max_attempts: 3,
    }
}

async fn serve(stls: Value) -> String {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("port");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let b = base.clone();
    let app = axum::Router::new()
        .route(
            "/printables/graphql/",
            post(move |Json(body): Json<Value>| {
                let (b, stls) = (b.clone(), stls.clone());
                async move {
                    let query = body["query"].as_str().unwrap_or_default();
                    Json(if query.contains("getDownloadLink") {
                        json!({"data": {"getDownloadLink": {"ok": true, "output": {"link": format!("{b}/files/{}", body["variables"]["id"].as_str().unwrap_or_default())}}}})
                    } else {
                        json!({"data": {"print": {
                            "id": "482910", "name": "Motorcycle Throttle Stop", "slug": "motorcycle-throttle-stop",
                            "summary": "After the original Go-Cruise.",
                            "license": {"name": "Creative Commons — Attribution"},
                            "user": {"publicUsername": "3Diocrity"},
                            "tags": [{"name": "Motorcycles"}, {"name": "cruisecontrol"}],
                            "images": [{"filePath": "media/prints/482910/images/go-cruise.webp"}, {"filePath": "media/prints/482910/images/fitted.webp"}],
                            "stls": stls
                        }}})
                    })
                }
            }),
        )
        .route("/printables-media/{*path}", get(|| async { ([(header::CONTENT_TYPE, "image/webp")], PICTURE) }))
        .route("/files/{id}", get(|| async { STL }))
        .route("/thingiverse/{*path}", get(|_: HeaderMap| async { (StatusCode::UNAUTHORIZED, "{}").into_response() }));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

fn handler(pool: &PgPool, root: &std::path::Path, base: &str) -> WorkerHandler {
    WorkerHandler {
        db: pool.clone(),
        ingest_dir: root.join("ingest"),
        inbox_dir: root.join("inbox"),
        blob_root: root.to_path_buf(),
        pause: None,
        sites: Sites::local(base),
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_printables_link_files_its_mesh_with_its_page_and_imports_again_as_a_rescan(
    pool: PgPool,
) {
    let root = tempfile::tempdir().expect("temp dir");
    let base = serve(json!([
        {"id": "2080405", "name": "Go-Cruise.stl", "fileSize": STL.len()},
        {"id": "2080406", "name": "Go-Cruise.f3d", "fileSize": 900}
    ]))
    .await;
    let worker = handler(&pool, root.path(), &base);

    assert_eq!(
        worker.handle(&job(PRINTABLES)).await.expect("imports"),
        Outcome::Ingested
    );
    let path = "printables/3Diocrity/Motorcycle Throttle Stop/Go-Cruise.stl";
    let part: uuid::Uuid = sqlx::query_scalar("SELECT id FROM part WHERE source_path = $1")
        .bind(path)
        .fetch_one(&pool)
        .await
        .expect("the mesh is a part; the .f3d is not");
    let part = lapidary_core::PartId::from_uuid(part);
    // (e) Written once into the inbox, under Lapidary's own `imports/`, and indexed from there.
    let inbox = root.path().join("inbox/imports").join(path);
    assert_eq!(std::fs::read(&inbox).expect("in the inbox"), STL);
    let (file_root, stored): (String, String) =
        sqlx::query_as("SELECT root, storage_path FROM file f JOIN revision r ON r.id = f.revision_id WHERE r.part_id = $1")
            .bind(part.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("the file row");
    assert_eq!(
        (file_root.as_str(), stored),
        ("inbox", format!("imports/{path}"))
    );
    assert!(
        !root.path().join("libraries").exists(),
        "nothing went into the store"
    );
    let parts = PgParts(pool.clone());
    let sources = parts.part_sources(part).await.expect("sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].vendor.as_deref(), Some("Printables"));
    assert_eq!(sources[0].external_id.as_deref(), Some("482910"));
    assert_eq!(
        sources[0].license.as_deref(),
        Some("Creative Commons — Attribution")
    );
    assert_eq!(
        sources[0].url.as_deref(),
        Some("https://www.printables.com/model/482910-motorcycle-throttle-stop")
    );
    assert_eq!(
        parts
            .detail(part)
            .await
            .expect("reads")
            .expect("the part")
            .tags,
        ["motorcycles", "cruisecontrol"]
    );
    let gallery = || async {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT origin, source_url FROM part_image WHERE part_id = $1 ORDER BY position",
        )
        .bind(part.as_uuid())
        .fetch_all(&pool)
        .await
        .expect("gallery")
    };
    // (M5) Every picture on the page, in the page's order.
    let page: Vec<_> = gallery().await;
    assert_eq!(page.len(), 2);
    assert!(page.iter().all(|(origin, _)| origin == "url_supplied"));
    assert!(
        page[0]
            .1
            .as_deref()
            .is_some_and(|u| u.ends_with("go-cruise.webp"))
    );
    assert!(
        page[1]
            .1
            .as_deref()
            .is_some_and(|u| u.ends_with("fitted.webp"))
    );
    // A person adds one of their own; the re-import below must neither move it nor repeat the page's.
    let webp = lapidary_storage::images::normalize(PICTURE)
        .expect("normalizes")
        .webp;
    parts
        .add_part_image(
            part,
            lapidary_db::NewPartImage {
                bytes: lapidary_db::ImageBytes::Inline(&webp),
                origin: "uploaded",
                source_url: None,
            },
        )
        .await
        .expect("an upload");

    // The same link again is a re-scan: nothing new, and no second source row.
    assert_eq!(
        worker
            .handle(&job(PRINTABLES))
            .await
            .expect("imports again"),
        Outcome::Skipped
    );
    assert_eq!(parts.part_sources(part).await.expect("sources").len(), 1);
    let again: Vec<_> = gallery().await;
    assert_eq!(again[..2], page[..]);
    assert_eq!(again.len(), 3);
    assert_eq!(again[2].0, "uploaded");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_model_with_no_mesh_or_a_site_with_no_key_is_refused_with_what_to_do(pool: PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let base = serve(json!([{"id": "1", "name": "go-cruise-pack.zip", "fileSize": 10}])).await;
    let worker = handler(&pool, root.path(), &base);

    match worker.handle(&job(PRINTABLES)).await {
        Err(HandlerError::Permanent { message }) => {
            assert!(
                message.contains("no STL, OBJ or 3MF") && message.contains("go-cruise-pack.zip"),
                "{message}"
            );
        }
        other => panic!("{other:?}"),
    }
    match worker
        .handle(&job("https://www.thingiverse.com/thing:4049220"))
        .await
    {
        Err(HandlerError::Permanent { message }) => {
            assert!(message.contains("Settings"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    // A saved key the site refuses says to check it in Settings. The key is the library owner's (A3).
    let owner = lapidary_core::UserId::new();
    sqlx::query(
        "INSERT INTO app_user (id, username, password_hash, is_admin) VALUES ($1, 'mira.okafor', 'x', true)",
    )
    .bind(owner.as_uuid())
    .execute(&pool)
    .await
    .expect("an owner");
    sqlx::query("UPDATE library SET owner_id = $1 WHERE id = $2")
        .bind(owner.as_uuid())
        .bind(seeded().as_uuid())
        .execute(&pool)
        .await
        .expect("the library is theirs");
    PgSiteKeys(pool.clone(), owner)
        .set("thingiverse", "expired-token")
        .await
        .expect("saves");
    match worker
        .handle(&job("https://www.thingiverse.com/thing:4049220"))
        .await
    {
        Err(HandlerError::Permanent { message }) => {
            assert!(
                message.contains("refused the key") && message.contains("Settings"),
                "{message}"
            )
        }
        other => panic!("{other:?}"),
    }
    match worker
        .handle(&job("https://makerworld.com/en/models/1234567"))
        .await
    {
        Err(HandlerError::Permanent { message }) => {
            assert!(message.contains("MakerWorld"), "{message}")
        }
        other => panic!("{other:?}"),
    }
}
