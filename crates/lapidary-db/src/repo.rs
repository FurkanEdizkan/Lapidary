use crate::DbError;
use lapidary_core::{
    BlobHash, DerivativeKind, FolderId, LibraryId, MeshMeasurements, PartId, PartSummary,
    Provenance, RevisionId,
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
}

/// Everything a `derive` job needs to re-read a revision's source bytes.
///
/// Two ways of naming the same file, and which one applies is `storage_path`'s
/// nullability: a row written since ingest started writing model directories carries the
/// path the bytes are actually at, and a row from before that carries NULL, meaning they
/// are still at `blobs/ab/cd/<hash>`. Migration `0008` states that rule, and it stays
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
    /// `file.format` — lowercase, no dot. The route synthesizes `{part_name}.{format}`.
    pub format: String,
    /// `part.name`, the download's filename stem. A renamed part downloads under its new
    /// name, which is the design decision spec §2.4 records; the byte-identity claim is
    /// about bytes, not labels.
    pub part_name: String,
    /// `file.storage_path`. Same nullability, same meaning, as [`RevisionSource::storage_path`]:
    /// `Some` names where the bytes actually sit, relative to the storage root; `None`
    /// means this row predates the folder tree and the bytes are still content-addressed.
    /// Migration `0008`'s comment states the rule and how long it holds — for as long as
    /// `migrate_storage` takes to drain every library, which is hours on a real corpus.
    pub storage_path: Option<String>,
    /// `blob.zstd_level` exactly as stored, `None` and all. Never `COALESCE`d to 0 — but
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

/// What a part looks like to the move route, before it moves.
///
/// The three location facts kept apart, exactly as migration `0008` insists: `folder` is
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
    /// (migration `0008`'s header states all three).
    pub folder: Option<FolderId>,
    /// Where the bytes were written, relative to the storage root. Distinct from
    /// `source_path`: that names a directory we only ever read, this names one we own.
    ///
    /// `None` writes the column NULL, which has the meaning `0008` gives it — *the bytes
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
                sqlx::query("UPDATE blob SET ref_count = ref_count + 1 WHERE blake3 = $1")
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

    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, storage_path) \
         VALUES ($1, $2, 'source', $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.format)
    .bind(req.blob.hash.to_hex())
    .bind(req.blob.size_bytes as i64)
    .bind(req.storage_path)
    .execute(&mut **tx)
    .await?;

    // One file inserted above -> one reference. Runs once per call to insert_part_chain,
    // i.e. once per file, whether the blob is new (record) or already held
    // (link_existing) — both paths route through here.
    sqlx::query("UPDATE blob SET ref_count = ref_count + 1 WHERE blake3 = $1")
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
        sqlx::query("UPDATE blob SET ref_count = ref_count + 1 WHERE blake3 = $1")
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

/// A `bigint` byte column as `u64`. Never `as u64`: that turns a negative row into 18
/// exabytes on a card instead of saying the row is wrong.
fn bytes_column(column: &'static str, value: i64) -> Result<u64, DbError> {
    u64::try_from(value).map_err(|_| DbError::NegativeByteCount { column, value })
}

pub struct PgParts(pub PgPool);

