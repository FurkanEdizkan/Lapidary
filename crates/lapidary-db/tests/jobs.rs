use lapidary_core::{BatchId, DerivativeKind, JobId, JobPayload, LibraryId, Outcome, RevisionId};
use lapidary_db::{JobRow, PgJobs};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const LEASE: Duration = Duration::from_secs(60);

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

/// Inserts one `job` row directly with an arbitrary `kind`/`state`/`leased_by` --
/// shapes `enqueue`'s public API cannot construct (there is no `JobPayload` for
/// "a `migrate_storage` row that is already `pending`", and `enqueue` never takes a
/// `leased_by`). `payload` is always `{}`, which every kind seeded this way accepts.
async fn insert_job(
    pool: &PgPool,
    library: LibraryId,
    kind: &str,
    state: &str,
    leased_by: Option<&str>,
) -> JobId {
    let id = JobId::new();
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, leased_by, \
                          lease_expires_at) \
         VALUES ($1, $2, $3, $4, '{}'::jsonb, $5, $6, \
                 CASE WHEN $6::text IS NOT NULL THEN now() + interval '1 hour' END)",
    )
    .bind(id.as_uuid())
    .bind(BatchId::new().as_uuid())
    .bind(library.as_uuid())
    .bind(kind)
    .bind(state)
    .bind(leased_by)
    .execute(pool)
    .await
    .expect("inserts a raw job row for the fixture");
    id
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

