//! Task 10: the category tree. `GET`/`POST /api/libraries/{id}/folders` and
//! `PATCH`/`DELETE /api/folders/{id}`, end to end against a live migrated Postgres.
//!
//! One rule shows up in several of these tests and is the reason for the file: none of
//! these routes touches a file. A delete hides rows and leaves every byte where it is, and
//! a rename changes a row and leaves the directory named as it was.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, FolderId, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, PgFolders, PgIngest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// These tests never read a blob, and the folder routes never open one — a path that does
/// not exist is the honest value for a store nothing here may reach.
fn blob_root() -> std::path::PathBuf {
    std::path::PathBuf::from("/nonexistent-blob-root")
}

fn app(pool: sqlx::PgPool) -> axum::Router {
    router(
        AppState {
            db: pool,
            blob_root: blob_root(),
        },
        Role::Api,
    )
}

async fn send(pool: &sqlx::PgPool, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app(pool.clone())
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("body reads");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn json_request(method: &str, uri: String, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

async fn tree(pool: &sqlx::PgPool) -> serde_json::Value {
    let (status, json) = send(
        pool,
        Request::builder()
            .uri(format!("/api/libraries/{SEEDED_LIBRARY}/folders"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json
}

async fn patch_folder(
    pool: &sqlx::PgPool,
    folder: FolderId,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        json_request("PATCH", format!("/api/folders/{folder}"), body),
    )
    .await
}

/// A second library, inserted directly: nothing in this slice creates one through the API,
/// the same way `crates/lapidary-db/tests/repo.rs` seeds its second.
async fn second_library(pool: &sqlx::PgPool) -> LibraryId {
    let id = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, $2)")
        .bind(id.as_uuid())
        .bind("Tabletop terrain")
        .execute(pool)
        .await
        .expect("second library");
    id
}

/// One model in a category, database only — these tests assert about rows and counts, and
/// the routes here cannot reach a file even if one existed.
async fn seed_part(
    pool: &sqlx::PgPool,
    folder: Option<FolderId>,
    name: &str,
    source_path: &str,
    seed: u8,
) {
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 624_384,
        stored_bytes: 197_012,
        zstd_level: 3,
    };
    let storage_path = format!("libraries/default/{source_path}");
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name,
            source_path,
            folder,
            storage_path: Some(&storage_path),
            blob: &blob,
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed part");
}

/// The asymmetry with parts, and it is deliberate: two parts named `bracket` are told apart
/// by `source_path`; two categories named `Terrain` are told apart by nothing. So there is
/// no acknowledgement to send — the refusal is the answer.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn renaming_into_a_collision_is_refused_with_no_override(pool: sqlx::PgPool) {
    let folders = PgFolders(pool.clone());
    folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");

    let (status, body) = patch_folder(&pool, bases, serde_json::json!({ "name": "Terrain" })).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "nameTaken");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("Terrain")),
        "the refusal names the category in the way: {body}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_folder_cannot_be_parented_into_another_library(pool: sqlx::PgPool) {
    let other = second_library(&pool).await;
    let folders = PgFolders(pool.clone());
    let mine = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("mine");
    let theirs = folders
        .get_or_create(other, None, "Terrain", "Terrain")
        .await
        .expect("theirs");

    let (status, body) = patch_folder(
        &pool,
        mine,
        serde_json::json!({ "parentId": theirs.to_string() }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "crossLibrary");
}

/// A category cannot be moved inside itself, and the refusal is made by the same
/// transaction that would have written the move — see `PgFolders::reparent`.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_folder_cannot_be_parented_into_its_own_subtree(pool: sqlx::PgPool) {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");

    let (status, body) = patch_folder(
        &pool,
        terrain,
        serde_json::json!({ "parentId": rocks.to_string() }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], "wouldCycle");
}

/// Reparenting to the library root is `parentId: null`, and leaving the field out is a
/// rename that must not move anything. The two are different requests and the route reads
/// them as such.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_rename_alone_does_not_move_a_category_to_the_root(pool: sqlx::PgPool) {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");

    let (status, _) = patch_folder(&pool, rocks, serde_json::json!({ "name": "Boulders" })).await;
    assert_eq!(status, StatusCode::OK);

    let rows = tree(&pool).await;
    let renamed = rows
        .as_array()
        .expect("an array")
        .iter()
        .find(|node| node["id"] == rocks.to_string())
        .expect("the renamed category")
        .clone();
    assert_eq!(renamed["name"], "Boulders");
    assert_eq!(
        renamed["parentId"],
        terrain.to_string(),
        "a rename that also moved the category to the root would be a silent data change"
    );

    let (moved, _) = patch_folder(&pool, rocks, serde_json::json!({ "parentId": null })).await;
    assert_eq!(moved, StatusCode::OK);
    let rows = tree(&pool).await;
    let at_root = rows
        .as_array()
        .expect("an array")
        .iter()
        .find(|node| node["id"] == rocks.to_string())
        .expect("the moved category")
        .clone();
    assert!(
        at_root["parentId"].is_null(),
        "`null` is a real destination"
    );
}

