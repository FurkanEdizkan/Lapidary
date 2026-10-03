//! The things a worker can still build from a part, and the strings `derivative.kind` holds.
//!
//! Two, both exports for another tool. The render ladder — `thumbnail`, `tessellation_l0/l1/l2`,
//! `structure`, `entities`, `pmi` — left with the pivot (R3): Lapidary shows the part's own
//! picture and loads the original file when somebody presses **view**, so nothing renders and
//! nothing reads a B-rep. **The variants were deleted rather than left unproduced on purpose**:
//! every producer then surfaced as a compile error instead of a judgement call about what a
//! function should now return.
//!
//! The strings those rows hold are still in the database until migration `0055` (R4) removes
//! them, and every reader of `derivative.kind` filters in SQL or drops what it does not know —
//! so a row of a kind this build cannot name is invisible, never an error.

/// Also the discriminator a `derive` job carries, which is why it lives here rather than
/// in `lapidary-cad`: the job queue names it and the database stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DerivativeKind {
    /// The part's mesh as a binary STL Lapidary wrote, for a slicer. Served as `*.lapidary.stl`.
    ExportStl,
    /// The part's mesh as a 3MF package Lapidary wrote, for a slicer. Served as `*.lapidary.3mf`.
    /// Named by hand: `snake_case` puts no `_` before a digit, and the job payload and the
    /// bindings must say what `derivative.kind` does.
    #[serde(rename = "export_3mf")]
    Export3mf,
}

impl DerivativeKind {
    /// Every kind, in `as_str` order — for a caller that genuinely wants both.
    pub const ALL: [DerivativeKind; 2] = [DerivativeKind::ExportStl, DerivativeKind::Export3mf];

    /// Exactly the strings already in `derivative.kind`. Changing one orphans every row
    /// written before the change.
    pub fn as_str(self) -> &'static str {
        match self {
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

    /// The render kinds are gone from this enum and must stay gone: a payload naming one is a
    /// job row from before the pivot, and it must not deserialise into anything.
    ///
    /// Pinned here because the deletion is the whole of R3 and a variant added back "to make
    /// the compiler quieter" would restart production without anybody deciding to.
    #[test]
    fn no_render_kind_deserialises_any_more() {
        for dead in [
            "thumbnail",
            "tessellation_l0",
            "tessellation_l1",
            "tessellation_l2",
            "structure",
            "entities",
            "pmi",
        ] {
            serde_json::from_value::<DerivativeKind>(serde_json::Value::from(dead))
                .expect_err(&format!("`{dead}` is not a kind anything builds"));
        }
    }

    #[test]
    fn all_is_every_kind() {
        // `as_str` above is exhaustive, so a new variant is a compile error there and the
        // person who adds it is pointed here. It does not catch an *addition* on its own: a
        // third variant leaves `[DerivativeKind; 2]` two elements long and this assertion
        // still passes. Making the omission itself a compile error needs a variant count the
        // language will not give us on stable — `std::mem::variant_count` is nightly, `strum`
        // is a new dependency, and a `macro_rules!` enum would have to carry `serde`,
        // `ts_rs::TS` and `#[ts(export)]` through the macro, putting `cargo xtask
        // export-bindings` in the blast radius of a two-variant enum. Not worth it; add the
        // variant here when you add it above.
        assert_eq!(
            DerivativeKind::ALL,
            [DerivativeKind::ExportStl, DerivativeKind::Export3mf]
        );
    }
}
