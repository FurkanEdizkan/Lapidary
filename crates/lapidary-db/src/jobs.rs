//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use lapidary_core::{BatchId, JobId, LibraryId, Outcome};
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
    pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError> {
        let outcome = match outcome {
            Outcome::Ingested => "ingested",
            Outcome::Skipped => "skipped",
        };
        sqlx::query(
            "UPDATE job SET state = 'done', outcome = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(outcome)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Terminal. `reason` is the handler's own message and is shown to a person.
    pub async fn fail(&self, id: JobId, reason: &str) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE job SET state = 'failed', last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1",
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
             WHERE id = $1",
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
}
