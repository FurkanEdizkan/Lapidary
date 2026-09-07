use crate::DbError;
use crate::folders::constraint_of;
use lapidary_core::{
    BlobHash, DerivativeKind, FolderId, LibraryId, MeshMeasurements, PartId, PartImageId,
    PartSourceId, PartSummary, Provenance, RevisionId,
};
use sqlx::PgPool;
use uuid::Uuid;

/// A grid row paired with its thumbnail bytes. Not `PartSummary` on its own:
/// `PartSummary.thumbnail` is a content hash for a future hash-addressed thumbnail
/// endpoint, and slice 1 has no such endpoint — it stores the WebP inline as `bytea`
/// and the grid renders it directly. `PgParts::page` already fetches that column (it
/// is a `LEFT JOIN` away from every row it reads regardless), so the bytes ride along
/// here rather than being fetched and thrown away.
#[derive(Debug)]
pub struct PartRow {
    pub summary: PartSummary,
    pub thumbnail_webp: Option<Vec<u8>>,
    /// The model's own directory in the store, relative to the storage root — the parent
    /// of `file.storage_path`, never an absolute host path, because the api runs in a
    /// container and the path the container sees is not the path the user's file manager
    /// would open.
    ///
    /// `None` means the same thing `storage_path` being NULL means: this part predates the
    /// folder layout and its bytes are still content-addressed, with no directory of its
    /// own to show or to rename. Here for the same reason `thumbnail_webp` is — it is a
    /// wire concern of the grid, not a fact `PartSummary` should carry into the viewer.
    pub directory: Option<String>,
    /// `file.storage_path` itself — the directory above with the model's filename back on.
    ///
    /// Both, rather than one and a split: the directory is what a *move* renames, and the
    /// full path is what the card *shows*. Deriving either from the other in the client
    /// would put a second definition of "the parent of a model file" in TypeScript beside
    /// [`model_directory`]'s here, and the client is the side that cannot know when the
    /// filename was disambiguated.
    pub storage_path: Option<String>,
}

/// Everything a `derive` job needs to re-read a revision's source bytes.
///
/// Two ways of naming the same file, and which one applies is `storage_path`'s
/// nullability: a row written since ingest started writing model directories carries the
/// path the bytes are actually at, and a row from before that carries NULL, meaning they
/// are still at `blobs/ab/cd/<hash>`. Migration `0009` states that rule, and it stays
/// true until the `migrate_storage` job has drained every library.
#[derive(Debug)]
pub struct RevisionSource {
    pub hash: BlobHash,
    /// `file.format` — lowercase, no dot. What the kernel is asked to parse.
    pub format: String,
    /// `file.storage_path`. `None` means the bytes are still content-addressed.
    pub storage_path: Option<String>,
    /// `blob.zstd_level` exactly as stored, for the same reason [`DownloadSource`] keeps
    /// it: a reader must follow the level that was recorded when the bytes were written,
    /// never re-derive one from the format.
    pub zstd_level: Option<i16>,
}

/// Everything the download route needs about a revision's source file, in one row.
///
/// Five columns off three tables, so it is a struct rather than a tuple: `format`,
/// `part_name`, `storage_path` and the hex hash are all text, and a tuple of them is
/// positions a call site can silently transpose into a file served under the wrong name.
#[derive(Debug)]
pub struct DownloadSource {
    pub hash: BlobHash,
    /// `file.size_bytes` — the *uncompressed* length, which is what the route sends as
    /// `Content-Length` and therefore what the user is promised. Read off `file` rather
    /// than the `blob` row it duplicates so that this query needs no `blob` join at all:
    /// since migration `0013` every other column it reads lives on `file`, and a join kept
    /// only for a duplicated value is a second table to keep in step for nothing.
    ///
    /// It matters more since the download began streaming: the body is no longer
    /// verified before its first byte goes out, so a declared length is what turns a
    /// mid-stream abort into a transfer the client can see was short rather than a file
    /// that merely looks complete.
    pub size_bytes: i64,
    /// `file.format` — lowercase, no dot. The route synthesizes `{part_name}.{format}`.
    pub format: String,
    /// `part.name`, the download's filename stem. A renamed part downloads under its new
    /// name, which is the design decision spec §2.4 records; the byte-identity claim is
    /// about bytes, not labels.
    pub part_name: String,
    /// `file.storage_path`. Same nullability, same meaning, as [`RevisionSource::storage_path`]:
    /// `Some` names where the bytes actually sit, relative to the storage root; `None`
    /// means this row predates the folder tree and the bytes are still content-addressed.
    /// Migration `0009`'s comment states the rule and how long it holds — for as long as
    /// `migrate_storage` takes to drain every library, which is hours on a real corpus.
    pub storage_path: Option<String>,
    /// `file.zstd_level` exactly as stored, `None` and all. Never `COALESCE`d to 0 — but
    /// not for the reason ruling T1-A first gave, which was wrong and is retracted here:
    /// a `COALESCE` could not serve a zstd frame as the file, because
    /// `SourceReader::get` decodes on `is_some_and(|level| level != 0)` and reads `None`
    /// and `Some(0)` identically. The two are byte-identical on the wire.
    ///
    /// The real reason is smaller. NULL means nobody recorded how these bytes were
    /// written, matching the nullable column is less code than erasing it, and the
    /// unknown is worth keeping because the route can then *say* so: spec §2.5.1 refuses
    /// an unrecorded level with a message naming the blob, instead of reading raw bytes
    /// and falling through to a hash mismatch that explains nothing.
    ///
    /// The hazard the retracted wording described is real but belongs to spec §2.7: a
    /// *recorded* `0` written over zstd bytes during slice 7's rewrite window. Nothing
    /// about `None` produces it.
    ///
    /// `file`, not `blob`, since migration `0013`. The per-hash column could not describe a
    /// hash that has a zstd-3 legacy copy and a raw model file at once, which is every hash
    /// a scan re-meets during the migration window.
    pub zstd_level: Option<i16>,
}

/// What one library occupies, by storage class. Bytes on disk, not ingested sizes — see
/// [`PgParts::storage_totals`], which is the only thing that builds one.
///
/// Exactly what is counted, because the two halves are counted differently and a reader
/// comparing this panel to `du` needs to know which difference they are looking at:
///
/// - **`source_bytes`** — one file per part, at the size that file occupies. Source bytes
///   are path-addressed and uncompressed since the store became a folder tree, so two
///   parts holding identical bytes are two files and are counted twice. Deduplication of
///   source bytes is gone by design (spec §0), and a total that still deduplicated them
///   would under-report a duplicated library by the duplication factor.
/// - **`derivative_bytes`** — rungs on disk, deduplicated, plus inline thumbnails from
///   Postgres. Derivatives are still content-addressed and genuinely shared, so bytes two
///   revisions point at are counted once, which is what `du` would report for them.
///
/// **Not counted: `metadata.json`.** This is a total of the files a library's *models* are
/// made of, not of every byte in its directory tree. Each manifest is on the order of a
/// kilobyte against a model's megabytes — 1,614 of them on the owner's measured corpus is
/// under 0.01% of it — and counting them would mean recording each manifest's length in a
/// column written for the purpose, for a figure nobody would see move. Named here rather
/// than left silent, because the gap is real and someone will eventually run `du`.
///
/// One caveat with an end date: a `file` row whose `storage_path` is still NULL has bytes
/// at the old content-addressed path, possibly compressed, and is counted at its
/// uncompressed size. Those rows over-report until the `migrate_storage` job drains them,
/// and the job's completion is what closes it.
///
/// No ratio here: it is one division over these two numbers, and a third field carrying
/// it would be a second place for the same fact to be computed differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageTotals {
    pub source_bytes: u64,
    pub derivative_bytes: u64,
    /// What this library's *removed* parts still occupy.
    ///
    /// Zero until somebody removes something, and the reason it exists at all is that the
    /// two figures above deliberately exclude soft-deleted parts: without this, removing a
    /// part would drop the panel's total by its size while the disk was unchanged, which
    /// is the panel telling a user bytes were freed. `CLAUDE.md` forbids exactly that
    /// reading, and `strings.removal` is written around never producing it.
    ///
    /// A blob shared with a live part is counted here *and* above, because it is genuinely
    /// serving both and purging the removed part would not reclaim it.
    pub removed_bytes: u64,
}

/// What the whole store holds, across every library — the figure a person means by "how
/// much space is this taking".
///
/// Four numbers rather than one, because they behave differently and a single total would
/// hide the two that surprise people: `removed_bytes` is still on the disk and comes back
/// if the part is restored, and `quarantined_bytes` is on the disk *and* belongs to no
/// library, so it appears in no other panel in the application.
///
/// See [`PgParts::instance_storage`] for what is deliberately not counted, and why the
/// route serves a real directory walk beside these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceStorage {
    /// Every live model file, at what it occupies. One file per model, never deduplicated —
    /// source dedup ended with the folder tree (`DATA.md` §1.1).
    pub source_bytes: u64,
    /// Rungs **on the storage volume**, each blob counted once however many revisions or
    /// libraries point at it.
    ///
    /// Kept apart from `inline_preview_bytes` below, and the reason is arithmetic rather
    /// than taste: adding the two gives a figure that is larger than the storage folder,
    /// because half of it is not in the storage folder. A caller comparing that against a
    /// directory walk finds the tracked total exceeding the disk, which reads as bytes
    /// having gone missing.
    pub derivative_bytes: u64,
    /// Thumbnails, which live in Postgres as `bytea` and not on the volume at all.
    ///
    /// A deliberate exception (`DATA.md` §1.5): a page of cards is one query and no
    /// per-card round trip because the previews come back with the row. They are real bytes
    /// and they cost real space — just not the space a `du` of the storage root measures.
    pub inline_preview_bytes: u64,
    /// What soft-deleted parts still occupy. Nothing has left the disk; a restore brings
    /// them back, and only a purge starts the clock that removes them.
    pub removed_bytes: u64,
    /// Purged, inside the thirty-day hold, on the disk and belonging to no library. The one
    /// figure this application can report nowhere else.
    pub quarantined_bytes: u64,
}

/// Which side of `deleted_at` a page reads.
///
/// A parameter rather than a second method, because the two pages are the same query with
/// one predicate flipped and a duplicate of four LATERALs would drift the moment either
/// side gained a column. A parameter rather than a `bool` for the ordinary reason: `page(
/// library, after, limit, true)` does not say what `true` selects, and this is a call site
/// where guessing wrong shows a user the wrong parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shows {
    /// The library as it stands. What the grid asks for.
    Live,
    /// Parts a person removed and has not purged. The only route back to a soft-deleted
    /// part — without it, delete is a trap, because every other read path filters these
    /// out and nothing would list an id to restore.
    Removed,
}

/// What a part looks like to the move route, before it moves.
///
/// The three location facts kept apart, exactly as migration `0009` insists: `folder` is
/// the mutable category, `directory`/`storage_path` are where the bytes actually sit, and
/// `source_path` — the immutable identity — is deliberately absent, because a move must
/// never read it, let alone write it.
#[derive(Debug, Clone)]
pub struct MoveSource {
    pub library: LibraryId,
    pub folder: Option<FolderId>,
    /// `part.name` — what a colliding sibling in the target category would share.
    pub name: String,
    /// `file.storage_path` for the latest revision's source file, `None` while the part is
    /// still content-addressed and the storage migration has not reached it.
    pub storage_path: Option<String>,
    /// The parent of `storage_path`: the model's own directory, which is what a move
    /// renames. `None` for exactly the same rows.
    pub directory: Option<String>,
    /// The source hash, which `disambiguate` needs to name a colliding destination
    /// directory the same way ingest would.
    pub source_hash: Option<BlobHash>,
}

/// One row of a part's move history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    pub from_folder: Option<FolderId>,
    pub to_folder: Option<FolderId>,
    pub moved_at: jiff::Timestamp,
}

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, newest first. `after` is the previous page's last
    /// id.
    ///
    /// `folder` filters to one category **and everything under it**; `None` is the whole
    /// library. Subtree-inclusive rather than exact-match because the sidebar's parent
    /// categories are real places a user clicks: a tree that showed nothing for `Terrain`
    /// while `Terrain/Rocks` held forty models would be hiding its own contents, and the
    /// count beside the row would contradict the grid next to it.
    async fn page(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        after: Option<lapidary_core::PartId>,
        limit: u16,
        shows: Shows,
    ) -> Result<Vec<PartRow>, DbError>;

    /// The same grid, filtered to a text query and ordered by relevance.
    ///
    /// A method and not a sixth parameter on [`Self::page`]: that one has around
    /// twenty-five call sites across three crates' tests, and widening it would be
    /// twenty-five mechanical `None`s bought for nothing. The two share their columns,
    /// their LATERALs and their decoder, which is where sharing actually matters.
    ///
    /// Same keyset contract: `after` is still a `PartId`, and the rank behind it is
    /// recomputed inside the query rather than carried on the wire. A float in a cursor
    /// drifts by one ULP and silently skips or repeats a row.
    async fn search(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: &str,
        after: Option<lapidary_core::PartId>,
        limit: u16,
        shows: Shows,
    ) -> Result<Vec<PartRow>, DbError>;
}

/// Mirrors `lapidary_storage::StoredBlob`. Not imported: both crates are L1, and
/// `cargo xtask check-layers` forbids L1 -> L1. The api layer converts between them.
pub struct StoredBlobRow {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

/// One LOD rung as it is stored.
///
/// Carries a whole `StoredBlobRow` rather than just a hash because every rung needs a
/// `blob` row of its own before `derivative_blake3_references_blob` will accept the
/// derivative that points at it, and that row needs the sizes.
pub struct TessellationRow<'a> {
    /// `derivative.kind` — `tessellation_l0`, `_l1` or `_l2`.
    pub kind: &'a str,
    pub blob: StoredBlobRow,
    /// Cells per axis, or `None` for the finest grid. Persisted as `params_json` so that
    /// `kernel_version` and `params_json` together reproduce these exact bytes, which is
    /// what lets a derivative be evicted and regenerated rather than backed up.
    pub grid: Option<u32>,
}

/// Where one derivative's bytes live — the two shapes `derivative` can hold.
///
/// An enum rather than two `Option` fields because `derivative_storage_is_exclusive`
/// (migration `0004`) requires exactly one of `thumb_bytes` and `blake3` to be non-null:
/// a pair of options can also express "both" and "neither", and each of those is a
/// constraint violation discovered at runtime instead of a shape the caller cannot write.
pub enum DerivativeBytes<'a> {
    /// Inline in `thumb_bytes`. How a thumbnail is stored: small enough that the grid
    /// serves it straight out of the row it already reads.
    Inline(&'a [u8]),
    /// Hash-addressed in `blake3`. How a tessellation rung is stored — the `blob` row is
    /// written by the same call, because `derivative_blake3_references_blob` refuses a
    /// derivative naming bytes the `blob` table has never heard of.
    Hashed {
        blob: &'a StoredBlobRow,
        /// Cells per axis, or `None` for the finest grid — see [`TessellationRow::grid`].
        grid: Option<u32>,
    },
}

/// `params_json` for a thumbnail, and for a rung.
///
/// Built here rather than by each caller because `kernel_version` and `params_json`
/// together are what let a derivative be evicted and regenerated instead of backed up: a
/// re-render must record the same parameters the original ingest did, and it can only do
/// that if both writers read the shape off the same line.
fn thumbnail_params() -> serde_json::Value {
    serde_json::json!({ "px": 512 })
}

fn rung_params(grid: Option<u32>) -> serde_json::Value {
    serde_json::json!({ "grid": grid })
}

