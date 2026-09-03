//! Task 11: the batch status route. `GET /api/libraries/{library}/jobs/{batch}`, seeded
//! through `PgJobs::enqueue_scan` — the same call the scan endpoint makes — against a
//! live, migrated Postgres, and read back through this crate's router.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BatchId, BatchStatus, LibraryId};
use lapidary_db::PgJobs;
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// GETs the batch status route under `role` and returns the status with the decoded body.
///
/// A body that is not JSON comes back as `Value::Null` rather than panicking, because
/// that is a real and meaningful response here: a route this router never mounted is
/// answered by axum's own 404, which has an empty body, while a mounted route that
/// answers 404 returns this crate's JSON message. `the_worker_role_does_not_serve_batch_status`
/// turns on exactly that difference.
async fn get_status(
    pool: sqlx::PgPool,
    role: Role,
    library: &LibraryId,
    batch: &BatchId,
) -> (StatusCode, serde_json::Value) {
    let app = router(AppState { db: pool }, role);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/libraries/{library}/jobs/{batch}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_running_batch_reports_its_counts(pool: sqlx::PgPool) {
    let (batch, queued) = PgJobs(pool.clone())
        .enqueue_scan(
            seeded(),
            &[
                "bracket-lp-1042-03.stl".to_owned(),
                "spacer-lp-2001-00.stl".to_owned(),
            ],
        )
        .await
        .expect("enqueues");
    assert_eq!(queued, 2);

    let (status, json) = get_status(pool, Role::Api, &seeded(), &batch).await;

    assert_eq!(status, StatusCode::OK);
    let body: BatchStatus = serde_json::from_value(json).expect("body is a BatchStatus");
    assert_eq!(body.batch_id, batch);
    assert_eq!(body.library_id, seeded());
    assert_eq!(body.total, 2);
    assert_eq!(body.pending, 2, "nothing has claimed them yet");
    assert_eq!(body.running, 0);
    assert_eq!(body.ingested, 0);
    assert_eq!(body.skipped, 0);
    assert_eq!(body.failed_total, 0);
    assert!(body.failed.is_empty());
    assert!(
        body.finished_at.is_none(),
        "an unfinished batch must not report a finish time -- this is what stops the poll"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_id_that_was_never_issued_is_not_found(pool: sqlx::PgPool) {
    let (status, json) = get_status(pool, Role::Api, &seeded(), &BatchId::new()).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .expect("message is a string")
            .contains("No scan with that id"),
        "the 404 must say what is wrong and what to do: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_belonging_to_another_library_is_not_found(pool: sqlx::PgPool) {
    // Content addressing is not authorization, and a job id is no different. Holding a
    // real batch id must not let a caller read it under a library it does not belong to,
    // and the answer must be indistinguishable from an id that was never issued -- a
    // different status or message here would confirm the batch exists somewhere.
    let (batch, _) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let (status, json) = get_status(pool, Role::Api, &LibraryId::new(), &batch).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .expect("message is a string")
            .contains("No scan with that id"),
        "the same answer as a never-issued id, verbatim: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_that_queued_nothing_has_no_status_resource(pool: sqlx::PgPool) {
    // An empty directory scans successfully and returns a real batch id with `queued: 0`.
    // With no rows there is nothing to aggregate, so there is no resource -- which is why
    // `ScanAccepted`'s doc tells the client not to poll when `queued` is zero.
    let (batch, queued) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &[])
        .await
        .expect("enqueues nothing, successfully");
    assert_eq!(queued, 0);

    let (status, _) = get_status(pool, Role::Api, &seeded(), &batch).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_batch_status(pool: sqlx::PgPool) {
    // The open path's reads mount under Role::Api only. The worker process serves health
    // and its own ingest router (merged in by bin/lapidary-server) and nothing else from
    // here, so a real batch id must still 404 against it.
    let (batch, _) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let (status, json) = get_status(pool, Role::Worker, &seeded(), &batch).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        json,
        serde_json::Value::Null,
        "an empty body proves the route was never mounted, rather than mounted and \
         answering 404 for a batch it could not find -- which is what a real batch id \
         makes this test able to tell apart"
    );
}
