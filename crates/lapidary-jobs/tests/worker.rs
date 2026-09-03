//! The loop, against a live database and a handler that records what it saw.

use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{JobRow, PgJobs};
use lapidary_jobs::{HandlerError, JobHandler, WorkerConfig, run};
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
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

/// Blocks inside `handle` until it is released, so a test can cancel the worker while a
/// job is genuinely in flight rather than merely leased. `CountingHandler` cannot stand
/// in for this: it returns immediately, so by the time anything observes the worker the
/// job is already finished.
struct BlockingHandler {
    /// Signalled once `handle` has actually been entered.
    started: Notify,
    /// Awaited inside `handle`; the test decides when the job may finish.
    release: Notify,
}

impl JobHandler for BlockingHandler {
    async fn handle(&self, _job: &JobRow) -> Result<Outcome, HandlerError> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(Outcome::Ingested)
    }
}

async fn drain(pool: PgPool, config: WorkerConfig) -> usize {
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

    wait_until_drained(&pool).await;

    shutdown.cancel();
    worker
        .await
        .expect("the worker task joins")
        .expect("the worker exits cleanly");
    handler.seen.load(Ordering::SeqCst)
}

/// Poll the database for what "drained" actually means -- no job left pending or
/// running -- rather than for the handler's call count. The exhausted-job case never
/// calls the handler at all, so waiting on `handler.seen` cannot tell "not drained yet"
/// apart from "will never be called": with a target of zero calls, a count-based wait is
/// satisfied before the worker gets a chance to run, which is exactly the race that made
/// this loop's own shutdown check look broken. Waiting on state gets every case right,
/// including that one. Bounded so a genuine hang still fails the test instead of
/// spinning forever.
async fn wait_until_drained(pool: &PgPool) {
    for _ in 0..200 {
        let outstanding: i64 =
            sqlx::query_scalar("SELECT count(*) FROM job WHERE state IN ('pending', 'running')")
                .fetch_one(pool)
                .await
                .expect("counts outstanding jobs");
        if outstanding == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn polling_discovers_work_enqueued_while_the_worker_sleeps(pool: PgPool) {
    // Spec S3.4: NOTIFY is a latency optimization and the polling floor is the
    // correctness mechanism. With `listen: false` no LISTEN connection is opened at all,
    // so the `pg_notify` that `enqueue_scan` issues below fires into the void and the
    // only thing that can find this work is a poll tick.
    //
    // The ordering here is the whole test. The queue starts EMPTY and the worker is left
    // to reach `wait_for_work` before anything is enqueued. Enqueue first instead and the
    // worker claims both jobs on its opening iterations, never taking the `Ok(None)` arm
    // at all -- what that proves is only that an idle loop notices cancellation, which is
    // a different property with a different name. If this test ever hangs, the polling
    // floor has stopped being what discovers work.
    let handler = Arc::new(CountingHandler {
        seen: AtomicUsize::new(0),
    });
    let shutdown = CancellationToken::new();
    let worker = tokio::spawn(run(
        PgJobs(pool.clone()),
        handler.clone(),
        WorkerConfig {
            worker_id: "test-worker".to_owned(),
            lease: Duration::from_secs(60),
            poll_interval: Duration::from_millis(100),
            concurrency: 2,
            listen: false,
        },
        shutdown.clone(),
    ));

    // Several poll intervals, so "the worker is asleep in wait_for_work" is not a race
    // even on a loaded machine.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        handler.seen.load(Ordering::SeqCst),
        0,
        "nothing is enqueued yet, so the handler cannot have run"
    );

    PgJobs(pool.clone())
        .enqueue_scan(
            seeded(),
            &[
                "bracket-lp-1042-03.stl".to_owned(),
                "spacer-lp-2001-00.stl".to_owned(),
            ],
        )
        .await
        .expect("enqueues");

    wait_until_drained(&pool).await;
    shutdown.cancel();
    worker
        .await
        .expect("the worker task joins")
        .expect("the worker exits cleanly");

    assert_eq!(
        handler.seen.load(Ordering::SeqCst),
        2,
        "polling alone must discover work that arrived while the worker slept"
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
    let handled = drain(pool.clone(), config).await;
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
    drain(pool.clone(), config).await;

    let state: String = sqlx::query_scalar("SELECT state FROM job")
        .fetch_one(&pool)
        .await
        .expect("reads back");
    assert_ne!(
        state, "running",
        "shutdown must not leave a job leased to a dead worker"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn shutdown_waits_for_a_job_in_flight_and_keeps_its_real_outcome(pool: PgPool) {
    // Spec S4.4's middle step, between "stop dequeuing" and "release their leases".
    // `shutting_down_hands_back_what_the_worker_still_holds` never reaches it: that test
    // pre-leases a row on its own `dequeue` call, so no handler is ever running and only
    // `release_leases` is exercised. Here a handler is genuinely mid-job when the token
    // is cancelled.
    //
    // Drop the await-in-flight step and `release_leases` reverts this row to 'pending'
    // while the handler is still working. The handler's own `complete` then arrives as a
    // stale write, which `AND state = 'running'` correctly drops -- so the file's real
    // outcome is lost and the row reads 'pending' instead of 'done'.
    //
    // Concurrency must be above 1 for any of that to be observable. At 1 the loop blocks
    // on `acquire_owned` until the handler releases its permit, and that only happens
    // after the outcome is already recorded -- so shutdown could not run early even if it
    // wanted to, and the mutation above would pass unnoticed.
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let handler = Arc::new(BlockingHandler {
        started: Notify::new(),
        release: Notify::new(),
    });
    let shutdown = CancellationToken::new();
    let worker = tokio::spawn(run(
        PgJobs(pool.clone()),
        handler.clone(),
        WorkerConfig {
            worker_id: "test-worker".to_owned(),
            lease: Duration::from_secs(60),
            poll_interval: Duration::from_millis(100),
            concurrency: 2,
            listen: false,
        },
        shutdown.clone(),
    ));

    tokio::time::timeout(Duration::from_secs(10), handler.started.notified())
        .await
        .expect("the handler is entered rather than the test hanging");

    shutdown.cancel();
    // Long enough for the loop to break and settle into the await-in-flight step. The row
    // must still be 'running' at this point: shutdown is waiting, not releasing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let leased: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE state = 'running'")
        .fetch_one(&pool)
        .await
        .expect("counts running jobs");
    assert_eq!(
        leased, 1,
        "shutdown must wait for a working handler rather than revert its job"
    );

    handler.release.notify_one();
    worker
        .await
        .expect("the worker task joins")
        .expect("the worker exits cleanly");

    let (state, outcome): (String, Option<String>) =
        sqlx::query_as("SELECT state, outcome FROM job")
            .fetch_one(&pool)
            .await
            .expect("reads back");
    assert_eq!(
        state, "done",
        "the handler's real outcome must be recorded, not reverted by release_leases"
    );
    assert_eq!(
        outcome.as_deref(),
        Some("ingested"),
        "and it must be the outcome the handler actually returned"
    );
}
