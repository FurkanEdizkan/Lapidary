//! Automatic format negotiation: slicers get 3MF/STL, CAD gets STEP, the viewer gets
//! glTF. Implementation lands in Phase 1; see docs/DATA.md.

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
