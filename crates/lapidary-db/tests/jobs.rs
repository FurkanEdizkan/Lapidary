use lapidary_core::{LibraryId, Outcome};
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

#[sqlx::test(migrations = "./migrations")]
async fn completing_a_job_records_how_it_finished(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    jobs.complete(job.id, Outcome::Skipped)
        .await
        .expect("completes");

    let (state, outcome): (String, Option<String>) =
        sqlx::query_as("SELECT state, outcome FROM job WHERE id = $1")
            .bind(job.id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads back");
    assert_eq!(state, "done");
    assert_eq!(outcome.as_deref(), Some("skipped"));
}

#[sqlx::test(migrations = "./migrations")]
async fn rescheduling_pushes_the_job_into_the_future_and_keeps_the_reason(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    jobs.reschedule(
        job.id,
        "the database was unreachable",
        Duration::from_secs(8),
    )
    .await
    .expect("reschedules");

    let (state, in_future, reason): (String, bool, Option<String>) =
        sqlx::query_as("SELECT state, run_after > now(), last_error FROM job WHERE id = $1")
            .bind(job.id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads back");

    assert_eq!(
        state, "pending",
        "a rescheduled job is queued again, not failed"
    );
    assert!(in_future, "backoff must actually delay the next attempt");
    assert_eq!(reason.as_deref(), Some("the database was unreachable"));
}

#[sqlx::test(migrations = "./migrations")]
async fn releasing_a_workers_leases_makes_its_jobs_immediately_available(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    jobs.dequeue("worker-shutting-down", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    let released = jobs
        .release_leases("worker-shutting-down")
        .await
        .expect("releases");
    assert_eq!(released, 1);

    // A planned restart resumes instantly instead of waiting out a 60-second lease.
    let picked_up = jobs
        .dequeue("worker-restarted", LEASE)
        .await
        .expect("dequeues");
    assert!(
        picked_up.is_some(),
        "a released job must be available at once"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn batch_status_counts_only_its_own_batch(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (first, _) = jobs
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let (second, _) = jobs
        .enqueue_scan(
            seeded(),
            &[
                "spacer-lp-2001-00.stl".to_owned(),
                "vee-block-lp-3072-02.stl".to_owned(),
            ],
        )
        .await
        .expect("enqueues");

    let a = jobs
        .batch_status(seeded(), first)
        .await
        .expect("reads")
        .expect("exists");
    let b = jobs
        .batch_status(seeded(), second)
        .await
        .expect("reads")
        .expect("exists");

    assert_eq!(a.total, 1, "the first batch must not see the second's jobs");
    assert_eq!(b.total, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_batch_is_unfinished_while_any_job_is_pending(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue_scan(
            seeded(),
            &[
                "bracket-lp-1042-03.stl".to_owned(),
                "spacer-lp-2001-00.stl".to_owned(),
            ],
        )
        .await
        .expect("enqueues");

    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.complete(job.id, Outcome::Ingested)
        .await
        .expect("completes");

    let mid = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");
    assert_eq!(mid.ingested, 1);
    assert_eq!(mid.pending, 1);
    assert!(
        mid.finished_at.is_none(),
        "one job still pending is not finished"
    );

    let last = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.fail(
        last.id,
        "Could not read this STL - the file ends mid-facet.",
    )
    .await
    .expect("fails");

    let done = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");
    assert!(
        done.finished_at.is_some(),
        "nothing left to run means finished"
    );
    assert_eq!(done.failed_total, 1);
    assert_eq!(done.failed.len(), 1);
    assert_eq!(done.failed[0].path, "spacer-lp-2001-00.stl");
}

/// `FAILED_SAMPLE` caps the failed-sample query at 100 rows, but no assertion anywhere
/// else in this file reaches that many failures, so nothing would notice the cap being
/// widened, dropped, or `failed_total` starting to track the sample instead of the real
/// count. 101 real jobs is a real batch, not an artificial fixture, so this bulk-updates
/// them straight to `failed` rather than driving 101 individual `dequeue`/`fail` round
/// trips through the queue's leasing machinery, which this test has no interest in.
#[sqlx::test(migrations = "./migrations")]
async fn the_failed_sample_is_capped_at_one_hundred_but_the_total_is_not(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let paths: Vec<String> = (0..101)
        .map(|n| format!("bracket-lp-{n:04}-00.stl"))
        .collect();
    let (batch, queued) = jobs.enqueue_scan(seeded(), &paths).await.expect("enqueues");
    assert_eq!(queued, 101);

    sqlx::query(
        "UPDATE job SET state = 'failed', \
                        last_error = 'Could not read this STL - the file ends mid-facet.' \
         WHERE batch_id = $1",
    )
    .bind(batch.as_uuid())
    .execute(&pool)
    .await
    .expect("fails all 101 at once");

    let status = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");
    assert_eq!(
        status.failed_total, 101,
        "the real count is never truncated"
    );
    assert_eq!(status.failed.len(), 100, "the sample itself is capped");
}

/// `batch_status`'s failed-sample query orders by `created_at`, but every job in a batch
/// enters through `enqueue_scan`'s single `INSERT ... SELECT`, and Postgres's `now()` is
/// constant for the whole transaction -- so in the system as it actually runs, every job
/// in one batch shares the *same* `created_at`, and "ordered by creation" ties for all of
/// them. This test cannot exercise that real path (a tie has no defined winner to assert
/// on), so it does what `a_job_whose_lease_expired_is_reclaimed...` above does: reach past
/// the public API with a direct UPDATE to force the rows into a distinct, known order,
/// which is the only way to pin the `ORDER BY created_at` clause itself rather than
/// Postgres's arbitrary tie-breaking. It intentionally fails the jobs in the *opposite*
/// order from their forced `created_at`, so a mutation that sorted by fail order (or
/// dropped the ORDER BY) would still be caught.
#[sqlx::test(migrations = "./migrations")]
async fn failed_jobs_are_ordered_by_creation_not_by_which_failed_first(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue_scan(
            seeded(),
            &[
                "bracket-lp-1042-03.stl".to_owned(),
                "spacer-lp-2001-00.stl".to_owned(),
                "vee-block-lp-3072-02.stl".to_owned(),
            ],
        )
        .await
        .expect("enqueues");

    // Force a known, distinct creation order -- bracket oldest, vee-block newest --
    // independent of the tied `created_at` `enqueue_scan` actually gave them.
    for (path, offset_secs) in [
        ("bracket-lp-1042-03.stl", 2i64),
        ("spacer-lp-2001-00.stl", 1),
        ("vee-block-lp-3072-02.stl", 0),
    ] {
        sqlx::query(
            "UPDATE job SET created_at = now() - make_interval(secs => $2) \
             WHERE batch_id = $1 AND payload->>'path' = $3",
        )
        .bind(batch.as_uuid())
        .bind(offset_secs as f64)
        .bind(path)
        .execute(&pool)
        .await
        .expect("backdates created_at");
    }

    // Fail them in the reverse of that order: vee-block (newest) first, bracket
    // (oldest) last. `dequeue`'s own ordering can't be trusted to pick a specific one
    // of three still-tied `run_after` rows, so each job's id is looked up by path and
    // failed directly -- `fail` has no state precondition to satisfy first. If the
    // status query sorted by this fail order instead of `created_at`, the assertion
    // below would see it reversed.
    for path in [
        "vee-block-lp-3072-02.stl",
        "spacer-lp-2001-00.stl",
        "bracket-lp-1042-03.stl",
    ] {
        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM job WHERE batch_id = $1 AND payload->>'path' = $2")
                .bind(batch.as_uuid())
                .bind(path)
                .fetch_one(&pool)
                .await
                .expect("finds the job by path");
        jobs.fail(
            lapidary_core::JobId::from_uuid(id),
            "Could not read this STL - the file ends mid-facet.",
        )
        .await
        .expect("fails");
    }

    let status = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");
    let paths: Vec<&str> = status.failed.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "bracket-lp-1042-03.stl",
            "spacer-lp-2001-00.stl",
            "vee-block-lp-3072-02.stl",
        ],
        "the failed sample must be oldest-created first, not fail-completion order"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_batch_id_from_another_library_does_not_resolve(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let elsewhere = LibraryId::new();
    let found = jobs.batch_status(elsewhere, batch).await.expect("reads");
    assert!(
        found.is_none(),
        "a batch id must not be a capability -- content addressing is not authorization"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_batch_with_no_jobs_has_no_status(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, queued) = jobs.enqueue_scan(seeded(), &[]).await.expect("enqueues");
    assert_eq!(queued, 0);
    assert!(
        jobs.batch_status(seeded(), batch)
            .await
            .expect("reads")
            .is_none(),
        "an empty batch is indistinguishable from an id never issued, and both 404"
    );
}
