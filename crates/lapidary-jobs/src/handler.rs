//! The seam between delivery and work.
//!
//! `lapidary-jobs` is L2, so it may not depend on `lapidary-cad` (also L2) or on
//! `lapidary-ingest` (L3) -- `cargo xtask check-layers` forbids exactly the edge a loop
//! that called ingest directly would need. The resulting design is the better one anyway:
//! the handler reports what happened, and the loop decides what to do about it, so the
//! rule about immutable bytes is stated in one place instead of three.

use lapidary_core::Outcome;
use lapidary_db::JobRow;
use std::future::Future;

pub trait JobHandler: Send + Sync + 'static {
    fn handle(&self, job: &JobRow) -> impl Future<Output = Result<Outcome, HandlerError>> + Send;
}

/// Why a job did not finish. The distinction is the whole retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// Re-running this job cannot succeed. Blobs are content-addressed and immutable, so
    /// parsing the same bytes again is guaranteed to produce the same error; retrying
    /// only delays an answer that was already available.
    Permanent { message: String },
    /// Something outside the job failed -- the database, the blob store, a lost lease.
    /// Worth another attempt.
    ///
    /// When in doubt, choose this: a retried permanent failure costs one wasted parse,
    /// while a non-retried transient failure costs the user a file.
    Transient { message: String },
}
