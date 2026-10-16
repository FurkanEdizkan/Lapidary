use crate::DbError;
use crate::folders::constraint_of;
use lapidary_core::{
    BlobHash, DerivativeKind, FolderId, LibraryId, MeasurementProvenance, MeshMeasurements, PartId,
    PartImageId, PartSourceId, PartSummary, Provenance, RevisionId,
};
use sqlx::PgPool;
use uuid::Uuid;

/// A grid row paired with the wire concerns of the grid itself — the part's own picture, its
/// format and where its file sits. Not folded into `PartSummary`, which is the shape the
/// viewer and the detail page share: these are facts a *card* needs.
#[derive(Debug)]
pub struct PartRow {
    pub summary: PartSummary,
    /// `file.format` for the revision's source — `stl`, `obj`, `3mf`, `step`. Here rather
    /// than on [`PartSummary`]: it is what a card with no
    /// picture says about the part instead of apologising for the picture it does not have,
    /// and what decides whether the part has a **view** at all.
    pub source_format: Option<String>,
    /// The part's own first picture, in gallery order, or `None` for a part that has none.
    ///
    /// The picture the card shows, read in the page's one query rather than fetched per
    /// card. Over the 64 KB line it carries a hash instead of bytes and the card addresses
    /// `/api/blob/{hash}`; under it the bytes ride on the row.
    pub image: Option<PartImageRow>,
    /// The directory holding the part's file, relative to its root — the parent of
    /// `file.storage_path`. Shown, never acted on: since M4 a move renames nothing.
    pub directory: Option<String>,
    /// `file.storage_path`, relative to the directory `root` names.
    pub storage_path: Option<String>,
    /// Which directory `storage_path` is relative to: the owner's folders, the inbox, or
    /// Lapidary's store (`0062`).
    pub root: Option<lapidary_core::FileRoot>,
}

/// Everything a `derive` job needs to re-read a revision's source bytes: where the file is
/// (`root` and `storage_path`, or the hash for a store row from before the folder tree) and
/// the level it was written at.
#[derive(Debug)]
pub struct RevisionSource {
    pub hash: BlobHash,
    /// `file.format` — lowercase, no dot. What the kernel is asked to parse.
    pub format: String,
    /// `file.root`: which directory `storage_path` is relative to.
    pub root: lapidary_core::FileRoot,
    /// `file.storage_path`. `None` means a store row whose bytes are content-addressed.
    pub storage_path: Option<String>,
    /// `file.on_disk`: false once a later revision's bytes replaced the file at that path, so
    /// there is nothing left to build an export from.
    pub on_disk: bool,
    /// `file.zstd_level` exactly as stored, for the same reason [`DownloadSource`] keeps
    /// it: a reader must follow the level that was recorded when the bytes were written,
    /// never re-derive one from the format. The `file` row's and never the `blob` row's
    /// (migration `0013`): an upload leaves the blob row at its zstd staging copy's level
    /// while the worker files the same bytes raw, and a derive job reading the filed copy
    /// at the staged level failed with "Unknown frame descriptor".
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
    /// `file.root`: which directory `storage_path` is relative to (`0062`).
    pub root: lapidary_core::FileRoot,
    /// `file.storage_path`, as [`RevisionSource::storage_path`].
    pub storage_path: Option<String>,
    /// `file.on_disk`: false once a later revision's bytes replaced this file where it was.
    /// The route answers 410 for such a row before it looks at the disk at all.
    pub on_disk: bool,
    /// `file.mtime_ns`: the modification time when the file was indexed. With `size_bytes`,
    /// what a download checks the file against before it sends a byte. `None` for a store row.
    pub mtime_ns: Option<i64>,
    /// `file.zstd_level` exactly as stored, `None` and all. Never `COALESCE`d to 0 — but
    /// not for the reason ruling T1-A first gave, which was wrong and is retracted here:
    /// a `COALESCE` could not serve a zstd frame as the file, because
    /// `LibraryReader` decodes on `is_some_and(|level| level != 0)` and reads `None`
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
}

/// What the whole store holds, across every library — the figure a person means by "how
/// much space is this taking".
///
/// Several numbers rather than one, because they behave differently and a single total would
/// hide the one that surprises people: `quarantined_bytes` is on the disk *and* belongs to no
/// library, so it appears in no other panel in the application.
///
/// **There is no inline figure since R4.** `derivative.thumb_bytes` held the one kind that
/// was ever stored in Postgres rather than on the volume, migration `0055` deleted every
/// such row, and `upsert_derivative` writes the column `NULL`. A field that could only ever
/// report zero was a row on the panel saying nothing, and the arithmetic it needed keeping
/// apart from — tracked bytes exceeding a walk of the storage root, which reads as bytes
/// having gone missing — cannot happen now that every tracked byte is on the volume.
///
/// See [`PgParts::instance_storage`] for what is deliberately not counted, and why the
/// route serves a real directory walk beside these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceStorage {
    /// Source files that are in Lapidary's own store: rows from before M4, whose bytes stayed
    /// where they were. A file in the owner's folders or the inbox is not on this volume and is
    /// not counted here (`0062`).
    pub source_bytes: u64,
    /// What Lapidary wrote **on the storage volume** — the slicer exports, since R3 — each
    /// blob counted once however many revisions or libraries point at it.
    pub derivative_bytes: u64,
    /// Inside the thirty-day hold, on the disk and belonging to no library. The one figure
    /// this application can report nowhere else.
    ///
    /// What reaches it is a derivative that lost its last reference: an evicted export, an
    /// export of a deleted part, and after `0055` the render derivatives this application
    /// deleted on its own behalf. A deleted part's own model file never does — delete leaves
    /// it where it is.
    pub quarantined_bytes: u64,
    /// Slicer exports Lapidary can write again and nobody has read in 90 days: what "free
    /// cache space" would put into quarantine (`DATA.md` §1.5). Only blobs no other row
    /// points at, so an export two revisions share counts nothing here.
    pub render_cache_bytes: u64,
}

/// What "free cache space" did: how many rows it removed, and how many bytes entered the
/// thirty-day quarantine because nothing else pointed at them. Never "freed": the bytes stay
/// on the disk until the quarantine ends and the sweep takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCacheFreed {
    /// Rows, and they were called rungs until R3 — there is no rung in the product now, and a
    /// field named for one would have the panel count something that cannot exist.
    pub removed: u64,
    pub quarantined_bytes: u64,
}

/// What "free cache space" may remove: a derive job writes any of them again from the source
/// when asked.
fn cache_kinds() -> Vec<&'static str> {
    // The exports, and nothing else, since R3: the L1 and L2 rungs that used to be here are
    // not rebuildable any more, so evicting one would be data loss dressed as a cache sweep.
    // Migration `0055` then deleted the rows themselves, so this list is now the whole of
    // what `derivative` can hold — and it stays a filter rather than becoming "every row",
    // because what makes a row cache is that something can rebuild it, not that it is there.
    DerivativeKind::ALL.map(DerivativeKind::as_str).to_vec()
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
}

/// One row of a part's move history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    pub from_folder: Option<FolderId>,
    pub to_folder: Option<FolderId>,
    pub moved_at: jiff::Timestamp,
}

/// Which grid a page reads: the library, the category, the filters, and where the last page
/// ended.
///
/// A struct because the filters kept arriving as parameters, and `search` had reached eight.
/// The defaults are the unfiltered first page of the live library, so a caller names only
/// what differs: `GridQuery { folder: Some(rocks), ..GridQuery::new(library, 50) }`.
#[derive(Debug, Clone, Copy)]
pub struct GridQuery<'a> {
    pub library: LibraryId,
    /// One category **and everything under it**; `None` is the whole library. See
    /// [`PartRepository::page`].
    pub folder: Option<FolderId>,
    /// The previous page's last id, or `None` for the first page.
    pub after: Option<PartId>,
    pub limit: u16,
    /// One source format, as ingest records it: the extension, lowercased.
    pub format: Option<&'a str>,
    /// One material, exactly as the file names it.
    pub material: Option<&'a str>,
    /// One tag, exactly as a person wrote it.
    pub tag: Option<&'a str>,
    /// One custom field's value, as the JSON object `{"<key>": value}` its `@>` filter matches
    /// (`docs/DATA.md` §3.5). The API builds it only from an indexed field of this library.
    pub field: Option<&'a str>,
    /// One number field's range, as the jsonpath `$."<key>" ? (@ >= min && @ <= max)` its `@?` filter
    /// matches, either bound left out. The API builds it only from an indexed number field of this
    /// library. A value that is not a number fails the comparison rather than the query: `@?` suppresses
    /// a comparison between different types.
    // ponytail: no index serves a range, so every part the other filters leave is compared. An expression
    // index on one field's value is the upgrade, when a large library measures slow.
    pub field_range: Option<&'a str>,
}

impl GridQuery<'_> {
    /// The first page of the whole live library.
    pub fn new(library: LibraryId, limit: u16) -> Self {
        Self {
            library,
            folder: None,
            after: None,
            limit,
            format: None,
            material: None,
            tag: None,
            field: None,
            field_range: None,
        }
    }
}

/// The order [`PartRepository::page`] returns.
///
/// `Newest` is the id order the grid has always had. The rest read the latest revision's
/// typed columns, largest first, with the parts that have no such figure last — an open mesh
/// has no volume. Largest first only: a direction is a second control nobody has asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Newest,
    Volume,
    SurfaceArea,
    /// The largest of the three bounding-box extents.
    LongestSide,
    Triangles,
}

impl Sort {
    const ALL: [Self; 5] = [
        Self::Newest,
        Self::Volume,
        Self::SurfaceArea,
        Self::LongestSide,
        Self::Triangles,
    ];

