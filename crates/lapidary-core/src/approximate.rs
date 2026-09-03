use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Wraps a measured value with whether it came from an analytic B-rep entity or from
/// tessellated geometry.
///
/// This is a type rather than a UI convention because `CLAUDE.md` makes it
/// non-negotiable: mesh-derived measurements are labelled "approximate" in the UI,
/// always. Making the flag inseparable from the value means a caller cannot render one
/// without the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Approximate<T> {
    value: T,
    approximate: bool,
}

impl<T> Approximate<T> {
    /// A value read from an analytic B-rep entity. Safe to present as exact.
    pub fn analytic(value: T) -> Self {
        Self {
            value,
            approximate: false,
        }
    }

    /// A value derived from tessellated geometry. The UI must label it.
    ///
    /// Named for its provenance rather than `approximate`, which would collide with the
    /// type name and trip `clippy::self_named_constructors` under `-D warnings`. The
    /// provenance is the more useful name at the call site anyway.
    pub fn tessellated(value: T) -> Self {
        Self {
            value,
            approximate: true,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn is_approximate(&self) -> bool {
        self.approximate
    }
}
