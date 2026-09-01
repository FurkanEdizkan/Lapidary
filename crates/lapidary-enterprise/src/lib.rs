#![deny(clippy::unwrap_used)]
//! The offline Ed25519 licence file: `max_workers` and a grace period. A contractual
//! and support boundary, not technical DRM. Implementation lands in Phase 1;
//! see docs/DATA.md.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnterpriseError {
    #[error(
        "The licence has expired, with {grace_days} days of grace left before enterprise features stop working. Install a renewed licence file to continue without interruption."
    )]
    LicenceExpired { grace_days: u32 },

    #[error(
        "This licence permits up to {max} workers, and that many are already registered. Retire a worker before adding another, or contact sales to raise the limit."
    )]
    WorkerLimitReached { max: u32 },
}
