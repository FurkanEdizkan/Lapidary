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
        "A device id is {expected} characters of Crockford base32, shown in groups of five; this one has {got}. Copy it from the sharing page of the machine it names rather than retyping it."
    )]
    DeviceIdLength { got: usize, expected: usize },

    #[error(
        "A device id uses only the characters Crockford base32 has — the digits and the letters except I, L, O and U, which are read as the digits they look like. {got:?} is not one of them. Copy it from the sharing page of the machine it names rather than retyping it."
    )]
    DeviceIdCharacter { got: char },

    #[error(
        "A device id's last character carries one bit of the digest and four that only pad it out, and {got:?} sets those four — so this is not an id any Lapidary printed. Copy it again from the sharing page of the machine it names."
    )]
    DeviceIdPadding { got: char },

    #[error(
        "`{got}` is not a measurement provenance. Expected `analytic` (read from a B-rep entity) or `tessellated` (derived from mesh geometry). A row written outside lapidary-db may have used a different vocabulary."
    )]
    ProvenanceUnknown { got: String },

    #[error(
        "A `{kind}` job's payload is not the shape that kind requires — {detail}. Check whether the row was written by an older Lapidary, or by something other than Lapidary inserting into the job table."
    )]
    MalformedJobPayload { kind: String, detail: String },

    #[error(
        "\"{kind}\" is not a job kind this build knows. A newer Lapidary may have written it; check that every worker and api container is running the same version."
    )]
    UnknownJobKind { kind: String },

    #[error(
        "Refused the path {got:?}: it points outside the directory it belongs to. Paths \
         stored by Lapidary are relative and may not contain `..` or start at the root. A \
         normal scan or upload never produces one; check whatever enqueued this job."
    )]
    PathEscapes { got: String },
}
