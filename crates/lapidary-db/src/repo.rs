use crate::DbError;
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, PartSummary, Provenance};
use sqlx::PgPool;
use uuid::Uuid;

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, newest first. `after` is the previous page's last
    /// id. Implementation lands in Phase 1.
    async fn page(
        &self,
        library: LibraryId,
        after: Option<lapidary_core::PartId>,
        limit: u16,
    ) -> Result<Vec<PartSummary>, DbError>;
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
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub thumbnail_webp: &'a [u8],
    pub kernel_version: &'a str,
}

pub struct PgBlobs(pub PgPool);

impl PgBlobs {
    /// Content addressing is not authorization: this only tells the caller whether the
    /// bytes are already held, never whether the caller may read them.
    pub async fn exists(&self, hash: &BlobHash) -> Result<bool, DbError> {
        let found: Option<String> = sqlx::query_scalar("SELECT blake3 FROM blob WHERE blake3 = $1")
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
    .bind(m.triangle_count as i32)
    .bind(m.is_watertight)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes) \
         VALUES ($1, $2, 'source', 'stl', $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
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
    ) -> Result<Vec<PartSummary>, DbError> {
        // One query: thumbnails travel inline as bytea rather than costing a round trip
        // per card. Keyset, not OFFSET — OFFSET degrades as the library grows.
        //
        // sqlx cannot decode jiff::Timestamp (it ships chrono/time support, not jiff), so
        // timestamps are pulled out as epoch microseconds and reassembled below rather
        // than adding a second date-time crate just to carry the value across.
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
             JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC LIMIT 1) r ON true \
             LEFT JOIN derivative d ON d.revision_id = r.id AND d.kind = 'thumbnail' \
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
                    _thumb,
                    triangles,
                    _watertight,
                    created_us,
                    updated_us,
                )| {
                    Ok(PartSummary {
                        id: PartId::from_uuid(id),
                        library: LibraryId::from_uuid(lib),
                        name,
                        part_number,
                        // The hash is not carried in slice 1: thumbnails arrive inline and
                        // the grid renders them directly. A hash-addressed thumbnail
                        // endpoint arrives with the viewer.
                        thumbnail: None,
                        triangle_count: triangles.map(|t| t as u32),
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
                    })
                },
            )
            .collect()
    }
}
