//! The things a kernel call can produce, and the strings `derivative.kind` holds.

/// Also the discriminator a `derive` job carries, which is why it lives here rather than
/// in `lapidary-cad`: the job queue names it and the database stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DerivativeKind {
    Thumbnail,
    TessellationL0,
    TessellationL1,
    TessellationL2,
    /// The assembly tree a CAD kernel read, as JSON. A mesh has none.
    Structure,
    /// The analytic faces and circular edges a CAD kernel read, as JSON. A mesh has none.
    Entities,
    /// The dimensions, tolerances and datums a CAD file specifies, as JSON. A mesh has none.
    Pmi,
    /// The part's mesh as a binary STL Lapidary wrote, for a slicer. Served as `*.lapidary.stl`.
    ExportStl,
    /// The part's mesh as a 3MF package Lapidary wrote, for a slicer. Served as `*.lapidary.3mf`.
    /// Named by hand: `snake_case` puts no `_` before a digit, and the job payload and the
    /// bindings must say what `derivative.kind` does.
    #[serde(rename = "export_3mf")]
    Export3mf,
}

impl DerivativeKind {
    /// Every kind, ascending — for a caller that genuinely wants all four.
    pub const ALL: [DerivativeKind; 9] = [
        DerivativeKind::Thumbnail,
        DerivativeKind::TessellationL0,
        DerivativeKind::TessellationL1,
        DerivativeKind::TessellationL2,
        DerivativeKind::Structure,
        DerivativeKind::Entities,
        DerivativeKind::Pmi,
        DerivativeKind::ExportStl,
        DerivativeKind::Export3mf,
    ];

    /// Exactly the strings already in `derivative.kind`. Changing one orphans every row
    /// written before the change.
    pub fn as_str(self) -> &'static str {
        match self {
            DerivativeKind::Thumbnail => "thumbnail",
            DerivativeKind::TessellationL0 => "tessellation_l0",
            DerivativeKind::TessellationL1 => "tessellation_l1",
            DerivativeKind::TessellationL2 => "tessellation_l2",
            DerivativeKind::Structure => "structure",
            DerivativeKind::Entities => "entities",
            DerivativeKind::Pmi => "pmi",
            DerivativeKind::ExportStl => "export_stl",
            DerivativeKind::Export3mf => "export_3mf",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_strings_match_what_the_database_holds() {
        // These strings are in `derivative.kind` on every row ever written.
        assert_eq!(DerivativeKind::Thumbnail.as_str(), "thumbnail");
        assert_eq!(DerivativeKind::TessellationL0.as_str(), "tessellation_l0");
        assert_eq!(DerivativeKind::TessellationL1.as_str(), "tessellation_l1");
        assert_eq!(DerivativeKind::TessellationL2.as_str(), "tessellation_l2");
        assert_eq!(DerivativeKind::Structure.as_str(), "structure");
        assert_eq!(DerivativeKind::Entities.as_str(), "entities");
        assert_eq!(DerivativeKind::Pmi.as_str(), "pmi");
        assert_eq!(DerivativeKind::ExportStl.as_str(), "export_stl");
        assert_eq!(DerivativeKind::Export3mf.as_str(), "export_3mf");
    }

    /// A derive job's payload and the bindings name a kind with serde, the database with `as_str`.
    #[test]
    fn serde_names_every_kind_as_the_database_does() {
        for kind in DerivativeKind::ALL {
            assert_eq!(
                serde_json::to_value(kind).expect("serializes"),
                serde_json::Value::from(kind.as_str())
            );
        }
    }

    #[test]
    fn all_is_every_kind_in_ladder_order() {
        // Ingest asks for `ALL` and relies on it meaning everything, in the order the
        // deleted `ladder()` used: thumbnail first, then rungs ascending. Pinning the
        // contents rather than the length is what catches a reorder or a removal.
        //
        // It does not catch an *addition*: a fifth variant leaves `[DerivativeKind; 4]`
        // four elements long and this assertion still passes. The compile errors that
        // point a person here are `as_str` above and `MeshKernel::process`, both
        // exhaustive. Making the omission itself a compile error needs a variant count
        // the language will not give us on stable — `std::mem::variant_count` is
        // nightly, `strum` is a new dependency, and a `macro_rules!` enum would have to
        // carry `serde`, `ts_rs::TS` and `#[ts(export)]` through the macro, putting
        // `cargo xtask export-bindings` in the blast radius of a four-variant enum. Not
        // worth it; add the variant here when you add it above.
        assert_eq!(
            DerivativeKind::ALL,
            [
                DerivativeKind::Thumbnail,
                DerivativeKind::TessellationL0,
                DerivativeKind::TessellationL1,
                DerivativeKind::TessellationL2,
                DerivativeKind::Structure,
                DerivativeKind::Entities,
                DerivativeKind::Pmi,
                DerivativeKind::ExportStl,
                DerivativeKind::Export3mf,
            ]
        );
    }
}
