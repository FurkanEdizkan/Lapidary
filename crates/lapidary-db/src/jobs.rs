//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use jiff::Timestamp;
use lapidary_core::{BatchId, BatchStatus, JobFailure, JobId, LibraryId, Outcome};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// The `LISTEN`/`NOTIFY` channel. A wake-up only: the payload is empty and nothing reads
/// it. Notification is a latency optimization over the worker's polling floor and must
/// never become the mechanism the queue depends on -- a NOTIFY fires into the void when
/// nothing is listening, so a worker that starts after an enqueue would never learn about
/// that work. See the design doc, section 3.4, and the test that disables the listener.
pub const JOB_CHANNEL: &str = "lapidary_jobs";

pub struct PgJobs(pub PgPool);

/// Rebuild a `jiff::Timestamp` from the microseconds the query selected. sqlx 0.9 has no
/// `jiff` feature, so every timestamp in this workspace crosses the boundary this way;
/// `PgParts::page` does the same, including reporting a corrupt row rather than clamping.
fn to_timestamp(column: &'static str, micros: i64) -> Result<Timestamp, DbError> {
    Timestamp::from_microsecond(micros).map_err(|_| DbError::TimestampOutOfRange {
        column,
        value: micros,
    })
}

/// The most failures one status response carries. A thousand-file disaster must return a
/// readable payload, and `failed_total` still reports the real number.
const FAILED_SAMPLE: i64 = 100;

/// One claimed job, as `dequeue` hands it to the worker loop.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: JobId,
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub kind: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

impl PgJobs {
    /// Enqueue one job per path under a fresh batch. One statement regardless of N: a
    /// thousand files is one insert, because this runs inside the HTTP request.
    pub async fn enqueue_scan(
        &self,
        library: LibraryId,
        paths: &[String],
    ) -> Result<(BatchId, u32), DbError> {
        let batch = BatchId::new();

        if paths.is_empty() {
            // No rows, and deliberately no NOTIFY: waking every worker to find nothing
            // is the one case where the optimization is pure cost.
            return Ok((batch, 0));
        }

        let ids: Vec<Uuid> = (0..paths.len()).map(|_| JobId::new().as_uuid()).collect();

        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT id, $2, $3, 'ingest_file', jsonb_build_object('path', path) \
             FROM unnest($1::uuid[], $4::text[]) AS t(id, path)",
        )
        .bind(&ids)
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(paths)
        .execute(&self.0)
        .await?;

        sqlx::query("SELECT pg_notify($1, '')")
            .bind(JOB_CHANNEL)
            .execute(&self.0)
            .await?;