pub struct IngestRequest<'a> {
    pub library: LibraryId,
    pub name: &'a str,
    /// Where the file sat, relative to the library's ingest root, `/`-separated.
    ///
    /// This — not `name` — is what identifies a part inside a library, and
    /// `part_source_path_unique_per_library` (migration `0007`) is what enforces it. Once
    /// the scan descends, two folders may each hold a `bracket.stl`; they are two parts
    /// with one name, and only the path tells them apart.
    pub source_path: &'a str,
    /// The category this model lands in. `None` is the library root.
    ///
    /// Location, never identity — the third column beside `source_path` and
    /// `storage_path`, and the only one of the three a user can change afterwards
    /// (migration `0009`'s header states all three).
    pub folder: Option<FolderId>,
    /// Where the bytes were written, relative to the storage root. Distinct from
    /// `source_path`: that names a directory we only ever read, this names one we own.
    ///
    /// `None` writes the column NULL, which has the meaning `0009` gives it — *the bytes
    /// are still at the old content-addressed path*. A caller that wrote through
    /// `SourceStore::put` rather than `put_at` says `None` and is telling the truth; an
    /// empty string would be a path that exists nowhere, and every reader would then have
    /// to know that `""` secretly means NULL.
    pub storage_path: Option<&'a str>,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    /// The rendered preview, or `None` when nothing rendered one — a library with
    /// `auto_thumbnail = false`, or a kernel that was not asked for a thumbnail.
    ///
    /// `None` writes no `derivative` row at all, rather than a row holding no bytes. An
    /// empty `bytea` is not NULL, so it satisfies `derivative_storage_is_exclusive`
    /// (migration `0004`), reads back as `Some(vec![])`, and reaches the grid as
    /// `data:image/webp;base64,` — a broken image where "no preview yet" belongs. The
    /// absent row is what the grid's `LEFT JOIN LATERAL` is already written to handle.
    pub thumbnail_webp: Option<&'a [u8]>,
    pub kernel_version: &'a str,
    /// The source format, lowercase and without a dot. Was the SQL literal `'stl'`.
    pub format: &'a str,
    /// L0, L1 and L2. Empty is legal and means the caller wrote no rungs — the schema
    /// does not require them, and a revision without them still shows a thumbnail.
    pub tessellations: &'a [TessellationRow<'a>],
}

pub struct PgBlobs(pub PgPool);

impl PgBlobs {
    /// Content addressing is not authorization: this only tells the caller whether the
    /// bytes are already held, never whether the caller may read them.
    ///
    /// It is therefore a *global* question, and must never on its own decide a
    /// per-library write. Ingest uses it for exactly one thing — whether the bytes still
    /// have to be written to the blob store — and asks [`PgBlobs::library_holds`] for
    /// anything about a particular library.
    pub async fn exists(&self, hash: &BlobHash) -> Result<bool, DbError> {
        let found: Option<String> = sqlx::query_scalar("SELECT blake3 FROM blob WHERE blake3 = $1")
            .bind(hash.to_hex())
            .fetch_optional(&self.0)
            .await?;
        Ok(found.is_some())
    }

    /// The stored form of a blob: its sizes and the level it was written at.
    ///
    /// `zstd_level` is the reason this exists. A reader that re-derived compression from
    /// the file's extension would hand out zstd frames as though they were the file the
    /// day ingest-time policy changed — `SourceReader::get` says so at length — and the
    /// upload path makes that gap wider than a re-scan ever did: the api chooses the
    /// level, in another process, on a build that may not be this one, and the worker
    /// decodes what it finds one job later.
    ///
    /// `None` means no such row, which the caller must not confuse with bytes it can
    /// read: the row is what makes a blob known.
    pub async fn blob(&self, hash: &BlobHash) -> Result<Option<StoredBlobRow>, DbError> {
        let row: Option<(i64, i64, Option<i16>)> = sqlx::query_as(
            "SELECT size_bytes, stored_bytes, zstd_level FROM blob WHERE blake3 = $1",
        )
        .bind(hash.to_hex())
        .fetch_optional(&self.0)
        .await?;
        Ok(
            row.map(|(size_bytes, stored_bytes, zstd_level)| StoredBlobRow {
                hash: *hash,
                size_bytes: size_bytes as u64,
                stored_bytes: stored_bytes as u64,
                // The column is nullable and reads as uncompressed, exactly like level 0 —
                // `0002_parts.sql`, and the same collapse `SourceReader` makes.
                zstd_level: zstd_level.unwrap_or(0),
            }),
        )
    }

    /// Record bytes that are on disk but that nothing references yet, `ref_count = 0`.
    ///
    /// The upload route's, and only the upload route's. Every other producer of source
    /// bytes writes the blob and the part chain within one call — `PgIngest::record`
    /// inserts this same row inside the transaction that creates the part, and reaps the
    /// bytes if it fails, which is the ordering `docs/prototype-notes.md` exists to
    /// protect.
    ///
    /// Upload cannot do that: the api writes the bytes and the *worker* writes the rows,
    /// one job later. Between the two there are bytes on disk that no `part` points at,
    /// and if that job fails permanently nothing ever reaps them — the failing process
    /// did not write the bytes and must not assume it may delete them. A `blob` row with
    /// a zero count is what makes those bytes *known* rather than lost: slice 7's
    /// reference-counted reaper is defined over exactly that row, and an orphan with no
    /// row at all is invisible to it forever.
    ///
    /// `ON CONFLICT DO NOTHING` for the same reason it is there in `record`: an upload of
    /// bytes some library already holds must not disturb the count on the row that
    /// library's parts are keeping alive.
    pub async fn record_unreferenced(&self, blob: &StoredBlobRow) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $3, $4, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(blob.hash.to_hex())
        .bind(blob.size_bytes as i64)
        .bind(blob.stored_bytes as i64)
        .bind(blob.zstd_level)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Step three of the three: remove bytes nothing has pointed at for `older_than`.
    ///
    /// This is the only code in Lapidary that destroys user data, and it is written on the
    /// assumption that everything upstream of it may be wrong.
    ///
    /// # It does not consult `ref_count`, and the `NOT EXISTS` pair is not why that is safe
    ///
    /// The counter is a hint. It is maintained by arithmetic on the ingest paths, and
    /// [`PgParts::purge`]'s recompute can itself miss a row committed while it waited for a
    /// lock. Both can be wrong, so nothing here reads it.
    ///
    /// What actually makes a referenced blob unremovable is neither this query nor that
    /// counter: it is `file.blake3` and `derivative.blake3`, both foreign keys to `blob`.
    /// A `DELETE` of a referenced row raises a constraint violation whatever this `WHERE`
    /// clause says, and the bytes survive because the statement never succeeds. Written
    /// down because it is easy to believe the clause below is the guard and quietly weaken
    /// it — it is not, and the schema is.
    ///
    /// What the `NOT EXISTS` pair buys is that such a row is *declined* rather than
    /// *failed on*. The whole sweep is one transaction, so one wrongly-quarantined blob
    /// would otherwise roll back every legitimate removal beside it — and would do so
    /// again every hour, forever, since nothing about the bad row heals on its own.
    /// Quarantine would silently stop collecting anything at all. The pair is an
    /// availability property, and the second statement below is what heals the row.
    ///
    /// So: a drifted-high counter means a blob never enters quarantine — wasted disk. A
    /// drifted-low one means a blob enters quarantine it should not have, and this declines
    /// it and clears the flag. Bytes are not lost in either direction.
    ///
    /// # A blob that came back is un-quarantined, whatever its clock says
    ///
    /// Re-ingesting quarantined bytes points a new `file` row at them, and that alone
    /// undoes the quarantine — the second statement below clears the flag for every
    /// reachable blob, not only for ones past the cutoff. Re-ingest un-quarantines by
    /// existing, and a person who deleted something by mistake and re-scanned the folder
    /// does not have to know this column exists.
    ///
    /// # The unlink happens before the commit, and that ordering is load-bearing
    ///
    /// `remove` is called while this transaction still holds the deleted `blob` row, which
    /// is what makes a concurrent re-ingest of the same bytes safe: an ingest linking to
    /// this hash blocks on the row and, once the delete commits, fails its own transaction
    /// on `file.blake3`'s foreign key rather than committing a `file` row for bytes that
    /// have just been unlinked. Its job retries, finds no `blob` row, and writes the bytes
    /// again. Unlinking after the commit would leave that window open.
    ///
    /// The cost of the ordering is the opposite failure: an unlink that succeeds followed
    /// by a commit that does not would leave a row naming bytes that are gone. So a failing
    /// `remove` aborts the whole sweep — the row and the bytes both survive, and the next
    /// hour tries again. Losing bytes is worse than keeping them, every time.
    pub async fn reap(
        &self,
        older_than: std::time::Duration,
        // `String` rather than an error type of the caller's, because this crate must not
        // depend on `lapidary-storage` to describe a failure to unlink a file. The message
        // is the caller's; all this does is carry it out through `DbError`.
        mut remove: impl FnMut(&BlobHash) -> Result<(), String>,
        // The path-addressed half, and a second closure rather than a widened first one:
        // the two name different things (a hash, a store-relative path) and the caller
        // does different work for each — one unlink against three plus a directory.
        mut remove_file: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<ReapReport, DbError> {
        let mut tx = self.0.begin().await?;

        let doomed: Vec<(String, i64)> = sqlx::query_as(
            "DELETE FROM blob b \
             WHERE b.quarantined_at < now() - make_interval(secs => $1) \
               AND NOT EXISTS (SELECT 1 FROM file f WHERE f.blake3 = b.blake3) \
               AND NOT EXISTS (SELECT 1 FROM derivative d WHERE d.blake3 = b.blake3) \
             RETURNING b.blake3, b.stored_bytes",
        )
        .bind(older_than.as_secs_f64())
        .fetch_all(&mut *tx)
        .await?;

        let mut removed = Vec::with_capacity(doomed.len());
        let mut bytes = 0u64;
        for (hex, stored_bytes) in &doomed {
            let hash = BlobHash::parse_hex(hex).map_err(|_| DbError::CorruptBlobHash {
                column: "blob.blake3",
                value: hex.clone(),
            })?;
            remove(&hash).map_err(|message| DbError::ReapRemove {
                hash: hex.clone(),
                message,
            })?;
            removed.push(hash);
            bytes += bytes_column("blob.stored_bytes", *stored_bytes)?;
        }

        // The path-addressed half, in this same transaction and under the same ordering:
        // the unlink happens while the transaction still holds the deleted row, so a crash
        // that loses the commit leaves a row naming bytes that are gone rather than bytes
        // that no row will ever collect.
        //
        // `NOT EXISTS` is the whole guard here, not the politeness in front of one that it
        // is above. `file.blake3` references `blob`, so a referenced blob row cannot be
        // deleted whatever this query says and the check merely lets the sweep decline;
        // nothing references `file.storage_path`, so this clause is the only thing standing
        // between a live part and its bytes.
        let doomed_files: Vec<(String, Option<i64>)> = sqlx::query_as(
            "DELETE FROM quarantined_file q \
             WHERE q.quarantined_at < now() - make_interval(secs => $1) \
               AND NOT EXISTS (SELECT 1 FROM file f WHERE f.storage_path = q.storage_path) \
             RETURNING q.storage_path, q.stored_bytes",
        )
        .bind(older_than.as_secs_f64())
        .fetch_all(&mut *tx)
        .await?;

        let mut removed_files = Vec::with_capacity(doomed_files.len());
        for (path, stored_bytes) in &doomed_files {
            remove_file(path).map_err(|message| DbError::ReapRemove {
                hash: path.clone(),
                message,
            })?;
            removed_files.push(path.clone());
            // A row written before migration `0013` may have no recorded size. It is still
            // removable; it just contributes nothing to the total, which is the same answer
            // `bytes` gives for a figure nobody wrote down.
            if let Some(stored) = stored_bytes {
                bytes += bytes_column("quarantined_file.stored_bytes", *stored)?;
            }
        }

        // A path a live `file` row has claimed since the purge. Dropped rather than
        // un-flagged, which is where this differs from the blob branch below: there is no
        // `ref_count` to correct and no state to go back to, so a record saying these bytes
        // are doomed is simply wrong and goes.
        let revived_files: Vec<String> = sqlx::query_scalar(
            "DELETE FROM quarantined_file q \
             WHERE EXISTS (SELECT 1 FROM file f WHERE f.storage_path = q.storage_path) \
             RETURNING q.storage_path",
        )
        .fetch_all(&mut *tx)
        .await?;

        // The other half, and it is not conditional on the cutoff: bytes somebody pointed
        // at again stop being candidates the moment they are pointed at, not thirty days
        // later. `ref_count` is recomputed here for the same reason purge recomputes it —
        // this is the one sweep that looks at every quarantined blob, so it is the cheapest
        // place to correct a counter that drifted.
        let revived: Vec<String> = sqlx::query_scalar(
            "WITH counts AS ( \
                 SELECT b.blake3, \
                        (SELECT count(*) FROM file f WHERE f.blake3 = b.blake3) \
                      + (SELECT count(*) FROM derivative d WHERE d.blake3 = b.blake3) AS actual \
                 FROM blob b WHERE b.quarantined_at IS NOT NULL \
             ) \
             UPDATE blob b SET quarantined_at = NULL, ref_count = c.actual \
             FROM counts c WHERE b.blake3 = c.blake3 AND c.actual > 0 \
             RETURNING b.blake3",
        )
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(ReapReport {
            removed,
            removed_files,
            bytes,
            un_quarantined: (revived.len() + revived_files.len()) as u32,
        })
    }

    /// Is this derivative reachable — does any part in any library that exists point at
    /// these bytes?
    ///
    /// The open path's authorization check, and it is a product rule rather than a
    /// nicety: content addressing is not authorization, so holding a hash must not by
    /// itself grant the bytes. There is no principal in Phase 1, so "the caller may read
    /// it" reduces to "some library reaches it"; the join is written now, while it is one
    /// query, rather than retrofitted in Phase 8 when it would be a security fix.
    ///
    /// Deliberately unable to distinguish "no such hash" from "on disk but unreferenced".
    /// The caller turns both into the same 404, because a different answer for the second
    /// confirms the bytes exist, which is exactly the capability a hash must not confer.
    pub async fn derivative_is_reachable(&self, hash: &BlobHash) -> Result<bool, DbError> {
        let found: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM derivative d \
             JOIN revision r ON r.id = d.revision_id \
             JOIN part p ON p.id = r.part_id \
             JOIN library l ON l.id = p.library_id \
             WHERE d.blake3 = $1 LIMIT 1",
        )
        .bind(hash.to_hex())
        .fetch_optional(&self.0)
        .await?;
        Ok(found.is_some())
    }

    /// Record that these bytes were just handed to somebody, for the age-based storage
    /// features `docs/superpowers/specs/2026-09-04-phase-1-slice-4-derivatives-design.md`
    /// §3.10 describes — nothing has written `last_accessed_at` since the column was
    /// added, and a column nobody has ever populated is worth nothing at the moment you
    /// first want it.
    ///
    /// Fire-and-forget, and that is why it returns `()` rather than a `Result` a call
    /// site could be tempted to `?`: the caller is midway through serving a read, and a
    /// timestamp that failed to move is not a reason to fail the read. `debug`, not
    /// `warn` — a missed touch costs one blob its place in an eviction ordering and
    /// nothing else, so it is not an incident.
    ///
    /// Deliberately its own statement, hence its own implicit transaction. `now()` is
    /// transaction-*start* time, so folding this into a surrounding transaction to save
    /// a round trip would make two touches of one blob report the same instant and
    /// destroy the ordering an eviction sweep reads.
    ///
    /// Only a deliberate read belongs here. Ingest's reads are the system writing and
    /// regenerating rather than somebody looking at data; counting them would mark every
    /// blob recently used the moment a sweep ran, which is the signal this column exists
    /// to carry.
    pub async fn touch_blob(&self, hash: &BlobHash) {
        if let Err(err) = sqlx::query("UPDATE blob SET last_accessed_at = now() WHERE blake3 = $1")
            .bind(hash.to_hex())
            .execute(&self.0)
            .await
        {
            tracing::debug!(
                hash = %hash.to_hex(),
                error = %err,
                "could not record that a blob was read"
            );
        }
    }

