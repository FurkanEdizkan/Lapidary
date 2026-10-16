//! A part's revisions after its first: which one is current at a path, recording the next,
//! and the history. See `docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md`.

use crate::DbError;
use crate::repo::{RevisionWrite, StoredBlobRow, detail_hash, detail_stamp, insert_revision_chain};
use lapidary_core::manifest::{ManifestFile, ManifestPart, ManifestRevision, ModelManifest};
use lapidary_core::{
    BlobHash, LibraryId, MeasurementProvenance, MeshMeasurements, PartId, RevisionId,
    RevisionOrigin,
};
use sqlx::PgPool;
use uuid::Uuid;

/// What a library holds at one source path: the part, its current revision, and the facts a
/// handler needs to tell a re-scan from a change to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentRevision {
    pub part: PartId,
    pub revision: RevisionId,
    /// The current revision's source file hash. `None` only for a half-repaired row.
    pub source_hash: Option<BlobHash>,
    /// `None` while the file is still in the content-addressed store the storage migration
    /// has not reached: there is no model directory to set the previous file aside in yet.
    pub storage_path: Option<String>,
}

/// The next revision of a part, as the kernel measured it.
pub struct RevisionRequest<'a> {
    pub part: PartId,
    /// The revision the job measured against. Refused if it is no longer current.
    pub parent: RevisionId,
    pub origin: RevisionOrigin,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub provenance: MeasurementProvenance,
    pub format: &'a str,
}

/// One revision in a part's history.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionRow {
    pub id: RevisionId,
    pub parent: Option<RevisionId>,
    pub rev_label: String,
    pub origin: RevisionOrigin,
    pub created_at: jiff::Timestamp,
    pub volume_mm3: Option<f64>,
    pub volume_source: Option<String>,
    pub surface_area_mm2: Option<f64>,
    pub surface_area_source: Option<String>,
    pub bbox_mm: Option<[f64; 3]>,
    pub bbox_source: Option<String>,
    pub triangle_count: Option<i32>,
    /// The B-rep's faces and edges, as the CAD kernel counted them. `None` for a mesh.
    pub face_count: Option<i32>,
    pub edge_count: Option<i32>,
    /// The centre of the volume, from `mass_props_json`, and where it came from. `None` for a revision
    /// recorded before centres were, or with no volume.
    pub centre_mm: Option<[f64; 3]>,
    pub centre_source: Option<String>,
    pub is_watertight: Option<bool>,
    pub units: Option<String>,
    pub format: Option<String>,
    pub source_hash: Option<BlobHash>,
    pub size_bytes: Option<i64>,
    pub storage_path: Option<String>,
}

/// Matched by column name, for `DetailColumns`' reason: more columns than sqlx implements
/// `FromRow` for as a tuple.
#[derive(sqlx::FromRow)]
struct HistoryColumns {
    id: Uuid,
    parent_revision_id: Option<Uuid>,
    rev_label: String,
    origin: String,
    created_us: i64,
    volume: Option<f64>,
    volume_source: Option<String>,
    surface_area: Option<f64>,
    surface_area_source: Option<String>,
    bbox_x: Option<f64>,
    bbox_y: Option<f64>,
    bbox_z: Option<f64>,
    bbox_source: Option<String>,
    triangle_count: Option<i32>,
    face_count: Option<i32>,
    edge_count: Option<i32>,
    centre_x: Option<f64>,
    centre_y: Option<f64>,
    centre_z: Option<f64>,
    centre_source: Option<String>,
    is_watertight: Option<bool>,
    units: Option<String>,
    format: Option<String>,
    blake3: Option<String>,
    size_bytes: Option<i64>,
    storage_path: Option<String>,
}

pub struct PgRevisions(pub PgPool);

