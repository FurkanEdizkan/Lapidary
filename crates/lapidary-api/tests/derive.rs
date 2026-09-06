//! Task 8: the three enqueue routes. `PATCH /api/libraries/{id}`,
//! `POST /api/parts/{id}/thumbnail` and `POST /api/libraries/{id}/thumbnails`, driven
//! through this crate's router against a live, migrated Postgres. Task 10 adds the read
//! half of the first of them, `GET /api/libraries/{id}`.
//!
//! Rows are seeded through `PgIngest` — the repository the scan handler itself uses — so
//! a part these tests create is indistinguishable from one a scan produced. The two
//! enqueue routes are then read back through the *existing* batch-status route rather
//! than through the `job` table, because "the polling works unchanged" is the claim being
//! made and a direct row count would not test it.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BatchId, BatchStatus, BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PartRepository, PgIngest, PgParts, Shows, StoredBlobRow};
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
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, 'Fixture library, other tenant')")
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

/// Records one part and hands back its id. `thumbnail` decides whether the revision gets
/// a `thumbnail` derivative, which is exactly what `revisions_missing` filters on — a
/// part seeded with `None` is one the sweep must find, and one seeded with `Some` is one
/// it must leave alone.
async fn seed_part(
    pool: &sqlx::PgPool,
    library: LibraryId,
    seed: u8,
    name: &str,
    thumbnail: Option<&[u8]>,
) -> PartId {
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 2_048,
        stored_bytes: 1_024,
        zstd_level: 3,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            library,
            name,
            source_path: name,
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp: thumbnail,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
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
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let app = router(
        AppState {
            db: pool,
            blob_root: blob_root(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        },
        role,
    );
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .expect("request builds"),
        None => builder.body(Body::empty()).expect("request builds"),
    };
    let response = app.oneshot(request).await.expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn patch_library(
    pool: sqlx::PgPool,
    role: Role,
    library: LibraryId,
    on: bool,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        role,
        "PATCH",
        format!("/api/libraries/{library}"),
        Some(serde_json::json!({ "autoThumbnail": on })),
    )
    .await
}

async fn get_library(
    pool: sqlx::PgPool,
    role: Role,
    library: LibraryId,
) -> (StatusCode, serde_json::Value) {
    send(pool, role, "GET", format!("/api/libraries/{library}"), None).await
}

async fn post_part_thumbnail(
    pool: sqlx::PgPool,
    role: Role,
    part: PartId,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        role,
        "POST",
        format!("/api/parts/{part}/thumbnail"),
        None,
    )
    .await
}

async fn post_sweep(
    pool: sqlx::PgPool,
    role: Role,
    library: LibraryId,
) -> (StatusCode, serde_json::Value) {
    send(
        pool,
        role,
        "POST",
        format!("/api/libraries/{library}/thumbnails"),
        None,
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
        None,
    )
    .await
}

