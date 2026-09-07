//! Task 9: moving a model between categories. `PATCH /api/parts/{id}` and
//! `GET /api/parts/{id}/moves`, end to end — a live migrated Postgres (via `sqlx::test`), a
//! real store on disk in a `tempfile` directory, and the router this crate builds.
//!
//! The store is real rather than mocked because the ordering these tests exist to pin is
//! about a real filesystem: the rename happens inside the transaction, and a rename that
//! cannot happen has to leave every row exactly where it was.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, FolderId, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgFolders, PgIngest, StoredBlobRow};
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`, and its slug — what
/// `PgParts::library_slug` derives from the name `Default` — is the `default` in every
/// path below.
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

/// One model, in the database and on disk at the same time. Returns its id.
///
/// Both halves, because a move is the one route where they have to agree: a part whose row
/// names a directory that is not there is precisely the state the ordering rule exists to
/// avoid producing, so a fixture that produced it would be testing the wrong thing.
async fn seed_model(
    pool: &sqlx::PgPool,
    store: &std::path::Path,
    folder: Option<FolderId>,
    name: &str,
    source_path: &str,
    storage_path: &str,
    seed: u8,
) -> PartId {
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 7_412_880,
        stored_bytes: 7_412_880,
        zstd_level: 0,
    };
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name,
            source_path,
            folder,
            storage_path: Some(storage_path),
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed model");

    let file = store.join(storage_path);
    std::fs::create_dir_all(file.parent().expect("a model directory")).expect("model directory");
    std::fs::write(&file, b"solid cliff\nendsolid cliff\n").expect("source file");
    // `metadata.json` beside it, as ingest writes: it is what makes a model directory
    // self-identifying, and therefore what makes a disk that is ahead of the database
    // repairable rather than a puzzle.
    std::fs::write(
        file.parent()
            .expect("a model directory")
            .join("metadata.json"),
        format!("{{\"part\":{{\"name\":\"{name}\",\"sourcePath\":\"{source_path}\"}}}}"),
    )
    .expect("manifest");
    part
}

fn app(pool: sqlx::PgPool, store: &std::path::Path) -> axum::Router {
    router(
        AppState {
            db: pool,
            blob_root: store.to_path_buf(),
        },
        Role::Api,
    )
}

/// `PATCH /api/parts/{id}` with the given target and acknowledgement.
async fn move_request(
    pool: &sqlx::PgPool,
    store: &std::path::Path,
    part: PartId,
    folder: Option<FolderId>,
    acknowledge: bool,
) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({
        "folderId": folder.map(|f| f.to_string()),
        "acknowledgeDuplicate": acknowledge,
    });
    let response = app(pool.clone(), store)
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
    // A 200 answers with no body at all, which is not JSON — `Null` stands in for it so
    // every caller can read `json["reason"]` without knowing which it got.
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// As text, so this file never has to name the `uuid` crate to say which category a part is
/// in — `FolderId`'s own `Display` is what every assertion below compares against.
async fn folder_of(pool: &sqlx::PgPool, part: PartId) -> Option<String> {
    sqlx::query_scalar("SELECT folder_id::text FROM part WHERE id = $1")
        .bind(part.as_uuid())
        .fetch_one(pool)
        .await
        .expect("reads folder")
}

async fn storage_path_of(pool: &sqlx::PgPool, part: PartId) -> Option<String> {
    sqlx::query_scalar(
        "SELECT f.storage_path FROM file f JOIN revision r ON r.id = f.revision_id \
         WHERE r.part_id = $1",
    )
    .bind(part.as_uuid())
    .fetch_one(pool)
    .await
    .expect("reads storage path")
}

async fn move_count(pool: &sqlx::PgPool, part: PartId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM part_move WHERE part_id = $1")
        .bind(part.as_uuid())
        .fetch_one(pool)
        .await
        .expect("counts moves")
}

/// THE test. `source_path` is immutable identity, `folder_id` is mutable location, and this
/// is the case that proves splitting them was right: after a move, the file is under its new
/// category on disk and in the row, and the path a re-scan matches on has not moved at all.
///
/// The re-scan itself is `lapidary-ingest`'s own test (`handler.rs`, the `Outcome::Skipped`
/// case keyed on `source_path`) — this crate cannot reach that handler without depending on
/// the one crate that links the CAD kernel, which is the dependency `xtask/src/layers.rs`
/// exists to forbid. What is asserted here is the half a move is responsible for: the thing
/// the re-scan keys on is untouched.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_move_changes_where_a_model_is_and_never_what_it_is(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
        "libraries/default/Terrain/rock/rock.stl",
        0x11,
    )
    .await;

    let (status, _) = move_request(&pool, store.path(), part, Some(bases), false).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(folder_of(&pool, part).await, Some(bases.to_string()));
    assert_eq!(
        storage_path_of(&pool, part).await.as_deref(),
        Some("libraries/default/Bases/rock/rock.stl"),
    );
    let source_path: String = sqlx::query_scalar("SELECT source_path FROM part WHERE id = $1")
        .bind(part.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("reads source path");
    assert_eq!(source_path, "Terrain/rock.stl", "identity never moved");
    assert!(
        store
            .path()
            .join("libraries/default/Bases/rock/rock.stl")
            .exists()
    );
    assert!(
        !store.path().join("libraries/default/Terrain/rock").exists(),
        "the directory was renamed, not copied"
    );
    assert!(
        store
            .path()
            .join("libraries/default/Bases/rock/metadata.json")
            .exists(),
        "the manifest travels with the directory, and needs no rewrite: it carries no path"
    );
}

/// Rename first, inside the transaction: a failure must move nothing and change nothing.
///
/// The rename is made to fail by removing the source directory rather than by revoking
/// write permission — a chmod is a no-op when the suite runs as root, and a test that
/// silently *succeeds* at moving is the one failure mode this test must not have.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failed_rename_leaves_the_database_untouched(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
        "libraries/default/Terrain/rock/rock.stl",
        0x22,
    )
    .await;
    std::fs::remove_dir_all(store.path().join("libraries/default/Terrain/rock"))
        .expect("the directory the rename would have moved");

    let (status, _) = move_request(&pool, store.path(), part, Some(bases), false).await;

    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the rename failed"
    );
    assert_eq!(
        folder_of(&pool, part).await,
        Some(terrain.to_string()),
        "no row moved"
    );
    assert_eq!(
        storage_path_of(&pool, part).await.as_deref(),
        Some("libraries/default/Terrain/rock/rock.stl"),
    );
    assert_eq!(
        move_count(&pool, part).await,
        0,
        "a move nothing performed is a move nothing recorded"
    );
}

/// The one 500 on this route that is not a `DbError`. It has to behave like the others:
/// the operator gets the real failure through the log, the caller gets prose that names no
/// host path. `StorageError::Io` carries the store's absolute path, which is the server's
/// filesystem layout and not something a browser has any business being told.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_store_that_cannot_take_the_directory_says_so_without_naming_the_disk(
    pool: sqlx::PgPool,
) {
    let store = tempfile::tempdir().expect("temp store");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let part = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "scree",
        "Terrain/scree.stl",
        "libraries/default/Terrain/scree/scree.stl",
        0x44,
    )
    .await;

    // A plain file standing where the category's directory has to go. `create_dir` is
    // `mkdir -p`, so it fails on the component that is not a directory — the cheapest
    // reachable stand-in for a volume that is full, read-only or unmounted.
    std::fs::write(
        store.path().join("libraries/default/Bases"),
        b"not a directory",
    )
    .expect("a file in the way");

    let (status, body) = move_request(&pool, store.path(), part, Some(bases), false).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body["reason"], "storageUnwritable",
        "the client tells this apart from a failed rename by `reason`, never by the prose"
    );
    let message = body["message"].as_str().expect("a message");
    assert!(
        !message.contains(store.path().to_str().expect("utf-8 temp dir")),
        "the store's absolute path is for the log, not for the response: {message}"
    );
    assert!(
        message.contains("storage"),
        "and it still has to tell the operator what to go and look at: {message}"
    );
}

/// Two models may share a name — slice 6a decided that is the truth — so the collision is a
/// warning the client can answer, not a refusal.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn moving_into_a_name_collision_needs_an_acknowledgement(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let moving = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "cliff",
        "Terrain/cliff.stl",
        "libraries/default/Terrain/cliff/cliff.stl",
        0x33,
    )
    .await;
    seed_model(
        &pool,
        store.path(),
        Some(bases),
        "cliff",
        "Bases/cliff.stl",
        "libraries/default/Bases/cliff/cliff.stl",
        0x44,
    )
    .await;

    let (refused, body) = move_request(&pool, store.path(), moving, Some(bases), false).await;
    assert_eq!(refused, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "duplicateName");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("cliff")),
        "the warning names the model that is in the way: {body}"
    );
    assert_eq!(folder_of(&pool, moving).await, Some(terrain.to_string()));

    let (accepted, _) = move_request(&pool, store.path(), moving, Some(bases), true).await;
    assert_eq!(accepted, StatusCode::OK);
    assert_eq!(folder_of(&pool, moving).await, Some(bases.to_string()));
    // The disambiguated directory, because `Bases/cliff` was already standing there. The
    // suffix is six hex characters of this model's own source hash, so re-ingesting the
    // same bytes would land on the same name.
    assert_eq!(
        storage_path_of(&pool, moving).await.as_deref(),
        Some("libraries/default/Bases/cliff_333333/cliff.stl"),
    );
    assert!(
        store
            .path()
            .join("libraries/default/Bases/cliff/cliff.stl")
            .exists(),
        "the model already there was not replaced"
    );
}

/// A model still in the shared store has no directory to rename, and the refusal says so in
/// the words the card shows for the same state rather than reading as a name collision.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_model_the_storage_migration_has_not_reached_cannot_be_moved(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let bases = PgFolders(pool.clone())
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([0x55; 32]),
        size_bytes: 212_004,
        stored_bytes: 64_118,
        zstd_level: 3,
    };
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "corner bracket",
            source_path: "corner bracket.stl",
            folder: None,
            // The live state migration 0009 documents: the bytes are still at
            // `blobs/ab/cd/<hash>`, and no model directory exists to move.
            storage_path: None,
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed unmigrated part");

    let (status, body) = move_request(&pool, store.path(), part, Some(bases), false).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "migrationPending");
    assert_eq!(move_count(&pool, part).await, 0);
}

/// The history is the audit trail `part_move` was created for, and it records both ends of
/// the move — where it came from as well as where it went.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_history_records_both_ends_of_a_move(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let terrain = PgFolders(pool.clone())
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let part = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
        "libraries/default/Terrain/rock/rock.stl",
        0x66,
    )
    .await;

    // Out of Terrain and into the library root, which is a real destination and not the
    // absence of one.
    let (status, _) = move_request(&pool, store.path(), part, None, false).await;
    assert_eq!(status, StatusCode::OK);

    let response = app(pool.clone(), store.path())
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
    assert_eq!(
        storage_path_of(&pool, part).await.as_deref(),
        Some("libraries/default/rock/rock.stl"),
        "the library root is `libraries/<library>` with no category between",
    );
}

/// Re-filing a model into the category it is already in changes nothing, and in particular
/// does not rename its directory to a disambiguated one because the directory it is
/// standing in is "already taken".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn moving_a_model_into_its_own_category_is_a_no_op(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("temp store");
    let terrain = PgFolders(pool.clone())
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let part = seed_model(
        &pool,
        store.path(),
        Some(terrain),
        "rock",
        "Terrain/rock.stl",
        "libraries/default/Terrain/rock/rock.stl",
        0x77,
    )
    .await;

    let (status, _) = move_request(&pool, store.path(), part, Some(terrain), false).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        storage_path_of(&pool, part).await.as_deref(),
        Some("libraries/default/Terrain/rock/rock.stl"),
    );
    assert_eq!(move_count(&pool, part).await, 0, "nothing moved, so no row");
}