    /// The query-string spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Volume => "volume",
            Self::SurfaceArea => "surface_area",
            Self::LongestSide => "longest_side",
            Self::Triangles => "triangles",
        }
    }

    /// `None` for a spelling that is not one of these.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.as_str() == value)
    }
}

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, in `sort` order. `grid.after` is the previous page's
    /// last id.
    ///
    /// `folder` filters to one category **and everything under it**; `None` is the whole
    /// library. Subtree-inclusive rather than exact-match because the sidebar's parent
    /// categories are real places a user clicks: a tree that showed nothing for `Terrain`
    /// while `Terrain/Rocks` held forty models would be hiding its own contents, and the
    /// count beside the row would contradict the grid next to it.
    ///
    /// Every order keeps the same keyset: `after` is a `PartId`, and the sort value behind it
    /// is read again inside the query rather than carried on the wire — see `search`.
    async fn page(&self, grid: &GridQuery<'_>, sort: Sort) -> Result<Vec<PartRow>, DbError>;

    /// The same grid, filtered to a text query and ordered by relevance.
    ///
    /// A method of its own because relevance *is* its order, so it takes no [`Sort`]. The two
    /// share their columns, their LATERALs and their decoder, which is where sharing actually
    /// matters.
    ///
    /// Same keyset contract: `after` is still a `PartId`, and the rank behind it is
    /// recomputed inside the query rather than carried on the wire. A float in a cursor
    /// drifts by one ULP and silently skips or repeats a row.
    async fn search(&self, grid: &GridQuery<'_>, query: &str) -> Result<Vec<PartRow>, DbError>;
}

/// Mirrors `lapidary_storage::StoredBlob`. Not imported: both crates are L1, and
/// `cargo xtask check-layers` forbids L1 -> L1. The api layer converts between them.
pub struct StoredBlobRow {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

pub struct IngestRequest<'a> {
    pub library: LibraryId,
    pub name: &'a str,
    /// How the bytes arrived, written on the part's first revision: `ingest` for a scan,
    /// `upload` for the browser (Phase 4 slice 2 spec §5). A later revision takes its own from
    /// `RevisionRequest`.
    pub origin: lapidary_core::RevisionOrigin,
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
    /// Where the file is, relative to the directory `root` names — for a scanned file, the
    /// library's root joined with `source_path`. Distinct from `source_path`, which is the
    /// part's identity and never changes.
    ///
    /// `None` only for a `store` row whose bytes are content-addressed, which nothing writes
    /// since M4; fixtures for those rows still say so.
    pub storage_path: Option<&'a str>,
    /// Which directory `storage_path` is relative to (`0062`).
    pub root: lapidary_core::FileRoot,
    /// The file's modification time when it was read, in nanoseconds; what a download checks
    /// it against with the size. `None` for a store row.
    pub mtime_ns: Option<i64>,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    /// Where each of `measurements`' figures came from; written to the `*_source` columns.
    pub provenance: MeasurementProvenance,
    /// The source format, lowercase and without a dot. Was the SQL literal `'stl'`.
    pub format: &'a str,
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
        Ok(!self.existing(std::slice::from_ref(hash)).await?.is_empty())
    }

    /// Which of `hashes` the store holds: [`PgBlobs::exists`] for a whole upload, in one query, and the
    /// same global answer.
    pub async fn existing(
        &self,
        hashes: &[BlobHash],
    ) -> Result<std::collections::HashSet<BlobHash>, DbError> {
        let hex: Vec<String> = hashes.iter().map(BlobHash::to_hex).collect();
        let found: Vec<String> =
            sqlx::query_scalar("SELECT blake3 FROM blob WHERE blake3 = ANY($1)")
                .bind(&hex)
                .fetch_all(&self.0)
                .await?;
        found
            .into_iter()
            .map(|hex| {
                BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                    column: "blob.blake3",
                    value: hex,
                })
            })
            .collect()
    }

    /// The stored form of a blob: its sizes and the level it was written at.
    ///
    /// `zstd_level` is the reason this exists. A reader that re-derived compression from
    /// the file's extension would hand out zstd frames as though they were the file the
    /// day ingest-time policy changed — `LibraryReader` says so at length — and the
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
                // `0002_parts.sql`, and the same collapse `LibraryReader` makes.
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

    /// Recount one blob and quarantine it when nothing points at it: bytes the api stored that only
    /// a job reads, such as an imported bundle, whose parts are replayed into files of their own.
    /// The bytes stay on disk for the 30-day hold, readable by the jobs still queued.
    pub async fn release(&self, hash: &BlobHash) -> Result<(), DbError> {
        let mut tx = self.0.begin().await?;
        recount(&mut tx, &[hash.to_hex()]).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Remove bytes nothing has pointed at for `older_than`.
    ///
    /// This is the only code in Lapidary that destroys user data, and it is written on the
    /// assumption that everything upstream of it may be wrong.
    ///
    /// # It does not consult `ref_count`, and the `NOT EXISTS` pair is not why that is safe
    ///
    /// The counter is a hint. It is maintained by arithmetic on the ingest paths, and
    /// [`recount`] can itself miss a row committed while it waited for a lock. Both can be wrong, so nothing here reads it.
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

        // The other half, and it is not conditional on the cutoff: bytes somebody pointed
        // at again stop being candidates the moment they are pointed at, not thirty days
        // later. `ref_count` is recomputed here for the same reason [`recount`] exists —
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
            bytes,
            un_quarantined: revived.len() as u32,
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
    /// A deleted part has no rows left, so a re-scan of the file it came from finds nothing
    /// here and ingests it again as a new part (owner, 2026-10-17: delete is delete).
    ///
    /// **The part's current revision, not any of them** (Phase 4 slice 1). Bytes an older
    /// revision held, arriving on top of a newer one, are a revert — history, not a re-scan —
    /// and both callers must see them as a change: ingest, to record the revision, and the
    /// upload probe, so the client sends the file at all. "Current" is ordered as the grid's
    /// LATERAL orders it.
    pub async fn library_holds(
        &self,
        library: LibraryId,
        source_path: &str,
        hash: &BlobHash,
    ) -> Result<bool, DbError> {
        let held = self
            .library_holds_each(library, &[(source_path, *hash)])
            .await?;
        Ok(held.contains(&true))
    }

    /// [`PgBlobs::library_holds`] for each of `files`, in one query: the question for a whole upload,
    /// answered per entry in order, so one path named twice with different bytes gets two answers.
    pub async fn library_holds_each(
        &self,
        library: LibraryId,
        files: &[(&str, BlobHash)],
    ) -> Result<Vec<bool>, DbError> {
        let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
        let hashes: Vec<String> = files.iter().map(|(_, hash)| hash.to_hex()).collect();
        let held: Vec<i64> = sqlx::query_scalar(
            "SELECT t.n FROM unnest($2::text[], $3::text[]) WITH ORDINALITY AS t(path, blake3, n) \
             JOIN part p ON p.library_id = $1 AND p.source_path = t.path \
             JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                           ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             JOIN file f ON f.revision_id = r.id AND f.role = 'source' AND f.blake3 = t.blake3",
        )
        .bind(library.as_uuid())
        .bind(&paths)
        .bind(&hashes)
        .fetch_all(&self.0)
        .await?;
        let mut each = vec![false; files.len()];
        for n in held {
            // `WITH ORDINALITY` counts from 1.
            if let Some(entry) = usize::try_from(n - 1).ok().and_then(|i| each.get_mut(i)) {
                *entry = true;
            }
        }
        Ok(each)
    }
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
    /// What they occupied — `stored_bytes`, so it is the space actually recovered.
    pub bytes: u64,
    /// Quarantined blobs something points at again. Their clocks are cleared, not paused.
    pub un_quarantined: u32,
}

pub struct PgIngest(pub PgPool);

impl PgIngest {
    /// A new blob: insert it, then the part chain, in one transaction. The caller has
    /// already written the bytes and reaps them if this fails.
    pub async fn record(&self, req: IngestRequest<'_>) -> Result<PartId, DbError> {
        let mut tx = self.0.begin().await?;
        // A blob row's `stored_bytes` is its content-addressed copy's, and a filed ingest writes
        // none: the model file's size is on its `file` row (migration `0013`). Writing the file's
        // size here too is what purge, the sweep and the instance figure counted twice (`0026`).
        let stored_bytes = if req.storage_path.is_some() {
            0
        } else {
            req.blob.stored_bytes as i64
        };
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $3, $4, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(req.blob.hash.to_hex())
        .bind(req.blob.size_bytes as i64)
        .bind(stored_bytes)
        .bind(req.blob.zstd_level)
        .execute(&mut *tx)
        .await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// A file seen again with the bytes its current revision already holds: its recorded
    /// modification time brought up to date, so a `touch` or a copy of the same bytes does not
    /// leave every download refused as "changed" with no scan able to clear it. Only when the
    /// current revision's file is at this same place — a pre-M4 store row stays as it is.
    pub async fn refresh_seen(
        &self,
        library: LibraryId,
        source_path: &str,
        root: lapidary_core::FileRoot,
        storage_path: &str,
        mtime_ns: i64,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE file f SET mtime_ns = $5 \
             FROM part p \
             JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                           ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             WHERE p.library_id = $1 AND p.source_path = $2 \
               AND f.revision_id = r.id AND f.role = 'source' \
               AND f.root = $3 AND f.storage_path = $4 \
               AND f.mtime_ns IS DISTINCT FROM $5",
        )
        .bind(library.as_uuid())
        .bind(source_path)
        .bind(root.as_str())
        .bind(storage_path)
        .bind(mtime_ns)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// A blob we already hold: skip the blob insert, everything else is identical.
    pub async fn link_existing(&self, req: IngestRequest<'_>) -> Result<PartId, DbError> {
        let mut tx = self.0.begin().await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Write one derivative onto a revision that already exists, replacing whatever was
    /// there. The `derive` job's only writer — ingest writes no derivative at all since R3,
    /// and the two exports arrive here, one kind at a time, when something asks for one.
    ///
    /// Hash-addressed, always: both remaining kinds are a file for another tool, and the
    /// inline `thumb_bytes` shape had exactly one producer — the thumbnail renderer that left
    /// with the pivot. Migration `0055` deleted every row that used it and the column is
    /// written `NULL` below, so nothing in the store is stored inline any more. The column
    /// itself stays: dropping it would need `derivative_storage_is_exclusive` restated and
    /// would return no space without the `VACUUM FULL` that the deleted rows need anyway.
    pub async fn upsert_derivative(
        &self,
        revision: RevisionId,
        kind: DerivativeKind,
        blob: &StoredBlobRow,
        grid: Option<u32>,
        kernel_version: &str,
    ) -> Result<(), DbError> {
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

        // The blob row first: the foreign key means a derivative cannot name bytes the blob
        // table has never heard of, and `ON CONFLICT DO NOTHING` because an export whose bytes
        // another revision already stored is the ordinary case.
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $2, NULL, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(blob.hash.to_hex())
        .bind(blob.size_bytes as i64)
        .execute(&mut *tx)
        .await?;
        let hash = Some(blob.hash.to_hex());

        // `thumb_bytes` is written from `excluded` too, never left alone: an upsert over a row
        // stored inline that set only `blake3` would leave the old bytes in place, and a row
        // with both non-null trips `derivative_storage_is_exclusive`.
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, blake3, kernel_version, params_json) \
             VALUES ($1, $2, $3, NULL, $4, $5, $6) \
             ON CONFLICT (revision_id, kind) DO UPDATE SET thumb_bytes = excluded.thumb_bytes, \
             blake3 = excluded.blake3, kernel_version = excluded.kernel_version, \
             params_json = excluded.params_json",
        )
        .bind(Uuid::now_v7())
        .bind(revision.as_uuid())
        .bind(kind.as_str())
        .bind(hash.as_deref())
        .bind(kernel_version)
        .bind(serde_json::json!({ "grid": grid }))
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

/// Recompute each blob's `ref_count` from the rows that point at it, and quarantine the ones
/// nothing does, in one statement. One row comes back per hash: its `stored_bytes` when nothing
/// points at it any more, `NULL` when something still does.
///
/// Delete's and the render cache's: two ways of taking references away, one rule for what that
/// does to a blob, so the two cannot drift apart.
///
/// It recomputes rather than decrementing. `ref_count` is maintained by arithmetic everywhere
/// else, and a counter maintained only by increments and decrements drifts invisibly until the
/// reaper acts on it — so the path that leads to removing bytes refuses to trust the number.
/// One statement, so a hash cannot be counted correct and left un-quarantined by a failure
/// between two. `coalesce` keeps a clock that is already running; a count above zero clears
/// it. The count is taken under the statement's snapshot and can miss a row committed while it
/// waited for the lock; that is survivable by design, because [`PgBlobs::reap`] re-asks
/// reachability inside the transaction that would delete the bytes.
async fn recount(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    hashes: &[String],
) -> Result<Vec<Option<i64>>, DbError> {
    Ok(sqlx::query_scalar(
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
    .bind(hashes)
    .fetch_all(&mut **tx)
    .await?)
}

/// Delete these parts and everything that hangs off them, child-first, inside the caller's
/// transaction. Shared by [`PgParts::delete`] and `PgFolders::delete_subtree`, so a part
/// deleted on its own and a part deleted with its category leave the same nothing behind.
///
/// Child-first because nothing in `0002_parts.sql` declares `ON DELETE CASCADE`, which is a
/// property worth keeping: a stray `DELETE FROM part` fails loudly on a foreign key rather than
/// quietly taking four tables with it.
///
/// # Which blob counts move
///
/// Only the derivatives'. Their hashes are collected before the chain comes down (afterwards
/// there is no path from a part to them) and [`recount`] runs over them, so a cached export
/// nothing points at any more enters the same quarantine cache eviction uses.
///
/// **Source-file hashes are deliberately left alone**, and no `quarantined_file` row is
/// written. Both are how the old purge handed a part's own file to the reaper, which unlinks
/// it; delete never removes the owner's model file, so it never starts that clock. The cost is
/// a `blob.ref_count` that stays above the true count for those bytes, which only ever errs
/// towards keeping them. Gallery images (`part_image.blake3`) are not recounted either, as
/// before: `recount` has never counted them.
pub(crate) async fn delete_parts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    parts: &[Uuid],
) -> Result<(), DbError> {
    if parts.is_empty() {
        return Ok(());
    }
    let derived: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT d.blake3 FROM derivative d \
         JOIN revision r ON r.id = d.revision_id \
         WHERE r.part_id = ANY($1) AND d.blake3 IS NOT NULL",
    )
    .bind(parts)
    .fetch_all(&mut **tx)
    .await?;

    for statement in [
        "DELETE FROM part_image WHERE part_id = ANY($1)",
        "DELETE FROM part_source WHERE part_id = ANY($1)",
        "DELETE FROM derivative WHERE revision_id IN (SELECT id FROM revision WHERE part_id = ANY($1))",
        "DELETE FROM file WHERE revision_id IN (SELECT id FROM revision WHERE part_id = ANY($1))",
        "DELETE FROM revision WHERE part_id = ANY($1)",
        // The move trail records where a part was filed; a deleted part is not filed anywhere.
        "DELETE FROM part_move WHERE part_id = ANY($1)",
        "DELETE FROM part WHERE id = ANY($1)",
    ] {
        sqlx::query(statement)
            .bind(parts)
            .execute(&mut **tx)
            .await?;
    }

    recount(tx, &derived).await?;
    Ok(())
}

async fn insert_part_chain(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &IngestRequest<'_>,
) -> Result<PartId, DbError> {
    let part = PartId::new();
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

    insert_revision_chain(
        tx,
        &RevisionWrite {
            part,
            revision: Uuid::now_v7(),
            rev_label: "1",
            parent: None,
            origin: req.origin,
            storage_path: req.storage_path,
            root: req.root,
            mtime_ns: req.mtime_ns,
            blob: req.blob,
            measurements: req.measurements,
            provenance: req.provenance,
            format: req.format,
        },
    )
    .await?;
    Ok(part)
}

/// One revision's rows, whichever write it belongs to: a new part's first revision
/// ([`insert_part_chain`]) or a later one (`PgRevisions::record_revision`). One writer, so a
/// revision's file row, its reference counts and its derivatives cannot drift between them.
pub(crate) struct RevisionWrite<'a> {
    pub part: PartId,
    pub revision: Uuid,
    pub rev_label: &'a str,
    pub parent: Option<Uuid>,
    pub origin: lapidary_core::RevisionOrigin,
    pub storage_path: Option<&'a str>,
    pub root: lapidary_core::FileRoot,
    pub mtime_ns: Option<i64>,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub provenance: MeasurementProvenance,
    pub format: &'a str,
}