    /// Does `library` already hold a part called `part_name` whose source file is
    /// exactly these bytes? In other words: is this the same file, seen again?
    ///
    /// This is the scoped counterpart to [`PgBlobs::exists`], and the two answer
    /// genuinely different questions. Ingest short-circuits on *this* one, because a
    /// file whose bytes some other library happens to hold is still a part this library
    /// does not have — short-circuiting on the global answer meant scanning a directory
    /// into a second library ingested nothing and reported success.
    ///
    /// Keyed on the source path as well as the hash: two files with identical bytes at
    /// two paths are two parts, and only "same library, same path, same bytes" is a
    /// re-scan.
    ///
    /// Was keyed on `part.name` until slice 6a. The name stopped being unique the moment
    /// the scan learned to descend — `brackets/bracket.stl` and `plates/bracket.stl` share
    /// a stem — so a name-keyed lookup would call the second file a re-scan of the first
    /// and skip it. The key here must agree with
    /// `part_source_path_unique_per_library`; `0003_jobs.sql` records what happens when
    /// the two disagree, and migration `0007` moved both together.
    ///
    /// Deliberately does *not* filter `part.deleted_at`. A part the user deleted stays
    /// deleted — re-scanning the directory it came from must not resurrect it, and
    /// delete is the one action in this product that is always explicit.
    pub async fn library_holds(
        &self,
        library: LibraryId,
        source_path: &str,
        hash: &BlobHash,
    ) -> Result<bool, DbError> {
        let found: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = $1 AND p.source_path = $2 AND f.blake3 = $3 \
             AND f.role = 'source' \
             LIMIT 1",
        )
        .bind(library.as_uuid())
        .bind(source_path)
        .bind(hash.to_hex())
        .fetch_optional(&self.0)
        .await?;
        Ok(found.is_some())
    }
}

/// What [`PgParts::purge`] did, so a caller can say it truthfully.
///
/// The counts are of *blobs entering quarantine*, not of bytes freed, and the
/// difference is the point: purging a part frees nothing today. Its bytes sit where
/// they were for thirty days, reachable by hash and restorable, and only then are they
/// removed. A route that reported "12.4 MB freed" would be describing something that
/// has not happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeReport {
    /// Blobs this purge left with nothing pointing at them.
    pub quarantined: u32,
    /// What those blobs occupy on disk — `stored_bytes`, the compressed figure, since
    /// that is the space that will actually come back.
    pub quarantined_bytes: u64,
}

/// Purging a part that cannot be purged, told apart — a caller has something different
/// to say about each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purged {
    /// No part with that id.
    NoSuchPart,
    /// The part is live. Purge is the *second* step and refuses to be the first: a
    /// single call that both hid a part and destroyed its chain would be the implicit
    /// deletion the product rule forbids.
    NotDeletedYet,
    Done(PurgeReport),
}

/// What one sweep of [`PgBlobs::reap`] did.
///
/// `removed` carries the hashes rather than a count because the operator log wants them:
/// this is the one place bytes leave for good, and "removed 3 blobs" is not something
/// anyone can check afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReapReport {
    /// Blobs whose rows and bytes are both gone.
    pub removed: Vec<BlobHash>,
    /// Model files whose rows and bytes are both gone, by store-relative path — which is
    /// what identifies one, the way a hash identifies a blob. Their `metadata.json` and,
    /// where nothing else was left in it, their directory went too.
    pub removed_files: Vec<String>,
    /// What all of that occupied — `stored_bytes` on both halves, so it is the space
    /// actually recovered rather than the length the bytes decompress to.
    pub bytes: u64,
    /// Quarantined blobs something points at again, and quarantined paths a live `file`
    /// row has claimed. Both clocks are cleared, not paused.
    pub un_quarantined: u32,
}

pub struct PgIngest(pub PgPool);

impl PgIngest {
    /// A new blob: insert it, then the part chain, in one transaction. The caller has
    /// already written the bytes and reaps them if this fails.
    pub async fn record(&self, req: IngestRequest<'_>) -> Result<PartId, DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $3, $4, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(req.blob.hash.to_hex())
        .bind(req.blob.size_bytes as i64)
        .bind(req.blob.stored_bytes as i64)
        .bind(req.blob.zstd_level)
        .execute(&mut *tx)
        .await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// A blob we already hold: skip the blob insert, everything else is identical.
    pub async fn link_existing(&self, req: IngestRequest<'_>) -> Result<PartId, DbError> {
        let mut tx = self.0.begin().await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Write one derivative onto a revision that already exists, replacing whatever was
    /// there. The `derive` job's only writer — ingest writes its derivatives inside
    /// [`insert_part_chain`]'s transaction, and everything produced afterwards arrives
    /// here, one kind at a time.
    ///
    /// `kind` is passed rather than read off `bytes`, because the storage shape does not
    /// name a rung: all three LOD levels are `Hashed`.
    ///
    /// Two shapes are refused before anything is written, and both are refused *here*
    /// rather than at the call sites so that a new caller is covered without having to
    /// know. Each writes a row that is perfectly valid and permanently invisible: `page`
    /// reads a thumbnail only out of `thumb_bytes`, so a hash-addressed one shows as "no
    /// preview yet" — and [`PgParts::revisions_missing`] then *excludes* that revision,
    /// because a row exists, so the sweep never heals it either. An empty `Inline` is the
    /// same failure one step further on: `Some(vec![])` reaches the grid as
    /// `data:image/webp;base64,`, the broken `<img>` `insert_part_chain` already refuses
    /// to write. Failing loudly is the trade `CLAUDE.md` asks for everywhere else.
    pub async fn upsert_derivative(
        &self,
        revision: RevisionId,
        kind: DerivativeKind,
        bytes: DerivativeBytes<'_>,
        kernel_version: &str,
    ) -> Result<(), DbError> {
        match bytes {
            DerivativeBytes::Hashed { .. } if kind == DerivativeKind::Thumbnail => {
                return Err(DbError::ThumbnailNotInline { revision });
            }
            DerivativeBytes::Inline([]) => {
                return Err(DbError::EmptyDerivative {
                    kind: kind.as_str(),
                    revision,
                });
            }
            _ => {}
        }

        let mut tx = self.0.begin().await?;
        // What this (revision, kind) pointed at before, locked so that a concurrent
        // upsert of the same row cannot interleave with the `ref_count` arithmetic below.
        //
        // `FOR UPDATE` locks nothing when the row does not exist yet, so two jobs racing
        // the *first* write of one kind both read `None`; the loser then blocks on
        // `derivative_kind_unique_per_revision` and takes the DO UPDATE arm, incrementing
        // its own blob without decrementing the winner's. That leaves an over-count,
        // which keeps bytes alive that nothing points at — the harmless direction. The
        // direction that matters, an under-count, would have eviction delete bytes a live
        // row still serves, and this ordering cannot produce one.
        let previous: Option<Option<String>> = sqlx::query_scalar(
            "SELECT blake3 FROM derivative WHERE revision_id = $1 AND kind = $2 FOR UPDATE",
        )
        .bind(revision.as_uuid())
        .bind(kind.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let previous_hash = previous.flatten();

        let (thumb_bytes, hash, params) = match bytes {
            DerivativeBytes::Inline(webp) => (Some(webp), None, thumbnail_params()),
            DerivativeBytes::Hashed { blob, grid } => {
                // The blob row first, exactly as the ladder does at ingest: the foreign
                // key means a derivative cannot name bytes the blob table has never heard
                // of, and `ON CONFLICT DO NOTHING` because a rung whose bytes another
                // revision already stored is the ordinary case.
                sqlx::query(
                    "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
                     VALUES ($1, $2, $2, NULL, 0) ON CONFLICT (blake3) DO NOTHING",
                )
                .bind(blob.hash.to_hex())
                .bind(blob.size_bytes as i64)
                .execute(&mut *tx)
                .await?;
                (None, Some(blob.hash.to_hex()), rung_params(grid))
            }
        };

        // Both storage columns are set from `excluded`, never only the one this call
        // fills: an upsert over a row stored the other way that set just `thumb_bytes`
        // would leave the old `blake3` in place, and a row with both non-null trips
        // `derivative_storage_is_exclusive`.
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, blake3, kernel_version, params_json) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (revision_id, kind) DO UPDATE SET thumb_bytes = excluded.thumb_bytes, \
             blake3 = excluded.blake3, kernel_version = excluded.kernel_version, \
             params_json = excluded.params_json",
        )
        .bind(Uuid::now_v7())
        .bind(revision.as_uuid())
        .bind(kind.as_str())
        .bind(thumb_bytes)
        .bind(hash.as_deref())
        .bind(kernel_version)
        .bind(params)
        .execute(&mut *tx)
        .await?;

        // A derivative row is one reference to its bytes, the same way a `file` row is —
        // that is what makes eviction safe. Replacing a hash-addressed rung *moves* the
        // reference: the bytes it used to name lose one, the bytes it now names gain one.
        // Without the decrement the old blob stays counted forever; without the increment
        // the new blob is reapable while a row still serves it.
        //
        // Identical hashes are left alone: re-rendering the same bytes rewrites the row
        // without changing what points at what.
        if previous_hash != hash {
            if let Some(old) = &previous_hash {
                sqlx::query("UPDATE blob SET ref_count = ref_count - 1 WHERE blake3 = $1")
                    .bind(old)
                    .execute(&mut *tx)
                    .await?;
            }
            if let Some(new) = &hash {
                sqlx::query(
                    // Rides along on the row this statement already locks and writes, so
                    // it costs nothing. Without it, a blob somebody re-ingested stays
                    // flagged until the next hourly sweep notices. That is harmless --
                    // the reaper re-checks reachability and declines it -- but it makes
                    // "a referenced blob is never quarantined" true only eventually
                    // rather than continuously, and an operator reading the column
                    // would see a lie for up to an hour.
                    "UPDATE blob SET ref_count = ref_count + 1, quarantined_at = NULL \
                     WHERE blake3 = $1",
                )
                .bind(new)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }
}

async fn insert_part_chain(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &IngestRequest<'_>,
) -> Result<PartId, DbError> {
    let part = PartId::new();
    let revision = Uuid::now_v7();
    let m = req.measurements;
    let tess = Provenance::Tessellated.as_str();
    // Converted — and, deliberately, checked — before the first INSERT below: a
    // triangle count that does not fit `revision.triangle_count`'s 32-bit column (a
    // mesh kernel bug, or corrupt input) must fail before any row is written, not
    // silently wrap to a negative count that a later read (see PgParts::page) would
    // then have to reject anyway. `as i32` here previously wrapped 3_000_000_000 to
    // -1_294_967_296 and stored it without complaint.
    let triangle_count =
        i32::try_from(m.triangle_count).map_err(|_| DbError::TriangleCountTooLarge {
            column: "revision.triangle_count",
            value: m.triangle_count,
        })?;

    sqlx::query(
        "INSERT INTO part (id, library_id, name, source_path, folder_id) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(part.as_uuid())
    .bind(req.library.as_uuid())
    .bind(req.name)
    .bind(req.source_path)
    .bind(req.folder.map(|folder| folder.as_uuid()))
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin, volume, volume_source, \
         surface_area, surface_area_source, bbox_x, bbox_y, bbox_z, bbox_source, \
         triangle_count, is_watertight, units) \
         VALUES ($1, $2, '1', 'ingest', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'mm')",
    )
    .bind(revision)
    .bind(part.as_uuid())
    .bind(m.volume_mm3)
    // No volume means no provenance for one — writing 'tessellated' beside a NULL would
    // claim we measured something we refused to measure.
    .bind(m.volume_mm3.map(|_| tess))
    .bind(m.surface_area_mm2)
    .bind(tess)
    .bind(m.bbox_mm[0])
    .bind(m.bbox_mm[1])
    .bind(m.bbox_mm[2])
    .bind(tess)
    .bind(triangle_count)
    .bind(m.is_watertight)
    .execute(&mut **tx)
    .await?;

    // `zstd_level` and `stored_bytes` are recorded on the file row and not read back off
    // `blob` (migration `0013`): the blob row is per-hash and a hash can have a compressed
    // legacy copy and a raw model file at the same time, all through the migration window.
    // `record` and `link_existing` both arrive here, and both pass what `put_at` reported
    // for the write they just did — so `link_existing` describes its own file instead of
    // inheriting whatever the shared row happened to say.
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, storage_path, \
         zstd_level, stored_bytes) VALUES ($1, $2, 'source', $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.format)
    .bind(req.blob.hash.to_hex())
    .bind(req.blob.size_bytes as i64)
    .bind(req.storage_path)
    .bind(req.blob.zstd_level)
    .bind(req.blob.stored_bytes as i64)
    .execute(&mut **tx)
    .await?;

    // One file inserted above -> one reference. Runs once per call to insert_part_chain,
    // i.e. once per file, whether the blob is new (record) or already held
    // (link_existing) — both paths route through here.
    sqlx::query(
        // Rides along on the row this statement already locks and writes, so
        // it costs nothing. Without it, a blob somebody re-ingested stays
        // flagged until the next hourly sweep notices. That is harmless --
        // the reaper re-checks reachability and declines it -- but it makes
        // "a referenced blob is never quarantined" true only eventually
        // rather than continuously, and an operator reading the column
        // would see a lie for up to an hour.
        "UPDATE blob SET ref_count = ref_count + 1, quarantined_at = NULL \
         WHERE blake3 = $1",
    )
    .bind(req.blob.hash.to_hex())
    .execute(&mut **tx)
    .await?;

    // No thumbnail, no row. Skipped entirely rather than written empty — see
    // `IngestRequest::thumbnail_webp` for why an empty `bytea` is worse than nothing.
    if let Some(thumbnail) = req.thumbnail_webp {
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(revision)
        .bind(DerivativeKind::Thumbnail.as_str())
        .bind(thumbnail)
        .bind(req.kernel_version)
        .bind(thumbnail_params())
        .execute(&mut **tx)
        .await?;
    }

    for rung in req.tessellations {
        // The blob row first: task 1's foreign key means a derivative cannot name bytes
        // the blob table has never heard of. `ON CONFLICT DO NOTHING` because a rung
        // whose bytes another revision already stored is the ordinary case for anything
        // under the L0 budget -- three identical rungs on a small part are one blob.
        //
        // `zstd_level` is NULL and `stored_bytes` equals `size_bytes`: derivatives are
        // never compressed, because they are regenerated rather than kept.
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $2, NULL, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(rung.blob.hash.to_hex())
        .bind(rung.blob.size_bytes as i64)
        .execute(&mut **tx)
        .await?;

        // One derivative inserted below -> one reference, exactly as the source file's
        // increment above works. This is what makes eviction safe: the reap only removes
        // bytes nothing points at.
        sqlx::query(
            // Rides along on the row this statement already locks and writes, so
            // it costs nothing. Without it, a blob somebody re-ingested stays
            // flagged until the next hourly sweep notices. That is harmless --
            // the reaper re-checks reachability and declines it -- but it makes
            // "a referenced blob is never quarantined" true only eventually
            // rather than continuously, and an operator reading the column
            // would see a lie for up to an hour.
            "UPDATE blob SET ref_count = ref_count + 1, quarantined_at = NULL \
             WHERE blake3 = $1",
        )
        .bind(rung.blob.hash.to_hex())
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, blake3, kernel_version, params_json) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(revision)
        .bind(rung.kind)
        .bind(rung.blob.hash.to_hex())
        .bind(req.kernel_version)
        .bind(serde_json::json!({ "grid": rung.grid }))
        .execute(&mut **tx)
        .await?;
    }

    Ok(part)
}

/// A hex column as a `BlobHash`. Refused rather than dropped: a `blake3` column that is
/// not a digest is a corrupt row, and reporting it as "this part has none" would hide the
/// corruption behind a state that looks entirely ordinary.
fn detail_hash(column: &'static str, hex: Option<String>) -> Result<Option<BlobHash>, DbError> {
    hex.map(|hex| {
        BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash { column, value: hex })
    })
    .transpose()
}