        Ok((batch, paths.len() as u32))
    }

    /// Claim one job, or reclaim one whose lease expired.
    ///
    /// Reclamation is folded in here rather than given to a sweeper, so there is no
    /// sweeper process to be the thing that died. `attempts` increments on reclamation
    /// exactly as it does on retry, which is what caps the poison-pill case: a file that
    /// panics the worker before it can record anything is tried `max_attempts` times and
    /// then abandoned by the caller, rather than re-leased forever.
    ///
    /// Exhausted rows are deliberately NOT excluded here. Filtering them out in SQL would
    /// leave them 'running' with a dead lease, invisible to this query and to any cleanup
    /// -- which is how this table would grow a permanent population of zombies. The
    /// caller claims them and fails them (see `lapidary_jobs::worker`).
    pub async fn dequeue(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> Result<Option<JobRow>, DbError> {
        let row: Option<(Uuid, Uuid, Uuid, String, serde_json::Value, i32, i32)> = sqlx::query_as(
            "UPDATE job SET state = 'running', \
                                attempts = attempts + 1, \
                                leased_by = $1, \
                                lease_expires_at = now() + make_interval(secs => $2), \
                                updated_at = now() \
                 WHERE id = ( \
                     SELECT id FROM job \
                     WHERE (state = 'pending' AND run_after <= now()) \
                        OR (state = 'running' AND lease_expires_at < now()) \
                     ORDER BY run_after \
                     FOR UPDATE SKIP LOCKED \
                     LIMIT 1 \
                 ) \
                 RETURNING id, batch_id, library_id, kind, payload, attempts, max_attempts",
        )
        .bind(worker_id)
        .bind(lease.as_secs_f64())
        .fetch_optional(&self.0)
        .await?;

        Ok(row.map(
            |(id, batch_id, library_id, kind, payload, attempts, max_attempts)| JobRow {
                id: JobId::from_uuid(id),
                batch_id: BatchId::from_uuid(batch_id),
                library_id: LibraryId::from_uuid(library_id),
                kind,
                payload,
                attempts,
                max_attempts,
            },
        ))
    }

    /// Terminal, successful. `last_error` is deliberately left as-is: a job that failed
    /// transiently, retried and then succeeded keeps the reason it retried. That is
    /// diagnostically useful, invisible to users (`BatchStatus` only reports `last_error`
    /// for rows in state `failed`), and `attempts` alone tells you it retried but not why.
    ///
    /// `AND state = 'running'` guards against the scenario spec §3.2 itself calls out: a
    /// worker stalls past its lease, a second worker reclaims and finishes the same job,
    /// and the first worker -- still alive, just slow -- eventually calls back in. Without
    /// this guard the stale write clobbers whatever the reclaiming worker recorded; with
    /// it, the row is no longer `running` by the time the stale caller arrives, so the
    /// `UPDATE` matches zero rows instead of overwriting a real result. "Measurement must
    /// not lie" (CLAUDE.md) -- a part that ingested must never be reported failed because
    /// a second, abandoned attempt finished after it.
    pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError> {
        let outcome = match outcome {
            Outcome::Ingested => "ingested",
            Outcome::Skipped => "skipped",
        };
        sqlx::query(
            "UPDATE job SET state = 'done', outcome = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running'",
        )
        .bind(id.as_uuid())
        .bind(outcome)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Terminal. `reason` is the handler's own message and is shown to a person.
    ///
    /// `AND state = 'running'` -- see `complete`'s doc comment for why: this is the same
    /// stale-writer guard, so a worker that stalled past its lease and is only now
    /// reporting failure cannot overwrite a row another worker already finished.
    pub async fn fail(&self, id: JobId, reason: &str) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE job SET state = 'failed', last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running'",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Back to the queue behind a backoff. `last_error` is kept -- not cleared -- so a job
    /// that is still retrying can say what went wrong last time. This is only legal
    /// because `job_failed_has_reason` is an implication (`state <> 'failed' or last_error
    /// is not null`), not a biconditional: a `pending` row carrying a `last_error` violates
    /// nothing.
    ///
    /// `AND state = 'running'` -- see `complete`'s doc comment for why: the same
    /// stale-writer guard applies here too.
    pub async fn reschedule(
        &self,
        id: JobId,
        reason: &str,
        backoff: Duration,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE job SET state = 'pending', \
                            run_after = now() + make_interval(secs => $3), \
                            last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running'",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .bind(backoff.as_secs_f64())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Graceful shutdown: hand back whatever this worker still holds so a restart
    /// resumes at once rather than waiting out every lease. A crash does not get this,
    /// which is what lease expiry is for -- the two paths are separate because only one
    /// of them can run cleanup code.
    pub async fn release_leases(&self, worker_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'pending', run_after = now(), leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE leased_by = $1 AND state = 'running'",
        )
        .bind(worker_id)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected())
    }

    /// What a scan turned into. `None` when the batch has no jobs -- an id never issued
    /// and a scan that enqueued nothing are indistinguishable, and both mean "no status
    /// resource" (Task 11 turns this into a 404).
    pub async fn batch_status(
        &self,
        library: LibraryId,
        batch: BatchId,
    ) -> Result<Option<BatchStatus>, DbError> {
        // `library_id = $2` is the ownership check, not a performance filter: it is what
        // stops a batch id alone from being a capability -- content addressing is not
        // authorization (CLAUDE.md).
        // `min(created_at)` is itself an aggregate, so it is just as capable of coming
        // back NULL over zero rows as the counts are -- decoding it as a bare i64
        // panics on the empty-batch case instead of falling into the `total == 0`
        // guard below. Both aggregate timestamp columns are `Option<i64>` for exactly
        // this reason.
        #[allow(clippy::type_complexity)]
        let counts: Option<(i64, i64, i64, i64, i64, i64, Option<i64>, Option<i64>)> =
            sqlx::query_as(
                "SELECT count(*), \
                    count(*) FILTER (WHERE state = 'pending'), \
                    count(*) FILTER (WHERE state = 'running'), \
                    count(*) FILTER (WHERE outcome = 'ingested'), \
                    count(*) FILTER (WHERE outcome = 'skipped'), \
                    count(*) FILTER (WHERE state = 'failed'), \
                    (extract(epoch FROM min(created_at)) * 1000000)::bigint, \
                    CASE WHEN count(*) FILTER (WHERE state IN ('pending','running')) = 0 \
                         THEN (extract(epoch FROM max(updated_at)) * 1000000)::bigint \
                    END \
             FROM job WHERE batch_id = $1 AND library_id = $2",
            )
            .bind(batch.as_uuid())
            .bind(library.as_uuid())
            .fetch_optional(&self.0)
            .await?;

        // An aggregate over zero rows still returns one row, with count 0 -- so "no
        // jobs" is detected on the count, not on fetch_optional returning None.
        let Some((total, pending, running, ingested, skipped, failed_total, started, finished)) =
            counts
        else {
            return Ok(None);
        };
        if total == 0 {
            return Ok(None);
        }

        // `, id` is load-bearing, not decoration: `enqueue_scan` inserts a whole batch
        // in one statement, and Postgres's `now()` is constant for the duration of a
        // transaction, so every job in a real batch shares the exact same
        // `created_at` -- `ORDER BY created_at` alone never actually discriminates
        // between same-batch failures and the list would reshuffle under a polling
        // reader from one request to the next (spec §7 promises it does not). `JobId`
        // is uuidv7 and `enqueue_scan` generates ids in insertion order, so ordering
        // by `id` as the tiebreaker reproduces enqueue order, which -- since Task 10
        // sorts paths before enqueueing -- is the alphabetical order a person expects.
        // Do not simplify this back to `ORDER BY created_at`.
        let failures: Vec<(String, String, i32)> = sqlx::query_as(
            "SELECT payload->>'path', last_error, attempts \
             FROM job WHERE batch_id = $1 AND library_id = $2 AND state = 'failed' \
             ORDER BY created_at, id LIMIT $3",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(FAILED_SAMPLE)
        .fetch_all(&self.0)
        .await?;

        Ok(Some(BatchStatus {
            batch_id: batch,
            library_id: library,
            total: total as u32,
            pending: pending as u32,
            running: running as u32,
            ingested: ingested as u32,
            skipped: skipped as u32,
            failed_total: failed_total as u32,
            failed: failures
                .into_iter()
                .map(|(path, reason, attempts)| JobFailure {
                    path,
                    reason,
                    attempts: attempts.max(0) as u32,
                })
                .collect(),
            // Microseconds in, jiff out -- the conversion belongs here, at the database
            // boundary, so no wire type ever carries a raw epoch integer. `started` is
            // provably `Some` here: `total != 0` means at least one row exists, and
            // `min(created_at)` over a non-empty set cannot be NULL. `unwrap_or_default`
            // (not `unwrap()` -- CLAUDE.md bans that outside tests) documents that this
            // is an invariant, not a real fallback.
            started_at: to_timestamp("job.created_at", started.unwrap_or_default())?,
            finished_at: finished
                .map(|us| to_timestamp("job.updated_at", us))
                .transpose()?,
        }))
    }
}
