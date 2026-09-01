#![deny(clippy::unwrap_used)]
//! Metadata extraction plus full-text search via `tsvector` and `pg_trgm`.
//! Implementation lands in Phase 1; see docs/DATA.md.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(
        "Metadata extraction failed at stage {stage}. The source file may be corrupt or use a feature this extractor does not support yet; check the ingest log for the underlying cause."
    )]
    ExtractionFailed { stage: u8 },

    #[error(
        "PostgreSQL has no text-search configuration named '{config}'. Install it on the database server or pick a different search language for this library."
    )]
    SearchConfigMissing { config: String },
}
