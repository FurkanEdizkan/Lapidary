use thiserror::Error;

/// Errors say what broke and what to do about it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error(
        "A blob hash must be 64 hex characters (a 32-byte BLAKE3 digest); got {got}. Copy the full hash from the part's detail panel."
    )]
    BlobHashLength { got: usize },

    #[error(
        "A blob hash must contain only the characters 0-9 and a-f. Copy the full hash from the part's detail panel rather than retyping it."
    )]
    BlobHashHex,
}