fn detail_stamp(column: &'static str, us: i64) -> Result<jiff::Timestamp, DbError> {
    jiff::Timestamp::from_microsecond(us)
        .map_err(|_| DbError::TimestampOutOfRange { column, value: us })
}

/// A provenance column, refused rather than defaulted — and the direction of the refusal
/// is the whole point. Defaulting to `tessellated` would label an analytic figure
/// approximate, which is a needless hedge; defaulting to `analytic` would present a
/// mesh-derived figure as exact, which is the one thing `CLAUDE.md` says a measurement
/// must never do. Neither default is safe, so there is no default.
fn detail_provenance(text: Option<String>) -> Result<Option<Provenance>, DbError> {
    text.map(|t| {
        t.parse::<Provenance>()
            .map_err(|_| DbError::UnknownProvenance { value: t })
    })
    .transpose()
}

/// A `bigint` byte column as `u64`. Never `as u64`: that turns a negative row into 18
/// exabytes on a card instead of saying the row is wrong.
fn bytes_column(column: &'static str, value: i64) -> Result<u64, DbError> {
    u64::try_from(value).map_err(|_| DbError::NegativeByteCount { column, value })
}

/// Everything the detail route shows about one part.
///
/// Wider than `PartRow` because a card and a page answer different questions: a card
/// shows what fits under a thumbnail, and a page shows what a person clicked through to
/// read. The measurement fields arrive as raw value-and-provenance pairs rather than as
/// `Approximate`, because two of them are independently nullable and `lapidary-api` is
/// where the wire shape is decided.
pub struct PartDetailRow {
    pub id: PartId,
    pub library: LibraryId,
    pub revision: RevisionId,
    pub name: String,
    pub part_number: Option<String>,
    /// The part's identity within its library since slice 6a, and the path a scanned or
    /// dropped folder reported for it.
    pub source_path: String,
    pub rev_label: String,
    pub thumbnail_webp: Option<Vec<u8>>,
    pub triangle_count: Option<u32>,
    pub is_watertight: Option<bool>,
    /// All three axes or none. A box missing one axis is not a box, and rendering two of
    /// its three numbers is a measurement that lies by omission.
    pub bbox_mm: Option<[f64; 3]>,
    pub volume_mm3: Option<f64>,
    pub volume_source: Option<Provenance>,
    pub surface_area_mm2: Option<f64>,
    pub surface_area_source: Option<Provenance>,
    pub kernel_version: Option<String>,
    pub source_hash: Option<BlobHash>,
    pub source_format: Option<String>,
    pub source_bytes: Option<u64>,
    pub stored_bytes: Option<u64>,
    pub compressed: Option<bool>,
    pub tessellation_l0: Option<BlobHash>,
    pub tessellation_l0_bytes: Option<u64>,
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
}

/// The detail query's columns, exactly as Postgres hands them back.
///
/// A `FromRow` struct rather than a tuple: sqlx implements `FromRow` for tuples only up
/// to sixteen elements and this query selects twenty-seven, but the better reason is that
/// a tuple of twenty-seven `Option<i64>`s is a shape nobody can read or safely reorder.
/// Matching is by column name, which is why every ambiguous column in the SQL carries an
/// explicit alias.
#[derive(sqlx::FromRow)]
struct DetailColumns {
    part_id: Uuid,
    library_id: Uuid,
    revision_id: Uuid,
    name: String,
    part_number: Option<String>,
    source_path: String,
    rev_label: String,
    thumb_bytes: Option<Vec<u8>>,
    triangle_count: Option<i32>,
    is_watertight: Option<bool>,
    bbox_x: Option<f64>,
    bbox_y: Option<f64>,
    bbox_z: Option<f64>,
    volume: Option<f64>,
    volume_source: Option<String>,
    surface_area: Option<f64>,
    surface_area_source: Option<String>,
    kernel_version: Option<String>,
    source_blake3: Option<String>,
    source_format: Option<String>,
    source_size_bytes: Option<i64>,
    source_stored_bytes: Option<i64>,
    source_zstd_level: Option<i16>,
    l0_blake3: Option<String>,
    l0_stored_bytes: Option<i64>,
    created_us: i64,
    updated_us: i64,
}

