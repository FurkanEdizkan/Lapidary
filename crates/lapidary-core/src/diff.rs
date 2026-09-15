//! What changed between two revisions of one part, figure by figure (`docs/DATA.md` §6.1).
//! The shapes only; `lapidary_vcs::diff` computes them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One figure's change between two revisions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Delta {
    pub from: f64,
    pub to: f64,
    /// `to - from`.
    pub change: f64,
    /// The change as a percentage of `from`. `None` when `from` is zero: a percentage of
    /// nothing is not a number anyone can read.
    pub percent: Option<f64>,
    /// When either figure is mesh-derived. A difference is no more exact than the less exact
    /// of the two figures it was taken between, and `CLAUDE.md` says the UI labels that.
    pub approximate: bool,
}

/// Every figure's change between two revisions of one part.
///
/// A figure that either revision did not record has no delta — `None`, never a zero. An open
/// mesh has no volume, and a volume change against it would be a number about nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RevisionDiff {
    pub volume_mm3: Option<Delta>,
    pub surface_area_mm2: Option<Delta>,
    /// Per axis: x, y, z.
    pub bbox_mm: Option<[Delta; 3]>,
    pub triangle_count: Option<Delta>,
    /// A CAD revision's B-rep faces and edges, exact. `None` when either revision is a mesh, or was
    /// read before the bridge counted them.
    pub face_count: Option<Delta>,
    pub edge_count: Option<Delta>,
}
