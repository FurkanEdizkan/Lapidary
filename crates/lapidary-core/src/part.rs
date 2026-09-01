use crate::{BlobHash, LibraryId, PartId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Governance is opt-in per library. Hobby libraries have no revisions, states or
/// approvals; flipping a library to `Controlled` turns that machinery on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryMode {
    Hobby,
    Controlled,
}

/// The grid row, in the shape the spec calls for: identity, part number, thumbnail
/// reference, approximate flag, timestamps. Deliberately narrow — the open path reads
/// metadata and derivatives only, never a source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartSummary {
    pub id: PartId,
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    /// The thumbnail derivative's content hash, not a URL. Holding it is not
    /// authorization to read it — the API still checks tenant and part reachability.
    pub thumbnail: Option<BlobHash>,
    pub triangle_count: Option<u32>,
    /// True when any geometric figure on this part is mesh-derived.
    pub approximate: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
