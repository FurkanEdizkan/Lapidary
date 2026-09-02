use crate::Approximate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a measured value came from. Persisted as text beside the value it describes,
/// because a single row-level flag cannot describe a revision whose volume is analytic
/// and whose triangle count is tessellated — which is every STEP part from Phase 2 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Provenance {
    /// Read from a B-rep entity. Safe to present as exact.
    Analytic,
    /// Derived from tessellated geometry. The UI must label it.
    Tessellated,
}

impl Provenance {
    /// The persisted form. `revision.volume_source` and friends store this string.
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Analytic => "analytic",
            Provenance::Tessellated => "tessellated",
        }
    }
}

impl std::str::FromStr for Provenance {
    type Err = crate::CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "analytic" => Ok(Provenance::Analytic),
            "tessellated" => Ok(Provenance::Tessellated),
            other => Err(crate::CoreError::ProvenanceUnknown {
                got: other.to_owned(),
            }),
        }
    }
}

/// What a mesh can tell us about itself. Every figure here is tessellated by
/// construction — a mesh has no analytic entities to read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeshMeasurements {
    pub bbox_mm: [f64; 3],
    pub triangle_count: u32,
    pub surface_area_mm2: f64,
    /// `None` when the mesh is not watertight. Signed-volume integration over an open
    /// mesh produces a plausible-looking number with no meaning, and a wrong number is
    /// worse than no number.
    pub volume_mm3: Option<f64>,
    pub is_watertight: bool,
}

impl MeshMeasurements {
    /// The volume, wrapped so a caller cannot render it without its approximate label.
    pub fn volume_approximate(&self) -> Option<Approximate<f64>> {
        self.volume_mm3.map(Approximate::tessellated)
    }
}
