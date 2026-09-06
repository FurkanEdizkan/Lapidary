//! The optional manual trigger: `POST /api/libraries/{id}/migrate-storage`. What it must
//! prove is small on purpose -- the guard itself is `lapidary-db`'s
//! `PgJobs::enqueue_migration_if_absent`, already pinned by its own tests. This is only
//! the wiring: the route reaches that guard, and a second call is a no-op rather than a
//! second job.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_core::ScanAccepted;
use lapidary_ingest::{AppState, router};
use sqlx::PgPool;
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn state(pool: PgPool) -> AppState {
    AppState {
        db: pool,
        ingest_dir: std::env::temp_dir(),
        blob_root: std::env::temp_dir(),
    }
}

/// POSTs `/api/libraries/{library}/migrate-storage` and returns the status alongside the
/// parsed body.
async fn migrate(pool: PgPool, library: &str) -> (StatusCode, ScanAccepted) {
    let app = router(state(pool));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/libraries/{library}/migrate-storage"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    let accepted: ScanAccepted = serde_json::from_slice(&bytes).expect("body is a ScanAccepted");
    (status, accepted)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_route_queues_one_job_and_a_second_call_queues_nothing(pool: PgPool) {
    let (status, first) = migrate(pool.clone(), SEEDED_LIBRARY).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        first.queued, 1,
        "the first call has nothing to collide with"
    );

    let kind: String = sqlx::query_scalar("SELECT kind FROM job WHERE batch_id = $1")
        .bind(first.batch_id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the job it queued is readable back");
    assert_eq!(kind, "migrate_storage");

    let (status, second) = migrate(pool.clone(), SEEDED_LIBRARY).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "queuing nothing is still a success"
    );
    assert_eq!(
        second.queued, 0,
        "a migration is already pending for this library"
    );

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'migrate_storage'")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(total, 1, "the second call must not have queued a duplicate");
}