/// The sixteen columns a card is made of.
///
/// A `const` and not two copies, because `page` and `search` must select the same list in
/// the same order or [`to_part_row`]'s positional tuple decodes one query's columns into
/// another query's fields — a failure that type-checks. `repo.rs`'s LATERAL comment already
/// argues this about the joins; two hand-copied SELECT lists would be worse, because the
/// tuple's safety would then *depend* on nobody editing one without the other.
///
/// A macro rather than a `const`, and only because `concat!` takes literals: a `const &str`
/// is not one. The effect is what a const would have given — one definition, spliced at
/// compile time, no string building anywhere near a query.
macro_rules! grid_columns {
    () => {
        "p.id, p.library_id, r.id, p.name, p.part_number, p.source_path, \
     d.thumb_bytes, r.triangle_count, \
     s.blake3, s.size_bytes, s.stored_bytes, s.zstd_level, s.storage_path, l0.blake3, \
     (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
     (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us"
    };
}

/// The four LATERALs behind those columns: the latest revision, its thumbnail, its L0 rung
/// and its source file.
///
/// Shared for the same reason as [`GRID_COLUMNS`], and named separately because search puts
/// a join between the two — its candidates come from a CTE, and the LATERALs then run for
/// the page it kept rather than for every row that matched.
macro_rules! grid_laterals {
    () => {
        "\
     JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
     LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = $4 ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
     LEFT JOIN LATERAL (SELECT blake3 FROM derivative WHERE revision_id = r.id AND kind = $5 ORDER BY created_at DESC, id DESC LIMIT 1) l0 ON true \
     LEFT JOIN LATERAL (SELECT f.blake3, f.storage_path, f.size_bytes, f.stored_bytes, f.zstd_level \
                        FROM file f \
                        WHERE f.revision_id = r.id AND f.role = 'source' \
                        ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true"
    };
}

pub struct PgParts(pub PgPool);

impl PgParts {
    /// Everything the detail route shows about one part, in one query.
    ///
    /// Four LATERALs, the same shape and the same reasons as `page`'s: the revision
    /// because a part may carry several, the thumbnail and the L0 rung because a revision
    /// carries derivatives of different kinds and a plain join would fan out on them, and
    /// the source because a revision missing its `file` row is exactly the part whose
    /// owner most needs to open its page. The source LATERAL's `role = 'source'` filter
    /// and its ordering are character for character `page`'s and `source_for_download`'s,
    /// so the figures on a card, the figures on its detail page, and the bytes behind its
    /// download link all describe one `file` row — `file` has no unique constraint on
    /// `(revision_id, role)`, so that agreement is a choice rather than a property of the
    /// schema.
    ///
    /// `None` is "no such part, or it is deleted", undistinguished, because the caller
    /// turns both into one 404 — telling them apart would confirm that a part exists to
    /// someone who cannot see it.
    pub async fn detail(&self, part: PartId) -> Result<Option<PartDetailRow>, DbError> {
        let row: Option<DetailColumns> = sqlx::query_as(
            "SELECT p.id AS part_id, p.library_id, r.id AS revision_id, p.name, \
                    p.part_number, p.source_path, r.rev_label, \
                    d.thumb_bytes, d.kernel_version, \
                    r.triangle_count, r.is_watertight, r.bbox_x, r.bbox_y, r.bbox_z, \
                    r.volume, r.volume_source, r.surface_area, r.surface_area_source, \
                    s.blake3 AS source_blake3, s.format AS source_format, \
                    s.size_bytes AS source_size_bytes, \
                    s.stored_bytes AS source_stored_bytes, \
                    s.zstd_level AS source_zstd_level, \
                    l0.blake3 AS l0_blake3, l0.stored_bytes AS l0_stored_bytes, \
                    (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
                    (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us \
             FROM part p \
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = $2 ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             LEFT JOIN LATERAL (SELECT dv.blake3, b.stored_bytes FROM derivative dv \
                                JOIN blob b ON b.blake3 = dv.blake3 \
                                WHERE dv.revision_id = r.id AND dv.kind = $3 \
                                ORDER BY dv.created_at DESC, dv.id DESC LIMIT 1) l0 ON true \
             LEFT JOIN LATERAL (SELECT f.blake3, f.format, b.size_bytes, b.stored_bytes, b.zstd_level \
                                FROM file f JOIN blob b ON b.blake3 = f.blake3 \
                                WHERE f.revision_id = r.id AND f.role = 'source' \
                                ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
             WHERE p.id = $1 AND p.deleted_at IS NULL",
        )
        .bind(part.as_uuid())
        // Off `DerivativeKind`, never a literal, for the reason `page` gives: a reader
        // spelling a kind differently from the writer reads nothing while looking
        // entirely correct.
        .bind(DerivativeKind::Thumbnail.as_str())
        .bind(DerivativeKind::TessellationL0.as_str())
        .fetch_optional(&self.0)
        .await?;

        let Some(c) = row else { return Ok(None) };
        let source_hash = detail_hash("file.blake3", c.source_blake3)?;
        Ok(Some(PartDetailRow {
            id: PartId::from_uuid(c.part_id),
            library: LibraryId::from_uuid(c.library_id),
            revision: RevisionId::from_uuid(c.revision_id),
            name: c.name,
            part_number: c.part_number,
            source_path: c.source_path,
            rev_label: c.rev_label,
            thumbnail_webp: c.thumb_bytes,
            triangle_count: c
                .triangle_count
                .map(|t| {
                    u32::try_from(t).map_err(|_| DbError::NegativeTriangleCount {
                        column: "revision.triangle_count",
                        value: t,
                    })
                })
                .transpose()?,
            is_watertight: c.is_watertight,
            bbox_mm: match (c.bbox_x, c.bbox_y, c.bbox_z) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            },
            volume_mm3: c.volume,
            volume_source: detail_provenance(c.volume_source)?,
            surface_area_mm2: c.surface_area,
            surface_area_source: detail_provenance(c.surface_area_source)?,
            kernel_version: c.kernel_version,
            source_hash,
            source_format: c.source_format,
            source_bytes: c
                .source_size_bytes
                .map(|v| bytes_column("blob.size_bytes", v))
                .transpose()?,
            stored_bytes: c
                .source_stored_bytes
                .map(|v| bytes_column("blob.stored_bytes", v))
                .transpose()?,
            // Keyed off the source row's presence, never off `zstd_level`'s — see
            // `PartSummary::compressed` for the whole argument.
            compressed: source_hash.map(|_| c.source_zstd_level.is_some_and(|l| l != 0)),
            tessellation_l0: detail_hash("derivative.blake3", c.l0_blake3)?,
            tessellation_l0_bytes: c
                .l0_stored_bytes
                .map(|v| bytes_column("blob.stored_bytes", v))
                .transpose()?,
            created_at: detail_stamp("part.created_at", c.created_us)?,
            updated_at: detail_stamp("part.updated_at", c.updated_us)?,
        }))
    }

    /// Whether this library wants a thumbnail rendered at ingest (migration `0005`,
    /// default true). `None` means there is no such library.
    ///
    /// The caller decides what a missing library means, because only it knows what it was
    /// about to do: a `NULL`-vs-absent distinction collapsed into `false` here would have
    /// a job for a deleted library quietly ingest without a preview instead of saying so.
    pub async fn auto_thumbnail(&self, library: LibraryId) -> Result<Option<bool>, DbError> {
        Ok(
            sqlx::query_scalar("SELECT auto_thumbnail FROM library WHERE id = $1")
                .bind(library.as_uuid())
                .fetch_optional(&self.0)
                .await?,
        )
    }

    /// This library's own directory inside `libraries/`. `None` means there is no such
    /// library, exactly as [`PgParts::auto_thumbnail`] means it.
    ///
    /// Lowercased after slugging, which is the one place in the store where case is
    /// flattened: the seeded library is named `Default` and the layout in the spec (§1)
    /// says `libraries/default/`. Category and model directories keep their case, because
    /// those are names a user typed for a folder they will look at; a library directory is
    /// one level of plumbing above that, and two libraries called `Parts` and `parts`
    /// colliding on a case-insensitive filesystem (macOS, Windows) is a worse outcome than
    /// a lowercase directory name.
    ///
    /// Read rather than stored: `library` has no slug column, and adding one would make
    /// this the second place a library's directory name is decided.
    pub async fn library_slug(&self, library: LibraryId) -> Result<Option<String>, DbError> {
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM library WHERE id = $1")
            .bind(library.as_uuid())
            .fetch_optional(&self.0)
            .await?;
        Ok(name.map(|name| lapidary_core::slug::slugify(&name).to_lowercase()))
    }

    /// Turn this library's ingest-time thumbnail on or off — the write side of
    /// [`PgParts::auto_thumbnail`], and the only statement in this crate that changes a
    /// `library` row. It sits here, beside its own reader, rather than on a `PgLibraries`
    /// newtype that would exist to hold one method and split library access across two
    /// types.
    ///
    /// Returns whether a row matched, because a caller cannot tell the two outcomes apart
    /// from an `Ok(())`: `UPDATE … WHERE id = $1` against an id no library has is a
    /// perfectly successful statement that changes nothing, and a route reporting 200 for
    /// it would tell a person their setting was saved when no such library exists.
    pub async fn set_auto_thumbnail(&self, library: LibraryId, on: bool) -> Result<bool, DbError> {
        let result = sqlx::query("UPDATE library SET auto_thumbnail = $2 WHERE id = $1")
            .bind(library.as_uuid())
            .bind(on)
            .execute(&self.0)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Step one of the three: hide the part, touch no bytes.
    ///
    /// One `UPDATE`, and that it is only one is a property of what came before rather
    /// than luck — [`PartRepository::page`], [`PgParts::detail`], [`PgParts::library_of`],
    /// [`PgBlobs::source_for_download`] and [`PgParts::storage_totals`] all already filter
    /// `deleted_at IS NULL`, so setting the column removes the part from the grid, its own
    /// page, its download and the library's totals at once.
    ///
    /// [`PgBlobs::library_holds`] is the deliberate exception and must stay one: it is
    /// what makes a re-scan of a deleted path a no-op instead of a resurrection. See its
    /// own doc comment.
    ///
    /// `WHERE deleted_at IS NULL` makes this idempotent in the direction that matters —
    /// deleting an already-deleted part reports `false` rather than moving its timestamp
    /// forward and quietly extending how long it has been gone.
    ///
    /// Returns whether a row matched, for [`PgParts::set_auto_thumbnail`]'s reason.
    pub async fn soft_delete(&self, part: PartId) -> Result<bool, DbError> {
        let result =
            sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
                .bind(part.as_uuid())
                .execute(&self.0)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Undo of [`PgParts::soft_delete`], and explicit for the same reason delete is: we do
    /// not un-delete implicitly either. A scan that found the file again will not do this
    /// — a person has to ask.
    ///
    /// Nothing needs restoring but the column. Delete left the revisions, files,
    /// derivatives and blobs exactly where they were, which is the whole point of it being
    /// soft.
    pub async fn restore(&self, part: PartId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE part SET deleted_at = NULL WHERE id = $1 AND deleted_at IS NOT NULL",
        )
        .bind(part.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Step two of the three: remove the part chain, and leave the bytes alone.
    ///
    /// # The reference update recomputes rather than decrementing
    ///
    /// `ref_count` is maintained by arithmetic everywhere else — `+1` per `file` and
    /// `derivative` row in [`insert_part_chain`], moved on rung replacement in
    /// [`PgParts::record_derivative`]. This is the one place that does not do that, and it
    /// is deliberate: a counter maintained only by increments and decrements is a counter
    /// that drifts, and the drift is invisible until something acts on it. The thing that
    /// acts on it is the reaper, and what it does is delete bytes.
    ///
    /// So the destructive path refuses to trust the number. It recomputes each affected
    /// hash from what actually points at it, which makes any accumulated drift self-heal
    /// the moment a purge touches that blob.
    ///
    /// A concurrent ingest of the same bytes is *not* fully excluded by this, and the
    /// honest version is worth writing down. The `UPDATE` takes the same `blob` row lock
    /// ingest's `ref_count + 1` takes, so the two serialize on the write — but the count
    /// itself is computed by a CTE evaluated under the statement's own snapshot, so a
    /// `file` row that another transaction commits while this one waits for the lock can
    /// be missed. The recompute would then land on zero for bytes something does point at.
    ///
    /// That is survivable, and it is survivable by design rather than by luck: reaching
    /// zero sets `quarantined_at`, which starts a thirty-day clock and removes nothing.
    /// [`PgBlobs::reap`] re-asks reachability inside the transaction that would delete the
    /// bytes, and a blob with a `file` row is not removed — it is un-quarantined. The
    /// layering is the point. This statement is allowed to be wrong; the one that destroys
    /// data is not, so it does not rely on this one being right.
    ///
    /// # Order matters twice
    ///
    /// The doomed hashes are collected *before* the chain is deleted, because afterwards
    /// there is no path from the part to its blobs. And the chain comes down child-first:
    /// nothing in `0002_parts.sql` declares `ON DELETE CASCADE`, which is a property worth
    /// keeping — a stray `DELETE FROM part` should fail loudly on a foreign key rather
    /// than quietly take four tables with it.
    pub async fn purge(&self, part: PartId) -> Result<Purged, DbError> {
        let mut tx = self.0.begin().await?;

        // `FOR UPDATE` holds the part row for the whole transaction, so two purges of the
        // same part cannot both pass the gate below and double-count the recompute.
        let state: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT (extract(epoch FROM deleted_at) * 1000000)::bigint FROM part \
             WHERE id = $1 FOR UPDATE",
        )
        .bind(part.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        match state {
            None => return Ok(Purged::NoSuchPart),
            Some(None) => return Ok(Purged::NotDeletedYet),
            Some(Some(_)) => {}
        }

        let doomed: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT h FROM ( \
                 SELECT f.blake3 AS h FROM file f \
                 JOIN revision r ON r.id = f.revision_id WHERE r.part_id = $1 \
                 UNION ALL \
                 SELECT d.blake3 FROM derivative d \
                 JOIN revision r ON r.id = d.revision_id \
                 WHERE r.part_id = $1 AND d.blake3 IS NOT NULL \
             ) hashes",
        )
        .bind(part.as_uuid())
        .fetch_all(&mut *tx)
        .await?;

        // Collected here, beside the hashes, and for the identical reason: after the chain
        // comes down there is no path from the part to its files, and `storage_path` is
        // the only record of where a migrated part's bytes are. `blob` cannot stand in for
        // it -- one hash, several model files (migration `0014`'s header states why).
        //
        // No `role` filter, unlike `storage_totals` and the grid's source LATERAL. Those
        // ask "what is this part's source file"; this asks "what has this part put on
        // disk", and a row carrying a path is a file in a model directory whatever its
        // role. Only `source` rows have one today, so the clause is inert now and right
        // when that stops being true.
        //
        // A part with no `storage_path` on any row writes nothing here. That is the
        // un-migrated case and the blob quarantine below already covers it: the two are
        // complementary, not alternatives.
        #[allow(clippy::type_complexity)]
        let doomed_files: Vec<(String, String, Option<i64>)> = sqlx::query_as(
            "SELECT f.storage_path, f.blake3, f.stored_bytes \
             FROM file f JOIN revision r ON r.id = f.revision_id \
             WHERE r.part_id = $1 AND f.storage_path IS NOT NULL",
        )
        .bind(part.as_uuid())
        .fetch_all(&mut *tx)
        .await?;

        for statement in [
            // The gallery and the provenance, both children of `part` and both added a
            // slice after this list was written -- the same shape as `part_move` below,
            // and this time the catalogue test caught them at the moment they were created
            // rather than a running stack catching them at the moment somebody purged.
            //
            // `part_image` first: its `blake3` references `blob`, and the `ref_count`
            // recompute further down has to see these rows gone or it will count a
            // reference that is on its way out. An inline image needs no such care -- its
            // bytes are the row.
            "DELETE FROM part_image WHERE part_id = $1",
            // Deleted with the part rather than kept as a record of where it came from. A
            // source row is a claim about a model, and after a purge there is no model for
            // it to be about; keeping it would leave a vendor and a price pointing at an id
            // nothing else in the database knows.
            "DELETE FROM part_source WHERE part_id = $1",
            "DELETE FROM derivative WHERE revision_id IN (SELECT id FROM revision WHERE part_id = $1)",
            "DELETE FROM file WHERE revision_id IN (SELECT id FROM revision WHERE part_id = $1)",
            "DELETE FROM revision WHERE part_id = $1",
            // The move audit trail. It references `part` and arrived a slice after this
            // list was written, so a purge of any part anyone had ever moved failed on
            // `part_move_part_id_fkey` -- caught on the running stack, not by the suite,
            // because slice 7's purge tests never move and the folder tree's move tests
            // never purge. `child-first` is the rule this list already follows; this row
            // is a child of `part` and belongs above it.
            //
            // Deleted rather than kept: the trail records where a part was filed, and a
            // purged part is not filed anywhere. Keeping it would leave rows pointing at
            // an id nothing else in the database knows, which is the shape of orphan the
            // no-`ON DELETE CASCADE` rule exists to make impossible.
            "DELETE FROM part_move WHERE part_id = $1",
            "DELETE FROM part WHERE id = $1",
        ] {
            sqlx::query(statement)
                .bind(part.as_uuid())
                .execute(&mut *tx)
                .await?;
        }

        // Recompute and quarantine in one statement, so a hash cannot be counted correct
        // and left un-quarantined by a failure between two of them.
        //
        // `quarantined_at` is cleared when the count comes back above zero, which is what
        // makes re-ingesting quarantined bytes un-quarantine them: `link_existing` points
        // a new `file` row at a blob whose clock was running, and the next purge that
        // touches it stops that clock. `coalesce` on the other branch is what stops a
        // second purge from restarting a clock that is already running.
        let sizes: Vec<Option<i64>> = sqlx::query_scalar(
            "WITH counts AS ( \
                 SELECT h AS blake3, \
                        (SELECT count(*) FROM file f WHERE f.blake3 = h) \
                      + (SELECT count(*) FROM derivative d WHERE d.blake3 = h) AS actual \
                 FROM unnest($1::text[]) AS h \
             ) \
             UPDATE blob b SET \
                 ref_count = c.actual, \
                 quarantined_at = CASE WHEN c.actual = 0 \
                                       THEN coalesce(b.quarantined_at, now()) \
                                       ELSE NULL END \
             FROM counts c \
             WHERE b.blake3 = c.blake3 \
             RETURNING CASE WHEN c.actual = 0 THEN b.stored_bytes ELSE NULL END",
        )
        .bind(&doomed)
        .fetch_all(&mut *tx)
        .await?;

        // The path half of quarantine. Written after the chain comes down rather than
        // before it, so a `storage_path` a `file` row still holds cannot briefly appear in
        // both tables at once -- the sweep's guard reads exactly that overlap and would
        // decline a row this transaction is about to make removable.
        //
        // `quarantined_at = now()` on conflict, where the blob branch above keeps the
        // running clock with `coalesce`. Not an inconsistency: the clock belongs to the
        // bytes, a hash identifies its bytes and a path does not. The same path can arrive
        // holding something else, and it takes a person to do it -- `model_dir_for`
        // disambiguates against a directory that exists, so what puts a quarantined path
        // back in play is the owner removing that directory in a file manager, which is
        // what a browsable store is for. Restarting is the safe answer, and it is this
        // sweep's own rule: losing bytes is worse than keeping them.
        if !doomed_files.is_empty() {
            let (paths, hashes, sizes) = doomed_files.iter().fold(
                (Vec::new(), Vec::new(), Vec::new()),
                |(mut paths, mut hashes, mut sizes), (path, hash, stored)| {
                    paths.push(path.as_str());
                    hashes.push(hash.as_str());
                    sizes.push(*stored);
                    (paths, hashes, sizes)
                },
            );
            sqlx::query(
                "INSERT INTO quarantined_file (storage_path, blake3, stored_bytes) \
                 SELECT * FROM unnest($1::text[], $2::text[], $3::bigint[]) \
                 ON CONFLICT (storage_path) DO UPDATE \
                     SET blake3 = excluded.blake3, \
                         stored_bytes = excluded.stored_bytes, \
                         quarantined_at = now()",
            )
            .bind(&paths)
            .bind(&hashes)
            .bind(&sizes)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // One row came back per hash the purge touched; the `NULL`s are the ones another
        // part still points at, which are exactly the blobs that must not be counted as
        // entering quarantine.
        let entering: Vec<i64> = sizes.into_iter().flatten().collect();
        let mut quarantined_bytes = 0u64;
        for stored in &entering {
            // `bytes_column`, never `as u64`: a negative row would otherwise reach a person
            // as 18 exabytes entering quarantine instead of saying the row is wrong.
            quarantined_bytes += bytes_column("blob.stored_bytes", *stored)?;
        }
        Ok(Purged::Done(PurgeReport {
            quarantined: entering.len() as u32,
            quarantined_bytes,
        }))
    }

    /// Which library owns `part`. `None` when there is no such part, or when it is
    /// soft-deleted.
    ///
    /// This is how a part-scoped route stays tenant-safe without a library in its path:
    /// the library it enqueues under is read off the part rather than taken from the
    /// caller, so a job can only ever name a revision of the library that owns it. Taking
    /// both from the caller would let any pair be posted together, and `CLAUDE.md`'s
    /// "content addressing is not authorization" applies to a part id exactly as
    /// `jobs.rs` applies it to a batch id — scoping makes the check structural instead of
    /// a step someone can forget.
    ///
    /// Soft-deleted parts answer `None` for the same reason
    /// [`PgParts::revisions_missing`] skips them: a deleted part is hidden everywhere the
    /// grid looks, so rendering for one is work whose output nothing will ever display.
    pub async fn library_of(&self, part: PartId) -> Result<Option<LibraryId>, DbError> {
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT library_id FROM part WHERE id = $1 AND deleted_at IS NULL")
                .bind(part.as_uuid())
                .fetch_optional(&self.0)
                .await?;
        Ok(id.map(LibraryId::from_uuid))
    }

    /// Which revision of `part` is the current one — the question
    /// [`PartRepository::page`]'s revision LATERAL answers about every row it returns,
    /// asked on its own for one part.
    ///
    /// The ordering is `created_at DESC, id DESC`, character for character the LATERAL's,
    /// and it has to stay that way. Two resolutions of "latest" that can disagree are a
    /// bug waiting for a second revision to exist: an enqueue route that names one
    /// revision while the grid shows another renders a picture nobody is looking at, and
    /// reports success doing it.
    pub async fn latest_revision(&self, part: PartId) -> Result<Option<RevisionId>, DbError> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM revision WHERE part_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(part.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        Ok(id.map(RevisionId::from_uuid))
    }

    /// The source blob and format of a revision the caller already holds: everything a
    /// `derive` job needs to fetch the bytes it must re-read.
    ///
    /// Scoped to a library, and through `part.library_id` rather than by a check the
    /// caller has to remember — exactly as [`PgJobs::batch_status`]'s failure join is
    /// (`jobs.rs`). Content addressing is not authorization (`CLAUDE.md`) and neither is
    /// a revision id: it is a uuid a caller might hold from anywhere, and without the
    /// scope a `derive` job naming another library's revision renders onto it. A revision
    /// this library cannot reach answers `Ok(None)` — the same answer as a revision with
    /// no source file, on purpose, so the reply never confirms that the other library's
    /// revision exists.
    ///
    /// Takes a revision, never a part. The derive payload names the revision precisely so
    /// that nothing resolves "latest" a second time (design §3.7) — a job enqueued
    /// against revision A must not render onto revision B because a second revision
    /// landed while it queued.
    ///
    /// The `ORDER BY ... LIMIT 1` is a deterministic pick, not a formality: `file` carries
    /// no unique constraint on `(revision_id, role)` — `0002_parts.sql` gives it two plain
    /// indexes and nothing else — so "a revision has exactly one source file" describes
    /// what today's ingest happens to write, not a promise the schema makes. A second
    /// source row (a re-upload, a later format conversion) must resolve to one answer
    /// every call, rather than to whichever row the planner handed back first.
    ///
    /// Deliberately does **not** filter `part.deleted_at`, where its neighbour
    /// [`PgParts::source_for_download`] does — the asymmetry is the point, not an
    /// oversight to tidy up. This one is reached only from a `derive` job, and both
    /// sweeps that enqueue those (`revisions_missing`, and the page query beside it)
    /// already filter deleted parts, so a deleted part never arrives here. The download
    /// route is reached from a URL a user can hold after deleting the part, which is a
    /// different question with a different answer.
    pub async fn revision_source(
        &self,
        library: LibraryId,
        revision: RevisionId,
    ) -> Result<Option<RevisionSource>, DbError> {
        let row: Option<(String, String, Option<String>, Option<i16>)> = sqlx::query_as(
            "SELECT f.blake3, f.format, f.storage_path, b.zstd_level FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id AND p.library_id = $2 \
             LEFT JOIN blob b ON b.blake3 = f.blake3 \
             WHERE f.revision_id = $1 AND f.role = 'source' \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((hex, format, storage_path, zstd_level)) = row else {
            return Ok(None);
        };
        let parsed = BlobHash::parse_hex(&hex);
        let hash = parsed.map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some(RevisionSource {
            hash,
            format,
            storage_path,
            zstd_level,
        }))
    }

    /// Everything `GET /api/revisions/{id}/download` needs, in one row: which bytes, what
    /// format, what to call the file, and how those bytes were stored.
    ///
    /// Sits beside [`PgParts::revision_source`] and does not replace it. That one feeds a
    /// `derive` job, which arrives holding a second id — its library — and cross-checking
    /// the two is a real test (ruling T7-C). A download arrives with the revision id and
    /// nothing else, so resolving the library from the revision and comparing it to itself
    /// would be a no-op that reads like a check, which is worse than no check: spec §2.1
    /// argues this at length and it is not re-derived here. The revision uuid is the
    /// capability, as it is on every other Phase 1 route; when auth lands the check becomes
    /// "is this revision's library reachable by this caller", which has a subject.
    ///
    /// Filters `part.deleted_at IS NULL`, which [`PgBlobs::library_holds`] deliberately
    /// does not. Different question, opposite answer: a re-scan must not resurrect a part
    /// the user deleted, and a download URL held from before the delete must not outlive
    /// it. A deleted part is not browsable, so it is not downloadable either.
    ///
    /// `zstd_level` comes off the `file` row itself — never from
    /// `Compression::for_source_format`. That is ingest-time policy and slice 7 is about to
    /// move it, so a reader that re-derived it would start serving zstd frames as files the
    /// day the policy changed (spec §2.5). It is passed through as the nullable column it
    /// is; see [`DownloadSource::zstd_level`].
    ///
    /// The `file` row and not the `blob` row, since migration `0013`: one hash can have a
    /// zstd-3 copy at the old content-addressed path and a raw copy in a model directory at
    /// the same time — that is the whole migration window — and the per-hash column cannot
    /// answer for both. `blob` is not joined here at all any more; nothing else on this
    /// route reads it.
    ///
    /// `storage_path` rides along the same way, for the same reason: the route picks its
    /// read by this column, not by guessing from `zstd_level` or from anything else on the
    /// row, so it has to be the value `file` actually holds.
    ///
    /// The `role = 'source'` filter and the `ORDER BY … LIMIT 1` are character for
    /// character [`PgParts::revision_source`]'s, and for its reason: `file` has no unique
    /// constraint on `(revision_id, role)`, so a second source row must resolve to the same
    /// answer on every call rather than to whichever row the planner returned first.
    pub async fn source_for_download(
        &self,
        revision: RevisionId,
    ) -> Result<Option<DownloadSource>, DbError> {
        #[allow(clippy::type_complexity)]
        let row: Option<(String, String, String, Option<String>, Option<i16>, i64)> =
            sqlx::query_as(
                "SELECT f.blake3, f.format, p.name, f.storage_path, f.zstd_level, f.size_bytes \
             FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE f.revision_id = $1 AND f.role = 'source' AND p.deleted_at IS NULL \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
            )
            .bind(revision.as_uuid())
            .fetch_optional(&self.0)
            .await?;
        let Some((hex, format, part_name, storage_path, zstd_level, size_bytes)) = row else {
            return Ok(None);
        };
        let hash = BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some(DownloadSource {
            hash,
            size_bytes,
            format,
            part_name,
            storage_path,
            zstd_level,
        }))
    }

    /// What this library occupies, split the way `DATA.md` §1.1 splits storage classes.
    /// `None` means there is no such library, so a route can 404 rather than report zero
    /// bytes for an id that names nothing — the same distinction
    /// [`PgParts::auto_thumbnail`] draws, and for the same reason.
    ///
    /// The two halves are summed differently, and the asymmetry is the whole accounting.
    /// See [`StorageTotals`] for what each figure includes.
    ///
    /// **Derivatives** are summed over `blob` rows selected by `IN (subquery)`, so bytes
    /// two revisions share are counted once — what `ref_count` exists for and what `du`
    /// would report. `stored_bytes`, never `size_bytes`: the question is what is on the
    /// volume, and spec §4 wants a figure Phase D's tiering work can be judged against.
    ///
    /// **Sources** are summed over `file` rows, and that inversion is deliberate. It was
    /// the derivative shape until the store became a folder tree, and it under-reported the
    /// moment it stopped being true that one `blob` row meant one file on disk: three parts
    /// sharing a hash are three files now (spec §0 — deduplication of source bytes is gone
    /// by design), and the blob-shaped sum reported one of them. A review measured 5,005 B
    /// against 7,173 B actually on disk on a four-part corpus, and the gap widens with
    /// duplication.
    ///
    /// `stored_bytes` and not `size_bytes`, since migration `0013` put both on the `file`
    /// row. The two are equal for a migrated file — those are written uncompressed, which
    /// is why the column moved off `blob` at all — and they differ by the compression ratio
    /// for one still awaiting `migrate_storage`, where the bytes really are a zstd frame at
    /// the old path. Summing `size_bytes` reported 204,800 for 91,204 bytes on a corpus
    /// mid-migration, under a panel whose own string says *on disk*
    /// (`strings.storage.totals`), which is the reading `CLAUDE.md`'s measurement rule
    /// forbids. `coalesce` to `size_bytes` covers the one row shape that has no recorded
    /// stored size — written by something outside `insert_part_chain`, and over-reporting
    /// it beats counting it as zero.
    ///
    /// Inline thumbnails are added to the derivative total from `octet_length`, because
    /// they are derivative bytes this library costs whatever holds them — `DATA.md` §1.5
    /// makes Postgres their deliberate exception to "blobs never live in Postgres", not
    /// an exemption from being counted. Leaving them out would report `0 B` of
    /// derivatives over a library holding megabytes of previews, which is the omission
    /// `CLAUDE.md`'s measurement rule forbids.
    ///
    /// `f.role = 'source'` is not decoration either, and it was missing until a review
    /// measured its absence: one `role = 'export'` row moved a library's source total
    /// from 176,543 to 180,864 while its cards did not move at all. Only `'source'` is
    /// written today, so the divergence was dormant — but [`PartRepository::page`]'s own
    /// source LATERAL filters that column seven lines from here, and two queries over one
    /// table disagreeing about which rows they mean is a bug waiting for the slice that
    /// writes the second role. The card figures and this total describe the same set, and
    /// this clause is what keeps that true.
    ///
    /// Soft-deleted parts are excluded from the first two figures and counted in the
    /// third. The two above match [`PartRepository::page`], so the panel that sits over
    /// the grid can be checked against the cards in it; `removed_bytes` is what stops the
    /// split from reading as a saving, because the bytes have not moved and
    /// `strings.storage.removed` says so in words.
    ///
    /// The folder-tree slice reached the same requirement from the other side and met it
    /// by counting removed parts inside the totals, while nothing yet reported them
    /// apart -- a delete that dropped the panel while the volume was unchanged is the one
    /// reading `CLAUDE.md` forbids, and both shapes refuse it. This one also keeps the
    /// panel checkable against the grid, so it is the one that survived the merge.
    ///
    /// **Quarantined bytes are not here, and cannot be.** A purge removes the part chain,
    /// so a quarantined blob has no `file` row, no `revision`, no `part` and therefore no
    /// library: the figure is library-less by construction rather than merely
    /// unimplemented. It belongs to an instance-wide storage view, which arrives with
    /// Phase 4's tiering job. Until then a purge does drop this panel while the bytes wait
    /// out their thirty days -- recorded in the slice 7 design, section 2.
    pub async fn storage_totals(
        &self,
        library: LibraryId,
    ) -> Result<Option<StorageTotals>, DbError> {
        // `sum()` over a bigint column is `numeric`, which sqlx will not decode into
        // i64 — hence the `::bigint` casts, not decoration.
        let row: Option<(i64, i64, i64)> = sqlx::query_as(
            "SELECT (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
             FROM file f \
             JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NULL \
             AND f.role = 'source'), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
             WHERE b.blake3 IN (SELECT d.blake3 FROM derivative d \
             JOIN revision r ON r.id = d.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NULL)) \
             + (SELECT coalesce(sum(octet_length(d.thumb_bytes)), 0)::bigint \
             FROM derivative d JOIN revision r ON r.id = d.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NULL), \
             (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
             FROM file f \
             JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NOT NULL \
             AND f.role = 'source') \
             + (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
             WHERE b.blake3 IN (SELECT d.blake3 FROM derivative d \
             JOIN revision r ON r.id = d.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NOT NULL \
             AND d.blake3 IS NOT NULL)) \
             + (SELECT coalesce(sum(octet_length(d.thumb_bytes)), 0)::bigint \
             FROM derivative d JOIN revision r ON r.id = d.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NOT NULL) \
             FROM library l WHERE l.id = $1",
        )
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((source, derivative, removed)) = row else {
            return Ok(None);
        };
        Ok(Some(StorageTotals {
            source_bytes: bytes_column("file.stored_bytes", source)?,
            derivative_bytes: bytes_column("blob.stored_bytes", derivative)?,
            removed_bytes: bytes_column("file.stored_bytes + blob.stored_bytes", removed)?,
        }))
    }

    /// What the whole instance occupies, across every library and including what no
    /// library can be charged for.
    ///
    /// **Not the sum of [`Self::storage_totals`] over the libraries**, and the difference is
    /// the reason this is its own query rather than a fold. Derivatives are still
    /// content-addressed and genuinely shared, so a blob two libraries both point at is
    /// charged in full to each of them there — correct for a panel answering "what does
    /// this library cost me", and double counting for one answering "what is on this disk".
    /// Here every blob is counted once.
    ///
    /// Quarantined bytes appear here and nowhere else. They are library-less by
    /// construction (`DATA.md` §1.6): a quarantined blob is keyed by hash, and the part
    /// that would have said which library it belonged to is the part that was purged. That
    /// is exactly why a per-library panel cannot show them and an instance-wide one must —
    /// they are bytes on the disk, for up to thirty days, that no other figure admits to.
    ///
    /// **Still not `du`, and the caller is expected to say so.** `metadata.json` beside
    /// every model is not counted (the same omission `StorageTotals` documents, for the
    /// same reason: a per-manifest length column written for a figure nobody would see
    /// move), and neither is anything a person dropped into the store themselves. The
    /// route pairs this with a real walk of the root for exactly that reason.
    pub async fn instance_storage(&self) -> Result<InstanceStorage, DbError> {
        // One row, four scalar subqueries, same `::bigint` casts as `storage_totals` and
        // for the same reason: `sum()` over bigint is `numeric`, which sqlx will not decode
        // into i64.
        let (source, derivative, inline, removed, quarantined): (i64, i64, i64, i64, i64) =
            sqlx::query_as(
                "SELECT \
             (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
              FROM file f JOIN revision r ON r.id = f.revision_id \
              JOIN part p ON p.id = r.part_id \
              WHERE p.deleted_at IS NULL AND f.role = 'source'), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
              WHERE b.blake3 IN (SELECT d.blake3 FROM derivative d WHERE d.blake3 IS NOT NULL)), \
             (SELECT coalesce(sum(octet_length(d.thumb_bytes)), 0)::bigint FROM derivative d), \
             (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
              FROM file f JOIN revision r ON r.id = f.revision_id \
              JOIN part p ON p.id = r.part_id \
              WHERE p.deleted_at IS NOT NULL AND f.role = 'source'), \
             (SELECT coalesce(sum(q.stored_bytes), 0)::bigint FROM quarantined_file q) \
             + (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
                WHERE b.quarantined_at IS NOT NULL)",
            )
            .fetch_one(&self.0)
            .await?;

        Ok(InstanceStorage {
            source_bytes: bytes_column("file.stored_bytes", source)?,
            derivative_bytes: bytes_column("blob.stored_bytes", derivative)?,
            inline_preview_bytes: bytes_column("derivative.thumb_bytes", inline)?,
            removed_bytes: bytes_column("file.stored_bytes", removed)?,
            quarantined_bytes: bytes_column("quarantined_file.stored_bytes", quarantined)?,
        })
    }

    /// Every revision in `library` with no generated derivative of `kind` — the set a
    /// sweep enqueues a `derive` job for.
    ///
    /// "Missing" deliberately means missing a *generated* derivative, and asks nothing
    /// about a user-supplied image. A part whose photo is removed later must fall back to
    /// a rendered thumbnail rather than to nothing, so the render is worth having even
    /// while a photo hides it. (`part_image` is slice 5's table and does not exist yet;
    /// this is written down now so that adding it does not turn into "and skip parts that
    /// have one".)
    ///
    /// Per revision, not per part: a rung belongs to the revision it was tessellated
    /// from, and Phase 2's second revision needs its own rather than inheriting the
    /// first's. Soft-deleted parts are excluded — they are hidden everywhere else, and
    /// rendering for one is work whose output nothing will display.
    ///
    /// Newest first, matching the grid's own order, so that a sweep over a large library
    /// fills the page the user is looking at before it works backwards through pages
    /// nobody has scrolled to.
    pub async fn revisions_missing(
        &self,
        library: LibraryId,
        kind: DerivativeKind,
    ) -> Result<Vec<RevisionId>, DbError> {
        let ids: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT r.id FROM revision r JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL \
               AND NOT EXISTS (SELECT 1 FROM derivative d \
                               WHERE d.revision_id = r.id AND d.kind = $2) \
             ORDER BY r.created_at DESC, r.id DESC",
        )
        .bind(library.as_uuid())
        .bind(kind.as_str())
        .fetch_all(&self.0)
        .await?;
        Ok(ids
            .into_iter()
            .map(|(id,)| RevisionId::from_uuid(id))
            .collect())
    }

    /// Everything the move route has to know before it can decide where a part goes.
    ///
    /// One query rather than four, and the same LATERALs the grid page uses, so the
    /// directory a move renames is the directory the card showed. `Ok(None)` is "no such
    /// live part" — a deleted or unknown id — which the route answers 404 for.
    pub async fn move_source(&self, part: PartId) -> Result<Option<MoveSource>, DbError> {
        // Five columns off three tables, read positionally and mapped into `MoveSource`
        // immediately below — the same shape (and the same allow) the grid page uses.
        #[allow(clippy::type_complexity)]
        let row: Option<(Uuid, Option<Uuid>, String, Option<String>, Option<String>)> =
            sqlx::query_as(
                "SELECT p.library_id, p.folder_id, p.name, s.storage_path, s.blake3 \
                 FROM part p \
                 LEFT JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                                    ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
                 LEFT JOIN LATERAL (SELECT f.storage_path, f.blake3 FROM file f \
                                    WHERE f.revision_id = r.id AND f.role = 'source' \
                                    ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
                 WHERE p.id = $1 AND p.deleted_at IS NULL",
            )
            .bind(part.as_uuid())
            .fetch_optional(&self.0)
            .await?;

        row.map(|(library, folder, name, storage_path, blake3)| {
            let source_hash = blake3
                .map(|hex| {
                    BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                        column: "file.blake3",
                        value: hex,
                    })
                })
                .transpose()?;
            Ok(MoveSource {
                library: LibraryId::from_uuid(library),
                folder: folder.map(FolderId::from_uuid),
                name,
                directory: storage_path.as_deref().and_then(model_directory),
                storage_path,
                source_hash,
            })
        })
        .transpose()
    }

    /// Is another live part in `folder` already called `name`?
    ///
    /// Two exclusions that both have to be there. `except` drops the part being moved, so
    /// re-filing a model into the category it is already in is a no-op rather than a
    /// collision with itself. `deleted_at IS NULL` drops a soft-deleted namesake, which is
    /// invisible everywhere else and must not block a move on the strength of a row nobody
    /// can see.
    ///
    /// `IS NOT DISTINCT FROM` for the folder, because the library root is NULL and `=` is
    /// never true against it — the same reason `PgFolders::get_or_create` uses it.
    pub async fn name_taken_in_folder(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        name: &str,
        except: PartId,
    ) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT exists(SELECT 1 FROM part \
             WHERE library_id = $1 AND folder_id IS NOT DISTINCT FROM $2 \
               AND name = $3 AND id <> $4 AND deleted_at IS NULL)",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(name)
        .bind(except.as_uuid())
        .fetch_one(&self.0)
        .await?)
    }

    /// File a part under a different category, moving its directory on the way.
    ///
    /// **`rename` runs inside the transaction, and the commit happens only if it
    /// succeeded.** A failed rename rolls the rows back and nothing moved. The remaining
    /// window is a rename that succeeds and a commit that then fails, which leaves the disk
    /// ahead of the database — repairable, because `metadata.json` makes every model
    /// directory self-identifying.
    ///
    /// The reverse order was considered and rejected, but not because its failure is worse
    /// in kind: both orderings can leave a `storage_path` that reads fail on — one naming a
    /// path the rename never created, the other naming the directory the rename just
    /// emptied. What separates them is how often each window opens. A rename fails for
    /// ordinary reasons and this order turns every one of those into a clean refusal with
    /// nothing moved; a commit failing after a successful rename needs the connection to
    /// drop between `COMMIT` and its acknowledgement, which is rare. The common failure is
    /// made total, the rare one is made repairable.
    ///
    /// `rename` is a closure rather than a storage handle because this crate cannot hold
    /// one: `lapidary-db` and `lapidary-storage` are both L1, and `cargo xtask check-layers`
    /// forbids an edge between them. So the caller — `lapidary-api`'s `moves.rs`, the one
    /// file allowed to name `SourceRelocator` — passes the filesystem half in, and its
    /// error text arrives here as a `String` for [`DbError::RenameFailed`].
    ///
    /// Every `file` row under the old directory is re-pointed, not only the latest
    /// revision's source: the rename moves the whole directory, so a second revision's file
    /// sitting beside the first would otherwise be left naming a path that no longer
    /// exists. `starts_with`, not `LIKE`: a model directory disambiguated to `cliff_a1b2c3`
    /// contains `_`, which `LIKE` reads as a wildcard.
    pub async fn move_to_folder<F>(
        &self,
        part: PartId,
        from: Option<FolderId>,
        to: Option<FolderId>,
        old_directory: &str,
        new_directory: &str,
        rename: F,
    ) -> Result<(), DbError>
    where
        F: FnOnce() -> Result<(), String>,
    {
        let mut tx = self.0.begin().await?;

        sqlx::query("UPDATE part SET folder_id = $2, updated_at = now() WHERE id = $1")
            .bind(part.as_uuid())
            .bind(to.map(|f| f.as_uuid()))
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "UPDATE file SET storage_path = $2 || substring(storage_path from length($3) + 1) \
             WHERE revision_id IN (SELECT id FROM revision WHERE part_id = $1) \
               AND starts_with(storage_path, $3 || '/')",
        )
        .bind(part.as_uuid())
        .bind(new_directory)
        .bind(old_directory)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO part_move (id, part_id, from_folder, to_folder) \
             VALUES (gen_random_uuid(), $1, $2, $3)",
        )
        .bind(part.as_uuid())
        .bind(from.map(|f| f.as_uuid()))
        .bind(to.map(|f| f.as_uuid()))
        .execute(&mut *tx)
        .await?;

        if let Err(detail) = rename() {
            // Explicit, not by drop: a rollback that only happens because the transaction
            // fell out of scope is a rollback nobody can see in this function.
            tx.rollback().await?;
            return Err(DbError::RenameFailed { detail });
        }

        tx.commit().await?;
        Ok(())
    }

    /// Where this part has been filed, newest first. The audit trail migration `0009`
    /// created the `part_move` table for.
    pub async fn moves(&self, part: PartId) -> Result<Vec<MoveRow>, DbError> {
        let rows: Vec<(Option<Uuid>, Option<Uuid>, i64)> = sqlx::query_as(
            "SELECT from_folder, to_folder, \
                    (extract(epoch FROM moved_at) * 1000000)::bigint AS moved_us \
             FROM part_move WHERE part_id = $1 ORDER BY moved_at DESC, id DESC",
        )
        .bind(part.as_uuid())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(from, to, moved_us)| {
                Ok(MoveRow {
                    from_folder: from.map(FolderId::from_uuid),
                    to_folder: to.map(FolderId::from_uuid),
                    moved_at: jiff::Timestamp::from_microsecond(moved_us).map_err(|_| {
                        DbError::TimestampOutOfRange {
                            column: "part_move.moved_at",
                            value: moved_us,
                        }
                    })?,
                })
            })
            .collect()
    }
}

