//! What a person decided about two parts that look alike (Phase 6; design in `docs/goals/phase-6.md`).
//!
//! The roadmap's "merge" is called **fold into** here, because CLAUDE.md reserves "no merge" for
//! versioning: folding soft-removes a duplicate and records which part it was folded into. Nothing
//! is moved and nothing is deleted; Restore undoes it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One decision about a pair of parts, as `part_link.kind` stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PartLinkKind {
    /// They belong together — a left and a right hand, a 20 mm and a 30 mm of one design. Shown on
    /// both parts, and never proposed as duplicates again.
    Variant,
    /// Not the same, whatever the shapes say. Never proposed again.
    Distinct,
    /// This part was folded into the other: soft-removed as its duplicate. Restore undoes it.
    FoldedInto,
}

impl PartLinkKind {
    /// The value `part_link.kind` holds.
    pub fn as_str(self) -> &'static str {
        match self {
            PartLinkKind::Variant => "variant",
            PartLinkKind::Distinct => "distinct",
            PartLinkKind::FoldedInto => "folded_into",
        }
    }

    /// A stored `part_link.kind`, or `None` for a value this build does not know.
    pub fn parse(stored: &str) -> Option<Self> {
        match stored {
            "variant" => Some(PartLinkKind::Variant),
            "distinct" => Some(PartLinkKind::Distinct),
            "folded_into" => Some(PartLinkKind::FoldedInto),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_stored_value() {
        for kind in [
            PartLinkKind::Variant,
            PartLinkKind::Distinct,
            PartLinkKind::FoldedInto,
        ] {
            assert_eq!(PartLinkKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(PartLinkKind::parse("merged"), None);
    }
}
