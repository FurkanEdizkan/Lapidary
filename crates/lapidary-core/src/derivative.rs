//! The four things a kernel call can produce, and the strings `derivative.kind` holds.

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
}

impl DerivativeKind {
    /// Exactly the strings already in `derivative.kind`. Changing one orphans every row
    /// written before the change.
    pub fn as_str(self) -> &'static str {
        match self {
            DerivativeKind::Thumbnail => "thumbnail",
            DerivativeKind::TessellationL0 => "tessellation_l0",
            DerivativeKind::TessellationL1 => "tessellation_l1",
            DerivativeKind::TessellationL2 => "tessellation_l2",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_strings_match_what_the_database_holds() {
        // These four strings are in `derivative.kind` on every row ever written.
        assert_eq!(DerivativeKind::Thumbnail.as_str(), "thumbnail");
        assert_eq!(DerivativeKind::TessellationL0.as_str(), "tessellation_l0");
        assert_eq!(DerivativeKind::TessellationL1.as_str(), "tessellation_l1");
        assert_eq!(DerivativeKind::TessellationL2.as_str(), "tessellation_l2");
    }
}