/// Soft delete: the rows are hidden, every byte stays where it was, and the confirmation
/// that stands in front of this route is where a person is told the difference.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_folder_hides_its_models_and_touches_no_file(pool: sqlx::PgPool) {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    seed_part(&pool, Some(terrain), "rock", "Terrain/rock.stl", 0x11).await;
    seed_part(&pool, Some(rocks), "cliff", "Terrain/Rocks/cliff.stl", 0x22).await;

    let (status, body) = send(
        &pool,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/folders/{terrain}"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["foldersHidden"], 2, "the subcategory goes too");
    assert_eq!(body["partsHidden"], 2);
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE deleted_at IS NULL")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(visible, 0);
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM file")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(files, 2, "soft delete removes no file row and no bytes");
    assert_eq!(
        tree(&pool).await.as_array().map(Vec::len),
        Some(0),
        "a deleted category is not in the tree"
    );
}

/// The number the delete confirmation names. Subtree-inclusive, because the delete
/// cascades that way — a count that stopped at the folder itself would understate what is
/// about to be hidden — and live parts only, or the dialog contradicts the grid beside it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_tree_counts_every_model_under_a_category_and_no_deleted_one(pool: sqlx::PgPool) {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    seed_part(&pool, Some(terrain), "rock", "Terrain/rock.stl", 0x11).await;
    seed_part(&pool, Some(rocks), "cliff", "Terrain/Rocks/cliff.stl", 0x22).await;
    seed_part(&pool, Some(rocks), "scree", "Terrain/Rocks/scree.stl", 0x33).await;
    seed_part(&pool, None, "loose", "loose.stl", 0x44).await;
    sqlx::query("UPDATE part SET deleted_at = now() WHERE name = $1")
        .bind("scree")
        .execute(&pool)
        .await
        .expect("hides one");

    let rows = tree(&pool).await;

    let count_of = |id: FolderId| {
        rows.as_array()
            .expect("an array")
            .iter()
            .find(|node| node["id"] == id.to_string())
            .expect("the category")["partCount"]
            .clone()
    };
    assert_eq!(
        count_of(terrain),
        2,
        "its own model plus the live one below"
    );
    assert_eq!(count_of(rocks), 1, "the deleted model is not counted");
    assert_eq!(count_of(bases), 0);
    // The part at the library root belongs to no category, so it is in no node's count —
    // the sidebar's "All models" row is what stands for those.
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn creating_a_category_refuses_a_name_a_sibling_already_has(pool: sqlx::PgPool) {
    let uri = format!("/api/libraries/{SEEDED_LIBRARY}/folders");

    let (created, body) = send(
        &pool,
        json_request(
            "POST",
            uri.clone(),
            serde_json::json!({ "name": "Terrain" }),
        ),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    assert_eq!(body["name"], "Terrain");
    assert!(body["parentId"].is_null());
    assert_eq!(body["partCount"], 0, "a new category holds nothing");

    let (again, refusal) = send(
        &pool,
        json_request(
            "POST",
            uri.clone(),
            serde_json::json!({ "name": "Terrain" }),
        ),
    )
    .await;
    assert_eq!(again, StatusCode::CONFLICT);
    assert_eq!(refusal["reason"], "nameTaken");

    // A sibling of a *different* parent is not a collision at all.
    let parent = body["id"].as_str().expect("an id").to_owned();
    let (nested, _) = send(
        &pool,
        json_request(
            "POST",
            uri,
            serde_json::json!({ "name": "Terrain", "parentId": parent }),
        ),
    )
    .await;
    assert_eq!(nested, StatusCode::CREATED);
    assert_eq!(tree(&pool).await.as_array().map(Vec::len), Some(2));
}

/// Two names that differ only where a filesystem cannot show it. The row constraint would
/// allow them; the directory they both want is what refuses.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn creating_a_category_refuses_a_name_that_needs_a_taken_directory(pool: sqlx::PgPool) {
    let uri = format!("/api/libraries/{SEEDED_LIBRARY}/folders");

    let (created, _) = send(
        &pool,
        json_request("POST", uri.clone(), serde_json::json!({ "name": "Rocks?" })),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let (again, refusal) = send(
        &pool,
        json_request("POST", uri, serde_json::json!({ "name": "Rocks*" })),
    )
    .await;

    assert_eq!(again, StatusCode::CONFLICT);
    assert_eq!(refusal["reason"], "slugTaken");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|m| m.contains("Rocks-")),
        "the refusal names the directory both wanted: {refusal}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_category_that_does_not_exist_is_a_404(pool: sqlx::PgPool) {
    let missing = FolderId::new();

    let (patched, _) = patch_folder(&pool, missing, serde_json::json!({ "name": "Terrain" })).await;
    assert_eq!(patched, StatusCode::NOT_FOUND);

    let (deleted, body) = send(
        &pool,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/folders/{missing}"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!(deleted, StatusCode::NOT_FOUND);
    assert_eq!(body["reason"], "noSuchFolder");
}
