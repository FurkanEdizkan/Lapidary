use crate::{BlobHash, LibraryId, PartId, RevisionId};
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
/// reference, approximate flag, storage figures, timestamps. Still narrow — the open
/// path reads metadata and derivatives only, and never a source file. The storage
/// fields below are `blob` and `file` *rows*, not bytes: describing a source file is
/// not opening one, and spec §4 wants the cost of a part legible on its card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartSummary {
    pub id: PartId,
    pub library: LibraryId,
    /// The revision the rest of this row describes — the latest one, resolved by the
    /// grid query. Carried because a download URL names a revision, not a part: a card
    /// cannot link to `GET /api/revisions/{id}/download` without it, and resolving
    /// "latest" a second time from the frontend could name a different revision than
    /// the numbers beside the link came from.
    pub revision: RevisionId,
    pub name: String,
    pub part_number: Option<String>,
    /// The thumbnail derivative's content hash, not a URL. Holding it is not
    /// authorization to read it — the API still checks tenant and part reachability.
    pub thumbnail: Option<BlobHash>,
    pub triangle_count: Option<u32>,
    /// True when any geometric figure on this part is mesh-derived.
    pub approximate: bool,
    /// The revision's source file, or `None` when it has none. All four fields below
    /// come from one `file` row and the `blob` row it names, so they are absent
    /// together and never disagree.
    ///
    /// A revision without a source file is not something ingest writes today — it is
    /// what a half-repaired database looks like — and such a part still has to appear
    /// in the grid, which is why these are optional rather than zeroed. A part the grid
    /// silently omits is a part its owner cannot find, delete or re-scan.
    pub source_hash: Option<BlobHash>,
    /// The ingested file's size. `u64` here, `number | null` on the wire: serde writes
    /// a JSON number, and ts-rs 12 would otherwise type a 64-bit integer as `bigint`,
    /// which is something `JSON.parse` never produces. The override has to spell the
    /// null half itself — it replaces the whole type, `Option` included. The precision
    /// ceiling this buys is 2^53 bytes, 9 petabytes in one file.
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    /// What those bytes actually occupy on disk, after compression. Read off `blob`
    /// beside `source_bytes` rather than off `file.size_bytes`, which duplicates the
    /// same number and could drift from it.
    #[ts(type = "number | null")]
    pub stored_bytes: Option<u64>,
    /// Whether the stored bytes are a zstd frame — `blob.zstd_level` is `Some(level)`
    /// with `level != 0`, the same predicate `SourceReader::get` decodes on.
    ///
    /// An unrecorded level (`NULL`) reports `false`, not "unknown" — and not because the
    /// download would hand those bytes over raw. It would not: `download.rs` refuses a
    /// `NULL` level outright, 500 naming the blob, per spec §2.5.1 and ruling T1-A, so
    /// the `None` half of that predicate is unreachable from the only caller.
    ///
    /// The reason is smaller. This field's own `None` is already spoken for: it means the
    /// revision has no source row at all, and a third state would put two unrelated facts
    /// on one display field. A `NULL` level is a data error, and the place that reports
    /// one is the route that has to refuse to act on it, not a card in a grid.
    ///
    /// That state is reachable in production, not only from a test's `UPDATE`:
    /// `link_existing` does not rewrite an existing `blob` row, and tessellation blobs
    /// are written with `zstd_level NULL`, so ingesting bytes byte-identical to a
    /// derivative writes a `role = 'source'` file row over a `NULL`-level blob.
    pub compressed: Option<bool>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
