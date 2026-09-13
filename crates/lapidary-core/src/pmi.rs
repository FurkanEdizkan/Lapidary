//! What a CAD file specifies about a part's sizes and form: its semantic PMI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The product and manufacturing information an AP242 file carries as data: dimensions with their
/// tolerances, geometric tolerances, and the datums those refer to.
///
/// **Specified, never measured.** These are the numbers a designer wrote into the file, so they are
/// neither exact nor approximate in [`Approximate`](crate::Approximate)'s sense, and nothing here
/// says whether a made part meets them. `OcctKernel` fills this from `occt-bridge`'s `pmi.json`; a
/// mesh has none, and neither does a STEP file whose exporter wrote only geometry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Pmi {
    pub dimensions: Vec<PmiDimension>,
    pub tolerances: Vec<PmiTolerance>,
    pub datums: Vec<PmiDatum>,
}

impl Pmi {
    /// Whether the file specified nothing, which ingest stores as no derivative at all.
    pub fn is_empty(&self) -> bool {
        self.dimensions.is_empty() && self.tolerances.is_empty() && self.datums.is_empty()
    }
}

/// A face an annotation applies to, named as an [`Entity`](crate::Entity) names one: the prototype,
/// and the face's 1-based index in that prototype's faces. `face` is `None` when the annotation
/// applies to the whole part rather than to one face of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PmiFace {
    pub prototype: String,
    pub face: Option<u32>,
}

/// A dimension, in millimetres or, for an angle, degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PmiDimension {
    /// `diameter`, `radius`, `spherical_diameter`, `spherical_radius`, `thickness`, `angle`,
    /// `distance`, or `other` for a kind this reads but does not name.
    #[serde(rename = "type")]
    pub kind: String,
    pub value: f64,
    /// How far above `value` the size may be, when the file gives plus and minus bounds.
    pub upper: Option<f64>,
    /// How far below `value`, as the file writes it: zero, or a negative deviation.
    pub lower: Option<f64>,
    pub faces: Vec<PmiFace>,
}

/// A geometric tolerance: the width of the zone a feature's form, orientation or location must
/// stay in, in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PmiTolerance {
    /// `flatness`, `perpendicularity`, `position` and the other ISO 1101 characteristics, in
    /// snake case, or `other`.
    #[serde(rename = "type")]
    pub kind: String,
    pub value: f64,
    /// The datums the tolerance is measured from, by name, in the file's order. Empty for a form
    /// tolerance such as flatness, which refers to none.
    pub datums: Vec<String>,
    pub faces: Vec<PmiFace>,
}

/// A datum: a named feature other tolerances are measured from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PmiDatum {
    pub name: String,
    pub faces: Vec<PmiFace>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge's JSON, as `occt-bridge` writes it for the PMI fixture, reads into these types
    /// and back without losing a field.
    #[test]
    fn the_bridges_pmi_json_reads_and_round_trips() {
        let json = r#"{"dimensions":[{"type":"diameter","value":22,"upper":0.05,"lower":0,"faces":[{"prototype":"0:1:1:1","face":1}]}],"tolerances":[{"type":"flatness","value":0.02,"datums":[],"faces":[{"prototype":"0:1:1:1","face":3}]}],"datums":[{"name":"A","faces":[{"prototype":"0:1:1:1","face":2}]}]}"#;
        let pmi: Pmi = serde_json::from_str(json).expect("reads");
        assert_eq!(pmi.dimensions[0].kind, "diameter");
        assert_eq!(pmi.dimensions[0].upper, Some(0.05));
        assert_eq!(pmi.datums[0].faces[0].face, Some(2));
        assert!(!pmi.is_empty());
        let again: Pmi =
            serde_json::from_value(serde_json::to_value(&pmi).expect("writes")).expect("reads");
        assert_eq!(again, pmi);
        assert!(Pmi::default().is_empty());
    }
}