/// Direct coverage for `fail` -- the brief that supplied `complete`, `reschedule` and
/// `release_leases`'s tests never exercised it, so until now it shipped verified by
/// nothing but the compiler.
#[sqlx::test(migrations = "./migrations")]
async fn failing_a_job_records_the_reason_and_clears_the_lease(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    jobs.fail(
        job.id,
        "Could not read this STL - it declares 24 facets but the file ends after 11. \
         Re-export from your CAD tool and retry.",
    )
    .await
    .expect("fails");

    let (state, last_error, leased_by_cleared, lease_expiry_cleared): (
        String,
        Option<String>,
        bool,
        bool,
    ) = sqlx::query_as(
        "SELECT state, last_error, leased_by IS NULL, lease_expires_at IS NULL \
         FROM job WHERE id = $1",
    )
    .bind(job.id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("reads back");

    assert_eq!(state, "failed");
    assert_eq!(
        last_error.as_deref(),
        Some(
            "Could not read this STL - it declares 24 facets but the file ends after 11. \
             Re-export from your CAD tool and retry."
        )
    );
    assert!(
        leased_by_cleared,
        "a terminal row must not still claim a worker"
    );
    assert!(
        lease_expiry_cleared,
        "a terminal row must not still carry a lease"
    );
}

/// spec §3.2's own acknowledged scenario: worker A's lease lapses (it stalled, but is
/// still alive), worker B reclaims and ingests the file successfully, and only then
/// does A's stale attempt finish and call `fail`. Without `AND state = 'running'` on
/// `fail`'s `WHERE` clause, A's write would clobber B's -- a part that landed in the
/// grid would be reported failed, which is exactly the "measurement must not lie"
/// non-negotiable from CLAUDE.md.
#[sqlx::test(migrations = "./migrations")]
async fn a_stale_workers_fail_does_not_overwrite_a_result_another_worker_already_recorded(
    pool: PgPool,
) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    // Stand in for worker B reclaiming after A's lease lapsed and finishing the job:
    // the row is `done` before A's own call ever lands.
    jobs.complete(job.id, Outcome::Ingested)
        .await
        .expect("completes");

    // A's stale attempt reports failure on the same id, after the fact.
    jobs.fail(job.id, "the database was unreachable")
        .await
        .expect("fail is not an error even when it changes nothing");

    let (state, outcome, last_error): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT state, outcome, last_error FROM job WHERE id = $1")
            .bind(job.id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads back");

    assert_eq!(state, "done", "the reclaiming worker's result must stand");
    assert_eq!(
        outcome.as_deref(),
        Some("ingested"),
        "a part that landed must not be reported failed"
    );
    assert!(
        last_error.is_none(),
        "the stale fail must not have written anything at all"
    );
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

/// Fix rounds 1 and 2 added, and fix round 3 (`0012_drop_migrate_storage_pending_index.sql`)
/// removed, a pair of exclusions that kept this bulk `UPDATE` from colliding with
/// `job_migrate_storage_pending_per_library`: one for a `migrate_storage` row whose
/// library already had a pending successor queued, one for a second `running`
/// `migrate_storage` row for the same library held by this same worker. Both guards are
/// gone because the index they served is gone -- duplicate `migrate_storage` rows are
/// tolerated now, not specially excluded, so this worker's whole holding, migrate rows
/// included, releases together in one statement like every other kind always did. What
/// survives from those two tests is the assertion that was never about the index at
/// all: an unrelated `ingest_file` row held by the same worker releases normally
/// alongside whatever `migrate_storage` rows are also on the worker.
#[sqlx::test(migrations = "./migrations")]
async fn releasing_a_workers_leases_moves_every_kind_it_holds_migrate_storage_included(
    pool: PgPool,
) {
    let jobs = PgJobs(pool.clone());
    let worker = "worker-shutting-down";
    // Two running `migrate_storage` rows for the same library, plus an unrelated
    // `ingest_file` row -- the exact fixture fix round 2 needed the guard for. Without
    // any guard left, every row this worker holds must move to `pending` together.
    let migrate_a = insert_job(&pool, seeded(), "migrate_storage", "running", Some(worker)).await;
    let migrate_b = insert_job(&pool, seeded(), "migrate_storage", "running", Some(worker)).await;
    let ingest = insert_job(&pool, seeded(), "ingest_file", "running", Some(worker)).await;

    let released = jobs
        .release_leases(worker)
        .await
        .expect("must not error just because a library holds more than one migrate row");
    assert_eq!(
        released, 3,
        "every row this worker holds moves, migrate_storage included -- nothing is \
         excluded now that nothing needs excluding"
    );

    let states: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, state FROM job WHERE id IN ($1, $2, $3) ORDER BY id")
            .bind(migrate_a.as_uuid())
            .bind(migrate_b.as_uuid())
            .bind(ingest.as_uuid())
            .fetch_all(&pool)
            .await
            .expect("reads back");
    assert!(
        states.iter().all(|(_, state)| state == "pending"),
        "both migrate rows and the unrelated ingest row must all be pending: {states:?}"
    );
}

// Fix round 3 deletes both concurrent-enqueue tests that used to live here:
// `two_workers_booting_concurrently_against_one_library_enqueue_exactly_one_migration`
// (a bare `tokio::join!`, admitting in its own doc comment that the two statements
// "rarely collide physically") and
// `two_workers_forced_to_start_together_still_enqueue_exactly_one_migration` (an
// `ACCESS EXCLUSIVE` table lock forcing the race open deterministically, run ten times).
// Both existed to prove `winners == 1` / `total == 1` under real concurrency, backed by
// `job_migrate_storage_pending_per_library` (migration 0011) and the `ON CONFLICT ...
// DO NOTHING` it gave `enqueue_migration_if_absent` something to target. Neither claim
// survives `0012_drop_migrate_storage_pending_index.sql`: two concurrent callers can now
// each see nothing under `WHERE NOT EXISTS` and each insert, so a library can briefly
// hold two pending `migrate_storage` rows -- tolerable, per `enqueue_migration_if_absent`'s
// doc comment, because the execution boundary in `lapidary-ingest` is what makes a
// redundant row harmless, not this check. Forcing the race with the table lock would now
// fail deterministically, not probabilistically, so re-scoping instead of deleting would
// have produced a test proving nothing across normal, non-forced usage.
//
// What both tests could still prove without concurrency -- that a single caller's
// `enqueue_migration_if_absent` is idempotent, and a second sequential call while one is
// already pending or running returns `None` rather than a duplicate -- is exactly what
// `a_library_with_a_pending_migration_gets_no_second_one` and
// `a_library_with_a_running_migration_gets_no_second_one` below already assert, so
// nothing here needed a renamed replacement.

#[sqlx::test(migrations = "./migrations")]
async fn a_library_with_a_pending_migration_gets_no_second_one(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_migration_if_absent(seeded())
        .await
        .expect("enqueues")
        .expect("the first call wins");

    let second = jobs
        .enqueue_migration_if_absent(seeded())
        .await
        .expect("must not error just because one is already queued");
    assert_eq!(second, None, "a pending migration already exists");

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job WHERE kind = 'migrate_storage' AND library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(total, 1, "the second call must not have queued a duplicate");
}

/// `WHERE NOT EXISTS` in `enqueue_migration_if_absent` checks `state IN ('pending',
/// 'running')`, not `'pending'` alone -- this is what proves the `'running'` half is
/// load-bearing, not a vestige.
#[sqlx::test(migrations = "./migrations")]
async fn a_library_with_a_running_migration_gets_no_second_one(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_migration_if_absent(seeded())
        .await
        .expect("enqueues")
        .expect("the first call wins");
    jobs.dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("the job is available to claim");

    let second = jobs
        .enqueue_migration_if_absent(seeded())
        .await
        .expect("must not error just because one is already running");
    assert_eq!(second, None, "a running migration already exists");

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job WHERE kind = 'migrate_storage' AND library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(total, 1, "the second call must not have queued a duplicate");
}

/// Fix round 1: `active_migration_batch` is what lets the manual trigger route hand back
/// a real, pollable batch id on `queued: 0` instead of a fabricated one that can only
/// ever 404. It must find the SAME batch whichever state the chain's current row is in
/// -- pending or running -- since a chain's every row shares one `batch_id`.
#[sqlx::test(migrations = "./migrations")]
async fn active_migration_batch_finds_the_running_and_the_pending_case_alike(pool: PgPool) {
    let jobs = PgJobs(pool.clone());

    assert_eq!(
        jobs.active_migration_batch(seeded())
            .await
            .expect("does not error"),
        None,
        "no chain exists yet"
    );

    let batch = jobs
        .enqueue_migration_if_absent(seeded())
        .await
        .expect("enqueues")
        .expect("the first call wins");
    assert_eq!(
        jobs.active_migration_batch(seeded())
            .await
            .expect("does not error"),
        Some(batch),
        "a pending chain is found"
    );

    jobs.dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("the job is available to claim");
    assert_eq!(
        jobs.active_migration_batch(seeded())
            .await
            .expect("does not error"),
        Some(batch),
        "the same chain, now running, is still found under the same batch id"
    );
}

/// `migrate_storage`'s own re-enqueue arm. Its caller is itself the currently RUNNING
/// job for this library, so `reenqueue_migration_if_absent` checks `state = 'pending'`
/// only -- never `'running'`, or it would see its own caller and refuse to chain at
/// all. `queued_elsewhere` below being `true` is what pins that: the currently running
/// job's own row must not count as a pending successor, or every migration would
/// silently stop after its first run while every other test in this file kept passing.
/// The rest of the test is the race this method actually exists for: some OTHER path (a
/// shutdown-grace release, or a second worker reclaiming an expired lease) already
/// queued the successor while the current run was still going, and the handler's own
/// re-enqueue call must find that and back off silently -- a no-op, not a duplicate row.
#[sqlx::test(migrations = "./migrations")]
async fn reenqueuing_when_a_pending_successor_already_exists_is_a_no_op_not_an_error(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let batch = jobs
        .enqueue_migration_if_absent(seeded())
        .await
        .expect("enqueues")
        .expect("the first call wins");
    jobs.dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("the job is available to claim");

    let queued_elsewhere = jobs
        .reenqueue_migration_if_absent(batch, seeded())
        .await
        .expect("nothing pending yet, so this queues one");
    assert!(
        queued_elsewhere,
        "the running job must not see itself and refuse to chain"
    );

    // The handler's OWN re-enqueue call, arriving after that -- must be a silent no-op,
    // not the raw unique-violation error a plain, unguarded INSERT would throw.
    let queued_again = jobs
        .reenqueue_migration_if_absent(batch, seeded())
        .await
        .expect("must not error even though a pending successor already exists");
    assert!(!queued_again, "a pending successor already exists");

    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job WHERE kind = 'migrate_storage' AND library_id = $1 \
          AND state = 'pending'",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(pending, 1, "exactly one pending successor, not two");
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

    // The batch has zero pending jobs and one genuinely `running` job right now --
    // the case the `finished_at` guard's `state IN ('pending','running')` filter
    // exists for, and which nothing before this queried: the rest of this test only
    // ever asks while a job is `pending` or after everything has reached a terminal
    // state, so a mutation dropping `'running'` from that list would have passed
    // every other assertion here. Task 13's grid stops polling once `finished_at` is
    // set, so reporting "finished" while a job is still in flight would freeze a
    // scan on screen with no error anywhere.
    let running = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");
    assert_eq!(running.pending, 0);
    assert_eq!(running.running, 1, "the second job is leased and in flight");
    assert!(
        running.finished_at.is_none(),
        "a job still running is not finished, even with nothing left pending"
    );

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

    // `fail` now requires `state = 'running'` (the stale-writer guard -- see
    // `complete`'s doc comment), so every row needs to pass through that state before
    // it can be failed. Set all three at once directly rather than through `dequeue`,
    // whose own ordering among three still-tied `run_after` rows would be exactly as
    // arbitrary as the thing this test is trying to pin.
    sqlx::query("UPDATE job SET state = 'running' WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .execute(&pool)
        .await
        .expect("marks all three running");

    // Fail them in the reverse of that order: vee-block (newest) first, bracket
    // (oldest) last. Each job's id is looked up by path and failed directly. If the
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
            JobId::from_uuid(id),
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

/// The test above pins the `ORDER BY created_at` clause itself, but by forcing
/// distinct `created_at` values it exercises a scenario production never produces:
/// every job in a real batch enters through one `enqueue_scan` statement, and
/// Postgres's `now()` is constant for the whole transaction, so real same-batch
/// failures always tie on `created_at`. This test is the real situation -- it never
/// touches `created_at` at all -- and checks that the `, id` tiebreaker alone still
/// produces a deterministic, enqueue-matching order, which is what makes spec §7's
/// "stable across polls, not reshuffling" promise actually true rather than true only
/// when an operator happens to backdate rows.
#[sqlx::test(migrations = "./migrations")]
async fn failed_jobs_sharing_one_created_at_still_come_back_in_enqueue_order(pool: PgPool) {
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

    // Confirm the premise before trusting the assertion below: this batch really
    // does tie on created_at, so any ordering observed here can only be coming from
    // the id tiebreaker, not from created_at doing any work.
    let distinct_created_at: i64 =
        sqlx::query_scalar("SELECT count(DISTINCT created_at) FROM job WHERE batch_id = $1")
            .bind(batch.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("counts distinct created_at values");
    assert_eq!(
        distinct_created_at, 1,
        "this test is only meaningful if enqueue_scan really does tie every row's \
         created_at within one batch -- if enqueue_scan changes to insert rows across \
         several statements, this premise (and the reason this test exists) no longer \
         holds"
    );

    // `fail` requires `state = 'running'`; mark all three at once rather than
    // through `dequeue`, whose ordering among tied `run_after` rows is exactly the
    // same kind of arbitrary this test exists to rule out for the *read* path.
    sqlx::query("UPDATE job SET state = 'running' WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .execute(&pool)
        .await
        .expect("marks all three running");

    // Fail in the reverse of enqueue order -- if the sample came back in fail order
    // instead of enqueue order, this would still (accidentally) look sorted, so the
    // assertion below checks the specific enqueue order, not just "some order".
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
            JobId::from_uuid(id),
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
        "same-created_at failures must still come back in enqueue order, via the id \
         tiebreaker"
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

/// Before this slice, `batch_status`'s failures query selected `payload->>'path'` into a
/// non-`Option<String>`. A `derive` payload has no `path` key, so this call used to fail
/// with a decode error -- a 500 for the whole batch -- rather than returning a status.
/// The fix falls back to the failed job's part name, so this also pins that a person
/// reading a failed derive job's status sees something they recognise, not an empty
/// string.
#[sqlx::test(migrations = "./migrations")]
async fn a_failed_derive_job_reports_the_parts_name_not_a_missing_path(pool: PgPool) {
    let jobs = PgJobs(pool.clone());

    let part_id = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(part_id)
        .bind(seeded().as_uuid())
        .bind("spacer-lp-2001-00")
        .bind("spacer-lp-2001-00.stl")
        .execute(&pool)
        .await
        .expect("inserts the part");

    let revision = RevisionId::new();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision.as_uuid())
    .bind(part_id)
    .execute(&pool)
    .await
    .expect("inserts the revision");

    let (batch, queued) = jobs
        .enqueue(
            seeded(),
            &[JobPayload::Derive {
                revision,
                produce: DerivativeKind::TessellationL2,
            }],
        )
        .await
        .expect("enqueues");
    assert_eq!(queued, 1);

    let job = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    assert_eq!(job.kind, "derive");

    jobs.fail(
        job.id,
        "Could not tessellate this revision - the kernel returned no output.",
    )
    .await
    .expect("fails");

    let status = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads -- this is the call that used to 500 on the payload->>'path' decode")
        .expect("exists");

    assert_eq!(status.failed_total, 1);
    assert_eq!(status.failed.len(), 1);
    assert_eq!(
        status.failed[0].path, "spacer-lp-2001-00",
        "a derive failure has no file path, so it falls back to its revision's part name"
    );
}

/// `rendered` was a hardcoded `0` until this slice wired up the real `FILTER` aggregate.
/// A batch with exactly one job in every state -- pending, running, and each of the
/// three terminal outcomes -- is what catches that placeholder coming back: if `rendered`
/// silently reverts to `0`, `total` no longer equals the sum of the per-state counts.
#[sqlx::test(migrations = "./migrations")]
async fn a_mixed_batchs_total_is_the_sum_of_its_per_state_counts(pool: PgPool) {
    let jobs = PgJobs(pool.clone());

    let payloads = vec![
        JobPayload::IngestFile {
            path: "bracket-lp-1042-03.stl".to_owned(),
        },
        JobPayload::IngestFile {
            path: "spacer-lp-2001-00.stl".to_owned(),
        },
        JobPayload::IngestFile {
            path: "vee-block-lp-3072-02.stl".to_owned(),
        },
        JobPayload::Derive {
            revision: RevisionId::new(),
            produce: DerivativeKind::Thumbnail,
        },
        JobPayload::Derive {
            revision: RevisionId::new(),
            produce: DerivativeKind::TessellationL0,
        },
        JobPayload::Derive {
            revision: RevisionId::new(),
            produce: DerivativeKind::TessellationL1,
        },
    ];
    let (batch, queued) = jobs.enqueue(seeded(), &payloads).await.expect("enqueues");
    assert_eq!(queued, 6);

    let ingested = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.complete(ingested.id, Outcome::Ingested)
        .await
        .expect("completes");

    let skipped = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.complete(skipped.id, Outcome::Skipped)
        .await
        .expect("completes");

    let rendered = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.complete(rendered.id, Outcome::Rendered)
        .await
        .expect("completes");

    let failed = jobs
        .dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");
    jobs.fail(
        failed.id,
        "Could not read this STL - the file ends mid-facet.",
    )
    .await
    .expect("fails");

    // Leased but never completed: this one stays `running`. The sixth job is never
    // dequeued at all, so it stays `pending`.
    jobs.dequeue("worker-a", LEASE)
        .await
        .expect("dequeues")
        .expect("a job");

    let status = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");

    assert_eq!(status.pending, 1);
    assert_eq!(status.running, 1);
    assert_eq!(status.ingested, 1);
    assert_eq!(status.skipped, 1);
    assert_eq!(status.rendered, 1);
    assert_eq!(status.failed_total, 1);
    assert_eq!(
        status.total,
        status.pending
            + status.running
            + status.ingested
            + status.skipped
            + status.rendered
            + status.failed_total,
        "total must equal the sum of every per-state count -- a `rendered` count stuck \
         at 0 is exactly what would break this"
    );
    assert_eq!(status.total, 6);
}

/// A `derive` payload carries a bare revision uuid, which is exactly the kind of value
/// content addressing warns about (CLAUDE.md: "content addressing is not authorization").
/// Nothing today can enqueue a `derive` job whose revision belongs to a different
/// library, so this reaches past `enqueue` and inserts the row by hand -- the shape a
/// future caller could produce if the reachability check that is supposed to run before
/// enqueueing were ever skipped or buggy.
#[sqlx::test(migrations = "./migrations")]
async fn a_derive_job_naming_another_librarys_revision_does_not_leak_its_name(pool: PgPool) {
    let other = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, 'Fixture jigs', 'fixture jigs')")
        .bind(other.as_uuid())
        .execute(&pool)
        .await
        .expect("seeds a second library");

    let other_part_name = "vee-block-lp-3072-02";
    let part_id = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(part_id)
        .bind(other.as_uuid())
        .bind(other_part_name)
        .bind("vee-block-lp-3072-02.stl")
        .execute(&pool)
        .await
        .expect("inserts the other library's part");

    let revision = RevisionId::new();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision.as_uuid())
    .bind(part_id)
    .execute(&pool)
    .await
    .expect("inserts the other library's revision");

    let payload = JobPayload::Derive {
        revision,
        produce: DerivativeKind::Thumbnail,
    };
    let batch = BatchId::new();
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, last_error) \
         VALUES ($1, $2, $3, 'derive', $4, 'failed', $5)",
    )
    .bind(JobId::new().as_uuid())
    .bind(batch.as_uuid())
    .bind(seeded().as_uuid())
    .bind(payload.to_json())
    .bind("Could not tessellate this revision - the kernel returned no output.")
    .execute(&pool)
    .await
    .expect("inserts a failed derive job naming another library's revision");

    let jobs = PgJobs(pool.clone());
    let status = jobs
        .batch_status(seeded(), batch)
        .await
        .expect("reads")
        .expect("exists");

    assert_eq!(status.failed.len(), 1);
    assert_eq!(
        status.failed[0].path, "",
        "a revision belonging to another library must not resolve to that library's part"
    );
    assert!(
        !status.failed[0].path.contains(other_part_name),
        "must never leak another library's part name: {}",
        status.failed[0].path
    );
}
