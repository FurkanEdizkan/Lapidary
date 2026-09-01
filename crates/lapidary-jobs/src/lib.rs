//! The job queue: PostgreSQL `FOR UPDATE SKIP LOCKED` plus `LISTEN`/`NOTIFY`, deliberately
//! with no Redis and no message broker. Workers take leases on jobs and heartbeat them.
//! Implementation lands in Phase 1; see docs/DATA.md.

use thiserror::Error;

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
