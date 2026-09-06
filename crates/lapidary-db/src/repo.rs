use crate::DbError;
use lapidary_core::{
    BlobHash, DerivativeKind, LibraryId, MeshMeasurements, PartId, PartSummary, Provenance,
    RevisionId,
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
}

/// Everything the download route needs about a revision's source file, in one row.
///
/// Four columns off three tables, so it is a struct rather than a tuple: `format`,
/// `part_name` and the hex hash are all text, and a tuple of them is three positions a
/// call site can silently transpose into a file served under the wrong name.
#[derive(Debug)]
pub struct DownloadSource {
    pub hash: BlobHash,
    /// `blob.size_bytes` — the *uncompressed* length, which is what the route sends as
    /// `Content-Length` and therefore what the user is promised.
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
/// No ratio here: it is one division over these two numbers, and a third field carrying
/// it would be a second place for the same fact to be computed differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageTotals {
    pub source_bytes: u64,
    pub derivative_bytes: u64,
}

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, newest first. `after` is the previous page's last
    /// id.
    async fn page(
        &self,
        library: LibraryId,
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

    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(part.as_uuid())
        .bind(req.library.as_uuid())
        .bind(req.name)
        .bind(req.source_path)
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
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes) \
         VALUES ($1, $2, 'source', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.format)
    .bind(req.blob.hash.to_hex())
    .bind(req.blob.size_bytes as i64)
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
    ) -> Result<Option<(BlobHash, String)>, DbError> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT f.blake3, f.format FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id AND p.library_id = $2 \
             WHERE f.revision_id = $1 AND f.role = 'source' \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((hex, format)) = row else {
            return Ok(None);
        };
        let parsed = BlobHash::parse_hex(&hex);
        let hash = parsed.map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: hex,
        })?;
        Ok(Some((hash, format)))
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
    /// The `role = 'source'` filter and the `ORDER BY … LIMIT 1` are character for
    /// character [`PgParts::revision_source`]'s, and for its reason: `file` has no unique
    /// constraint on `(revision_id, role)`, so a second source row must resolve to the same
    /// answer on every call rather than to whichever row the planner returned first.
    pub async fn source_for_download(
        &self,
        revision: RevisionId,
    ) -> Result<Option<DownloadSource>, DbError> {
        let row: Option<(String, String, String, Option<i16>, i64)> = sqlx::query_as(
            "SELECT f.blake3, f.format, p.name, b.zstd_level, b.size_bytes FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             JOIN blob b ON b.blake3 = f.blake3 \
             WHERE f.revision_id = $1 AND f.role = 'source' AND p.deleted_at IS NULL \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
        .bind(revision.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((hex, format, part_name, zstd_level, size_bytes)) = row else {
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
            zstd_level,
        }))
    }

    /// What this library occupies, split the way `DATA.md` §1.1 splits storage classes.
    /// `None` means there is no such library, so a route can 404 rather than report zero
    /// bytes for an id that names nothing — the same distinction
    /// [`PgParts::auto_thumbnail`] draws, and for the same reason.
    ///
    /// Both blob totals are `stored_bytes`, never `size_bytes`: the question is what is
    /// on the volume, and spec §4 wants a figure Phase D's tiering work can be judged
    /// against. They are summed over `blob` rows selected by `IN (subquery)`, so a blob
    /// two parts share is counted once — which is what `ref_count` exists for and what
    /// `du` would report. Summing over `file` rows instead would count identical STLs
    /// twice and inflate a deduplicated library.
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
            "SELECT (SELECT coalesce(sum(b.stored_bytes), 0)::bigint FROM blob b \
             WHERE b.blake3 IN (SELECT f.blake3 FROM file f \
             JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = l.id AND p.deleted_at IS NULL \
             AND f.role = 'source')), \
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
            source_bytes: bytes_column("blob.stored_bytes", source)?,
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
}

#[async_trait::async_trait]
impl PartRepository for PgParts {
    async fn page(
        &self,
        library: LibraryId,
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
            "SELECT p.id, p.library_id, r.id, p.name, p.part_number, d.thumb_bytes, \
                    r.triangle_count, r.is_watertight, \
                    s.blake3, s.size_bytes, s.stored_bytes, s.zstd_level, l0.blake3, \
                    (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
                    (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us \
             FROM part p \
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = $4 ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             LEFT JOIN LATERAL (SELECT blake3 FROM derivative WHERE revision_id = r.id AND kind = $5 ORDER BY created_at DESC, id DESC LIMIT 1) l0 ON true \
             LEFT JOIN LATERAL (SELECT f.blake3, b.size_bytes, b.stored_bytes, b.zstd_level \
                                FROM file f JOIN blob b ON b.blake3 = f.blake3 \
                                WHERE f.revision_id = r.id AND f.role = 'source' \
                                ORDER BY f.created_at DESC, f.id DESC LIMIT 1) s ON true \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL \
               AND ($2::uuid IS NULL OR p.id < $2) \
             ORDER BY p.id DESC LIMIT $3",
        )
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
                    tessellation_l0,
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
                            tessellation_l0,
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
                    })
                },
            )
            .collect()
    }
}
