use crate::DbError;
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, PartSummary, Provenance};
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

pub struct IngestRequest<'a> {
    pub library: LibraryId,
    pub name: &'a str,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub thumbnail_webp: &'a [u8],
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

    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
         VALUES ($1, $2, 'thumbnail', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.thumbnail_webp)
    .bind(req.kernel_version)
    .bind(serde_json::json!({ "px": 512 }))
    .execute(&mut **tx)
    .await?;

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
             LEFT JOIN LATERAL (SELECT * FROM derivative WHERE revision_id = r.id AND kind = 'thumbnail' ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             WHERE p.library_id = $1 AND p.deleted_at IS NULL \
               AND ($2::uuid IS NULL OR p.id < $2) \
             ORDER BY p.id DESC LIMIT $3",
        )
        .bind(library.as_uuid())
        .bind(after.map(|a| a.as_uuid()))
        .bind(i64::from(limit))
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