/// The directory holding a stored file, as the store-relative path it is written with.
///
/// `None` for a path with no separator at all, which no `storage_path` this code writes
/// has — every one is `libraries/<library>/<category…>/<model>/<file>` — but a row edited
/// by hand could, and a directory guessed from one would be worse than none.
fn model_directory(storage_path: &str) -> Option<String> {
    storage_path
        .rsplit_once('/')
        .map(|(directory, _file)| directory.to_owned())
}

#[async_trait::async_trait]
impl PartRepository for PgParts {
    async fn page(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        after: Option<PartId>,
        limit: u16,
        shows: Shows,
    ) -> Result<Vec<PartRow>, DbError> {
        // One query: thumbnails travel inline as bytea rather than costing a round trip
        // per card. Keyset, not OFFSET — OFFSET degrades as the library grows.
        //
        // sqlx cannot decode jiff::Timestamp (it ships chrono/time support, not jiff), so
        // timestamps are pulled out as epoch microseconds and reassembled below rather
        // than adding a second date-time crate just to carry the value across.
        //
        // Both LATERALs below pick one row deterministically out of a set that could
        // hold more than one, newest (`created_at DESC, id DESC`) first: the revision
        // LATERAL because a part could in principle carry more than one revision (only
        // Phase 2 will actually write a second one), and the derivative LATERAL because a
        // revision legitimately carries several derivatives of *different* kinds — the
        // LOD ladder slice 3 adds writes exactly that. `derivative_kind_unique_per_revision`
        // (migration 0003) forbids a second row of the *same* kind, so two thumbnail rows
        // for one revision cannot happen — but nothing stops `lod0`, `lod1`, thumbnail all
        // coexisting on one revision, and a plain (non-LATERAL) LEFT JOIN would fan out on
        // those: several derivative rows for one revision means several identical grid
        // cards for one part, and a page of `limit` rows holding fewer than `limit` distinct
        // parts, silently under-reporting `next`.
        //
        // The source LATERAL is a third of the same shape, and it is a LEFT one for the
        // reason the derivative's is: a revision whose source `file` row is missing is a
        // part whose owner most needs to see it in the grid, to delete or re-scan it. An
        // inner join would answer that by hiding the part. Its `role = 'source'` filter
        // and `created_at DESC, id DESC` ordering are character for character
        // `source_for_download`'s, so the sizes on a card and the bytes behind its
        // download link always describe the same `file` row — `file` has no unique
        // constraint on `(revision_id, role)`, so that agreement is a choice, not a
        // property of the schema. Every column here now comes off that one row, `blob`
        // included no longer: since migration `0013` a file records its own
        // `stored_bytes` and `zstd_level`, because a hash can have a compressed legacy
        // copy and a raw model file at once and the shared row could only describe one of
        // them. That also makes this grid checkable against the storage panel over it —
        // `storage_totals` sums the same `file` rows.
        //
        // `is_watertight` is deliberately not selected. It was read into `_watertight` and
        // dropped, and the merge that brought the folder filter alongside the LOD rung put
        // this select one column over sqlx's sixteen-tuple `FromRow` ceiling -- so the
        // column nothing reads is the one that goes. A card that ever needs it can select
        // it again, against a row type that is a struct by then.
        //
        // The folder filter is the recursive CTE at the top, and it is inline here rather
        // than a separate "give me the descendants" call for one reason: the descent and
        // the page are one question — "the cards in this category" — and splitting them
        // would put query composition in `lapidary-api` (an id list bound into a filter) or
        // give the tree two descent implementations to keep in step. `down` is empty and
        // costs nothing when `$7` is NULL, and the `IS NULL` guard beside it is what makes
        // an absent filter mean the whole library.
        let rows: Vec<GridRow> = sqlx::query_as(concat!(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $7 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT ",
            grid_columns!(),
            " FROM part p ",
            grid_laterals!(),
            " WHERE p.library_id = $1 AND (p.deleted_at IS NOT NULL) = $6 \
               AND ($2::uuid IS NULL OR p.id < $2) \
               AND ($7::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
             ORDER BY p.id DESC LIMIT $3",
        ))
        .bind(library.as_uuid())
        .bind(after.map(|a| a.as_uuid()))
        .bind(i64::from(limit))
        // The kind string comes off `DerivativeKind`, never a literal: the write side
        // stopped spelling it out in task 5, and a reader spelling it differently from
        // the writer reads nothing while looking entirely correct.
        .bind(DerivativeKind::Thumbnail.as_str())
        // Same rule as the line above: off `DerivativeKind`, never a literal. The two
        // kinds are read by one query now, so a reader spelling either differently from
        // the writer reads nothing while looking entirely correct.
        .bind(DerivativeKind::TessellationL0.as_str())
        // The predicate is `(deleted_at IS NOT NULL) = $6` rather than two branches of
        // SQL, so both pages are provably the same query: a column added to one is added
        // to the other, and the removed list cannot quietly fall behind the grid.
        .bind(shows == Shows::Removed)
        .bind(folder.map(|f| f.as_uuid()))
        .fetch_all(&self.0)
        .await?;

        // `file`'s size columns are `bigint`, so sqlx hands them back signed, and
        // `bytes_column` refuses a negative one rather than wrapping it — the same
        // silent wraparound the triangle count below refuses.
        //
        // Read off `file` and not `blob` since migration `0013`, and that is also what
        // makes this grid checkable against the storage panel above it: `storage_totals`
        // sums `f.stored_bytes` over the same rows, which is the figure this card calls
        // `stored_bytes` too.
        rows.into_iter().map(to_part_row).collect()
    }

