//! The assembly tree a CAD kernel reads from a STEP or IGES file.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An assembly's tree as the CAD file describes it. `None` on [`KernelOutput::structure`]
/// for a mesh, which has no tree to describe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssemblyTree {
    pub roots: Vec<AssemblyNode>,
    /// Leaves: placed parts, counting every instance.
    pub parts: u32,
    /// Distinct part definitions the leaves are instances of.
    pub prototypes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssemblyNode {
    pub name: String,
    pub prototype: String,
    /// Row-major 4×4, relative to the parent node.
    pub transform: [f64; 16],
    #[serde(default)]
    pub children: Vec<AssemblyNode>,
}
