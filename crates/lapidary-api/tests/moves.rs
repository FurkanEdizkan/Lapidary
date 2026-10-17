//! Moving a model between categories. `PATCH /api/parts/{id}` and `GET /api/parts/{id}/moves`,
//! end to end — a live migrated Postgres (via `sqlx::test`), the owner's folders in a `tempfile`
//! directory, and the router this crate builds.
//!
//! Since M4 a move is a database change and nothing else: the folders are real so the tests can
//! prove nothing in them was touched.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{
    BlobHash, FileRoot, FolderId, LibraryId, MeshMeasurements, PartId, RevisionOrigin,
};
use lapidary_db::{
    IngestRequest, PgFolders, PgIngest, PgRevisions, RevisionRequest, StoredBlobRow,
};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [182.0, 96.5, 74.0],
        triangle_count: 148_302,
        surface_area_mm2: 41_207.75,
        volume_mm3: Some(96_411.0),
        is_watertight: true,
    }
}

/// Every file and directory under `root`, with its size, modification time and, for a file, its
/// BLAKE3 — what "nothing under it changed" is checked against.
fn snapshot(root: &std::path::Path) -> Vec<String> {
    let mut seen = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir).expect("reads a directory") {
            let path = entry.expect("an entry").path();
            let meta = std::fs::symlink_metadata(&path).expect("stats");
            let rel = path
                .strip_prefix(root)
                .expect("under root")
                .display()
                .to_string();
            let mtime = meta.modified().expect("an mtime");
            if meta.is_dir() {
                seen.push(format!("{rel}/ {mtime:?}"));
                queue.push(path);
            } else {
                let hash = blake3::hash(&std::fs::read(&path).expect("reads"));
                seen.push(format!("{rel} {} {mtime:?} {hash}", meta.len()));
            }
        }
    }
    seen.sort();
    seen
}

/// One model, in the database and in the owner's folders at the same time, read where it is.
async fn seed_model(
    pool: &sqlx::PgPool,
    folders: &std::path::Path,
    folder: Option<FolderId>,
    name: &str,
    source_path: &str,
) -> PartId {
    let bytes = format!("solid {name}\nendsolid {name}\n").into_bytes();
    let file = folders.join(source_path);
    std::fs::create_dir_all(file.parent().expect("a directory")).expect("directory");
    std::fs::write(&file, &bytes).expect("the owner's file");
    let stat = lapidary_storage::LibraryReader::open(folders)
        .stat_at(source_path)
        .expect("stats");
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes()),
        size_bytes: bytes.len() as u64,
        stored_bytes: bytes.len() as u64,
        zstd_level: 0,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path,
            folder,
            storage_path: Some(source_path),
            root: FileRoot::Library,
            mtime_ns: Some(stat.mtime_ns),
            blob: &blob,
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed model")
}

fn app(pool: sqlx::PgPool, folders: &std::path::Path) -> axum::Router {
    router(
        AppState {
            db: pool,
            blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: folders.to_path_buf(),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    )
}

/// `PATCH /api/parts/{id}` with the given target and acknowledgement.
async fn move_request(
    pool: &sqlx::PgPool,
    folders: &std::path::Path,
    part: PartId,
    folder: Option<FolderId>,
    acknowledge: bool,
) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({
        "folderId": folder.map(|f| f.to_string()),
        "acknowledgeDuplicate": acknowledge,
    });
    let response = app(pool.clone(), folders)
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/parts/{part}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn folder_of(pool: &sqlx::PgPool, part: PartId) -> Option<String> {
    sqlx::query_scalar("SELECT folder_id::text FROM part WHERE id = $1")
        .bind(part.as_uuid())
        .fetch_one(pool)
        .await
        .expect("reads folder")
}

/// Every revision's file path, oldest first.
async fn storage_paths_of(pool: &sqlx::PgPool, part: PartId) -> Vec<Option<String>> {
    sqlx::query_scalar(
        "SELECT f.storage_path FROM file f JOIN revision r ON r.id = f.revision_id \
         WHERE r.part_id = $1 ORDER BY r.created_at, r.id",
    )
    .bind(part.as_uuid())
    .fetch_all(pool)
    .await
    .expect("reads storage paths")
}

async fn move_count(pool: &sqlx::PgPool, part: PartId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part_move WHERE part_id = $1")
        .bind(part.as_uuid())
        .fetch_one(pool)
        .await
        .expect("counts moves")
}

/// (b) THE test. A move changes the category and nothing else: the row's path, the part's identity
/// and every byte, name and timestamp in the owner's folders are what they were, and the file still
/// downloads from where it always was.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_move_changes_the_category_and_writes_nothing_on_disk(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let tree = PgFolders(pool.clone());
    let terrain = tree
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = tree
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = seed_model(
        &pool,
        folders.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
    )
    .await;
    let before = snapshot(folders.path());

    let (status, json) = move_request(&pool, folders.path(), part, Some(bases), false).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(folder_of(&pool, part).await, Some(bases.to_string()));
    assert_eq!(
        storage_paths_of(&pool, part).await,
        [Some("Terrain/rock.stl".to_owned())],
        "the file is where it was"
    );
    let source_path: String = sqlx::query_scalar("SELECT source_path FROM part WHERE id = $1")
        .bind(part.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("reads source path");
    assert_eq!(source_path, "Terrain/rock.stl", "identity never moved");
    assert_eq!(
        snapshot(folders.path()),
        before,
        "nothing in the owner's folders changed"
    );

    let response = app(pool.clone(), folders.path())
        .oneshot(
            Request::builder()
                .uri(format!("/api/parts/{part}/file"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    assert_eq!(&bytes[..], b"solid rock\nendsolid rock\n");
}

/// A revised part moves with its whole history, and no row's path changes.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_revised_part_moves_with_every_revision_where_it_was(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let bases = PgFolders(pool.clone())
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = seed_model(&pool, folders.path(), None, "rock", "rock.stl").await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), "rock.stl")
        .await
        .expect("reads")
        .expect("the part")
        .revision;
    revisions
        .record_revision(RevisionRequest {
            part,
            parent: first,
            origin: RevisionOrigin::Ingest,
            root: FileRoot::Library,
            storage_path: "rock.stl",
            mtime_ns: Some(1_760_000_100_000_000_000),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0x23; 32]),
                size_bytes: 31,
                stored_bytes: 31,
                zstd_level: 0,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("revision 2");
    let before = snapshot(folders.path());

    let (status, json) = move_request(&pool, folders.path(), part, Some(bases), false).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        storage_paths_of(&pool, part).await,
        [Some("rock.stl".to_owned()), Some("rock.stl".to_owned())]
    );
    assert_eq!(snapshot(folders.path()), before);
}