    async fn search(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: &str,
        after: Option<PartId>,
        limit: u16,
        shows: Shows,
    ) -> Result<Vec<PartRow>, DbError> {
        // Same sixteen columns, same LATERALs, same `Shows` predicate, same subtree filter.
        // What differs is which parts are candidates and in what order they come back.
        //
        // # The three things that make this query the shape it is
        //
        // **1. `coalesce(part_number, '')` belongs in the rank and must NOT be in the
        // match.** Two different reasons, pulling opposite ways, and getting either wrong
        // is silent.
        //
        // In the *rank* it is load-bearing: with a NULL `part_number` — which is every row
        // in a real library today, because nothing writes that column yet —
        // `(p.part_number ILIKE $9)::int * 4 + …` is NULL for the whole expression. The
        // `WHERE` is unaffected, so page one comes back full and plausibly ordered and
        // looks entirely correct; then every row ties under `ORDER BY rank DESC` and page
        // two returns nothing. `0002_parts.sql:43` coalesces inside the generated column
        // for the same reason.
        //
        // In the *match* it is the opposite: `coalesce(part_number, '') ILIKE $9` is an
        // expression, and `part_number_trgm` indexes the column — so the coalesced form is
        // unindexable and the term becomes a sequential scan. Verified with `EXPLAIN` on
        // this deployment: bare `part_number ILIKE` takes a `Bitmap Index Scan on
        // part_number_trgm`, and the coalesced one takes a `Seq Scan` with the predicate as
        // a filter. `NULL ILIKE x` is NULL, and `NULL OR …` is exactly the "not a match"
        // this wants, so the coalesce buys nothing here and costs the index.
        //
        // **2. `hits` must stay materialized**, which multiple references give it. `anchor`
        // and `top` both read it, so both read the *same stored* `real` out of one
        // tuplestore rather than recomputing a float and comparing two results of it.
        // Collapse it to one reference, or add `NOT MATERIALIZED`, and an exact comparison
        // silently becomes a recomputation — which is a cursor that skips or repeats a row
        // by one ULP.
        //
        // **3. The `hits`/`top` split is what keeps this affordable.** The four LATERALs run
        // `limit` times, not once per matching row: `top` picks the page first and the joins
        // happen after. Without the split a five-thousand-hit query would do twenty thousand
        // index lookups per page and throw all but a screenful away.
        //
        // # What it costs, plainly
        //
        // `rank` is not indexable, so page 20 re-scans and re-ranks the whole match set
        // exactly as page 1 did. It is no *dearer* than page 1 either, which is the thing
        // `OFFSET` cannot promise — and the keyset stays a `PartId`, so `PartsPage`,
        // `fetchParts` and every binding are untouched.
        //
        // One honest hole: if the anchor part is renamed out of the result set mid-scroll,
        // `anchor` is empty, the row comparison is NULL, and paging ends early rather than
        // repeating rows. Documented rather than branched around — a fallback for a
        // rename-during-scroll window would be more machinery than the case deserves.
        let rows: Vec<GridRow> = sqlx::query_as(concat!(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $7 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen, \
             hits AS ( \
               SELECT p.id, \
                      ( (coalesce(p.part_number, '') ILIKE $9)::int * 4 \
                      + (p.name ILIKE $9)::int * 2 \
                      + (p.search @@ plainto_tsquery('simple', $8))::int \
                      + ts_rank(p.search, plainto_tsquery('simple', $8)) ) AS rank \
                 FROM part p \
                WHERE p.library_id = $1 AND (p.deleted_at IS NOT NULL) = $6 \
                  AND ($7::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
                  AND ( p.part_number ILIKE $9 \
                     OR p.name ILIKE $9 \
                     OR p.search @@ plainto_tsquery('simple', $8) ) ), \
             anchor AS (SELECT rank, id FROM hits WHERE id = $2), \
             top AS ( \
               SELECT id, rank FROM hits \
                WHERE $2::uuid IS NULL OR (rank, id) < (SELECT rank, id FROM anchor) \
                ORDER BY rank DESC, id DESC LIMIT $3) \
             SELECT ",
            grid_columns!(),
            " FROM top JOIN part p ON p.id = top.id ",
            grid_laterals!(),
            " ORDER BY top.rank DESC, top.id DESC",
        ))
        .bind(library.as_uuid())
        .bind(after.map(|a| a.as_uuid()))
        .bind(i64::from(limit))
        .bind(DerivativeKind::Thumbnail.as_str())
        .bind(DerivativeKind::TessellationL0.as_str())
        .bind(shows == Shows::Removed)
        .bind(folder.map(|f| f.as_uuid()))
        // The raw query, for `plainto_tsquery`, which must not see the LIKE escapes.
        .bind(query)
        // And the escaped one, wrapped. Two bindings of one input, and they are not
        // interchangeable.
        .bind(like_pattern(query))
        .fetch_all(&self.0)
        .await?;

        rows.into_iter().map(to_part_row).collect()
    }
}

/// One image in a part's gallery, as a reader gets it.
///
/// `bytes` or `hash`, never both and never neither — `part_image_inline_or_blob` is what
/// makes that the database's opinion. Which one a row uses is a size decision made when it
/// was written (`DATA.md` §1.5's 64 KB line) and not a fact about the image, so a reader
/// takes whichever is there.
#[derive(Debug)]
pub struct PartImageRow {
    pub id: PartImageId,
    /// Inline WebP, for an image small enough to travel with the row.
    pub inline_webp: Option<Vec<u8>>,
    /// The content-addressed blob, for one that is not.
    pub hash: Option<BlobHash>,
    pub origin: String,
    pub source_url: Option<String>,
    pub position: i32,
    /// How the picture is framed. `cover` or `contain`, and CSS's `object-fit` values on
    /// purpose — the framing is applied by the browser at display time rather than encoded
    /// into the bytes, so changing it costs nothing and loses nothing.
    pub fit: String,
    /// What to keep when `cover` crops, as a fraction of each edge. CSS's `object-position`.
    pub focus: (f64, f64),
}

/// A part's gallery, and what goes into it.
impl PgParts {
    /// Every image for one part, in gallery order.
    ///
    /// `position` then `id`, matching `part_image_part_id_position_idx`: two images written
    /// at the same position get a stable order rather than whatever the heap hands back,
    /// which is what stops a gallery reshuffling itself between two reads of the same page.
    pub async fn part_images(&self, part: PartId) -> Result<Vec<PartImageRow>, DbError> {
        /// `part_image` as it comes off the wire: id, inline bytes, hash, origin, source
        /// URL, position. Named because six columns of mostly-optionals is exactly the
        /// shape clippy asks to be given a name, and because the order is the SELECT's.
        type GalleryRow = (
            Uuid,
            Option<Vec<u8>>,
            Option<String>,
            String,
            Option<String>,
            i32,
            String,
            f64,
            f64,
        );

        let rows: Vec<GalleryRow> = sqlx::query_as(
            "SELECT id, image_webp, blake3, origin, source_url, position, fit, focus_x, focus_y \
                 FROM part_image WHERE part_id = $1 ORDER BY position, id",
        )
        .bind(part.as_uuid())
        .fetch_all(&self.0)
        .await?;

        rows.into_iter()
            .map(
                |(id, inline_webp, hex, origin, source_url, position, fit, focus_x, focus_y)| {
                    let hash = hex
                        .map(|hex| {
                            BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                                column: "part_image.blake3",
                                value: hex,
                            })
                        })
                        .transpose()?;
                    Ok(PartImageRow {
                        id: PartImageId::from_uuid(id),
                        inline_webp,
                        hash,
                        origin,
                        source_url,
                        position,
                        fit,
                        focus: (focus_x, focus_y),
                    })
                },
            )
            .collect()
    }

    /// Re-frame one image. Nothing is re-encoded and no bytes move.
    ///
    /// **Scoped by `part_id` as well as by `id`, and that is not belt-and-braces.** An image
    /// id on its own is a bare handle to a row, which is the same shape of mistake
    /// `CLAUDE.md` names for blobs: knowing an identifier must not be what grants access to
    /// what it identifies. The pair is what the route has and what this checks, so an id
    /// guessed or copied from another part updates nothing.
    ///
    /// `false` is "no such image on that part", undistinguished from "no such part", because
    /// the caller turns both into one 404 — telling them apart would confirm a row exists to
    /// someone who cannot see it.
    pub async fn set_image_framing(
        &self,
        part: PartId,
        image: PartImageId,
        framing: Framing<'_>,
    ) -> Result<bool, DbError> {
        let updated = sqlx::query(
            "UPDATE part_image SET fit = $3, focus_x = $4, focus_y = $5 \
             WHERE id = $2 AND part_id = $1",
        )
        .bind(part.as_uuid())
        .bind(image.as_uuid())
        .bind(framing.fit)
        .bind(framing.focus.0)
        .bind(framing.focus.1)
        .execute(&self.0)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    /// Add one image to the end of a part's gallery.
    ///
    /// **The blob half takes a reference**, which is the whole reason this is a transaction:
    /// `record_unreferenced` puts bytes on disk at `ref_count = 0`, and a sweep that ran
    /// between that and the `part_image` row would find a blob nothing references and
    /// quarantine an image somebody had just uploaded. Inserting the row and taking the
    /// count in one transaction is what closes that window.
    ///
    /// `position` is read and incremented inside the same transaction for a smaller version
    /// of the same reason: two uploads racing would otherwise both read the same maximum and
    /// land on top of each other.
    pub async fn add_part_image(
        &self,
        part: PartId,
        image: NewPartImage<'_>,
    ) -> Result<PartImageId, DbError> {
        let mut tx = self.0.begin().await?;

        let next: i32 = sqlx::query_scalar(
            "SELECT coalesce(max(position), -1) + 1 FROM part_image WHERE part_id = $1",
        )
        .bind(part.as_uuid())
        .fetch_one(&mut *tx)
        .await?;

        let id = PartImageId::new();
        let (inline, hex) = match image.bytes {
            ImageBytes::Inline(bytes) => (Some(bytes), None),
            ImageBytes::Blob(hash) => (None, Some(hash.to_hex())),
        };
        sqlx::query(
            "INSERT INTO part_image (id, part_id, image_webp, blake3, origin, source_url, position) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id.as_uuid())
        .bind(part.as_uuid())
        .bind(inline)
        .bind(&hex)
        .bind(image.origin)
        .bind(image.source_url)
        .bind(next)
        .execute(&mut *tx)
        .await?;

        if hex.is_some() {
            sqlx::query("UPDATE blob SET ref_count = ref_count + 1, quarantined_at = NULL WHERE blake3 = $1")
                .bind(&hex)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(id)
    }

    /// Where a part came from, oldest first.
    ///
    /// A part genuinely can have more than one: the model from one place and the hardware
    /// from another. Oldest first because the first one recorded is usually where the model
    /// itself came from, and a list that reorders itself as things are added is a list
    /// nobody can point at.
    pub async fn part_sources(&self, part: PartId) -> Result<Vec<PartSourceRow>, DbError> {
        /// The SELECT's order. Named for the same reason `GalleryRow` is.
        type SourceRow = (
            Uuid,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
        );

        let rows: Vec<SourceRow> = sqlx::query_as(
            "SELECT id, url, vendor, external_id, title, license, price_minor, currency \
             FROM part_source WHERE part_id = $1 ORDER BY created_at, id",
        )
        .bind(part.as_uuid())
        .fetch_all(&self.0)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, url, vendor, external_id, title, license, price_minor, currency)| {
                    PartSourceRow {
                        id: PartSourceId::from_uuid(id),
                        url,
                        vendor,
                        external_id,
                        title,
                        license,
                        price_minor,
                        currency,
                    }
                },
            )
            .collect())
    }

    /// Record where a part came from.
    ///
    /// **`ON CONFLICT` on `(part_id, url)` updates rather than refuses**, because the same
    /// URL twice is somebody correcting what they typed, not a second source. `unique
    /// (part_id, url)` in `0015` treats a NULL url as distinct from every other NULL, which
    /// is what lets a part carry two sources that have a vendor and a price and no link —
    /// exactly the trade-show case `0015` describes.
    pub async fn add_part_source(
        &self,
        part: PartId,
        source: NewPartSource<'_>,
    ) -> Result<PartSourceId, DbError> {
        let id = PartSourceId::new();
        let written: Uuid = sqlx::query_scalar(
            "INSERT INTO part_source \
                 (id, part_id, url, vendor, external_id, title, license, price_minor, currency, \
                  retrieved_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now()) \
             ON CONFLICT (part_id, url) DO UPDATE SET \
                 vendor = excluded.vendor, external_id = excluded.external_id, \
                 title = excluded.title, license = excluded.license, \
                 price_minor = excluded.price_minor, currency = excluded.currency, \
                 retrieved_at = excluded.retrieved_at \
             RETURNING id",
        )
        .bind(id.as_uuid())
        .bind(part.as_uuid())
        .bind(source.url)
        .bind(source.vendor)
        .bind(source.external_id)
        .bind(source.title)
        .bind(source.license)
        .bind(source.price_minor)
        .bind(source.currency)
        .fetch_one(&self.0)
        .await?;
        Ok(PartSourceId::from_uuid(written))
    }

    /// Whether any gallery references these bytes.
    ///
    /// The other half of `derivative_is_reachable`, and it exists for the same rule:
    /// `CLAUDE.md` says content addressing is not authorization, so `GET /api/blob/{hash}`
    /// has to ask whether anything in this instance points at a hash before serving it.
    /// Without this, an image blob would be on disk and unreachable — served to nobody,
    /// including the person who uploaded it.
    pub async fn image_is_reachable(&self, hash: &BlobHash) -> Result<bool, DbError> {
        let reachable: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM part_image WHERE blake3 = $1)")
                .bind(hash.to_hex())
                .fetch_one(&self.0)
                .await?;
        Ok(reachable)
    }
}

