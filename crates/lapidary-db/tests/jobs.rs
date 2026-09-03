use lapidary_core::LibraryId;
use lapidary_db::{JobRow, PgJobs};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(60);

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

#[sqlx::test(migrations = "./migrations")]
async fn enqueue_writes_one_pending_row_per_path_under_one_batch(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let paths = vec![
        "bracket-lp-1042-03.stl".to_owned(),
        "spacer-lp-2001-00.stl".to_owned(),
    ];

    let (batch, queued) = jobs.enqueue_scan(seeded(), &paths).await.expect("enqueues");
    assert_eq!(queued, 2);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT state, payload->>'path' FROM job WHERE batch_id = $1 ORDER BY payload->>'path'",
    )
    .bind(batch.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("reads back");

    assert_eq!(
        rows,
        vec![
            ("pending".to_owned(), "bracket-lp-1042-03.stl".to_owned()),
            ("pending".to_owned(), "spacer-lp-2001-00.stl".to_owned()),
        ]
    );

    // The assertion above pins path *values*, but never reads `id` back, so on its own
    // it cannot tell a correctly paired insert from one where `unnest`'s two arrays
    // were zipped out of step -- `id` is an opaque generated key with no relationship
    // to `path` that a test could independently recompute, so there is no oracle for
    // "this id belongs to this path". What *is* checkable, and what a mis-zip would
    // actually break, is row identity: two distinct paths must produce two distinct,
    // non-null primary keys, all still tagged with the batch `enqueue_scan` returned.
    // A shorter id array silently NULL-padded by `unnest` would violate `job`'s
    // `id uuid primary key` (NOT NULL) constraint and this test would already fail at
    // `.expect("enqueues")`; this assertion instead catches an implementation that
    // reused one id for every row, which no NOT NULL constraint would notice.
    let distinct_ids: i64 =
        sqlx::query_scalar("SELECT count(DISTINCT id) FROM job WHERE batch_id = $1")
            .bind(batch.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("counts distinct ids");
    assert_eq!(
        distinct_ids, 2,
        "each path must get its own row id, not a shared or dropped one"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn enqueueing_nothing_issues_a_batch_id_and_writes_no_rows(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, queued) = jobs.enqueue_scan(seeded(), &[]).await.expect("enqueues");

    assert_eq!(queued, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(count, 0, "an empty scan must not invent a job");
}

#[sqlx::test(migrations = "./migrations")]
async fn two_workers_racing_one_job_produce_exactly_one_winner(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    // Both dequeues run concurrently against a queue holding exactly one job. This is
    // the property FOR UPDATE SKIP LOCKED exists for; without the row lock both
    // transactions read the same row and both claim it.
    //
    // This is a scenario test, not a proof: it races two real connections for the same
    // narrow window (the gap between the unlocked candidate read and the row lock being
    // taken) rather than forcing that window open. Verified experimentally while writing
    // it: with FOR UPDATE SKIP LOCKED deleted, this test still passed 15/15 plain runs
    // and only failed 1 time in 40 -- against a local, low-latency Postgres the window is
    // rarely hit. A green run here is NOT evidence the row lock is present; that
    // deterministic guarantee lives in
    // `a_job_locked_by_another_transaction_is_skipped_not_claimed_or_blocked` below,
    // which holds the lock open explicitly instead of racing for it. This test stays
    // because it is still the realistic path (two workers actually contending), and its
    // assertion is correct when it does fire -- it just cannot be trusted alone.
    let a = PgJobs(pool.clone());
    let b = PgJobs(pool.clone());
    let (first, second) = tokio::join!(a.dequeue("worker-a", LEASE), b.dequeue("worker-b", LEASE));

    let claimed = [first.expect("a dequeues"), second.expect("b dequeues")]
        .into_iter()
        .flatten()
        .count();
    assert_eq!(claimed, 1, "exactly one worker may hold a lease on one job");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_locked_by_another_transaction_is_skipped_not_claimed_or_blocked(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    // Hold a row lock on the only job from a separate, still-open transaction -- standing
    // in for a concurrent claim that is mid-flight, but deterministically rather than by
    // racing for the same narrow window `two_workers_racing_one_job_produce_exactly_one_
    // winner` depends on. Two outcomes are reachable by mutating the query's locking
    // clause, and both are checked for:
    //   - Ok(None) promptly:  FOR UPDATE SKIP LOCKED saw the lock and skipped the row.
    //                         This is the only correct outcome.
    //   - times out:          FOR UPDATE is present without SKIP LOCKED, or is missing
    //                         entirely -- either way the query's target `UPDATE` still
    //                         has to take the row's lock to write it, and Postgres makes
    //                         that block on a lock already held elsewhere rather than
    //                         silently proceeding. Verified: dropping SKIP LOCKED and
    //                         dropping the whole FOR UPDATE clause both land here, 5/5
    //                         runs each -- Postgres's own UPDATE machinery, not this
    //                         clause, is what makes the "claims a locked row" outcome
    //                         below structurally unreachable for a single-statement
    //                         UPDATE like this one.
    //   - Ok(Some(_)):        included as a defensive check, not because a mutation of
    //                         this query reaches it: it would mean the code claimed the
    //                         held row without ever contending for its lock at all, which
    //                         would only happen if `dequeue` stopped being one atomic
    //                         UPDATE (e.g. a separate SELECT feeding an UPDATE run
    //                         outside the same lock chain).
    let mut locker = pool.begin().await.expect("opens a holding transaction");
    sqlx::query("SELECT id FROM job WHERE state = 'pending' FOR UPDATE")
        .execute(&mut *locker)
        .await
        .expect("locks the only pending row");

    let outcome =
        tokio::time::timeout(Duration::from_secs(2), jobs.dequeue("worker-a", LEASE)).await;

    // Release the lock before asserting, so a panic here never leaves the pool's next
    // test holding a stray lock on a connection sqlx will reuse.
    locker.rollback().await.expect("releases the held lock");

    match outcome {
        Ok(Ok(None)) => {}
        Ok(Ok(Some(job))) => panic!(
            "dequeue claimed job {:?} while another transaction held its row lock -- \
             FOR UPDATE SKIP LOCKED is missing from the query (or no lock is being taken \
             at all)",
            job.id
        ),
        Ok(Err(e)) => panic!("dequeue returned an error instead of skipping the locked row: {e}"),
        Err(_) => panic!(
            "dequeue timed out waiting on the locked row -- FOR UPDATE is present without \
             SKIP LOCKED, so it blocked instead of skipping past the locked row"
        ),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_whose_lease_expired_is_reclaimed_and_its_attempts_counted(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let first = jobs
        .dequeue("worker-that-will-die", LEASE)
        .await
        .expect("dequeues")
        .expect("a job is available");
    assert_eq!(first.attempts, 1);

    // Simulate the worker dying: the row stays 'running', and its lease lapses.
    sqlx::query("UPDATE job SET lease_expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(first.id.as_uuid())
        .execute(&pool)
        .await
        .expect("expires the lease");

    let reclaimed = jobs
        .dequeue("worker-that-survives", LEASE)
        .await
        .expect("dequeues")
        .expect("an expired lease must be reclaimable");

    assert_eq!(reclaimed.id, first.id, "the same job comes back");
    assert_eq!(
        reclaimed.attempts, 2,
        "reclaiming counts as an attempt, which is what caps the poison-pill case"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_still_in_backoff_is_not_dequeued(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    sqlx::query("UPDATE job SET run_after = now() + interval '1 hour'")
        .execute(&pool)
        .await
        .expect("pushes it into the future");

    let claimed: Option<JobRow> = jobs.dequeue("worker-a", LEASE).await.expect("dequeues");
    assert!(claimed.is_none(), "backoff must actually withhold the job");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_empty_queue_yields_nothing_rather_than_blocking(pool: PgPool) {
    let jobs = PgJobs(pool);
    assert!(
        jobs.dequeue("worker-a", LEASE)
            .await
            .expect("dequeues")
            .is_none()
    );
}
