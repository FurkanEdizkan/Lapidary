//! Automatic format negotiation: slicers get 3MF/STL, CAD gets STEP, the viewer gets
//! glTF.
//!
//! Empty. Implementation lands in **Phase 4** (`ROADMAP.md`), beside the `Target` trait
//! and the round-trip the format negotiation exists to serve. See `docs/DATA.md` §5.

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