/// Two models may share a name — slice 6a decided that is the truth — so the collision is a
/// warning the client can answer, not a refusal.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn moving_into_a_name_collision_needs_an_acknowledgement(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let tree = PgFolders(pool.clone());
    let terrain = tree
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = tree
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let moving = seed_model(
        &pool,
        folders.path(),
        Some(terrain),
        "cliff",
        "Terrain/cliff.stl",
    )
    .await;
    seed_model(
        &pool,
        folders.path(),
        Some(bases),
        "cliff",
        "Bases/cliff.stl",
    )
    .await;

    let (refused, body) = move_request(&pool, folders.path(), moving, Some(bases), false).await;
    assert_eq!(refused, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "duplicateName");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("cliff")),
        "the warning names the model that is in the way: {body}"
    );
    assert_eq!(folder_of(&pool, moving).await, Some(terrain.to_string()));

    let (accepted, _) = move_request(&pool, folders.path(), moving, Some(bases), true).await;
    assert_eq!(accepted, StatusCode::OK);
    assert_eq!(folder_of(&pool, moving).await, Some(bases.to_string()));
    assert_eq!(
        storage_paths_of(&pool, moving).await,
        [Some("Terrain/cliff.stl".to_owned())]
    );
}

/// A part from before the folder tree, whose bytes are content-addressed in the store, moves like
/// any other: a move needs no directory. It used to be refused as "migration pending".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_content_addressed_part_moves_too(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let bases = PgFolders(pool.clone())
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "corner bracket",
            source_path: "corner bracket.stl",
            folder: None,
            storage_path: None,
            root: FileRoot::Store,
            mtime_ns: None,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0x55; 32]),
                size_bytes: 212_004,
                stored_bytes: 64_118,
                zstd_level: 3,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed a store part");

    let (status, body) = move_request(&pool, folders.path(), part, Some(bases), false).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(move_count(&pool, part).await, 1);
}

/// The history is the audit trail `part_move` was created for, and it records both ends of
/// the move — where it came from as well as where it went.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_history_records_both_ends_of_a_move(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let terrain = PgFolders(pool.clone())
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let part = seed_model(
        &pool,
        folders.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
    )
    .await;

    // Out of Terrain and into the library root, which is a real destination and not the
    // absence of one.
    let (status, _) = move_request(&pool, folders.path(), part, None, false).await;
    assert_eq!(status, StatusCode::OK);

    let response = app(pool.clone(), folders.path())
        .oneshot(
            Request::builder()
                .uri(format!("/api/parts/{part}/moves"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let history: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");

    assert_eq!(history.as_array().map(Vec::len), Some(1));
    assert_eq!(history[0]["fromFolder"], terrain.to_string());
    assert!(history[0]["toFolder"].is_null());
    assert!(
        history[0]["movedAt"].is_string(),
        "a move that cannot say when it happened is not an audit trail: {history}"
    );
}

/// Re-filing a model into the category it is already in changes nothing.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn moving_a_model_into_its_own_category_is_a_no_op(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let terrain = PgFolders(pool.clone())
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let part = seed_model(
        &pool,
        folders.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
    )
    .await;

    let (status, _) = move_request(&pool, folders.path(), part, Some(terrain), false).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(move_count(&pool, part).await, 0, "nothing moved, so no row");
}