pub(crate) async fn insert_revision_chain(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &RevisionWrite<'_>,
) -> Result<(), DbError> {
    let part = req.part;
    let revision = req.revision;
    let m = req.measurements;
    let p = req.provenance;
    // Converted — and, deliberately, checked — before this function's first INSERT (a new
    // part's own row is in the same transaction and goes with it): a
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
        "INSERT INTO revision (id, part_id, rev_label, parent_revision_id, origin, volume, \
         volume_source, surface_area, surface_area_source, bbox_x, bbox_y, bbox_z, \
         bbox_source, triangle_count, is_watertight, units) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 'mm')",
    )
    .bind(revision)
    .bind(part.as_uuid())
    .bind(req.rev_label)
    .bind(req.parent)
    .bind(req.origin.as_str())
    .bind(m.volume_mm3)
    // No volume means no provenance for one — writing 'tessellated' beside a NULL would
    // claim we measured something we refused to measure.
    .bind(m.volume_mm3.map(|_| p.volume.as_str()))
    .bind(m.surface_area_mm2)
    .bind(p.surface_area.as_str())
    .bind(m.bbox_mm[0])
    .bind(m.bbox_mm[1])
    .bind(m.bbox_mm[2])
    .bind(p.bbox.as_str())
    .bind(triangle_count)
    .bind(m.is_watertight)
    .execute(&mut **tx)
    .await?;

    // The grid's sort keys ride on the part row, where one index per key hands parts back in
    // order (`0035`). The revision just written is the part's latest, so its figures are the copy.
    sqlx::query(
        "UPDATE part SET latest_volume = $2, latest_surface_area = $3, \
         latest_longest_side = greatest($4::double precision, $5::double precision, $6::double precision), \
         latest_triangle_count = $7 WHERE id = $1",
    )
    .bind(part.as_uuid())
    .bind(m.volume_mm3)
    .bind(m.surface_area_mm2)
    .bind(m.bbox_mm[0])
    .bind(m.bbox_mm[1])
    .bind(m.bbox_mm[2])
    .bind(triangle_count)
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
         zstd_level, stored_bytes, root, mtime_ns) \
         VALUES ($1, $2, 'source', $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.format)
    .bind(req.blob.hash.to_hex())
    .bind(req.blob.size_bytes as i64)
    .bind(req.storage_path)
    .bind(req.blob.zstd_level)
    .bind(req.blob.stored_bytes as i64)
    .bind(req.root.as_str())
    .bind(req.mtime_ns)
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

    Ok(())
}

/// A hex column as a `BlobHash`. Refused rather than dropped: a `blake3` column that is
/// not a digest is a corrupt row, and reporting it as "this part has none" would hide the
/// corruption behind a state that looks entirely ordinary.
pub(crate) fn detail_hash(
    column: &'static str,
    hex: Option<String>,
) -> Result<Option<BlobHash>, DbError> {
    hex.map(|hex| {
        BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash { column, value: hex })
    })
    .transpose()
}

pub(crate) fn detail_stamp(column: &'static str, us: i64) -> Result<jiff::Timestamp, DbError> {
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
    /// The tags a person gave the part, in the order they gave them.
    pub tags: Vec<String>,
    /// What the part is made of: what a person typed, or else what its file states.
    pub materials: Vec<String>,
    /// Whether a person typed `materials`, so a file's statement no longer replaces them.
    pub materials_typed: bool,
    /// The part's custom field values as stored, `{}` when it has none (`docs/DATA.md` §3.5).
    pub custom: serde_json::Value,
    /// The part's identity within its library since slice 6a, and the path a scanned or
    /// dropped folder reported for it.
    pub source_path: String,
    pub rev_label: String,
    pub triangle_count: Option<u32>,
    pub is_watertight: Option<bool>,
    /// All three axes or none. A box missing one axis is not a box, and rendering two of
    /// its three numbers is a measurement that lies by omission.
    pub bbox_mm: Option<[f64; 3]>,
    pub volume_mm3: Option<f64>,
    pub volume_source: Option<Provenance>,
    pub surface_area_mm2: Option<f64>,
    pub surface_area_source: Option<Provenance>,
    pub source_hash: Option<BlobHash>,
    pub source_format: Option<String>,
    pub source_bytes: Option<u64>,
    pub stored_bytes: Option<u64>,
    pub compressed: Option<bool>,
    /// The model's own directory in the store, relative to the storage root — the same
    /// value and the same nullability as [`PartRow::directory`], derived the same way.
    ///
    /// Carried here as well as on the card because the detail page is the surface a
    /// keyboard reaches, and for a while "where does this file live" was answerable only
    /// from a panel a keyboard could not open.
    pub directory: Option<String>,
    /// The directory above with the model's filename back on. `None` alongside
    /// `directory`, and for the same reason: the bytes are still content-addressed.
    pub storage_path: Option<String>,
    /// Which directory `storage_path` is relative to, as [`PartRow::root`].
    pub root: Option<lapidary_core::FileRoot>,
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
    tags: Vec<String>,
    materials: Vec<String>,
    materials_typed: bool,
    custom_json: Option<String>,
    source_path: String,
    rev_label: String,
    triangle_count: Option<i32>,
    is_watertight: Option<bool>,
    bbox_x: Option<f64>,
    bbox_y: Option<f64>,
    bbox_z: Option<f64>,
    volume: Option<f64>,
    volume_source: Option<String>,
    surface_area: Option<f64>,
    surface_area_source: Option<String>,
    source_blake3: Option<String>,
    source_format: Option<String>,
    source_size_bytes: Option<i64>,
    source_stored_bytes: Option<i64>,
    source_zstd_level: Option<i16>,
    storage_path: Option<String>,
    file_root: Option<String>,
    created_us: i64,
    updated_us: i64,
}

