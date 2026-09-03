//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use lapidary_core::{BatchId, JobId, LibraryId};
use sqlx::PgPool;
use uuid::Uuid;

/// The `LISTEN`/`NOTIFY` channel. A wake-up only: the payload is empty and nothing reads
/// it. Notification is a latency optimization over the worker's polling floor and must
/// never become the mechanism the queue depends on -- a NOTIFY fires into the void when
/// nothing is listening, so a worker that starts after an enqueue would never learn about
/// that work. See the design doc, section 3.4, and the test that disables the listener.
pub const JOB_CHANNEL: &str = "lapidary_jobs";

pub struct PgJobs(pub PgPool);

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
}
