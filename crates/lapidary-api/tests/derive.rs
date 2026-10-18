//! `POST /api/parts/{id}/exports/{format}` — the one enqueue route left in `derive.rs`, driven
//! through this crate's router against a live, migrated Postgres.
//!
//! **R3 deleted the rest of this file with the routes it covered.** `GET`/`PATCH
//! /api/libraries/{id}` (the ingest-time thumbnail setting), `POST /api/parts/{id}/thumbnail`,
//! `POST /api/libraries/{id}/thumbnails` and `POST /api/parts/{id}/rungs/{level}` are gone:
//! nothing renders, so a rung request answered `202` with a batch id for work that would never
//! land and the viewer polled it for ever. The two exports are still built on demand, and this
//! is how something asks.
//!
//! Rows are seeded through `PgIngest` — the repository the scan handler itself uses — so
//! a part these tests create is indistinguishable from one a scan produced. The batch is read
//! back through the *existing* batch-status route rather than through the `job` table, because
//! "the polling works unchanged" is the claim being made and a direct row count would not test
//! it.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use lapidary_core::{BatchId, BatchStatus, BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{
    GridQuery, IngestRequest, PartRepository, PgIngest, PgParts, Sort, StoredBlobRow,
};
use tower::ServiceExt;

/// These tests never read a blob; the field is required to build the state, and a path
/// that does not exist is the honest value for a test that must not reach the store.
fn blob_root() -> std::path::PathBuf {
    std::path::PathBuf::from("/nonexistent-blob-root")
}

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// A second library, standing in for another tenant. Nothing creates a library through
/// the API, so it is inserted directly — the same fixture shape
/// `crates/lapidary-api/tests/parts.rs` and `crates/lapidary-db/tests/repo.rs` use.
async fn other_library(pool: &sqlx::PgPool) -> LibraryId {
    let id = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, 'Fixture library, other tenant', 'fixture library, other tenant')")
        .bind(id.as_uuid())
        .execute(pool)
        .await
        .expect("insert second library");
    id
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

/// Records one part and hands back its id. No derivative: ingest writes none since R3, and
/// every derivative in these tests is filed afterwards, as a `derive` job files one.
async fn seed_part(pool: &sqlx::PgPool, library: LibraryId, seed: u8, name: &str) -> PartId {
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 2_048,
        stored_bytes: 1_024,
        zstd_level: 3,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: None,
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library,
            name,
            source_path: name,
            blob: &blob,
            measurements: Some(&measurements()),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed part")
}

/// Sends one request through the router built for `role` and decodes the response.
///
/// A body that is not JSON comes back as `Value::Null` rather than panicking, and that
/// distinction is the whole absence test: a route this router never mounted is answered
/// by axum's own 404, which has an empty body, while a mounted route answering 404
/// returns this crate's JSON message.
async fn send(
    pool: sqlx::PgPool,
    role: Role,
    method: &str,
    uri: String,
) -> (StatusCode, serde_json::Value) {
    let app = router(
        AppState {
            db: pool,
            blob_root: blob_root(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        role,
    );
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request builds");
    let response = app.oneshot(request).await.expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn post_export(
    pool: sqlx::PgPool,
    role: Role,
    part: PartId,
    format: &str,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        role,
        "POST",
        format!("/api/parts/{part}/exports/{format}"),
    )
    .await
}

/// Reads a batch back through the route the frontend already polls.
async fn get_batch(
    pool: sqlx::PgPool,
    library: LibraryId,
    batch: BatchId,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        Role::Api,
        "GET",
        format!("/api/libraries/{library}/jobs/{batch}"),
    )
    .await
}

/// An export not built yet is queued as a derive job for that format; asking again while that
/// build is queued watches the same job; once it exists, asking answers with its hash and queues
/// nothing. A format Lapidary does not write a mesh to is refused by name.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_export_not_built_yet_is_queued_and_one_that_exists_answers_with_its_hash(
    pool: sqlx::PgPool,
) {
    let part = seed_part(&pool, library(), 0x42, "Ball knob, LP-9020-00").await;

    let (status, json) = post_export(pool.clone(), Role::Api, part, "3mf").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["queued"], 1);
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM job WHERE kind = 'derive'")
            .fetch_one(&pool)
            .await
            .expect("one derive job");
    assert_eq!(
        payload["produce"],
        serde_json::to_value(lapidary_core::DerivativeKind::Export3mf).expect("serializes")
    );

    // Asking again while that build is still queued watches the same job rather than queuing a
    // second build of the same file.
    let batch = json["batchId"].clone();
    let (status, json) = post_export(pool.clone(), Role::Api, part, "3mf").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["batchId"], batch, "the batch already building it");
    let derives: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'derive'")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(derives, 1, "no second build of an export already queued");

    let revision = PgParts(pool.clone())
        .latest_revision(part)
        .await
        .expect("query")
        .expect("a revision");
    let export = StoredBlobRow {
        hash: BlobHash::from_bytes([0x5a; 32]),
        size_bytes: 11_264,
        stored_bytes: 11_264,
        zstd_level: 0,
    };
    PgIngest(pool.clone())
        .upsert_derivative(
            revision,
            lapidary_core::DerivativeKind::Export3mf,
            &export,
            None,
            "mesh stl-1+glb-2+cpu-1",
        )
        .await
        .expect("the export lands");

    let (status, json) = post_export(pool.clone(), Role::Api, part, "3mf").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["hash"], export.hash.to_hex());
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(jobs, 1, "an export that exists queues nothing");

    let (status, json) = post_export(pool.clone(), Role::Api, part, "step").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("`step`") && message.contains("`3mf` or `stl`")),
        "names what was sent and what to send: {json}"
    );

    let (status, _) = post_export(pool, Role::Api, PartId::new(), "stl").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// **`?variant=l0` is not this route's any more, and neither is `rungs/l1`.** Both are links
