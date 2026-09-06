//! Metadata extraction plus full-text search via `tsvector` and `pg_trgm`.
//!
//! Empty. Implementation lands in **Phase 2** (`ROADMAP.md`), with CAD ingest — the
//! phase whose exit criterion is finding `A1234-56-B` by the fragment `1234`. See
//! `docs/DATA.md` §3.

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