impl PgRevisions {
    /// The part at `source_path` in `library`, and its current revision —
    /// "current" ordered exactly as the grid's LATERAL orders it.
    pub async fn current(
        &self,
        library: LibraryId,
        source_path: &str,
    ) -> Result<Option<CurrentRevision>, DbError> {
        #[allow(clippy::type_complexity)]
        let row: Option<(Uuid, Uuid, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT p.id, r.id, f.blake3, f.storage_path \
                 FROM part p \
                 JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                               ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
                 LEFT JOIN LATERAL (SELECT blake3, storage_path FROM file \
                                    WHERE revision_id = r.id AND role = 'source' \
                                    ORDER BY id LIMIT 1) f ON true \
                 WHERE p.library_id = $1 AND p.source_path = $2",
        )
        .bind(library.as_uuid())
        .bind(source_path)
        .fetch_optional(&self.0)
        .await?;
        row.map(|(part, revision, hash, storage_path)| {
            Ok(CurrentRevision {
                part: PartId::from_uuid(part),
                revision: RevisionId::from_uuid(revision),
                source_hash: detail_hash("file.blake3", hash)?,
                storage_path,
            })
        })
        .transpose()
    }

    /// The next revision of `req.part`, in one transaction shaped like
    /// [`crate::PgParts::move_to_folder`] (spec §3.1):
    ///
    /// 1. The part row is locked first and held to the commit. A move takes the same row (its
    ///    `UPDATE part`) and so does a second revision, so neither renames files under this one.
    /// 2. The parent must still be current, checked before any file is touched.
    /// 3. The previous file's `storage_path` moves to `revisions/<its label>/<name>`; the new
    ///    revision, its file at the path the previous one held, and its derivatives are written
    ///    by the same writer a part's first revision uses.
    /// 4. `relocate(current, aside)` does the filesystem half — the previous file to `aside`, the
    ///    new bytes to `current` — and a failure rolls the transaction back.
    ///
    /// If this returns an error after `relocate` succeeded (the commit failed), the files have
    /// moved and the rows have not. Undoing that is the caller's: only it holds the bytes.
    pub async fn record_revision<F>(
        &self,
        req: RevisionRequest<'_>,
        relocate: F,
    ) -> Result<RevisionId, DbError>
    where
        F: FnOnce(&str, &str) -> Result<(), String>,
    {
        let mut tx = self.0.begin().await?;

        // A part deleted while the file was read is not revised: the conflict sends the job back
        // round, and the retry finds no part at this path and ingests the file as a new one.
        let locked: Option<i32> = sqlx::query_scalar("SELECT 1 FROM part WHERE id = $1 FOR UPDATE")
            .bind(req.part.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
        let current: Option<(Uuid, String, Option<Uuid>, Option<String>)> = sqlx::query_as(
            "SELECT r.id, r.rev_label, f.id, f.storage_path FROM revision r \
             LEFT JOIN LATERAL (SELECT id, storage_path FROM file \
                                WHERE revision_id = r.id AND role = 'source' \
                                ORDER BY id LIMIT 1) f ON true \
             WHERE r.part_id = $1 \
             ORDER BY r.created_at DESC, r.id DESC LIMIT 1",
        )
        .bind(req.part.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        let (Some(_), Some((current, current_label, Some(file), Some(path)))) = (locked, current)
        else {
            tx.rollback().await?;
            return Err(DbError::RevisionConflict { part: req.part });
        };
        if current != req.parent.as_uuid() {
            tx.rollback().await?;
            return Err(DbError::RevisionConflict { part: req.part });
        }

        let Some((directory, name)) = path.rsplit_once('/') else {
            tx.rollback().await?;
            return Err(DbError::RevisionFilesFailed {
                detail: format!("`{path}` names no model directory to keep the previous file in"),
            });
        };

        // Labels this build writes are whole numbers. The filter keeps a hand-written label
        // from failing the cast; `revision_label_unique_per_part` refuses a collision.
        let label: String = sqlx::query_scalar(
            "SELECT (coalesce(max(rev_label::bigint), 0) + 1)::text FROM revision \
             WHERE part_id = $1 AND rev_label ~ '^[0-9]+$'",
        )
        .bind(req.part.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        // Under the previous revision's own label: `revisions/1/` holds revision 1.
        let aside = format!("{directory}/revisions/{current_label}/{name}");

        sqlx::query("UPDATE file SET storage_path = $2 WHERE id = $1")
            .bind(file)
            .bind(&aside)
            .execute(&mut *tx)
            .await?;

        // The source blob's row, as `PgIngest::record` writes it: these may be bytes no row
        // names yet, or bytes an older revision already holds.
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $3, $4, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(req.blob.hash.to_hex())
        .bind(req.blob.size_bytes as i64)
        // A revision is always filed in its model directory, so its blob row claims no
        // content-addressed copy (`0026`).
        .bind(0_i64)
        .bind(req.blob.zstd_level)
        .execute(&mut *tx)
        .await?;

        let revision = Uuid::now_v7();
        insert_revision_chain(
            &mut tx,
            &RevisionWrite {
                part: req.part,
                revision,
                rev_label: &label,
                parent: Some(current),
                origin: req.origin,
                storage_path: Some(&path),
                blob: req.blob,
                measurements: req.measurements,
                provenance: req.provenance,
                format: req.format,
            },
        )
        .await?;

        if let Err(detail) = relocate(&path, &aside) {
            // Explicit, for `move_to_folder`'s reason.
            tx.rollback().await?;
            return Err(DbError::RevisionFilesFailed { detail });
        }
        tx.commit().await?;
        Ok(RevisionId::from_uuid(revision))
    }

    /// The centre of `revision`'s volume and where it came from, written after the revision commits as its
    /// counts are: `mass_props_json = {"centre_mm": [x, y, z], "source": "analytic" | "tessellated"}`. It
    /// needs no density. A revision with no volume never gets one.
    pub async fn set_centre_of_mass(
        &self,
        revision: RevisionId,
        centre_mm: [f64; 3],
        source: lapidary_core::Provenance,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE revision SET mass_props_json = \
             jsonb_build_object('centre_mm', $2::jsonb, 'source', $3::text) WHERE id = $1",
        )
        .bind(revision.as_uuid())
        .bind(serde_json::json!(centre_mm))
        .bind(source.as_str())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Every revision of `part`, newest first — the order the grid calls the first one current.
    pub async fn history(&self, part: PartId) -> Result<Vec<RevisionRow>, DbError> {
        Self::history_on(&mut *self.0.acquire().await?, part).await
    }

    /// [`Self::history`] on `conn`, for a caller already holding a connection.
    async fn history_on(
        conn: &mut sqlx::PgConnection,
        part: PartId,
    ) -> Result<Vec<RevisionRow>, DbError> {
        let rows: Vec<HistoryColumns> = sqlx::query_as(
            "SELECT r.id, r.parent_revision_id, r.rev_label, r.origin, \
                    (extract(epoch FROM r.created_at) * 1000000)::bigint AS created_us, \
                    r.volume, r.volume_source, r.surface_area, r.surface_area_source, \
                    r.bbox_x, r.bbox_y, r.bbox_z, r.bbox_source, \
                    r.triangle_count, r.face_count, r.edge_count, r.is_watertight, r.units, \
                    (r.mass_props_json->'centre_mm'->>0)::float8 AS centre_x, \
                    (r.mass_props_json->'centre_mm'->>1)::float8 AS centre_y, \
                    (r.mass_props_json->'centre_mm'->>2)::float8 AS centre_z, \
                    r.mass_props_json->>'source' AS centre_source, \
                    f.format, f.blake3, f.size_bytes, f.storage_path \
             FROM revision r \
             LEFT JOIN LATERAL (SELECT format, blake3, size_bytes, storage_path FROM file \
                                WHERE revision_id = r.id AND role = 'source' \
                                ORDER BY id LIMIT 1) f ON true \
             WHERE r.part_id = $1 \
             ORDER BY r.created_at DESC, r.id DESC",
        )
        .bind(part.as_uuid())
        .fetch_all(&mut *conn)
        .await?;

        rows.into_iter()
            .map(|c| {
                // Refused rather than defaulted, for `UnknownProvenance`'s reason: calling an
                // unknown word `ingest` would be deciding where somebody's bytes came from.
                let origin =
                    RevisionOrigin::parse(&c.origin).ok_or_else(|| DbError::UnknownOrigin {
                        value: c.origin.clone(),
                    })?;
                Ok(RevisionRow {
                    id: RevisionId::from_uuid(c.id),
                    parent: c.parent_revision_id.map(RevisionId::from_uuid),
                    rev_label: c.rev_label,
                    origin,
                    created_at: detail_stamp("revision.created_at", c.created_us)?,
                    volume_mm3: c.volume,
                    volume_source: c.volume_source,
                    surface_area_mm2: c.surface_area,
                    surface_area_source: c.surface_area_source,
                    bbox_mm: match (c.bbox_x, c.bbox_y, c.bbox_z) {
                        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                        _ => None,
                    },
                    bbox_source: c.bbox_source,
                    triangle_count: c.triangle_count,
                    face_count: c.face_count,
                    edge_count: c.edge_count,
                    centre_mm: match (c.centre_x, c.centre_y, c.centre_z) {
                        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                        _ => None,
                    },
                    centre_source: c.centre_source,
                    is_watertight: c.is_watertight,
                    units: c.units,
                    format: c.format,
                    source_hash: detail_hash("file.blake3", c.blake3)?,
                    size_bytes: c.size_bytes,
                    storage_path: c.storage_path,
                })
            })
            .collect()
    }

    /// `metadata.json` for `part`, read and handed to `write` while the part's row is held. A revision or a
    /// custom value committing meanwhile waits for the file, so two writers never put an older description
    /// over a newer one. `write` gets `None` when there is no such part.
    pub async fn write_manifest<T>(
        &self,
        part: PartId,
        write: impl FnOnce(Option<ModelManifest>) -> T,
    ) -> Result<T, DbError> {
        let mut held = self.0.begin().await?;
        sqlx::query("SELECT 1 FROM part WHERE id = $1 FOR NO KEY UPDATE")
            .bind(part.as_uuid())
            .execute(&mut *held)
            .await?;
        // On the connection holding the row, not a second one: writers each holding one and waiting for
        // another would stall a pool that every one of them had filled.
        let written = write(Self::manifest_on(&mut held, part).await?);
        held.commit().await?;
        Ok(written)
    }

    /// `metadata.json` for `part`, from its rows: every revision, oldest first, so
    /// `revisions[0]` stays the original (spec §3.2). `None` means there is no such part.
    pub async fn manifest(&self, part: PartId) -> Result<Option<ModelManifest>, DbError> {
        Self::manifest_on(&mut *self.0.acquire().await?, part).await
    }

    /// [`Self::manifest`] on `conn`, for a caller already holding a connection.
    async fn manifest_on(
        conn: &mut sqlx::PgConnection,
        part: PartId,
    ) -> Result<Option<ModelManifest>, DbError> {
        #[allow(clippy::type_complexity)]
        let row: Option<(Uuid, String, Option<String>, Option<String>, String, String)> =
            sqlx::query_as(
                "SELECT library_id, name, part_number, classification, source_path, \
                        metadata_json::text \
                 FROM part WHERE id = $1",
            )
            .bind(part.as_uuid())
            .fetch_optional(&mut *conn)
            .await?;
        let Some((library, name, part_number, classification, source_path, metadata)) = row else {
            return Ok(None);
        };

        let mut revisions = Self::history_on(conn, part).await?;
        revisions.reverse();
        Ok(Some(ModelManifest {
            schema: ModelManifest::SCHEMA,
            part: ManifestPart {
                id: part,
                library: LibraryId::from_uuid(library),
                name,
                part_number,
                classification,
                source_path,
                // `jsonb` rendered as text is JSON by construction; `Null` is unreachable.
                metadata: serde_json::from_str(&metadata).unwrap_or(serde_json::Value::Null),
            },
            revisions: revisions
                .into_iter()
                .map(|r| ManifestRevision {
                    id: r.id,
                    rev_label: r.rev_label,
                    origin: r.origin.as_str().to_owned(),
                    volume_mm3: r.volume_mm3,
                    volume_source: r.volume_source,
                    bbox_mm: r.bbox_mm,
                    triangle_count: r.triangle_count,
                    is_watertight: r.is_watertight,
                    units: r.units,
                    files: match (r.format, r.source_hash, r.size_bytes, r.storage_path) {
                        (Some(format), Some(blake3), Some(size_bytes), Some(path)) => {
                            vec![ManifestFile {
                                role: "source".to_owned(),
                                format,
                                blake3,
                                size_bytes,
                                file_name: path.rsplit('/').next().unwrap_or(&path).to_owned(),
                            }]
                        }
                        _ => Vec::new(),
                    },
                })
                .collect(),
        }))
    }
}