/// Where a part came from. `retrieved_at` and `created_at` are on the row and not here:
/// nothing shows them, and a field nobody reads is a field that goes stale silently.
#[derive(Debug)]
pub struct PartSourceRow {
    pub id: PartSourceId,
    pub url: Option<String>,
    pub vendor: Option<String>,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub license: Option<String>,
    /// Minor units, so 12.50 EUR is 1250 — `0015` explains why this is never a float.
    pub price_minor: Option<i64>,
    pub currency: Option<String>,
}

/// What [`PgParts::add_part_source`] writes. Every field optional, because a source with a
/// vendor and a licence and no link is a real answer — `0015`'s trade-show case.
#[derive(Debug, Default)]
pub struct NewPartSource<'a> {
    pub url: Option<&'a str>,
    pub vendor: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub license: Option<&'a str>,
    pub price_minor: Option<i64>,
    pub currency: Option<&'a str>,
}

/// How a picture sits in its frame — `part_image_known_fit` and `part_image_focus_in_range`
/// are what keep both fields meaningful, so a caller passing nonsense gets a constraint
/// violation rather than a picture rendered somewhere off screen.
#[derive(Debug, Clone)]
pub struct Framing<'a> {
    /// `cover` or `contain`.
    pub fit: &'a str,
    /// Fractions of the width and height, each 0 to 1.
    pub focus: (f64, f64),
}

/// Where an image's bytes are. Exactly one, matching `part_image_inline_or_blob`.
#[derive(Debug)]
pub enum ImageBytes<'a> {
    /// Small enough to live on the row (`DATA.md` §1.5's 64 KB line).
    Inline(&'a [u8]),
    /// Already written to the content-addressed store, and recorded in `blob`.
    Blob(&'a BlobHash),
}

/// What [`PgParts::add_part_image`] needs to write a gallery row.
#[derive(Debug)]
pub struct NewPartImage<'a> {
    pub bytes: ImageBytes<'a>,
    /// `uploaded`, `url_supplied`, `og_fetched` or `rendered` — `part_image_known_origin`
    /// is what refuses anything else, so a typo here is a failed insert rather than a value
    /// no reader understands.
    pub origin: &'a str,
    /// Where it was fetched from, for one that was. Never used to load the image: our own
    /// copy is what is served, because hotlinking leaks a referrer on every grid scroll.
    pub source_url: Option<&'a str>,
}

/// One grid row, decoded. `page` and `search` select the same sixteen columns and both end
/// here, so a card cannot mean one thing on the grid and another in a set of results.
///
/// The tuple is positional and stays that way: `#[derive(sqlx::FromRow)]` maps by column
/// *name*, and this SELECT has two `id`s and two `blake3`s. Adopting it would mean aliasing
/// the grid's crown-jewel query for a benefit nothing needs yet.
///
/// ponytail: a positional 16-tuple at sqlx's ceiling. A seventeenth column is the trigger to
/// alias the SELECT and move to a named `FromRow` struct, not a reason to drop a column
/// again.
#[allow(clippy::type_complexity)]
type GridRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    Option<String>,
    String,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i16>,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

fn to_part_row(row: GridRow) -> Result<PartRow, DbError> {
    fn bytes(column: &'static str, value: Option<i64>) -> Result<Option<u64>, DbError> {
        value.map(|v| bytes_column(column, v)).transpose()
    }

    let (
        id,
        lib,
        revision,
        name,
        part_number,
        source_path,
        thumb_bytes,
        triangles,
        source_hash,
        source_bytes,
        stored_bytes,
        zstd_level,
        storage_path,
        tessellation_l0,
        created_us,
        updated_us,
    ) = row;
    {
        // `as u32` previously turned a negative column value into a number
        // near 4.29 billion instead of failing — the same silent-wraparound
        // shape as the write side above, just in the other direction.
        let triangle_count = triangles
            .map(|t| {
                u32::try_from(t).map_err(|_| DbError::NegativeTriangleCount {
                    column: "revision.triangle_count",
                    value: t,
                })
            })
            .transpose()?;
        let source_hash = source_hash
            .map(|hex| {
                BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                    column: "file.blake3",
                    value: hex,
                })
            })
            .transpose()?;
        // Refused rather than dropped, same as the source hash above: a
        // derivative row whose `blake3` is not a digest is a corrupt row, and
        // reporting it as "this part has no rung" would hide the corruption
        // behind a state that looks ordinary.
        let tessellation_l0 = tessellation_l0
            .map(|hex| {
                BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                    column: "derivative.blake3",
                    value: hex,
                })
            })
            .transpose()?;
        // Keyed off the source row's presence, never off `zstd_level`'s:
        // the column is nullable, so a `None` level on a row that exists
        // means "nobody recorded how these bytes were stored", which is a
        // different fact from "this revision has no source file" and must
        // not collapse into it. The predicate matches `SourceReader::get`'s
        // for every level actually recorded, which is what keeps a card
        // claiming "compressed" from sitting over a raw download. A `NULL`
        // one is not a download this card describes at all: `download.rs`
        // answers 500 for it rather than serving anything (spec §2.5.1), so
        // reporting `false` is the display field declining to be the place a
        // data error surfaces. Reachable only from outside `insert_part_chain`
        // now that migration `0013` records the level on the `file` row: every
        // row this crate writes carries the level `put_at` reported for it,
        // including `link_existing`'s, which used to inherit whatever the
        // shared `blob` row said. See `PartSummary::compressed`.
        let compressed = source_hash
            .as_ref()
            .map(|_| zstd_level.is_some_and(|level| level != 0));
        Ok(PartRow {
            summary: PartSummary {
                id: PartId::from_uuid(id),
                library: LibraryId::from_uuid(lib),
                revision: RevisionId::from_uuid(revision),
                name,
                part_number,
                source_path,
                // The hash is not carried in slice 1: thumbnails arrive inline
                // and the grid renders them directly. A hash-addressed
                // thumbnail endpoint arrives with the viewer.
                thumbnail: None,
                triangle_count,
                // Every figure on a mesh part is tessellated, so any is all.
                approximate: true,
                source_hash,
                tessellation_l0,
                source_bytes: bytes("file.size_bytes", source_bytes)?,
                stored_bytes: bytes("file.stored_bytes", stored_bytes)?,
                compressed,
                created_at: jiff::Timestamp::from_microsecond(created_us).map_err(|_| {
                    DbError::TimestampOutOfRange {
                        column: "part.created_at",
                        value: created_us,
                    }
                })?,
                updated_at: jiff::Timestamp::from_microsecond(updated_us).map_err(|_| {
                    DbError::TimestampOutOfRange {
                        column: "part.updated_at",
                        value: updated_us,
                    }
                })?,
            },
            thumbnail_webp: thumb_bytes,
            directory: storage_path.as_deref().and_then(model_directory),
            storage_path,
        })
    }
}

/// A user's text as a `LIKE` pattern: escaped, then wrapped in `%`.
///
/// **Not cosmetic.** `'%' || $q || '%'` with a query of `%` is `'%%%'`, which matches every
/// row — so a search box would answer a single percent sign with the entire library. It is
/// not injection, because the value is bound; it is a search that lies about what it found,
/// which is the same class of thing `CLAUDE.md` forbids about measurement.
///
/// Backslash first, or escaping `%` would then have its own escape escaped. The result goes
/// to the `ILIKE` terms only — `plainto_tsquery` gets the raw query, because a tsquery has
/// no idea what a LIKE escape is and would tokenize the backslashes as text.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// A library, as a picker sees it.
///
/// `part_count` rides along for the same reason `FolderNode`'s does: a switcher listing
/// libraries wants to say which one has anything in it, and a count per library would be N
/// requests for a control that is one.
#[derive(Debug)]
pub struct LibraryRow {
    pub id: LibraryId,
    pub name: String,
    /// `hobby` or `controlled`. Text on the wire and in the column, matching every other
    /// discriminator here: a new mode must not need a migration.
    pub mode: String,
    pub part_count: i64,
}

/// Libraries: list them, make them.
impl PgParts {
    /// Every library, with how many live parts each holds, oldest first.
    ///
    /// Ordered by `created_at` rather than by name so the switcher does not reshuffle when
    /// somebody renames one — and so the seeded library, which is the one an existing
    /// deployment has been using, stays first.
    pub async fn libraries(&self) -> Result<Vec<LibraryRow>, DbError> {
        let rows: Vec<(Uuid, String, String, i64)> = sqlx::query_as(
            "SELECT l.id, l.name, l.mode, \
                    (SELECT count(*) FROM part p \
                      WHERE p.library_id = l.id AND p.deleted_at IS NULL) \
               FROM library l ORDER BY l.created_at, l.id",
        )
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, mode, part_count)| LibraryRow {
                id: LibraryId::from_uuid(id),
                name,
                mode,
                part_count,
            })
            .collect())
    }

    /// Create one, refusing a name another library already has.
    ///
    /// **The name is unique across the instance**, which the migration enforces and this
    /// reports. Not a technical requirement — nothing joins on it — but a switcher listing
    /// two entries called `Terrain` is a switcher nobody can use, and the cost of finding
    /// that out later is a rename plus an explanation.
    ///
    /// `mode` is `hobby` or `controlled` and is checked by the database, so a value this
    /// code does not know is a failed insert rather than a row every reader has to cope
    /// with. Nothing reads it yet — governance is Phase 8 — but choosing it is a decision
    /// made once, at creation, and asking later would mean asking about a library somebody
    /// has already filled.
    pub async fn create_library(&self, name: &str, mode: &str) -> Result<LibraryId, DbError> {
        let id = LibraryId::new();
        // Derived here and never sent by a client, exactly as a category's is: `slugify` is
        // the one place that decides what a filesystem may hold, and a caller who could name
        // the directory could name one outside the store. Lowercased because that is what
        // `library_slug` has always returned and what every existing directory is called.
        let slug = lapidary_core::slug::slugify(name).to_lowercase();
        sqlx::query("INSERT INTO library (id, name, mode, slug) VALUES ($1, $2, $3, $4)")
            .bind(id.as_uuid())
            .bind(name)
            .bind(mode)
            .bind(&slug)
            .execute(&self.0)
            .await
            .map_err(|err| match constraint_of(&err).as_deref() {
                Some("library_name_unique") => DbError::LibraryNameTaken {
                    name: name.to_owned(),
                },
                // Distinct names, one directory. The pair a user can see is refused above;
                // this is the pair that looks different on screen and is not on disk.
                Some("library_slug_unique") => DbError::LibrarySlugTaken {
                    name: name.to_owned(),
                    slug,
                },
                _ => DbError::Query(err),
            })?;
        Ok(id)
    }
}
