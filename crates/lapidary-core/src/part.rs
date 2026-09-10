use crate::{BlobHash, LibraryId, PartId, RevisionId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Governance is opt-in per library. Hobby libraries have no revisions, states or
/// approvals; flipping a library to `Controlled` turns that machinery on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryMode {
    /// No revisions, no states, no approvals. What a library is unless somebody says
    /// otherwise, which is why it is the `Default` — `CLAUDE.md`: governance is opt-in per
    /// library.
    #[default]
    Hobby,
    Controlled,
}

impl LibraryMode {
    /// The value the `library.mode` column holds. One spelling, here, so a writer and a
    /// reader cannot disagree about it — the same rule `DerivativeKind::as_str` follows and
    /// for the same reason: a kind spelled differently on the two sides reads nothing while
    /// looking entirely correct.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hobby => "hobby",
            Self::Controlled => "controlled",
        }
    }
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
    /// The path this part is known by, which since slice 6a is its identity in the
    /// library — two parts called `bracket` in two folders are one name and two paths.
    ///
    /// Carried on the summary and not only on `PartDetail` because the removed list needs
    /// it: that page names a part in a purge confirmation, purge is the one irreversible
    /// action in the product, and a confirmation reading "Purge “bracket” permanently?"
    /// against two identically named parts is a confirmation that cannot be answered
    /// correctly.
    pub source_path: String,
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
    /// The L0 tessellation's content hash — the rung the viewer paints first, and today
    /// the only thing that makes those bytes addressable at all.
    ///
    /// Every ingest writes an L0 glTF, reference-counted and never read, because nothing
    /// carried its hash: `GET /api/blob/{blake3}` had no possible caller and roughly
    /// 7.5 MB per 1,000 parts was written and unreachable. That is the whole reason this
    /// field lands before the viewer that will use it.
    ///
    /// A hash, not a URL, and holding one is not authorization — `blob::by_hash` asks
    /// `PgBlobs::derivative_is_reachable` before serving a byte, exactly as it does for
    /// `thumbnail` above. `None` means this revision has no L0 rung: a part ingested
    /// before the LOD ladder, or one whose derive job has not run.
    pub tessellation_l0: Option<BlobHash>,
    /// The ingested file's size. `u64` here, `number | null` on the wire: serde writes
    /// a JSON number, and ts-rs 12 would otherwise type a 64-bit integer as `bigint`,
    /// which is something `JSON.parse` never produces. The override has to spell the
    /// null half itself — it replaces the whole type, `Option` included. The precision
    /// ceiling this buys is 2^53 bytes, 9 petabytes in one file.
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    /// What this part's file actually occupies on disk, after compression. Read off the
    /// `file` row beside `source_bytes`, not off the shared `blob` row: source bytes are
    /// no longer deduplicated, so a hash can have a compressed copy at the old
    /// content-addressed path and a raw copy in a model directory at the same time, and
    /// the per-hash column could only ever describe one of them — it reported 91,204 bytes
    /// for a 204,800-byte file it had just called uncompressed.
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
    /// The box this part has to fit in, in millimetres, X/Y/Z.
    ///
    /// On the summary and not only on [`PartDetail`] because the grid asks a question the
    /// detail page cannot answer: *which* of these fits. A card showing a triangle count
    /// and nothing else tells a person how heavy a mesh is, which is not a reason anyone
    /// opens a parts library. Every layout the grid now offers reads this — the tile's
    /// overlay, the list row's dimension column, and the height filter that hides what
    /// will not go on the plate — and none of them could exist while the only route to a
    /// bounding box was one request per part.
    ///
    /// `None` where the revision has no measurement: a part whose derive job has not run,
    /// or one ingested before measurement existed. Absent, never zeroed — a zero-size box
    /// is a real (if degenerate) measurement, and reporting one for a part nobody has
    /// measured would put a figure on a card that no one computed.
    ///
    /// Three `f64`s and not a struct, matching [`MeshMeasurements::bbox_mm`] and
    /// [`PartDetail::bbox_mm`] so a dimension means one thing in all three places.
    #[ts(type = "[number, number, number] | null")]
    pub bbox_mm: Option<[f64; 3]>,
    /// Solid volume in cubic millimetres, or `None` where the revision has none.
    ///
    /// Here for the same reason `bbox_mm` is, and it is the figure the list layout sorts a
    /// column by. Millimetres cubed on the wire and displayed in cm³ — the conversion
    /// belongs to the one place that formats it, not to a field every reader would have to
    /// remember to divide.
    ///
    /// ponytail: no per-figure provenance on the card, unlike [`PartDetail`]'s
    /// `Approximate<f64>`. The card carries `approximate` for the whole part, and Phase 1
    /// ingests only meshes so that flag is unconditionally true — every figure a card shows
    /// is tessellated. The trigger to widen this into an `Approximate<f64>` is the first
    /// analytic figure reaching a card, which is Phase 2's STEP ingest: from then on a part
    /// can carry an analytic volume beside a tessellated triangle count, and one flag for
    /// the whole card is wrong about one of them whichever way it is set.
    #[ts(type = "number | null")]
    pub volume_mm3: Option<f64>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Would this relative source path leave the directory it is relative to?
