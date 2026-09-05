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
    /// Keyed on the name as well as the hash: two differently-named files with identical
    /// bytes are two parts, and only "same library, same name, same bytes" is a re-scan.
    ///
    /// Deliberately does *not* filter `part.deleted_at`. A part the user deleted stays
    /// deleted — re-scanning the directory it came from must not resurrect it, and
    /// delete is the one action in this product that is always explicit.
    pub async fn library_holds(
        &self,
        library: LibraryId,
        part_name: &str,
        hash: &BlobHash,
    ) -> Result<bool, DbError> {
        let found: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM file f \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id \
             WHERE p.library_id = $1 AND p.name = $2 AND f.blake3 = $3 AND f.role = 'source' \
             LIMIT 1",
        )
        .bind(library.as_uuid())
        .bind(part_name)
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

    sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
        .bind(part.as_uuid())
        .bind(req.library.as_uuid())
        .bind(req.name)
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
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<Vec<u8>>,
            Option<i32>,
            Option<bool>,
            i64,
            i64,
        )> = sqlx::query_as(
            "SELECT p.id, p.library_id, p.name, p.part_number, d.thumb_bytes, \
                    r.triangle_count, r.is_watertight, \
                    (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us, \
                    (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us \
             FROM part p \
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = $4 ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
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
        .fetch_all(&self.0)
        .await?;

        rows.into_iter()
            .map(
                |(
                    id,
                    lib,
                    name,
                    part_number,
                    thumb_bytes,
                    triangles,
                    _watertight,
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
                    Ok(PartRow {
                        summary: PartSummary {
                            id: PartId::from_uuid(id),
                            library: LibraryId::from_uuid(lib),
                            name,
                            part_number,
                            // The hash is not carried in slice 1: thumbnails arrive inline
                            // and the grid renders them directly. A hash-addressed
                            // thumbnail endpoint arrives with the viewer.
                            thumbnail: None,
                            triangle_count,
                            // Every figure on a mesh part is tessellated, so any is all.
                            approximate: true,
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
