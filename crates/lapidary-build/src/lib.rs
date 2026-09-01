#![deny(clippy::unwrap_used)]
//! The build graph of manufacturing process steps. Implementation lands in Phase 1;
//! see docs/DATA.md.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(
        "The build graph has a cycle at {at}. Process steps must form a DAG — find the edge that loops back through {at} and remove it."
    )]
    CycleRejected { at: String },

    #[error(
        "'{name}' is not a recognized process type. Check for a typo, or register the process type before using it in a build graph."
    )]
    ProcessTypeUnknown { name: String },
}
