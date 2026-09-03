//! The loop, against a live database and a handler that records what it saw.

use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{JobRow, PgJobs};
use lapidary_jobs::{HandlerError, JobHandler, WorkerConfig, run};
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

/// Counts what it handled. Real enough to prove the loop delivers work; it does not
/// touch a mesh, which is the point of the seam.
struct CountingHandler {
    seen: AtomicUsize,
}

impl JobHandler for CountingHandler {
    async fn handle(&self, _job: &JobRow) -> Result<Outcome, HandlerError> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome::Ingested)
    }
}

async fn drain(pool: PgPool, config: WorkerConfig, expect: usize) -> usize {
    let handler = Arc::new(CountingHandler {
        seen: AtomicUsize::new(0),
    });
    let shutdown = CancellationToken::new();
    let worker = tokio::spawn(run(
        PgJobs(pool.clone()),
        handler.clone(),
        config,
        shutdown.clone(),
    ));

    // Poll for completion rather than sleeping a fixed amount: a fixed sleep makes this
    // test's pass/fail depend on how loaded the machine is.
    for _ in 0..200 {
        if handler.seen.load(Ordering::SeqCst) >= expect {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    shutdown.cancel();
    worker
        .await
        .expect("the worker task joins")
        .expect("the worker exits cleanly");
    handler.seen.load(Ordering::SeqCst)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_queue_drains_with_the_listener_disabled(pool: PgPool) {
    // NOTIFY is a latency optimization, never the correctness mechanism. A NOTIFY fires
    // into the void when nothing is listening, so a worker that starts after an enqueue
    // -- or whose listener connection dropped -- must still find the work by polling.
    // If this test ever hangs, the polling floor has stopped being the mechanism.
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(
        seeded(),
        &[
            "bracket-lp-1042-03.stl".to_owned(),
            "spacer-lp-2001-00.stl".to_owned(),
        ],
    )
    .await
    .expect("enqueues");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 2,
        listen: false,
    };

    assert_eq!(
        drain(pool, config, 2).await,
        2,
        "polling alone must drain the queue"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_past_its_attempt_cap_is_abandoned_without_running_the_handler(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    // The poison-pill shape: a worker that dies before it can record anything. attempts
    // is already past the cap, so claiming it must fail it rather than hand it out.
    sqlx::query("UPDATE job SET attempts = 5, max_attempts = 3")
        .execute(&pool)
        .await
        .expect("exhausts the job");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 1,
        listen: false,
    };
    let handled = drain(pool.clone(), config, 0).await;
    assert_eq!(handled, 0, "an exhausted job must never reach the handler");

    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state, last_error FROM job")
            .fetch_one(&pool)
            .await
            .expect("reads back");
    assert_eq!(state, "failed");
    assert!(
        reason.unwrap_or_default().contains("stopped responding"),
        "the message must blame the worker, not the file"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn shutting_down_hands_back_what_the_worker_still_holds(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    jobs.dequeue("test-worker", Duration::from_secs(60))
        .await
        .expect("dequeues")
        .expect("a job");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 1,
        listen: false,
    };
    drain(pool.clone(), config, 1).await;

    let state: String = sqlx::query_scalar("SELECT state FROM job")
        .fetch_one(&pool)
        .await
        .expect("reads back");
    assert_ne!(
        state, "running",
        "shutdown must not leave a job leased to a dead worker"
    );
}