/// The columns a card is made of, each aliased to its [`GridRow`] field.
///
/// A `const` and not two copies, because `page` and `search` must select the same list in
/// the same order or [`to_part_row`] decodes one query's columns into another query's
/// fields — a failure that type-checks. `repo.rs`'s LATERAL comment already argues this
/// about the joins; two hand-copied SELECT lists would be worse, because the safety would
/// then *depend* on nobody editing one without the other.
///
/// A macro rather than a `const`, and only because `concat!` takes literals: a `const &str`
/// is not one. The effect is what a const would have given — one definition, spliced at
/// compile time, no string building anywhere near a query.
///
/// The nine columns after the timestamps are the picture the card shows: the source
/// file's format, for a part with no picture at all, and the part's first `part_image`
/// (`R1`). The grid carries only **inline** bytes, so an image over the 64 KB line arrives
/// as its hash and the card addresses it at `/api/blob/{hash}` — which `blob.rs` serves with
/// immutable caching after checking reachability, because content addressing is not
/// authorization.
///
/// **R3 took the rendered thumbnail and the L0 hash out of this list**, and with them the two
/// derivative LATERALs that bound `$4` and `$5` — which renumbered every later bind in `page`,
/// `search` and [`sorted_page!`]. The picture is the part's own image or an honest
/// placeholder; nothing renders a card any more.
macro_rules! grid_columns {
    () => {
        "p.id AS id, p.library_id AS library_id, r.id AS revision_id, p.name AS name, \
     p.part_number AS part_number, p.source_path AS source_path, \
     r.triangle_count AS triangle_count, \
     s.blake3 AS source_blake3, s.size_bytes AS source_bytes, s.stored_bytes AS stored_bytes, \
     s.zstd_level AS zstd_level, s.storage_path AS storage_path, s.root AS file_root, \
     (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
     (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us, \
     s.format AS source_format, \
     img.id AS image_id, img.image_webp AS image_webp, img.blake3 AS image_blake3, \
     img.origin AS image_origin, img.source_url AS image_source_url, \
     img.position AS image_position, img.fit AS image_fit, \
     img.focus_x AS image_focus_x, img.focus_y AS image_focus_y"
    };
}

/// The three LATERALs behind those columns: the latest revision, its source file and the
/// part's own first picture.
///
/// Shared for the same reason as [`grid_columns`], and named separately because search puts
/// a join between the two — its candidates come from a CTE, and the LATERALs then run for
/// the page it kept rather than for every row that matched.
///
/// The picture's `ORDER BY position, id LIMIT 1` is `part_images`' order, cut to one row, so
/// the card shows the same picture the gallery shows first. It rides
/// `part_image_part_id_position_idx`, which `0015` created for exactly this read.
macro_rules! grid_laterals {
    () => {
        "\
     JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
     LEFT JOIN LATERAL (SELECT f.blake3, f.storage_path, f.root, f.size_bytes, f.stored_bytes, f.zstd_level, f.format \
                        FROM file f \
                        WHERE f.revision_id = r.id AND f.role = 'source' \
                        ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
     LEFT JOIN LATERAL (SELECT i.id, i.image_webp, i.blake3, i.origin, i.source_url, i.position, \
                               i.fit, i.focus_x, i.focus_y \
                        FROM part_image i \
                        WHERE i.part_id = p.id \
                        ORDER BY i.position, i.id LIMIT 1) img ON true"
    };
}

/// [`PgParts::sorted`]'s query for one sort key. `$key` is the part row's copy of the key, spelled as that
/// key's index spells it (`0035`), so the page is read off the index in order. The filters are `page`'s
/// newest-first query's, with the same numbering — which R3 shifted down by two when the thumbnail
/// and L0 LATERALs went, so the two must be read together — and a part with no revision is left out
/// before the limit: the cards'
/// join to the latest revision would drop it after, and a short page reads as the last one.
macro_rules! sorted_page {
    ($key:literal) => {
        concat!(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $4 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen, \
             anchor AS (SELECT ",
            $key,
            " AS value, p.id FROM part p WHERE p.id = $2 AND p.library_id = $1), \
             top AS ( \
               SELECT p.id, ",
            $key,
            " AS value FROM part p \
                WHERE p.library_id = $1 \
                  AND EXISTS (SELECT 1 FROM revision WHERE part_id = p.id) \
                  AND ($2::uuid IS NULL OR (",
            $key,
            ", p.id) < (SELECT value, id FROM anchor)) \
                  AND ($4::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
                  AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                       AND f.format = $5 AND f.revision_id = (SELECT id FROM revision \
                       WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
                  AND ($6::text IS NULL OR p.materials @> ARRAY[$6::text]) \
                  AND ($7::text IS NULL OR p.tags @> ARRAY[$7::text]) \
                  AND ($8::jsonb IS NULL OR p.metadata_json->'custom' @> $8::jsonb) \
                  AND ($9::jsonpath IS NULL OR p.metadata_json->'custom' @? $9::jsonpath) \
                ORDER BY ",
            $key,
            " DESC, p.id DESC LIMIT $3) \
             SELECT ",
            grid_columns!(),
            " FROM top JOIN part p ON p.id = top.id ",
            grid_laterals!(),
            " ORDER BY top.value DESC, top.id DESC",
        )
    };
}

pub struct PgParts(pub PgPool);

/// One value of a facet, and how many matching parts carry it. `count` is `None` past
/// [`EXACT_FACET_ROWS`], where `docs/DATA.md` §3.4 shows which values occur without counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetValue {
    pub value: String,
    pub count: Option<u64>,
}

/// Matching parts up to which a facet's counts are exact.
pub const EXACT_FACET_ROWS: i64 = 10_000;

/// Grouped `(value, count)` rows as a facet: counts kept when every matching part together
/// is within `exact_up_to`, withheld past it.
fn facet_values(rows: Vec<(String, i64)>, exact_up_to: i64) -> Vec<FacetValue> {
    let exact = rows.iter().map(|(_, count)| count).sum::<i64>() <= exact_up_to;
    rows.into_iter()
        .map(|(value, count)| FacetValue {
            value,
            count: exact.then(|| u64::try_from(count).unwrap_or(0)),
        })
        .collect()
}

#[cfg(test)]
mod facet_tests {
    use super::*;

    #[test]
    fn past_the_threshold_the_values_stay_and_the_counts_go() {
        let rows = || vec![("step".to_owned(), 3), ("stl".to_owned(), 8)];
        assert_eq!(
            facet_values(rows(), 11),
            [
                FacetValue {
                    value: "step".to_owned(),
                    count: Some(3)
                },
                FacetValue {
                    value: "stl".to_owned(),
                    count: Some(8)
                },
            ]
        );
        assert_eq!(
            facet_values(rows(), 10),
            [
                FacetValue {
                    value: "step".to_owned(),
                    count: None
                },
                FacetValue {
                    value: "stl".to_owned(),
                    count: None
                },
            ],
            "eleven matching parts is past a threshold of ten"
        );
    }
}

impl PgParts {
    /// The source formats among the parts the grid would show for the same library,
    /// category, query and state, per [`facet_values`]: narrowed by the grid's chosen material
    /// and tag, and never by its own chosen format.
    // One argument per filter the grid can hold, each narrowing the same query.
    #[allow(clippy::too_many_arguments)]
    pub async fn format_facet(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: Option<&str>,
        material: Option<&str>,
        tag: Option<&str>,
        field: Option<&str>,
        field_range: Option<&str>,
    ) -> Result<Vec<FacetValue>, DbError> {
        // The grid's own predicates, so a count never includes a part the grid would not show:
        // the library, the state, the category subtree, and `search`'s match when there is a
        // query. The format is the latest revision's source file's, as the card's is.
        //
        // ponytail: every match is counted even past the threshold, where the counts are only
        // withheld. A rollup table refreshed on ingest (`docs/DATA.md` §3.4) is the upgrade,
        // when a large library measures slow.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $2 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT s.format, count(*) FROM part p \
             JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                           ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             JOIN LATERAL (SELECT format FROM file WHERE revision_id = r.id AND role = 'source' \
                           ORDER BY created_at DESC, id DESC LIMIT 1) s ON true \
             WHERE p.library_id = $1 \
               AND ($2::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
               AND ($3::text IS NULL OR p.part_number ILIKE $4 OR p.name ILIKE $4 \
                    OR p.source_path ILIKE $4 OR p.search @@ plainto_tsquery('simple', $3)) \
               AND ($5::text IS NULL OR p.materials @> ARRAY[$5::text]) \
               AND ($6::text IS NULL OR p.tags @> ARRAY[$6::text]) \
               AND ($7::jsonb IS NULL OR p.metadata_json->'custom' @> $7::jsonb) \
               AND ($8::jsonpath IS NULL OR p.metadata_json->'custom' @? $8::jsonpath) \
             GROUP BY s.format ORDER BY s.format",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(query)
        .bind(query.map(like_pattern))
        .bind(material)
        .bind(tag)
        .bind(field)
        .bind(field_range)
        .fetch_all(&self.0)
        .await?;
        Ok(facet_values(rows, EXACT_FACET_ROWS))
    }

    /// The materials among the same parts, per [`facet_values`]: narrowed by the grid's chosen
    /// format and tag, and never by its own chosen material. Counts that obeyed their own choice would
    /// show every other material as zero; counts that ignored the format would offer parts the
    /// grid is not showing. `format_facet` keeps the same rule the other way round.
    ///
    /// `count(DISTINCT p.id)`, because a file can name one material for several bodies.
    // One argument per filter the grid can hold, each narrowing the same query.
    #[allow(clippy::too_many_arguments)]
    pub async fn material_facet(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: Option<&str>,
        format: Option<&str>,
        tag: Option<&str>,
        field: Option<&str>,
        field_range: Option<&str>,
    ) -> Result<Vec<FacetValue>, DbError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $2 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT m.material, count(DISTINCT p.id) FROM part p \
             CROSS JOIN LATERAL unnest(p.materials) AS m(material) \
             WHERE p.library_id = $1 \
               AND ($2::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
               AND ($3::text IS NULL OR p.part_number ILIKE $4 OR p.name ILIKE $4 \
                    OR p.source_path ILIKE $4 OR p.search @@ plainto_tsquery('simple', $3)) \
               AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                    AND f.format = $5 AND f.revision_id = (SELECT id FROM revision \
                    WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
               AND ($6::text IS NULL OR p.tags @> ARRAY[$6::text]) \
               AND ($7::jsonb IS NULL OR p.metadata_json->'custom' @> $7::jsonb) \
               AND ($8::jsonpath IS NULL OR p.metadata_json->'custom' @? $8::jsonpath) \
             GROUP BY m.material ORDER BY m.material",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(query)
        .bind(query.map(like_pattern))
        .bind(format)
        .bind(tag)
        .bind(field)
        .bind(field_range)
        .fetch_all(&self.0)
        .await?;
        Ok(facet_values(rows, EXACT_FACET_ROWS))
    }

    /// The tags among the same parts, per [`facet_values`]: narrowed by the grid's chosen format
    /// and material, and never by its own chosen tag, for the reason `material_facet` gives.
    // One argument per filter the grid can hold, each narrowing the same query.
    #[allow(clippy::too_many_arguments)]
    pub async fn tag_facet(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: Option<&str>,
        format: Option<&str>,
        material: Option<&str>,
        field: Option<&str>,
        field_range: Option<&str>,
    ) -> Result<Vec<FacetValue>, DbError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $2 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT t.tag, count(DISTINCT p.id) FROM part p \
             CROSS JOIN LATERAL unnest(p.tags) AS t(tag) \
             WHERE p.library_id = $1 \
               AND ($2::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
               AND ($3::text IS NULL OR p.part_number ILIKE $4 OR p.name ILIKE $4 \
                    OR p.source_path ILIKE $4 OR p.search @@ plainto_tsquery('simple', $3)) \
               AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                    AND f.format = $5 AND f.revision_id = (SELECT id FROM revision \
                    WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
               AND ($6::text IS NULL OR p.materials @> ARRAY[$6::text]) \
               AND ($7::jsonb IS NULL OR p.metadata_json->'custom' @> $7::jsonb) \
               AND ($8::jsonpath IS NULL OR p.metadata_json->'custom' @? $8::jsonpath) \
             GROUP BY t.tag ORDER BY t.tag",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(query)
        .bind(query.map(like_pattern))
        .bind(format)
        .bind(material)
        .bind(field)
        .bind(field_range)
        .fetch_all(&self.0)
        .await?;
        Ok(facet_values(rows, EXACT_FACET_ROWS))
    }

    /// How many of the same parts hold each value of the choice fields `keys`, per [`facet_values`] for
    /// each field: narrowed by the grid's chosen format, material and tag and by its field filter, except
    /// that the field filter never narrows the counts of `filtered`, the field it is on, for the reason
    /// `material_facet` gives. A part with no value for a field is not counted under it.
    // One argument per filter the grid can hold, each narrowing the same query.
    #[allow(clippy::too_many_arguments)]
    pub async fn choice_facet(
        &self,
        library: LibraryId,
        folder: Option<FolderId>,
        query: Option<&str>,
        format: Option<&str>,
        material: Option<&str>,
        tag: Option<&str>,
        field: Option<&str>,
        field_range: Option<&str>,
        filtered: Option<&str>,
        keys: &[String],
    ) -> Result<std::collections::BTreeMap<String, Vec<FacetValue>>, DbError> {
        // `jsonb_each_text` over an object only: a `custom` that is anything else holds no field values,
        // and would fail the query rather than count nothing.
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $2 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT kv.key, kv.value, count(*) FROM part p \
             CROSS JOIN LATERAL jsonb_each_text(CASE jsonb_typeof(p.metadata_json->'custom') \
                  WHEN 'object' THEN p.metadata_json->'custom' END) AS kv(key, value) \
             WHERE p.library_id = $1 \
               AND ($2::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
               AND ($3::text IS NULL OR p.part_number ILIKE $4 OR p.name ILIKE $4 \
                    OR p.source_path ILIKE $4 OR p.search @@ plainto_tsquery('simple', $3)) \
               AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                    AND f.format = $5 AND f.revision_id = (SELECT id FROM revision \
                    WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
               AND ($6::text IS NULL OR p.materials @> ARRAY[$6::text]) \
               AND ($7::text IS NULL OR p.tags @> ARRAY[$7::text]) \
               AND kv.key = ANY($8::text[]) \
               AND (kv.key = $9 \
                    OR (($10::jsonb IS NULL OR p.metadata_json->'custom' @> $10::jsonb) \
                        AND ($11::jsonpath IS NULL OR p.metadata_json->'custom' @? $11::jsonpath))) \
             GROUP BY kv.key, kv.value ORDER BY kv.key, kv.value",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(query)
        .bind(query.map(like_pattern))
        .bind(format)
        .bind(material)
        .bind(tag)
        .bind(keys)
        .bind(filtered)
        .bind(field)
        .bind(field_range)
        .fetch_all(&self.0)
        .await?;
        let mut by_key: std::collections::BTreeMap<String, Vec<(String, i64)>> =
            std::collections::BTreeMap::new();
        for (key, value, count) in rows {
            by_key.entry(key).or_default().push((value, count));
        }
        Ok(by_key
            .into_iter()
            .map(|(key, rows)| (key, facet_values(rows, EXACT_FACET_ROWS)))
            .collect())
    }

    /// Stage 4 of `docs/DATA.md` §3.1, semantic: what the file says about itself. Written after
    /// the part commits and on its own, so a refusal here leaves a part already searchable.
    ///
    /// The materials go to their column in the same statement, so the facet and the JSON they are
    /// read from agree about one part — unless a person typed the part's materials
    /// ([`PgParts::set_materials`]), which a file's statement never replaces.
    pub async fn set_metadata(
        &self,
        part: PartId,
        metadata: &serde_json::Value,
        materials: &[String],
    ) -> Result<(), DbError> {
        // `||`, not `=`: the file's own statement replaces the keys it carries (`cad`) and leaves a
        // person's `custom` values alone.
        sqlx::query(
            "UPDATE part SET metadata_json = metadata_json || $2, \
             materials = CASE WHEN materials_typed THEN materials ELSE $3 END WHERE id = $1",
        )
        .bind(part.as_uuid())
        .bind(metadata)
        .bind(materials)
        .execute(&self.0)
        .await?;
        Ok(())
    }

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
                    p.part_number, p.tags, p.materials, p.materials_typed, \
                    (p.metadata_json->'custom')::text AS custom_json, \
                    p.source_path, r.rev_label, \
                    r.triangle_count, r.is_watertight, r.bbox_x, r.bbox_y, r.bbox_z, \
                    r.volume, r.volume_source, r.surface_area, r.surface_area_source, \
                    s.blake3 AS source_blake3, s.format AS source_format, \
                    s.size_bytes AS source_size_bytes, \
                    s.stored_bytes AS source_stored_bytes, \
                    s.zstd_level AS source_zstd_level, s.storage_path, s.root AS file_root, \
                    (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
                    (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us \
             FROM part p \
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT f.blake3, f.format, f.storage_path, f.root, f.size_bytes, f.stored_bytes, f.zstd_level \
                                FROM file f \
                                WHERE f.revision_id = r.id AND f.role = 'source' \
                                ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
             WHERE p.id = $1",
        )
        .bind(part.as_uuid())
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
            custom: c
                .custom_json
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(|| serde_json::json!({})),
            tags: c.tags,
            materials: c.materials,
            materials_typed: c.materials_typed,
            source_path: c.source_path,
            rev_label: c.rev_label,
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
            source_hash,
            source_format: c.source_format,
            source_bytes: c
                .source_size_bytes
                .map(|v| bytes_column("file.size_bytes", v))
                .transpose()?,
            stored_bytes: c
                .source_stored_bytes
                .map(|v| bytes_column("file.stored_bytes", v))
                .transpose()?,
            // Keyed off the source row's presence, never off `zstd_level`'s — see
            // `PartSummary::compressed` for the whole argument.
            compressed: source_hash.map(|_| c.source_zstd_level.is_some_and(|l| l != 0)),
            directory: c.storage_path.as_deref().and_then(model_directory),
            storage_path: c.storage_path,
            root: c.file_root.as_deref().map(file_root).transpose()?,
            created_at: detail_stamp("part.created_at", c.created_us)?,
            updated_at: detail_stamp("part.updated_at", c.updated_us)?,
        }))
    }

    /// Whether a library with this id exists at all.
    ///
    /// This was `auto_thumbnail`, and it was three things at once: the ingest-time thumbnail
    /// setting (migration `0005`), and — because it was the one question this crate knew how
    /// to ask of a `library` row — the existence probe for ingest and for every route that
    /// must answer 404 before a sweep collapses "no such library" and "nothing to do" into the
    /// same empty answer. R3 dropped the setting and the column with it (`0054`), and kept
    /// the probe, renamed to say what every caller was really asking.
    ///
    /// A `bool` rather than an `Option<()>`: there is nothing left to read, only to confirm.
    pub async fn library_exists(&self, library: LibraryId) -> Result<bool, DbError> {
        Ok(
            sqlx::query_scalar::<_, bool>("SELECT true FROM library WHERE id = $1")
                .bind(library.as_uuid())
                .fetch_optional(&self.0)
                .await?
                .unwrap_or(false),
        )
    }

    /// The library's own folder, relative to the ingest mount (`0062`): `''` is the whole mount.
    /// `None` when there is no such library.
    pub async fn library_root(&self, library: LibraryId) -> Result<Option<String>, DbError> {
        Ok(sqlx::query_scalar("SELECT root FROM library WHERE id = $1")
            .bind(library.as_uuid())
            .fetch_optional(&self.0)
            .await?)
    }

    /// Delete the part, for good: its revisions, files, derivatives, pictures, sources and
    /// move trail, in one transaction. There is no undo (owner, 2026-10-17: "if it is deleted
    /// it is deleted").
    ///
    /// **The model file is never touched.** Delete takes rows out of the database and nothing
    /// else: the file at a `storage_path`, and a content-addressed source blob, stay exactly where
    /// they are. See [`delete_parts`] for which blob counts are recomputed and which are not.
    ///
    /// Returns whether there was such a part, so a second delete of the same id is a `404`.
    pub async fn delete(&self, part: PartId) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;
        // `FOR UPDATE` holds the row for the whole transaction, so a revision being recorded
        // against this part (which takes the same lock) and this delete cannot interleave.
        let found: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM part WHERE id = $1 FOR UPDATE")
                .bind(part.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        let Some(found) = found else {
            return Ok(false);
        };
        delete_parts(&mut tx, &[found]).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// The number a person gave this part, or `None` to clear it. Only a live part: a deleted
    /// one reports `false`, so an edit nobody could see is a 404 rather than a silent write.
    pub async fn set_part_number(
        &self,
        part: PartId,
        number: Option<&str>,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE part SET part_number = $2, updated_at = now() \
             WHERE id = $1",
        )
        .bind(part.as_uuid())
        .bind(number)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// The tags a person gave this part, replacing the ones it had; an empty list clears them.
    /// Only a live part, for [`PgParts::set_part_number`]'s reason.
    pub async fn set_tags(&self, part: PartId, tags: &[String]) -> Result<bool, DbError> {
        let result = sqlx::query("UPDATE part SET tags = $2, updated_at = now() WHERE id = $1")
            .bind(part.as_uuid())
            .bind(tags)
            .execute(&self.0)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// The materials a person gave this part, replacing the ones it had and kept over whatever its file
    /// states from then on. Only a live part, for [`PgParts::set_part_number`]'s reason.
    ///
    /// **An empty list is a decision, not a reset.** A part whose file names a material is a part
    /// somebody may know holds none — a machined blank, a file whose header names the stock it was cut
    /// from — and before L3 there was no way to say so: the empty list handed the part straight back to
    /// its file, which named the material again. So typed-and-empty is its own state, `materials_typed`
    /// with `materials = '{}'`, which `set_metadata` already respects like any other typed list.
    /// [`PgParts::unset_materials`] is the reset.
    pub async fn set_materials(&self, part: PartId, materials: &[String]) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE part SET materials = $2::text[], materials_typed = true, updated_at = now() \
             WHERE id = $1",
        )
        .bind(part.as_uuid())
        .bind(materials)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Hand the part back to its file: the materials its file stated when last read
    /// (`metadata_json.cad.materials`) at once, and each later read's after that. The undo of
    /// [`PgParts::set_materials`], whichever list it wrote — including the empty one.
    pub async fn unset_materials(&self, part: PartId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE part SET \
               materials = ARRAY(SELECT jsonb_array_elements_text( \
                 CASE WHEN jsonb_typeof(metadata_json->'cad'->'materials') = 'array' \
                      THEN metadata_json->'cad'->'materials' ELSE '[]'::jsonb END)), \
               materials_typed = false, \
               updated_at = now() \
             WHERE id = $1",
        )
        .bind(part.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Which library owns `part`. `None` when there is no such part.
    ///
    /// This is how a part-scoped route stays tenant-safe without a library in its path:
    /// the library it enqueues under is read off the part rather than taken from the
    /// caller, so a job can only ever name a revision of the library that owns it. Taking
    /// both from the caller would let any pair be posted together, and `CLAUDE.md`'s
    /// "content addressing is not authorization" applies to a part id exactly as
    /// `jobs.rs` applies it to a batch id — scoping makes the check structural instead of
    /// a step someone can forget.
    pub async fn library_of(&self, part: PartId) -> Result<Option<LibraryId>, DbError> {
        let id: Option<Uuid> = sqlx::query_scalar("SELECT library_id FROM part WHERE id = $1")
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
    /// The hash of `revision`'s newest derivative of `kind`, or `None` when it has none yet.
    /// Hash-addressed kinds only: a thumbnail lives inline in its row and has no hash to give.
    pub async fn derivative_hash(
        &self,
        revision: RevisionId,
        kind: DerivativeKind,
    ) -> Result<Option<BlobHash>, DbError> {
        let hex: Option<String> = sqlx::query_scalar(
            "SELECT blake3 FROM derivative WHERE revision_id = $1 AND kind = $2 \
               AND blake3 IS NOT NULL ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .bind(kind.as_str())
        .fetch_optional(&self.0)
        .await?;
        detail_hash("derivative.blake3", hex)
    }

    /// The exports stored as `hash`, on parts not deleted: what the blob route rebuilds when
    /// those bytes have gone from the store.
    ///
    /// **Only what something still builds.** This was `rungs_for_blob` and it named the three
    /// tessellations; `blob.rs` turns a non-empty answer into "that file is being rebuilt",
    /// which after R3 would have been a promise nothing could keep for a rung. A gallery image
    /// is not rebuilt this way either — it is bytes a person uploaded, and the route says so.
    pub async fn exports_for_blob(
        &self,
        hash: &BlobHash,
    ) -> Result<Vec<(LibraryId, RevisionId, DerivativeKind)>, DbError> {
        let rebuildable = DerivativeKind::ALL;
        let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT p.library_id, d.revision_id, d.kind FROM derivative d \
             JOIN revision r ON r.id = d.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE d.blake3 = $1 AND d.kind = ANY($2)",
        )
        .bind(hash.to_hex())
        .bind(rebuildable.map(DerivativeKind::as_str).to_vec())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(library, revision, kind)| {
                let kind = rebuildable.into_iter().find(|made| made.as_str() == kind)?;
                Some((
                    LibraryId::from_uuid(library),
                    RevisionId::from_uuid(revision),
                    kind,
                ))
            })
            .collect())
    }

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
    /// A deleted part has no revision left, so a `derive` job queued before the delete finds
    /// nothing here.
    pub async fn revision_source(
        &self,
        library: LibraryId,
        revision: RevisionId,
    ) -> Result<Option<RevisionSource>, DbError> {
        #[allow(clippy::type_complexity)]
        let row: Option<(String, String, Option<String>, Option<i16>, String, bool)> = sqlx::query_as(
            "SELECT f.blake3, f.format, f.storage_path, f.zstd_level, f.root, f.on_disk FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id AND p.library_id = $2 \
             WHERE f.revision_id = $1 AND f.role = 'source' \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((hex, format, storage_path, zstd_level, root, on_disk)) = row else {
            return Ok(None);
        };
        let root = file_root(&root)?;
        let parsed = BlobHash::parse_hex(&hex);
        let hash = parsed.map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some(RevisionSource {
            hash,
            format,
            root,
            storage_path,
            zstd_level,
            on_disk,
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
    /// A download URL held from before a delete does not outlive it: the part's rows are gone,
    /// so this answers `None`.
    ///
    /// `zstd_level` comes off the `file` row itself — never from
    /// the old per-format compression policy. That is ingest-time policy and slice 7 is about to
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
        let row: Option<(
            String,
            String,
            String,
            Option<String>,
            Option<i16>,
            i64,
            String,
            bool,
            Option<i64>,
        )> = sqlx::query_as(
            "SELECT f.blake3, f.format, p.name, f.storage_path, f.zstd_level, f.size_bytes, \
                        f.root, f.on_disk, f.mtime_ns \
             FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE f.revision_id = $1 AND f.role = 'source' \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((
            hex,
            format,
            part_name,
            storage_path,
            zstd_level,
            size_bytes,
            root,
            on_disk,
            mtime_ns,
        )) = row
        else {
            return Ok(None);
        };
        let root = file_root(&root)?;
        let hash = BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some(DownloadSource {
            hash,
            size_bytes,
            format,
            part_name,
            root,
            storage_path,
            on_disk,
            mtime_ns,
            zstd_level,
        }))
    }

    /// Every part of `parts` that is in `library`, not removed and holding a revision, in the order asked, with
    /// its sources and every revision's download source, in one query. A part missing from the answer is one a
    /// bundle cannot hold, and the caller refuses the bundle for it.
    ///
    /// The revisions are [`PgRevisions::history`]'s, oldest first. Each file is [`Self::source_for_download`]'s,
    /// by the same `role = 'source'` filter and ordering, and the sources are [`Self::part_sources`]'s, in its
    /// order. Each source field is aggregated on its own, all in that one order, so the arrays line up, and once per
    /// part rather than once per revision.
    ///
    /// [`PgRevisions::history`]: crate::PgRevisions::history
    pub async fn bundle_parts(
        &self,
        library: LibraryId,
        parts: &[PartId],
    ) -> Result<Vec<BundlePartRow>, DbError> {
        let ids: Vec<Uuid> = parts.iter().map(|part| part.as_uuid()).collect();
        let rows: Vec<BundleColumns> = sqlx::query_as(
            "WITH chosen AS MATERIALIZED ( \
               SELECT p.id, p.name, p.part_number, p.source_path, p.tags, wanted.position, \
                      ps.urls, ps.vendors, ps.external_ids, ps.titles, ps.licenses \
                 FROM unnest($2::uuid[]) WITH ORDINALITY AS wanted(id, position) \
                 JOIN part p ON p.id = wanted.id AND p.library_id = $1 \
                 LEFT JOIN LATERAL (SELECT array_agg(url ORDER BY created_at, id) AS urls, \
                                           array_agg(vendor ORDER BY created_at, id) AS vendors, \
                                           array_agg(external_id ORDER BY created_at, id) AS external_ids, \
                                           array_agg(title ORDER BY created_at, id) AS titles, \
                                           array_agg(license ORDER BY created_at, id) AS licenses \
                                    FROM part_source WHERE part_id = p.id) ps ON true) \
             SELECT c.id AS part_id, c.name, c.part_number, c.source_path, c.tags, \
                    c.urls, c.vendors, c.external_ids, c.titles, c.licenses, \
                    r.id AS revision_id, r.parent_revision_id, r.rev_label, r.origin, \
                    (extract(epoch FROM r.created_at) * 1000000)::bigint AS created_us, \
                    f.blake3, f.format, f.storage_path, f.zstd_level, f.size_bytes, \
                    f.root, f.on_disk, f.mtime_ns \
             FROM chosen c \
             JOIN revision r ON r.part_id = c.id \
             LEFT JOIN LATERAL (SELECT blake3, format, storage_path, zstd_level, size_bytes, \
                                       root, on_disk, mtime_ns FROM file \
                                WHERE revision_id = r.id AND role = 'source' \
                                ORDER BY created_at DESC, id DESC LIMIT 1) f ON true \
             ORDER BY c.position, r.created_at, r.id",
        )
        .bind(library.as_uuid())
        .bind(&ids)
        .fetch_all(&self.0)
        .await?;

        let mut bundled: Vec<BundlePartRow> = Vec::new();
        for c in rows {
            if bundled
                .last()
                .is_none_or(|part| part.id.as_uuid() != c.part_id)
            {
                let texts =
                    |values: Option<Vec<Option<String>>>| values.unwrap_or_default().into_iter();
                let sources = texts(c.urls)
                    .zip(texts(c.vendors))
                    .zip(texts(c.external_ids))
                    .zip(texts(c.titles))
                    .zip(texts(c.licenses))
                    .map(
                        |((((url, vendor), external_id), title), license)| BundleSource {
                            url,
                            vendor,
                            external_id,
                            title,
                            license,
                        },
                    )
                    .collect();
                bundled.push(BundlePartRow {
                    id: PartId::from_uuid(c.part_id),
                    name: c.name,
                    part_number: c.part_number,
                    source_path: c.source_path,
                    tags: c.tags,
                    sources,
                    revisions: Vec::new(),
                });
            }
            let Some(part) = bundled.last_mut() else {
                continue;
            };
            let origin = lapidary_core::RevisionOrigin::parse(&c.origin).ok_or_else(|| {
                DbError::UnknownOrigin {
                    value: c.origin.clone(),
                }
            })?;
            let source = match (c.blake3, c.format, c.size_bytes, c.root) {
                (Some(hex), Some(format), Some(size_bytes), Some(root)) => Some(DownloadSource {
                    hash: BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                        column: "file.blake3",
                        value: hex,
                    })?,
                    size_bytes,
                    format,
                    part_name: part.name.clone(),
                    root: file_root(&root)?,
                    storage_path: c.storage_path,
                    on_disk: c.on_disk.unwrap_or(false),
                    mtime_ns: c.mtime_ns,
                    zstd_level: c.zstd_level,
                }),
                _ => None,
            };
            part.revisions.push(BundleRevisionRow {
                id: RevisionId::from_uuid(c.revision_id),
                parent: c.parent_revision_id.map(RevisionId::from_uuid),
                rev_label: c.rev_label,
                origin,
                created_at: detail_stamp("revision.created_at", c.created_us)?,
                source,
            });
        }
        Ok(bundled)
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
    /// **Inline thumbnails were added to the derivative total from `octet_length` and are
    /// not any more.** They were real bytes a library cost, `DATA.md` §1.5's deliberate
    /// exception to "blobs never live in Postgres", and leaving them out would have reported
    /// `0 B` of derivatives over a library holding megabytes of previews. Migration `0055`
    /// deleted every such row and nothing writes `thumb_bytes` now, so what is left of that
    /// clause is `+ 0` over every library in every installation — arithmetic that cannot
    /// move, in a figure somebody will one day have to check. The omission the rule forbids
    /// is a figure that under-reports what is there; there is nothing there.
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
    /// Both figures match [`PartRepository::page`], so the panel that sits over the grid can
    /// be checked against the cards in it.
    ///
    /// **A deleted part leaves this panel at once, and its model file stays on the disk.**
    /// Delete takes the rows and never the file (owner, 2026-10-17), so the panel drops by the
    /// part's size while the volume does not: the figure is what the library catalogues, not
    /// what the disk holds. The instance view's directory walk answers the second question.
    ///
    /// **Quarantined bytes are not here, and cannot be.** A quarantined blob has nothing
    /// pointing at it and therefore no library: the figure is library-less by construction.
    /// It belongs to [`Self::instance_storage`].
    pub async fn storage_totals(
        &self,
        library: LibraryId,
    ) -> Result<Option<StorageTotals>, DbError> {
        // `sum()` over a bigint column is `numeric`, which sqlx will not decode into
        // i64 — hence the `::bigint` casts, not decoration.
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
             FROM file f \
             JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id \
             AND f.role = 'source'), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
             WHERE b.blake3 IN (SELECT d.blake3 FROM derivative d \
             JOIN revision r ON r.id = d.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id)) \
             FROM library l WHERE l.id = $1",
        )
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((source, derivative)) = row else {
            return Ok(None);
        };
        Ok(Some(StorageTotals {
            source_bytes: bytes_column("file.stored_bytes", source)?,
            derivative_bytes: bytes_column("blob.stored_bytes", derivative)?,
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
    /// construction (`DATA.md` §1.6): a quarantined blob is keyed by hash, and nothing points
    /// at it any more to say which library it belonged to. That is exactly why a per-library
    /// panel cannot show them and an instance-wide one must — they are bytes on the disk, for
    /// up to thirty days, that no other figure admits to.
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
        let (source, derivative, quarantined, cache): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT \
             (SELECT coalesce(sum(coalesce(f.stored_bytes, f.size_bytes)), 0)::bigint \
              FROM file f JOIN revision r ON r.id = f.revision_id \
              WHERE f.role = 'source' AND f.root = 'store'), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
              WHERE b.blake3 IN (SELECT d.blake3 FROM derivative d WHERE d.blake3 IS NOT NULL)), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
              WHERE b.quarantined_at IS NOT NULL), \
             (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
              WHERE b.quarantined_at IS NULL \
                AND coalesce(b.last_accessed_at, b.created_at) < now() - interval '90 days' \
                AND EXISTS (SELECT 1 FROM derivative d \
                            WHERE d.blake3 = b.blake3 AND d.kind = ANY($1)) \
                AND NOT EXISTS (SELECT 1 FROM derivative d \
                                WHERE d.blake3 = b.blake3 AND NOT d.kind = ANY($1)) \
                AND NOT EXISTS (SELECT 1 FROM file f WHERE f.blake3 = b.blake3))",
        )
        .bind(cache_kinds())
        .fetch_one(&self.0)
        .await?;

        Ok(InstanceStorage {
            source_bytes: bytes_column("file.stored_bytes", source)?,
            derivative_bytes: bytes_column("blob.stored_bytes", derivative)?,
            quarantined_bytes: bytes_column("blob.stored_bytes", quarantined)?,
            render_cache_bytes: bytes_column("blob.stored_bytes", cache)?,
        })
    }

    /// "Free cache space" (`DATA.md` §1.5): remove every slicer export row whose blob nobody
    /// has read in 90 days, then recount those blobs as delete does, so a blob nothing else
    /// points at enters the thirty-day quarantine and the hourly sweep takes its bytes after.
    ///
    /// Rows, not only bytes: a row naming bytes that are gone would answer 404 for good, where
    /// an export whose row is gone is written again when it is next asked for. **Every source
    /// file stays**, and so does anything of a kind nothing can rebuild — the predicate is
    /// `cache_kinds`, not "every derivative", because what makes a row cache is that something
    /// can write it again. Nothing a person opens is touched: a part's picture is a
    /// `part_image` row and **view** hands the browser the original mesh.
    pub async fn free_render_cache(&self) -> Result<RenderCacheFreed, DbError> {
        let mut tx = self.0.begin().await?;
        // **Two clauses name a kind, and the second one is the guard.** Measured by mutation
        // (R4): widening `d.kind = ANY($1)` to match every row still evicts nothing of a kind
        // nothing can rebuild, because such a row is itself an `o` the `NOT EXISTS` below finds
        // on its own blob. So the first clause chooses what to sweep and the second is what
        // keeps an unrebuildable row — and the bytes it is the only copy of — out of a cache
        // sweep. Dropping either on its own changes nothing, which is exactly how both come to
        // be dropped. Neither is redundant.
        let mut hashes: Vec<String> = sqlx::query_scalar(
            "DELETE FROM derivative d USING blob b \
             WHERE b.blake3 = d.blake3 AND d.kind = ANY($1) \
               AND coalesce(b.last_accessed_at, b.created_at) < now() - interval '90 days' \
               AND NOT EXISTS (SELECT 1 FROM derivative o \
                               WHERE o.blake3 = b.blake3 AND NOT o.kind = ANY($1)) \
               AND NOT EXISTS (SELECT 1 FROM file f WHERE f.blake3 = b.blake3) \
             RETURNING d.blake3",
        )
        .bind(cache_kinds())
        .fetch_all(&mut *tx)
        .await?;
        let removed = hashes.len() as u64;
        hashes.sort_unstable();
        hashes.dedup();
        let sizes = recount(&mut tx, &hashes).await?;
        tx.commit().await?;

        let mut quarantined_bytes = 0u64;
        for stored in sizes.into_iter().flatten() {
            quarantined_bytes += bytes_column("blob.stored_bytes", stored)?;
        }
        Ok(RenderCacheFreed {
            removed,
            quarantined_bytes,
        })
    }

    /// Everything the move route has to know before it can decide where a part goes.
    ///
    /// One query rather than four, and the same LATERALs the grid page uses, so the
    /// directory a move renames is the directory the card showed. `Ok(None)` is "no such
    /// live part" — a deleted or unknown id — which the route answers 404 for.
    pub async fn move_source(&self, part: PartId) -> Result<Option<MoveSource>, DbError> {
        let row: Option<(Uuid, Option<Uuid>, String)> =
            sqlx::query_as("SELECT library_id, folder_id, name FROM part WHERE id = $1")
                .bind(part.as_uuid())
                .fetch_optional(&self.0)
                .await?;
        Ok(row.map(|(library, folder, name)| MoveSource {
            library: LibraryId::from_uuid(library),
            folder: folder.map(FolderId::from_uuid),
            name,
        }))
    }

    /// Is another part in `folder` already called `name`?
    ///
    /// `except` drops the part being moved, so re-filing a model into the category it is
    /// already in is a no-op rather than a collision with itself.
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
               AND name = $3 AND id <> $4)",
        )
        .bind(library.as_uuid())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(name)
        .bind(except.as_uuid())
        .fetch_one(&self.0)
        .await?)
    }

    /// File a part under a different category. **A database change and nothing else** (M4): a
    /// category is the library's own grouping, and the file stays exactly where it is on disk.
    pub async fn move_to_folder(
        &self,
        part: PartId,
        from: Option<FolderId>,
        to: Option<FolderId>,
    ) -> Result<(), DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query("UPDATE part SET folder_id = $2, updated_at = now() WHERE id = $1")
            .bind(part.as_uuid())
            .bind(to.map(|f| f.as_uuid()))
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