///
/// Two callers ask, for two different failures, which is why the predicate lives here
/// rather than beside either of them. `lapidary-ingest` joins the path onto the ingest
/// mount, and `Path::join` resolves nothing and refuses nothing: `ingest_dir.join(
/// "/etc/passwd")` *is* `/etc/passwd`, and `../../etc/passwd` walks out of the mount.
/// `lapidary-api`'s upload route joins nothing at all — it writes the string into
/// `part.source_path`, from where the download route spells it into a
/// `Content-Disposition` filename. Different surface, same refusal, and a guard written
/// on one caller is a guard the other never gets.
///
/// A Windows-style prefix (`C:\`, `\\server\share`) is caught by `is_absolute` on
/// Windows and is a harmless literal filename elsewhere.
///
/// Empty escapes too. It joins to the ingest directory itself, which reads as a
/// directory and fails later with a confusing I/O error instead of the real reason, and
/// as a `source_path` it names a part nobody can identify.
pub fn path_escapes(source_path: &str) -> bool {
    let path = std::path::Path::new(source_path);
    source_path.is_empty()
        || path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
}

/// The source format, lowercase and without a dot, taken from a source path.
///
/// Two crates ask now. `lapidary-ingest` writes the answer into `file.format` and hands
/// it to the kernel; `lapidary-api`'s upload route asks `Compression::for_source_format`
/// whether the bytes are worth compressing. Nothing forces those two to agree — the level
/// actually used is recorded on the `blob` row and read back from there — but one
/// definition is cheaper than a second that only looks the same.
///
/// An extension the kernel has no parser for reaches `process` and comes back as a
/// per-file `Permanent` failure naming the format; the scan admits only `stl`, `obj` and
/// `3mf`, so that path is reachable today only through an upload or a hand-written job.
pub fn source_format(source_path: &str) -> String {
    std::path::Path::new(source_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod path_tests {
    use super::path_escapes;

    #[test]
    fn an_ordinary_nested_path_does_not_escape() {
        assert!(!path_escapes("brackets/steel/LP-1042-03.stl"));
        assert!(!path_escapes("bracket.stl"));
        // A leading `./` is a CurDir component, which resolves to the same directory.
        assert!(!path_escapes("./bracket.stl"));
        // `..` as part of a name is not a `..` component.
        assert!(!path_escapes("v..2/bracket.stl"));
    }

    #[test]
    fn absolute_and_parent_paths_escape() {
        assert!(path_escapes("/etc/passwd"));
        assert!(path_escapes("../../etc/passwd"));
        assert!(path_escapes("brackets/../../etc/passwd"));
        assert!(path_escapes(".."));
    }

    #[test]
    fn the_empty_path_escapes() {
        assert!(path_escapes(""));
    }
}
