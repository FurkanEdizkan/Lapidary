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

pub use handler::{HandlerError, JobHandler};
pub use policy::{BACKOFF, Next, next_state};

#[derive(Debug, Error)]
pub enum JobsError {
    #[error(
        "The lease on job {job_id} expired before it was renewed. Another worker may have already picked it up; if this worker is still alive, check that it is heartbeating and not stalled."
    )]
    LeaseExpired { job_id: String },

    #[error(
        "The job queue is unreachable. Lapidary queues jobs in PostgreSQL itself, so this means the database connection is down, not a broker — check the `db` service."
    )]
    QueueUnavailable,
}