/// The `batchId` an enqueue answered with, parsed the way a client would have to.
fn accepted_batch(json: &serde_json::Value) -> BatchId {
    json["batchId"]
        .as_str()
        .expect("the body carries a batchId")
        .parse()
        .expect("the batchId is an id")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn turning_the_ingest_thumbnail_off_answers_with_the_value_that_landed(pool: sqlx::PgPool) {
    let (status, json) = patch_library(pool.clone(), Role::Api, library(), false).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, serde_json::json!({ "autoThumbnail": false }));
    assert_eq!(
        PgParts(pool)
            .auto_thumbnail(library())
            .await
            .expect("reads"),
        Some(false),
        "the row itself has to change, not just the response"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_setting_can_be_turned_back_on(pool: sqlx::PgPool) {
    patch_library(pool.clone(), Role::Api, library(), false).await;
    let (status, json) = patch_library(pool.clone(), Role::Api, library(), true).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, serde_json::json!({ "autoThumbnail": true }));
    assert_eq!(
        PgParts(pool)
            .auto_thumbnail(library())
            .await
            .expect("reads"),
        Some(true)
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn patching_a_library_that_does_not_exist_is_a_404_not_a_silent_success(pool: sqlx::PgPool) {
    // A `UPDATE ... WHERE id = $1` that matches nothing is a successful statement. Without
    // `set_auto_thumbnail`'s row count this answers 200 and tells a person their setting
    // was saved.
    let absent = LibraryId::new();
    let (status, json) = patch_library(pool, Role::Api, absent, false).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("No library with that id")),
        "the 404 has to be this crate's message, not axum's empty body: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_that_renders_nothing_reads_back_as_off(pool: sqlx::PgPool) {
    // Switched off through the repository rather than through `PATCH`, so this cannot pass
    // by the route echoing its own request back: the value on the wire has to have come
    // out of the `library` row. Without a `GET` at all the grid renders design §3.2's
    // default here, and an owner who turned rendering off sees it reported as on.
    PgParts(pool.clone())
        .set_auto_thumbnail(library(), false)
        .await
        .expect("switches the setting off");

    let (status, json) = get_library(pool, Role::Api, library()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, serde_json::json!({ "autoThumbnail": false }));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_nobody_has_touched_reads_back_as_the_migrations_default(pool: sqlx::PgPool) {
    let (status, json) = get_library(pool, Role::Api, library()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, serde_json::json!({ "autoThumbnail": true }));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn reading_a_library_that_does_not_exist_is_a_404_not_the_documented_default(
    pool: sqlx::PgPool,
) {
    // T8-B again, on the read side: answering `{ "autoThumbnail": true }` for an id that
    // names nothing is a plausible-looking body that gets believed. The message is asserted
    // whole rather than by its shared opening, because `no_such_library`'s "so nothing was
    // changed" would satisfy a prefix check while telling a reader their edit did not land.
    let absent = LibraryId::new();
    let (status, json) = get_library(pool, Role::Api, absent).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("no settings to show")),
        "the read gets the read's 404, not the writer's: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn patching_one_library_leaves_another_tenants_setting_alone(pool: sqlx::PgPool) {
    let other = other_library(&pool).await;

    let (status, _) = patch_library(pool.clone(), Role::Api, library(), false).await;
    assert_eq!(status, StatusCode::OK);

    let parts = PgParts(pool);
    assert_eq!(
        parts.auto_thumbnail(library()).await.expect("reads"),
        Some(false)
    );
    assert_eq!(
        parts.auto_thumbnail(other).await.expect("reads"),
        Some(true),
        "the other library keeps the migration's default"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_body_that_is_not_the_expected_shape_is_a_400_that_says_what_to_send(pool: sqlx::PgPool) {
    let (status, json) = send(
        pool,
        Role::Api,
        "PATCH",
        format!("/api/libraries/{}", library()),
        Some(serde_json::json!({ "autoThumbnail": "yes" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("autoThumbnail")),
        "the message names the field a caller has to send: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn one_parts_thumbnail_is_a_batch_of_one_the_existing_poll_can_read(pool: sqlx::PgPool) {
    let part = seed_part(&pool, library(), 0x11, "Bracket, LP-1042-03", None).await;

    let (status, json) = post_part_thumbnail(pool.clone(), Role::Api, part).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["queued"], 1);
    let batch = accepted_batch(&json);

    // The claim this route makes is that it reuses the scan's response shape so the
    // existing polling works unchanged. Reading the batch back through that very route is
    // the only thing that tests it.
    let (status, json) = get_batch(pool, library(), batch).await;
    assert_eq!(status, StatusCode::OK);
    let body: BatchStatus = serde_json::from_value(json).expect("body is a BatchStatus");
    assert_eq!(body.batch_id, batch);
    assert_eq!(body.library_id, library());
    assert_eq!(body.total, 1);
    assert_eq!(body.pending, 1);
    assert_eq!(body.rendered, 0);
    assert_eq!(body.failed_total, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_thumbnail_for_a_part_that_does_not_exist_is_a_404(pool: sqlx::PgPool) {
    let (status, json) = post_part_thumbnail(pool, Role::Api, PartId::new()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("No part with that id")),
        "expected this crate's 404 body, got {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_part_is_not_worth_rendering_and_answers_the_same_404(pool: sqlx::PgPool) {
    let part = seed_part(&pool, library(), 0x12, "Spacer, LP-2001-00", None).await;
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(part.as_uuid())
        .execute(&pool)
        .await
        .expect("soft-deletes the part");

    let (status, json) = post_part_thumbnail(pool.clone(), Role::Api, part).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json["message"].as_str().is_some_and(|m| m.contains("part")));
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts jobs");
    assert_eq!(queued, 0, "a deleted part must enqueue nothing at all");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_sweep_queues_one_job_for_each_revision_with_no_thumbnail(pool: sqlx::PgPool) {
    seed_part(&pool, library(), 0x21, "Bracket, LP-1042-03", None).await;
    seed_part(&pool, library(), 0x22, "Spacer, LP-2001-00", None).await;
    seed_part(&pool, library(), 0x23, "Jig, LP-3072-01", Some(b"webp-3")).await;

    let (status, json) = post_sweep(pool.clone(), Role::Api, library()).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        json["queued"], 2,
        "the part that already has one is skipped"
    );
    let batch = accepted_batch(&json);

    let (status, json) = get_batch(pool, library(), batch).await;
    assert_eq!(status, StatusCode::OK);
    let body: BatchStatus = serde_json::from_value(json).expect("body is a BatchStatus");
    assert_eq!(body.total, 2);
    assert_eq!(body.pending, 2);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_with_nothing_missing_queues_nothing_and_that_is_a_success(pool: sqlx::PgPool) {
    seed_part(
        &pool,
        library(),
        0x31,
        "Bracket, LP-1042-03",
        Some(b"webp-1"),
    )
    .await;
    seed_part(
        &pool,
        library(),
        0x32,
        "Spacer, LP-2001-00",
        Some(b"webp-2"),
    )
    .await;

    let (status, json) = post_sweep(pool.clone(), Role::Api, library()).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["queued"], 0);
    let batch = accepted_batch(&json);

    // `ScanAccepted`'s doc: a batch with zero jobs has no status resource and must not be
    // polled. Pinning the 404 keeps the two halves of that contract in one place.
    let (status, _) = get_batch(pool, library(), batch).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_sweep_over_a_library_that_does_not_exist_is_a_404_like_patch_is(pool: sqlx::PgPool) {
    // `revisions_missing` finds nothing for a library that does not exist and nothing for
    // one with every preview already rendered, so without an existence check the route
    // answers both with `202 queued: 0` and someone who mistyped an id reads it as
    // "nothing was missing". The non-disclosure that asymmetry was defended with is not
    // obtained either: `PATCH` on this very id says 404 and discloses the same fact.
    let absent = LibraryId::new();
    let (status, json) = post_sweep(pool.clone(), Role::Api, absent).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("No library with that id")),
        "the 404 has to be this crate's message, not axum's empty body: {json}"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts jobs");
    assert_eq!(
        queued, 0,
        "a 404 must not leave a batch behind on its way out"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_sweep_counts_only_the_revisions_of_the_library_it_names(pool: sqlx::PgPool) {
    let other = other_library(&pool).await;
    seed_part(&pool, library(), 0x41, "Bracket, LP-1042-03", None).await;
    seed_part(&pool, library(), 0x42, "Spacer, LP-2001-00", None).await;
    seed_part(&pool, other, 0x43, "Vee block, LP-3072-02", None).await;
    seed_part(&pool, other, 0x44, "Collet, LP-4110-07", None).await;
    seed_part(&pool, other, 0x45, "Shim, LP-5003-11", None).await;

    let (status, json) = post_sweep(pool.clone(), Role::Api, library()).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        json["queued"], 2,
        "the other tenant's three revisions are not this library's work"
    );
    let batch = accepted_batch(&json);

    let (status, json) = get_batch(pool.clone(), library(), batch).await;
    assert_eq!(status, StatusCode::OK);
    let body: BatchStatus = serde_json::from_value(json).expect("body is a BatchStatus");
    assert_eq!(body.total, 2);

    // And the other library still has all three to do, so nothing was rendered onto it.
    let (status, json) = post_sweep(pool, Role::Api, other).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["queued"], 3);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_is_enqueued_under_the_parts_own_library_and_no_other(pool: sqlx::PgPool) {
    let other = other_library(&pool).await;
    let part = seed_part(&pool, other, 0x51, "Vee block, LP-3072-02", None).await;

    let (status, json) = post_part_thumbnail(pool.clone(), Role::Api, part).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let batch = accepted_batch(&json);

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
        .page(library(), None, 50, Shows::Live)
        .await
        .expect("reads the grid");
    assert!(rows.is_empty());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_serves_none_of_these_routes(pool: sqlx::PgPool) {
    // Every id here is one that *works* under `Role::Api` — the seeded library, and a real
    // part. A 404 for an id that does not exist would prove nothing, because a mounted
    // handler answers 404 for those too. What separates "not mounted" from "mounted and
    // says no" is the body: axum's own 404 is empty.
    let part = seed_part(&pool, library(), 0x61, "Bracket, LP-1042-03", None).await;

    let (status, json) = patch_library(pool.clone(), Role::Worker, library(), false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json, serde_json::Value::Null, "PATCH is mounted on worker");

    let (status, json) = get_library(pool.clone(), Role::Worker, library()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json, serde_json::Value::Null, "GET is mounted on worker");

    let (status, json) = post_part_thumbnail(pool.clone(), Role::Worker, part).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        json,
        serde_json::Value::Null,
        "the part thumbnail route is mounted on worker"
    );

    let (status, json) = post_sweep(pool.clone(), Role::Worker, library()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        json,
        serde_json::Value::Null,
        "the sweep route is mounted on worker"
    );

    // Nothing reached the database through any of them.
    assert_eq!(
        PgParts(pool.clone())
            .auto_thumbnail(library())
            .await
            .expect("reads"),
        Some(true)
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts jobs");
    assert_eq!(queued, 0);
}
