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

    #[error(
        "\"{got}\" is not a valid id — ids are UUIDs. Copy the id from the part, library, or revision it identifies rather than retyping it."
    )]
    IdParse { got: String },

    #[error(
        "`{got}` is not a measurement provenance. Expected `analytic` (read from a B-rep entity) or `tessellated` (derived from mesh geometry). A row written outside lapidary-db may have used a different vocabulary."
    )]
    ProvenanceUnknown { got: String },

    #[error(
        "A {kind} job's payload is not the shape that kind requires — {detail}. It was not written by Lapidary; check whether something else is inserting into the job table."
    )]
    MalformedJobPayload { kind: String, detail: String },

    #[error(
        "\"{kind}\" is not a job kind this build knows. A newer Lapidary may have written it; check that every worker and api container is running the same version."
    )]
    UnknownJobKind { kind: String },
}
