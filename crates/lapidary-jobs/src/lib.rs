//! The job queue: PostgreSQL `FOR UPDATE SKIP LOCKED` plus `LISTEN`/`NOTIFY`, deliberately
//! with no Redis and no message broker. Workers take leases on jobs.
//!
//! Leases are NOT heartbeated yet. A single-file mesh ingest measures roughly 200 ms
//! against a 60-second lease, so a renewal task would be ceremony that still has to be
//! tested and can still be wrong. `renew_lease` arrives when a job can realistically
//! outlive its lease -- STEP ingest through the OCCT sidecar in Phase 2.

use thiserror::Error;

mod handler;
mod policy;
mod worker;

pub use handler::{HandlerError, JobHandler};
pub use policy::{BACKOFF, Next, next_state};
pub use worker::{WorkerConfig, run};

#[derive(Debug, Error)]
pub enum JobsError {
    #[error(
        "The lease on job {job_id} elapsed before this worker finished it. Leases are not renewed yet (see this crate's module docs), so any job that runs longer than its lease trips this — another worker may have already reclaimed and finished it. Check whether this worker stalled or was simply slower than the lease; if jobs are legitimately outliving their lease, that is what `renew_lease` in Phase 2 is for."
    )]
    LeaseExpired { job_id: String },

    #[error(
        "The job queue is unreachable. Lapidary queues jobs in PostgreSQL itself, so this means the database connection is down, not a broker — check the `db` service."
    )]
    QueueUnavailable,
}