impl PgParts {
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
    /// `zstd_level` comes from the `blob` row joined off the same `file` row that carried
    /// the hash — never from `Compression::for_source_format`. That is ingest-time policy
    /// and slice 7 is about to move it, so a reader that re-derived it would start serving
    /// zstd frames as files the day the policy changed (spec §2.5). It is passed through as
    /// the nullable column it is; see [`DownloadSource::zstd_level`].
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
        let row: Option<(String, String, String, Option<String>, Option<i16>)> = sqlx::query_as(
            "SELECT f.blake3, f.format, p.name, f.storage_path, b.zstd_level FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             JOIN blob b ON b.blake3 = f.blake3 \
             WHERE f.revision_id = $1 AND f.role = 'source' AND p.deleted_at IS NULL \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((hex, format, part_name, storage_path, zstd_level)) = row else {
            return Ok(None);
        };
        let hash = BlobHash::parse_hex(&hex).map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some(DownloadSource {
            hash,
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
    /// **Sources** are summed over `file` rows, at `size_bytes`, and that inversion is
    /// deliberate. It was the derivative shape until the store became a folder tree, and
    /// it under-reported the moment it stopped being true that one `blob` row meant one
    /// file on disk: three parts sharing a hash are three files now (spec §0 —
    /// deduplication of source bytes is gone by design), and the blob-shaped sum reported
    /// one of them. A review measured 5,005 B against 7,173 B actually on disk on a
    /// four-part corpus, and the gap widens with duplication. `size_bytes` rather than
    /// `stored_bytes` because a source file is written uncompressed, so the ingested size
    /// *is* the size on disk; a row still awaiting `migrate_storage` is the documented
    /// exception, and it over-reports rather than hiding bytes.
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
    /// Soft-deleted parts are excluded, matching [`PartRepository::page`]. The panel this
    /// feeds sits over that grid, and a total counting parts the grid does not show could
    /// not be checked against it. Their bytes are still on the volume until a purge, so
    /// whichever slice adds delete owns telling an operator about the difference — today
    /// nothing writes `deleted_at`, so the two answers are the same answer.
    pub async fn storage_totals(
        &self,
        library: LibraryId,
    ) -> Result<Option<StorageTotals>, DbError> {
        // `sum()` over a bigint column is `numeric`, which sqlx will not decode into
        // i64 — hence the `::bigint` casts, not decoration.
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT (SELECT coalesce(sum(f.size_bytes), 0)::bigint FROM file f \
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
             WHERE p.library_id = l.id AND p.deleted_at IS NULL) \
             FROM library l WHERE l.id = $1",
        )
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((source, derivative)) = row else {
            return Ok(None);
        };
        Ok(Some(StorageTotals {
            source_bytes: bytes_column("file.size_bytes", source)?,
            derivative_bytes: bytes_column("blob.stored_bytes", derivative)?,
        }))
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
    /// directory self-identifying. The reverse order was considered and rejected: its
    /// failure leaves the database naming a path that does not exist, and every subsequent
    /// read hits it. A disk ahead of the database is a repair job; a database ahead of the
    /// disk is a broken grid.
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

    /// Where this part has been filed, newest first. The audit trail migration `0008`
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
        // property of the schema. `blob` is joined inside the LATERAL because both sizes
        // must come off one row: `file.size_bytes` duplicates `blob.size_bytes`, and a
        // card built from one of each would report a ratio between two tables.
        //
        // The folder filter is the recursive CTE at the top, and it is inline here rather
        // than a separate "give me the descendants" call for one reason: the descent and
        // the page are one question — "the cards in this category" — and splitting them
        // would put query composition in `lapidary-api` (an id list bound into a filter) or
        // give the tree two descent implementations to keep in step. `down` is empty and
        // costs nothing when `$5` is NULL, and the `IS NULL` guard beside it is what makes
        // an absent filter mean the whole library.
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            Uuid,
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<Vec<u8>>,
            Option<i32>,
            Option<bool>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i16>,
            Option<String>,
            i64,
            i64,
        )> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id, 1 AS depth FROM folder WHERE id = $5 \
             UNION ALL \
             SELECT f.id, down.depth + 1 FROM folder f \
             JOIN down ON f.parent_id = down.id WHERE down.depth < $6) \
             SELECT p.id, p.library_id, r.id, p.name, p.part_number, d.thumb_bytes, \
                    r.triangle_count, r.is_watertight, \
                    s.blake3, s.size_bytes, s.stored_bytes, s.zstd_level, s.storage_path, \
                    (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
                    (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us \
             FROM part p \
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = $4 ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             LEFT JOIN LATERAL (SELECT f.blake3, f.storage_path, b.size_bytes, b.stored_bytes, b.zstd_level \
                                FROM file f JOIN blob b ON b.blake3 = f.blake3 \
                                WHERE f.revision_id = r.id AND f.role = 'source' \
                                ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL \
               AND ($2::uuid IS NULL OR p.id < $2) \
               AND ($5::uuid IS NULL OR p.folder_id IN (SELECT id FROM down)) \
             ORDER BY p.id DESC LIMIT $3",
        )
        .bind(library.as_uuid())
        .bind(after.map(|a| a.as_uuid()))
        .bind(i64::from(limit))
        // The kind string comes off `DerivativeKind`, never a literal: the write side
        // stopped spelling it out in task 5, and a reader spelling it differently from
        // the writer reads nothing while looking entirely correct.
        .bind(DerivativeKind::Thumbnail.as_str())
        .bind(folder.map(|f| f.as_uuid()))
        .bind(crate::folders::MAX_DEPTH)
        .fetch_all(&self.0)
        .await?;

        // `blob`'s size columns are `bigint`, so sqlx hands them back signed, and
        // `bytes_column` refuses a negative one rather than wrapping it — the same
        // silent wraparound the triangle count below refuses.
        fn bytes(column: &'static str, value: Option<i64>) -> Result<Option<u64>, DbError> {
            value.map(|v| bytes_column(column, v)).transpose()
        }

        rows.into_iter()
            .map(
                |(
                    id,
                    lib,
                    revision,
                    name,
                    part_number,
                    thumb_bytes,
                    triangles,
                    _watertight,
                    source_hash,
                    source_bytes,
                    stored_bytes,
                    zstd_level,
                    storage_path,
                    created_us,
                    updated_us,
                )| {
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
                    // not collapse into it. The predicate matches `SourceReader::get`'s
                    // for every level actually recorded, which is what keeps a card
                    // claiming "compressed" from sitting over a raw download. A `NULL`
                    // one is not a download this card describes at all: `download.rs`
                    // answers 500 for it rather than serving anything (spec §2.5.1), so
                    // reporting `false` is the display field declining to be the place a
                    // data error surfaces. Reachable in production, not only by a direct
                    // UPDATE — `link_existing` leaves an existing `blob` row alone and
                    // tessellation blobs carry `zstd_level NULL`, so bytes byte-identical
                    // to a derivative arrive as a source file over one. See
                    // `PartSummary::compressed`.
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
                            // The hash is not carried in slice 1: thumbnails arrive inline
                            // and the grid renders them directly. A hash-addressed
                            // thumbnail endpoint arrives with the viewer.
                            thumbnail: None,
                            triangle_count,
                            // Every figure on a mesh part is tessellated, so any is all.
                            approximate: true,
                            source_hash,
                            source_bytes: bytes("blob.size_bytes", source_bytes)?,
                            stored_bytes: bytes("blob.stored_bytes", stored_bytes)?,
                            compressed,
                            created_at: jiff::Timestamp::from_microsecond(created_us).map_err(
                                |_| DbError::TimestampOutOfRange {
                                    column: "part.created_at",
                                    value: created_us,
                                },
                            )?,
                            updated_at: jiff::Timestamp::from_microsecond(updated_us).map_err(
                                |_| DbError::TimestampOutOfRange {
                                    column: "part.updated_at",
                                    value: updated_us,
                                },
                            )?,
                        },
                        thumbnail_webp: thumb_bytes,
                        directory: storage_path.as_deref().and_then(model_directory),
                    })
                },
            )
            .collect()
    }
}
