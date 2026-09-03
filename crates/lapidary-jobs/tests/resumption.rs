//! The slice's central claim, tested as stated: killing the worker mid-scan loses nothing
//! but the files actually in flight.
//!
//! This is deliberately not inferred from unit tests of the parts. `tests/worker.rs`
//! proves the loop delivers and shuts down; `crates/lapidary-db/tests/jobs.rs` proves the
//! dequeue reclaims an expired lease. Neither says what a person actually gets after a
//! worker dies, which is the sentence the slice is for, and which is about rows in `part`
//! rather than rows in `job`.

use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{JobRow, PgJobs};
use lapidary_ingest::IngestHandler;
use lapidary_jobs::{HandlerError, JobHandler, WorkerConfig, run};
use sqlx::PgPool;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// The six real generated fixtures from `example/parts/` — the same set the live stack
/// ingests, and the same six the slice's exit criterion names.
///
/// Do **not** invent filenames here. `flange-lp-4400-02.stl` in particular is
/// `lapidary-cad`'s mock fixture *key* and is deliberately fictional; slice 1 renamed it
/// precisely because it once collided with a real fixture and gave two contradictory
/// answers for one filename.
const FIXTURES: [&str; 6] = [
    "flange-dn40-lp-3310-02.stl",
    "hex-spacer-m4x20-lp-2145-01.stl",
    "idler-pulley-lp-4820-00.stl",
    "mounting-plate-lp-1180-01.stl",
    "spur-gear-m2-20t-lp-5140-00.stl",
    "vee-block-lp-3072-02.stl",
];

/// How many files the first worker finishes *and reports* before it dies.
const REPORTED_BEFORE_DEATH: usize = 2;

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

fn handler_over(pool: &PgPool, ingest_dir: &Path, blob_root: &Path) -> IngestHandler {
    IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
    }
}

fn worker_config(id: &str) -> WorkerConfig {
    WorkerConfig {
        worker_id: id.to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(50),
        // One at a time, so "the file it was holding" is exactly one file and the row
        // counts below are not a race.
        concurrency: 1,
        listen: false,
    }
}

/// The real ingest handler, wrapped so the worker holding it can be killed at the worst
/// possible moment.
///
/// The first `REPORTED_BEFORE_DEATH` jobs run and report normally. The next one runs to
/// completion — its `part` row is committed — and then never returns, so the worker is
/// aborted holding a job whose work is *done* and whose outcome was never recorded.
///
/// Stalling after the write rather than before it is the point. Dying mid-parse would
/// leave nothing behind and prove much less; dying here means the reclaiming worker finds
/// the part already there, which is the case that decides whether a redo is a duplicate
/// or a skip.
struct DiesHolding {
    inner: IngestHandler,
    finished: AtomicUsize,
    /// Signalled once the doomed job has done its work and is about to be abandoned.
    stalled: Notify,
}

impl JobHandler for DiesHolding {
    async fn handle(&self, job: &JobRow) -> Result<Outcome, HandlerError> {
        let result = self.inner.handle(job).await;
        if self.finished.fetch_add(1, Ordering::SeqCst) >= REPORTED_BEFORE_DEATH {
            self.stalled.notify_one();
            std::future::pending::<()>().await;
        }
        result
    }
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count query")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_worker_dying_mid_scan_loses_only_what_it_held(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    for name in FIXTURES {
        std::fs::copy(
            format!("{}/../../example/parts/{name}", env!("CARGO_MANIFEST_DIR")),
            ingest_dir.path().join(name),
        )
        .unwrap_or_else(|error| panic!("stages {name}: {error}"));
    }

    let paths: Vec<String> = FIXTURES.iter().map(|name| (*name).to_owned()).collect();
    let (_, queued) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &paths)
        .await
        .expect("enqueues");
    assert_eq!(queued, 6);

    // --- the worker that dies ------------------------------------------------------
    let doomed = Arc::new(DiesHolding {
        inner: handler_over(&pool, ingest_dir.path(), blob_root.path()),
        finished: AtomicUsize::new(0),
        stalled: Notify::new(),
    });
    let first = tokio::spawn(run(
        PgJobs(pool.clone()),
        doomed.clone(),
        worker_config("doomed-worker"),
        // Never cancelled. This worker does not get a graceful shutdown.
        CancellationToken::new(),
    ));

    tokio::time::timeout(Duration::from_secs(60), doomed.stalled.notified())
        .await
        .expect("the third job reaches the point of no return rather than the test hanging");

    // `kill -9`, not a restart: no cancellation, so no drain of in-flight work and no
    // release_leases. `abort` drops the task at its await point with the job still leased.
    first.abort();

    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE state = 'done'").await,
        REPORTED_BEFORE_DEATH as i64,
        "two files were finished and reported before the worker died"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE state = 'running'").await,
        1,
        "and one is still leased to a worker that no longer exists"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM part").await,
        3,
        "including the part whose ingest committed but whose outcome was never recorded"
    );

    // Expire the dead worker's lease the way time would. Nothing else about the row is
    // touched: it stays `running`, leased to a worker that will never call back.
    sqlx::query(
        "UPDATE job SET lease_expires_at = now() - interval '1 second' WHERE state = 'running'",
    )
    .execute(&pool)
    .await
    .expect("expires the dead worker's lease");

    // --- the worker that picks up after it -----------------------------------------
    let relief = Arc::new(handler_over(&pool, ingest_dir.path(), blob_root.path()));
    let shutdown = CancellationToken::new();
    let second = tokio::spawn(run(
        PgJobs(pool.clone()),
        relief,
        worker_config("relief-worker"),
        shutdown.clone(),
    ));

    // Bounded, so a queue that never drains fails the test instead of hanging it.
    for _ in 0..200 {
        if count(
            &pool,
            "SELECT count(*) FROM job WHERE state IN ('pending', 'running')",
        )
        .await
            == 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    shutdown.cancel();
    second
        .await
        .expect("the relief worker joins")
        .expect("the relief worker exits cleanly");

    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE state = 'done'").await,
        6,
        "every file lands, including the one the dead worker was holding"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM part").await,
        6,
        "and none is ingested twice"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE state = 'failed'").await,
        0,
        "a worker dying is not a file's fault, and must not be reported as one"
    );

    // How the reclaimed job avoided a duplicate, stated rather than assumed. The relief
    // worker re-ran the whole handler on that file, and `library_holds(library, name,
    // hash)` answered before the insert -- so the redo is a `skipped`, and
    // `part_name_unique_per_library` is never reached on this path at all.
    //
    // Worth pinning because it is the reason a mutation the plan expected to bite does
    // not: dropping that constraint leaves this test green. The constraint guards the
    // CONCURRENT race -- two live workers on one file -- which is
    // `losing_the_race_for_a_file_is_a_skip_rather_than_a_failure` in
    // crates/lapidary-ingest/tests/handler.rs, not this sequential reclaim.
    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE outcome = 'skipped'").await,
        1,
        "exactly the reclaimed file, short-circuited by library_holds on the redo"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM job WHERE outcome = 'ingested'").await,
        5,
        "and the other five were genuinely new to this library"
    );
}
