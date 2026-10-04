//! What can go wrong between a file's bytes and its figures. Four cases, every one of them
//! the file's own verdict: `lapidary-ingest`'s `classify_cad` maps all four to `Permanent`
//! because the bytes are content-addressed and immutable, so a retry reaches the same answer.
//!
//! **Nine variants went with the kernel.** `UnsupportedSchema`, `Timeout`, `KernelCrashed`,
//! `KernelUnavailable`, `KernelOutputUnreadable` and `CadRefused` were `OcctKernel`'s, which
//! R2 deleted; `NoFixture` was `MockKernel`'s and `Unrenderable` belonged to the glTF writer
//! and the thumbnail rasteriser, which went here. They were kept as unreachable arms for one
//! goal each so the deletions could be made one at a time; nothing raises them now, and an
//! error nothing raises is a sentence nobody can read back to you.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CadError {
    #[error(
        "Could not read this {format} — {detail}. Re-export it from your CAD or slicing tool and retry; if it came from a download, the transfer may have been cut short."
    )]
    MalformedMesh { format: String, detail: String },

    #[error(
        "Could not write this part's {format} export: {detail}. This is a bug in Lapidary; please report it with the part's file."
    )]
    ExportFailed { format: String, detail: String },

    #[error(
        "This build has no parser for the {format} format. Lapidary reads STL, OBJ and 3MF; export the part as one of those and retry."
    )]
    UnsupportedFormat { format: String },

    #[error(
        "Refused this {format} — {detail}. The file may be corrupt or deliberately crafted; re-export it from a trusted tool and retry."
    )]
    ArchiveRefused { format: String, detail: String },
}