/// `file.root` read back. Refused rather than defaulted: a guessed root is a guessed folder.
pub(crate) fn file_root(value: &str) -> Result<lapidary_core::FileRoot, DbError> {
    lapidary_core::FileRoot::parse(value).ok_or_else(|| DbError::UnknownRoot {
        value: value.to_owned(),
    })
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
    async fn page(&self, grid: &GridQuery<'_>, sort: Sort) -> Result<Vec<PartRow>, DbError> {
        let sorted = match sort {
            Sort::Newest => None,
            Sort::Volume => Some(sorted_page!("coalesce(p.latest_volume, '-infinity')")),
            Sort::SurfaceArea => Some(sorted_page!("coalesce(p.latest_surface_area, '-infinity')")),
            Sort::LongestSide => Some(sorted_page!("coalesce(p.latest_longest_side, '-infinity')")),
            Sort::Triangles => Some(sorted_page!(
                "coalesce(p.latest_triangle_count::double precision, '-infinity')"
            )),
        };
        if let Some(query) = sorted {
            return self.sorted(grid, query).await;
        }
        let GridQuery {
            library,
            folder,
            after,
            limit,
            format,
            material,
            tag,
            field,
            field_range,
        } = *grid;
        // One query: thumbnails travel inline as bytea rather than costing a round trip
        // per card. Keyset, not OFFSET — OFFSET degrades as the library grows.
        //
        // sqlx cannot decode jiff::Timestamp (it ships chrono/time support, not jiff), so
        // timestamps are pulled out as epoch microseconds and reassembled below rather
        // than adding a second date-time crate just to carry the value across.
        //
        // Every LATERAL below picks one row deterministically out of a set that could
        // hold more than one, newest (`created_at DESC, id DESC`) first: the revision
        // LATERAL because a part could in principle carry more than one revision, and the
        // picture LATERAL because a part legitimately carries several `part_image` rows.
        // A plain (non-LATERAL) LEFT JOIN would fan out on those: several rows for one
        // part means several identical grid cards for it, and a page of `limit` rows
        // holding fewer than `limit` distinct parts, silently under-reporting `next`.
        //
        // **R3 removed two of them** — the thumbnail and the L0 rung — which is why this
        // query binds ten parameters where it used to bind twelve. The derivative table is
        // not read by the grid at all now.
        //
        // The source LATERAL is a LEFT one for the picture's reason: a revision whose source `file` row is missing is a
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
        // `is_watertight` is deliberately not selected: nothing on a card reads it.
        // `GridRow` is a named struct, so a card that ever needs it costs a field.
        //
        // The folder filter is the recursive CTE at the top, and it is inline here rather
        // than a separate "give me the descendants" call for one reason: the descent and
        // the page are one question — "the cards in this category" — and splitting them
        // would put query composition in `lapidary-api` (an id list bound into a filter) or
        // give the tree two descent implementations to keep in step. `down` is empty and
        // costs nothing when `$5` is NULL, and the `IS NULL` guard beside it is what makes
        // an absent filter mean the whole library.
        let rows: Vec<GridRow> = sqlx::query_as(concat!(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $4 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen \
             SELECT ",
            grid_columns!(),
            " FROM part p ",
            grid_laterals!(),
            " WHERE p.library_id = $1 \
               AND ($2::uuid IS NULL OR p.id < $2) \
               AND ($4::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
               AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                    AND f.format = $5 AND f.revision_id = (SELECT id FROM revision \
                    WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
               AND ($6::text IS NULL OR p.materials @> ARRAY[$6::text]) \
               AND ($7::text IS NULL OR p.tags @> ARRAY[$7::text]) \
               AND ($8::jsonb IS NULL OR p.metadata_json->'custom' @> $8::jsonb) \
               AND ($9::jsonpath IS NULL OR p.metadata_json->'custom' @? $9::jsonpath) \
             ORDER BY p.id DESC LIMIT $3",
        ))
        .bind(library.as_uuid())
        .bind(after.map(|a| a.as_uuid()))
        .bind(i64::from(limit))
        .bind(folder.map(|f| f.as_uuid()))
        .bind(format)
        .bind(material)
        .bind(tag)
        .bind(field)
        .bind(field_range)
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

    async fn search(&self, grid: &GridQuery<'_>, query: &str) -> Result<Vec<PartRow>, DbError> {
        let GridQuery {
            library,
            folder,
            after,
            limit,
            format,
            material,
            tag,
            field,
            field_range,
        } = *grid;
        // Same seventeen columns, same LATERALs, same subtree filter.
        // What differs is which parts are candidates and in what order they come back.
        //
        // # The four tiers, and why they are 8/4/2/1 rather than 4/2/1
        //
        // Identifier, then title, then word, then filename — a part's name is usually
        // derived from its filename, so a path hit that is *not* also a name hit is the
        // weaker signal by construction: a directory fragment, or a filename that outlived
        // a rename of the part.
        //
        // **The tiers double because they must sum without crossing.** When `source_path`
        // joined at 1 under the old 4/2/1, the tiers stopped being ordered: `source_path`
        // repeats the filename that a part's `name` was derived from, so a name hit almost
        // always carries a path hit *and* a `tsvector` hit with it — 2 + 1 + 1 + `ts_rank`
        // is more than the 4 a bare `part_number` match scores, and the part merely *named*
        // for a fragment overtook the part identified by it. That is `ROADMAP.md`'s Phase 2
        // exit criterion, and the test guarding it is what caught this.
        //
        // Powers of two fix it by construction rather than by argument: every lower tier
        // added together (4 + 2 + 1) is less than the tier above them, so no accumulation
        // of weak evidence can outrank one strong match. Which is what a tier *is*.
        //
        // `least(…, 0.999)` is the same property for the fraction. `ts_rank` is documented
        // as unbounded — it grows with the number of matching query terms, measured at 0.24
        // for one and 0.40 for six on this corpus — and one that reached 1.0 would carry a
        // tsvector hit into the tier above it. Capping it keeps it what it is meant to be:
        // a tiebreaker *within* the word tier, never a promotion out of it.
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
             SELECT id FROM folder WHERE id = $4 \
             UNION ALL \
             SELECT f.id FROM folder f \
             JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen, \
             hits AS ( \
               SELECT p.id, \
                      ( (coalesce(p.part_number, '') ILIKE $6)::int * 8 \
                      + (p.name ILIKE $6)::int * 4 \
                      + (p.search @@ plainto_tsquery('simple', $5))::int * 2 \
                      + (p.source_path ILIKE $6)::int \
                      + least(ts_rank(p.search, plainto_tsquery('simple', $5)), 0.999) ) AS rank \
                 FROM part p \
                WHERE p.library_id = $1 \
                  AND ($4::uuid IS NULL OR p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
                  AND ($7::text IS NULL OR EXISTS (SELECT 1 FROM file f WHERE f.role = 'source' \
                       AND f.format = $7 AND f.revision_id = (SELECT id FROM revision \
                       WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1))) \
                  AND ($8::text IS NULL OR p.materials @> ARRAY[$8::text]) \
                  AND ($9::text IS NULL OR p.tags @> ARRAY[$9::text]) \
                  AND ($10::jsonb IS NULL OR p.metadata_json->'custom' @> $10::jsonb) \
                  AND ($11::jsonpath IS NULL OR p.metadata_json->'custom' @? $11::jsonpath) \
                  AND ( p.part_number ILIKE $6 \
                     OR p.name ILIKE $6 \
                     OR p.source_path ILIKE $6 \
                     OR p.search @@ plainto_tsquery('simple', $5) ) ), \
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
        .bind(folder.map(|f| f.as_uuid()))
        // The raw query, for `plainto_tsquery`, which must not see the LIKE escapes.
        .bind(query)
        // And the escaped one, wrapped. Two bindings of one input, and they are not
        // interchangeable.
        .bind(like_pattern(query))
        .bind(format)
        .bind(material)
        .bind(tag)
        .bind(field)
        .bind(field_range)
        .fetch_all(&self.0)
        .await?;

        rows.into_iter().map(to_part_row).collect()
    }
}

impl PgParts {
    /// [`PartRepository::page`] in an order other than newest: `query` is [`sorted_page!`] for
    /// one key.
    ///
    /// The key is read off the part row, where every revision write copies its latest
    /// revision's figures (`0035`), so the page is read off that key's index in order rather
    /// than sorting the library. The figures are stored values compared as they are, so the
    /// comparison is exact.
    ///
    /// **A missing figure is `-infinity`, not NULL.** A NULL inside a row comparison makes the
    /// whole comparison NULL, so with `NULLS LAST` the page after the first part without a
    /// volume would come back empty. `-infinity` sorts below every real figure and ties the
    /// missing ones on id.
    ///
    /// `anchor` reads the previous page's last part by id, filters aside, so a part removed
    /// since that page still marks where the next one starts. Its key is read when the next page
    /// is asked for, though: a part revised since then moves the anchor with it, and the next
    /// page repeats rows or skips them. One deleted since ends the paging early, as `search`'s does.
    async fn sorted(
        &self,
        grid: &GridQuery<'_>,
        query: &'static str,
    ) -> Result<Vec<PartRow>, DbError> {
        let rows: Vec<GridRow> = sqlx::query_as(query)
            .bind(grid.library.as_uuid())
            .bind(grid.after.map(|a| a.as_uuid()))
            .bind(i64::from(grid.limit))
            .bind(grid.folder.map(|f| f.as_uuid()))
            .bind(grid.format)
            .bind(grid.material)
            .bind(grid.tag)
            .bind(grid.field)
            .bind(grid.field_range)
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
        self.insert_part_image(part, image, false)
            .await
            .map(|id| id.expect("an unconditional insert always inserts"))
    }

    /// [`add_part_image`](Self::add_part_image) for a picture the application found rather than
    /// one a person chose — a 3MF's own thumbnail — so it goes only to a part that has none.
    /// `Ok(None)` when the part already has a picture, which is the person's and always wins.
    ///
    /// The part row is locked before the gallery is counted, so an upload landing at the same
    /// moment either comes first and this adds nothing, or comes second and sits after it.
    pub async fn adopt_part_image(
        &self,
        part: PartId,
        image: NewPartImage<'_>,
    ) -> Result<Option<PartImageId>, DbError> {
        self.insert_part_image(part, image, true).await
    }

    async fn insert_part_image(
        &self,
        part: PartId,
        image: NewPartImage<'_>,
        only_if_none: bool,
    ) -> Result<Option<PartImageId>, DbError> {
        let mut tx = self.0.begin().await?;
        if only_if_none {
            sqlx::query("SELECT 1 FROM part WHERE id = $1 FOR UPDATE")
                .bind(part.as_uuid())
                .execute(&mut *tx)
                .await?;
        }

        let next: i32 = sqlx::query_scalar(
            "SELECT coalesce(max(position), -1) + 1 FROM part_image WHERE part_id = $1",
        )
        .bind(part.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if only_if_none && next > 0 {
            tx.rollback().await?;
            return Ok(None);
        }

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
        Ok(Some(id))
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
    /// `uploaded`, `url_supplied`, `embedded` (a 3MF's own thumbnail) or `snapshot` (the
    /// viewer's) — `part_image_known_origin`
    /// is what refuses anything else, so a typo here is a failed insert rather than a value
    /// no reader understands.
    pub origin: &'a str,
    /// Where it was fetched from, for one that was. Never used to load the image: our own
    /// copy is what is served, because hotlinking leaks a referrer on every grid scroll.
    pub source_url: Option<&'a str>,
}

/// One grid row, decoded. `page`, `search` and the sorted pages select the same columns
/// ([`grid_columns`]) and all end here, so a card cannot mean one thing on the grid and
/// another in a set of results.
///
/// A named `FromRow` struct since the seventeenth column went past sqlx's
/// sixteen-tuple ceiling. The SELECT aliases every column, including the three `id`s and
/// three `blake3`s that kept this positional before, so a column is decoded by what it is
/// called rather than by where it sits — **which is also why adding a column here is a
/// field and nothing more**, and why a card that ever needs one costs a field.
///
/// Decoded by name at run time, so a query that selects fewer columns than this struct
/// declares still compiles and fails only when something reads it. Every grid query goes
/// through [`grid_columns`] for that reason; do not hand-write a SELECT into `GridRow`.
#[derive(sqlx::FromRow)]
struct GridRow {
    id: Uuid,
    library_id: Uuid,
    revision_id: Uuid,
    name: String,
    part_number: Option<String>,
    source_path: String,
    triangle_count: Option<i32>,
    source_blake3: Option<String>,
    source_bytes: Option<i64>,
    stored_bytes: Option<i64>,
    zstd_level: Option<i16>,
    storage_path: Option<String>,
    file_root: Option<String>,
    created_us: i64,
    updated_us: i64,
    source_format: Option<String>,
    /// The part's first picture, from the `img` LATERAL. Every column is optional because
    /// the join is a LEFT one and a part may have no picture; `image_id` being present is
    /// what says the row exists, and the four NOT NULL columns beside it are present
    /// whenever it is.
    image_id: Option<Uuid>,
    image_webp: Option<Vec<u8>>,
    image_blake3: Option<String>,
    image_origin: Option<String>,
    image_source_url: Option<String>,
    image_position: Option<i32>,
    image_fit: Option<String>,
    image_focus_x: Option<f64>,
    image_focus_y: Option<f64>,
}

fn to_part_row(row: GridRow) -> Result<PartRow, DbError> {
    fn bytes(column: &'static str, value: Option<i64>) -> Result<Option<u64>, DbError> {
        value.map(|v| bytes_column(column, v)).transpose()
    }

    let GridRow {
        id,
        library_id: lib,
        revision_id: revision,
        name,
        part_number,
        source_path,
        triangle_count: triangles,
        source_blake3: source_hash,
        source_bytes,
        stored_bytes,
        zstd_level,
        storage_path,
        file_root: root,
        created_us,
        updated_us,
        source_format,
        image_id,
        image_webp,
        image_blake3,
        image_origin,
        image_source_url,
        image_position,
        image_fit,
        image_focus_x,
        image_focus_y,
    } = row;
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
        // Keyed off the source row's presence, never off `zstd_level`'s:
        // the column is nullable, so a `None` level on a row that exists
        // means "nobody recorded how these bytes were stored", which is a
        // different fact from "this revision has no source file" and must
        // not collapse into it. The predicate matches `LibraryReader`'s
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
        // The card's picture, when the part has one. Refused rather than dropped on a
        // corrupt hash, for the reason the two hashes above are: reporting it as "this part
        // has no picture" would hide the corruption behind an ordinary-looking state.
        //
        // `unwrap_or` on the four NOT NULL columns, never `unwrap`: they are `Option` only
        // because the join is a LEFT one, and `image_id` has already said the row is there.
        // The fallbacks are `0019`'s own defaults, so a column that somehow arrived NULL
        // frames the picture the way an unadjusted one is framed rather than failing.
        let image = image_id
            .map(|id| -> Result<PartImageRow, DbError> {
                Ok(PartImageRow {
                    id: PartImageId::from_uuid(id),
                    inline_webp: image_webp,
                    hash: image_blake3
                        .map(|hex| {
                            BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
                                column: "part_image.blake3",
                                value: hex,
                            })
                        })
                        .transpose()?,
                    origin: image_origin.unwrap_or_default(),
                    source_url: image_source_url,
                    position: image_position.unwrap_or(0),
                    fit: image_fit.unwrap_or_else(|| "cover".to_owned()),
                    focus: (image_focus_x.unwrap_or(0.5), image_focus_y.unwrap_or(0.5)),
                })
            })
            .transpose()?;
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
                triangle_count,
                // Every figure on a mesh part is tessellated, so any is all.
                approximate: true,
                source_hash,
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
            source_format,
            image,
            directory: storage_path.as_deref().and_then(model_directory),
            storage_path,
            root: root.as_deref().map(file_root).transpose()?,
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
        let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
            "SELECT l.id, l.name, \
                    (SELECT count(*) FROM part p \
                      WHERE p.library_id = l.id) \
               FROM library l ORDER BY l.created_at, l.id",
        )
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, part_count)| LibraryRow {
                id: LibraryId::from_uuid(id),
                name,
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
    /// `root` is the library's folder under the ingest mount, `''` for the whole of it. The
    /// caller has refused one that escapes the mount.
    pub async fn create_library(&self, name: &str, root: &str) -> Result<LibraryId, DbError> {
        let id = LibraryId::new();
        // Derived here and never sent by a client, exactly as a category's is: `slugify` is
        // the one place that decides what a filesystem may hold, and a caller who could name
        // the directory could name one outside the store. Lowercased because that is what
        // `library_slug` has always returned and what every existing directory is called.
        let slug = lapidary_core::slug::slugify(name).to_lowercase();
        sqlx::query("INSERT INTO library (id, name, slug, root) VALUES ($1, $2, $3, $4)")
            .bind(id.as_uuid())
            .bind(name)
            .bind(&slug)
            .bind(root)
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

/// One part of a bundle, as [`PgParts::bundle_parts`] reads it.
#[derive(Debug)]
pub struct BundlePartRow {
    pub id: PartId,
    pub name: String,
    pub part_number: Option<String>,
    pub source_path: String,
    pub tags: Vec<String>,
    /// Where it came from, oldest first.
    pub sources: Vec<BundleSource>,
    /// Every revision, oldest first.
    pub revisions: Vec<BundleRevisionRow>,
}

/// The fields of a part's source that a bundle's manifest carries.
#[derive(Debug)]
pub struct BundleSource {
    pub url: Option<String>,
    pub vendor: Option<String>,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub license: Option<String>,
}

/// One revision of a bundled part, and the file a bundle reads for it.
#[derive(Debug)]
pub struct BundleRevisionRow {
    pub id: RevisionId,
    pub parent: Option<RevisionId>,
    pub rev_label: String,
    pub origin: lapidary_core::RevisionOrigin,
    pub created_at: jiff::Timestamp,
    /// `None` for a revision with no source file, which a bundle refuses.
    pub source: Option<DownloadSource>,
}

/// [`PgParts::bundle_parts`]'s row: one per revision, its part's columns beside it.
#[derive(sqlx::FromRow)]
struct BundleColumns {
    part_id: Uuid,
    name: String,
    part_number: Option<String>,
    source_path: String,
    tags: Vec<String>,
    urls: Option<Vec<Option<String>>>,
    vendors: Option<Vec<Option<String>>>,
    external_ids: Option<Vec<Option<String>>>,
    titles: Option<Vec<Option<String>>>,
    licenses: Option<Vec<Option<String>>>,
    revision_id: Uuid,
    parent_revision_id: Option<Uuid>,
    rev_label: String,
    origin: String,
    created_us: i64,
    blake3: Option<String>,
    format: Option<String>,
    storage_path: Option<String>,
    zstd_level: Option<i16>,
    root: Option<String>,
    on_disk: Option<bool>,
    mtime_ns: Option<i64>,
    size_bytes: Option<i64>,
}
