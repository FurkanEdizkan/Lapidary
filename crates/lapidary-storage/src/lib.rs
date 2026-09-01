//! Blob content-addressed storage: BLAKE3 addressing, zstd compression, tiering and
//! quarantine. Implementation lands in Phase 1; see docs/DATA.md.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "No blob is stored under hash {hash}. It may have been purged after its 30-day quarantine, or the hash may be from a different library."
    )]
    NotFound { hash: String },

    #[error(
        "Could not write to the blob store at {path}. Check that the volume is mounted and writable."
    )]
    WriteFailed { path: String },
}
