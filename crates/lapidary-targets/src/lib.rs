//! Automatic format negotiation: slicers get 3MF/STL, CAD gets STEP, the viewer gets
//! glTF.
//!
//! Export bundles live here (`bundle`, Phase 4 slice 2). The `Target` trait does not yet: download
//! and `lapidary open` both hand out `variant=original`, so nothing negotiates a format (spec §3).
//! See `docs/DATA.md` §5.

pub mod bundle;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TargetsError {
    #[error(
        "No format satisfies {target}. This part has {available} available, and Lapidary will not hand a mesh to a target that needs B-rep. Generate a compatible derivative, or export from the original in the CAD tool."
    )]
    NoFormatMatch { target: String, available: String },

    #[error(
        "Export failed: {reason}. Retry, and if it keeps failing, check the source derivative is not corrupt."
    )]
    ExportFailed { reason: String },
}
