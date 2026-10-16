//! `DELETE /api/parts/{id}` — delete is delete (owner, 2026-10-17).
//!
//! What is under test is two things at once: the part is gone from every read path and leaves
//! no row behind, and **the model file on disk is never touched** — no bytes move, nothing is
//! queued for the reaper. "Gone" is four separate reads — the grid, the part page, the download
//! and the library's storage totals — so each is asserted on its own.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    // Most assertions below are about rows. The download case checks that the route refuses
    // *before* it would reach the store, and a path that does not exist is what makes that a
    // real assertion rather than a coincidence.
    state_at(pool, std::path::PathBuf::from("/nonexistent-blob-root"))
}

fn state_at(pool: sqlx::PgPool, blob_root: std::path::PathBuf) -> AppState {
    AppState {
        db: pool,
        blob_root,
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        touches: Default::default(),
    }
}

async fn call(pool: sqlx::PgPool, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
    call_with(state(pool), method, uri).await
}

async fn call_with(state: AppState, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state, Role::Api)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body is JSON")
    };
    (status, json)
}

/// Seeded through `PgIngest::record`, the repository the scan handler itself uses, so the
/// fixture carries a `blob` row with a real `ref_count` rather than a hand-built one.
async fn seed(pool: &sqlx::PgPool, seed: u8, name: &str, path: &str) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: path,
            // Staged through the content-addressed writer, so this row is one that has
            // not migrated: no folder, no storage path.
            folder: None,
            storage_path: None,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed part")
}

async fn grid_names(pool: &sqlx::PgPool) -> Vec<String> {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?limit=50"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json["parts"]
        .as_array()
        .expect("a parts array")
        .iter()
        .map(|p| p["name"].as_str().expect("a name").to_owned())
        .collect()
}

async fn source_bytes(pool: &sqlx::PgPool) -> u64 {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/storage"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json["sourceBytes"].as_u64().expect("a JSON number")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_part_takes_it_off_every_read_path_and_leaves_no_row(pool: sqlx::PgPool) {
    let gone = seed(
        &pool,
        0xa1,
        "Bracket, LP-1042-03",
        "brackets/LP-1042-03.stl",
    )
    .await;
    seed(&pool, 0xa2, "Impeller, LP-5501-02", "pumps/LP-5501-02.stl").await;
    let revision = PgParts(pool.clone())
        .latest_revision(gone)
        .await
        .expect("revision lookup")
        .expect("the seeded part has one");

    assert_eq!(grid_names(&pool).await.len(), 2);
    assert_eq!(source_bytes(&pool).await, 91_204 * 2);

    let (status, body) = call(pool.clone(), "DELETE", &format!("/api/parts/{gone}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, serde_json::Value::Null, "204 carries no body");

    // Four read paths, asserted one at a time.
    assert_eq!(grid_names(&pool).await, vec!["Impeller, LP-5501-02"]);
    assert_eq!(
        call(pool.clone(), "GET", &format!("/api/parts/{gone}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            pool.clone(),
            "GET",
            &format!("/api/revisions/{revision}/download?variant=original")
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "the download must refuse before it reaches the store, not after"
    );
    assert_eq!(
        source_bytes(&pool).await,
        91_204,
        "the library panel counts what the library holds"
    );

    // No row of the deleted part is left behind.
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM part WHERE id = $1) \
              + (SELECT count(*) FROM revision WHERE part_id = $1) \
              + (SELECT count(*) FROM file WHERE revision_id = $2)",
    )
    .bind(gone.as_uuid())
    .bind(revision.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(left, 0);
    // And nothing was queued to remove its bytes: the source blob keeps its count and no clock.
    assert_eq!(blob_state(&pool, 0xa1).await, (1, false));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_delete_is_a_404_and_restore_and_purge_are_gone(pool: sqlx::PgPool) {
    let part = seed(
        &pool,
        0xb1,
        "Bracket, LP-1042-03",
        "brackets/LP-1042-03.stl",
    )
    .await;
    assert_eq!(
        call(pool.clone(), "DELETE", &format!("/api/parts/{part}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, json) = call(pool.clone(), "DELETE", &format!("/api/parts/{part}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("has been deleted"),
        "the body must not distinguish a deleted part from one that never existed: {json}"
    );
    for route in ["restore", "purge"] {
        let status = call(pool.clone(), "POST", &format!("/api/parts/{part}/{route}"))
            .await
            .0;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "there is no {route} any more: {status}"
        );
    }
}

/// **The model file stays.** A part filed in the folder tree has its file at `storage_path`
/// under the storage root; deleting the part leaves that file byte for byte where it was, and
/// writes no `quarantined_file` row that would let the hourly sweep take it later.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_part_never_touches_its_model_file(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("a storage root");
    let storage_path = "libraries/default/bracket-lp-1042-03/bracket-lp-1042-03.stl";
    let on_disk = root.path().join(storage_path);
    std::fs::create_dir_all(on_disk.parent().expect("a model directory")).expect("mkdir");
    let bytes = b"solid bracket_lp_1042_03\nendsolid bracket_lp_1042_03\n";
    std::fs::write(&on_disk, bytes).expect("the model file");

    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            folder: None,
            storage_path: Some(storage_path),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()),
                size_bytes: bytes.len() as u64,
                stored_bytes: bytes.len() as u64,
                zstd_level: 0,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("a filed part");

    let (status, _) = call_with(
        state_at(pool.clone(), root.path().to_path_buf()),
        "DELETE",
        &format!("/api/parts/{part}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        std::fs::read(&on_disk).expect("the model file is still there"),
        bytes,
        "and it is byte for byte what it was"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM quarantined_file")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(queued, 0, "nothing tells the sweep to remove it later");
}

/// Seeds a second part at a different path over the *same* blob, the way ingest does when
/// it recognises bytes it already holds.
async fn seed_sharing(pool: &sqlx::PgPool, seed: u8, name: &str, path: &str) -> PartId {
    PgIngest(pool.clone())
        .link_existing(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: path,
            // Staged through the content-addressed writer, so this row is one that has
            // not migrated: no folder, no storage path.
            folder: None,
            storage_path: None,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed sharing part")
}

async fn blob_state(pool: &sqlx::PgPool, seed: u8) -> (i32, bool) {
    sqlx::query_as("SELECT ref_count, quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([seed; 32]).to_hex())
        .fetch_one(pool)
        .await
        .expect("blob state reads")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_one_of_two_parts_over_the_same_blob_leaves_the_other_whole(pool: sqlx::PgPool) {
    let first = seed(
        &pool,
        0xc1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;
    seed_sharing(
        &pool,
        0xc1,
        "Bracket, LP-1042-03 (spare)",
        "spares/LP-1042-03.stl",
    )
    .await;
    assert_eq!(blob_state(&pool, 0xc1).await, (2, false));

    assert_eq!(
        call(pool.clone(), "DELETE", &format!("/api/parts/{first}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(grid_names(&pool).await, vec!["Bracket, LP-1042-03 (spare)"]);
    let (_, clock) = blob_state(&pool, 0xc1).await;
    assert!(
        !clock,
        "the bytes the spare still needs are never queued for removal"
    );
}