/// people hold — the viewer's own download and the rung request behind it — so each must answer
/// in words rather than as a 500 or a route that half works. The rung route is gone from the
/// router entirely, so axum answers it with an empty 404.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_rung_route_is_gone_and_a_stale_bookmark_meets_axums_own_404(pool: sqlx::PgPool) {
    let part = seed_part(&pool, library(), 0x43, "Bracket, LP-1042-03").await;
    for level in ["l0", "l1", "l2"] {
        let (status, json) = send(
            pool.clone(),
            Role::Api,
            "POST",
            format!("/api/parts/{part}/rungs/{level}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json,
            serde_json::Value::Null,
            "an empty body is what says the route is not mounted rather than refusing"
        );
    }
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts jobs");
    assert_eq!(queued, 0, "and nothing was queued on the way");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_is_enqueued_under_the_parts_own_library_and_no_other(pool: sqlx::PgPool) {
    let other = other_library(&pool).await;
    let part = seed_part(&pool, other, 0x51, "Vee block, LP-3072-02").await;

    let (status, json) = post_export(pool.clone(), Role::Api, part, "stl").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let batch: BatchId = json["batchId"]
        .as_str()
        .expect("the body carries a batchId")
        .parse()
        .expect("the batchId is an id");

    // The route has no library in its path, so the only thing that can scope the batch is
    // the part's own `library_id`. It has to be readable from that library...
    let (status, json) = get_batch(pool.clone(), other, batch).await;
    assert_eq!(status, StatusCode::OK);
    let body: BatchStatus = serde_json::from_value(json).expect("body is a BatchStatus");
    assert_eq!(body.library_id, other);
    assert_eq!(body.total, 1);

    // ...and from no other, exactly as a scan's batch is. A batch id is a uuid a caller
    // might hold from anywhere; content addressing is not authorization.
    let (status, json) = get_batch(pool.clone(), library(), batch).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("No scan with that id")),
        "the other library must not be able to read this batch: {json}"
    );

    // The seeded library also gained no part, so nothing was written across the boundary.
    let rows = PgParts(pool)
        .page(&GridQuery::new(library(), 50), Sort::Newest)
        .await
        .expect("reads the grid");
    assert!(rows.is_empty());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_the_export_route(pool: sqlx::PgPool) {
    // The id here is one that *works* under `Role::Api`. A 404 for an id that does not exist
    // would prove nothing, because a mounted handler answers 404 for those too. What separates
    // "not mounted" from "mounted and says no" is the body: axum's own 404 is empty.
    let part = seed_part(&pool, library(), 0x61, "Bracket, LP-1042-03").await;

    let (status, json) = post_export(pool.clone(), Role::Worker, part, "stl").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        json,
        serde_json::Value::Null,
        "the export route is mounted on worker"
    );

    // Nothing reached the database through it.
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts jobs");
    assert_eq!(queued, 0);
}
