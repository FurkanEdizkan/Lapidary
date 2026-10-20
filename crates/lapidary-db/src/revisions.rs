//! A part's revisions after its first: which one is current at a path, recording the next,
//! and the history. See `docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md`.

use crate::DbError;
use crate::repo::{RevisionWrite, StoredBlobRow, detail_hash, detail_stamp, insert_revision_chain};
use lapidary_core::{
    BlobHash, FileRoot, LibraryId, MeasurementProvenance, MeshMeasurements, PartId, RevisionId,
    RevisionOrigin,
};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// What a library holds at one source path: the part, its current revision, and the facts a
/// handler needs to tell a re-scan from a change to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentRevision {
    pub part: PartId,
    pub revision: RevisionId,
    /// The current revision's source file hash. `None` only for a half-repaired row.
    pub source_hash: Option<BlobHash>,
    /// Where the current revision's file is, relative to its root. `None` for a pre-M4 store
    /// row whose bytes are content-addressed.
    pub storage_path: Option<String>,
    /// That file's size and modified time when it was indexed: a pack unchanged since is not hashed
    /// again (A13).
    pub size_bytes: Option<i64>,
    pub mtime_ns: Option<i64>,
}

impl CurrentRevision {
    /// Whether the file at `path`, of `size` bytes modified at `mtime_ns`, is the one this revision indexed: the same
    /// place, size and modified time. The one test of "unchanged since it was indexed", for the scan that decides what
    /// to queue (A33) and the job that decides what to read (A31).
    pub fn is_file(&self, path: &str, size: u64, mtime_ns: i64) -> bool {
        self.storage_path.as_deref() == Some(path)
            && self.size_bytes == i64::try_from(size).ok()
            && self.mtime_ns == Some(mtime_ns)
    }
}

/// A part's current revision and its source file, "current" ordered exactly as the grid's LATERAL orders it.
macro_rules! current_from {
    () => {
        " FROM part p \
     JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id \
                   ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
     LEFT JOIN LATERAL (SELECT blake3, storage_path, size_bytes, mtime_ns FROM file \
                        WHERE revision_id = r.id AND role = 'source' \
                        ORDER BY id LIMIT 1) f ON true "
    };
}

type CurrentRow = (
    Uuid,
    Uuid,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

fn current_revision(
    (part, revision, hash, storage_path, size_bytes, mtime_ns): CurrentRow,
) -> Result<CurrentRevision, DbError> {
    Ok(CurrentRevision {
        part: PartId::from_uuid(part),
        revision: RevisionId::from_uuid(revision),
        source_hash: detail_hash("file.blake3", hash)?,
        storage_path,
        size_bytes,
        mtime_ns,
    })
}

/// The next revision of a part, as the kernel measured it.
pub struct RevisionRequest<'a> {
    pub part: PartId,
    /// The revision the job measured against. Refused if it is no longer current.
    pub parent: RevisionId,
    pub origin: RevisionOrigin,
    /// Where the new revision's file is (M4): usually exactly where the previous one was, since
    /// the owner overwrote it there.
    pub root: FileRoot,
    pub storage_path: &'a str,
    pub mtime_ns: Option<i64>,
    pub blob: &'a StoredBlobRow,
    /// `None` for a pack (A13).
    pub measurements: Option<&'a MeshMeasurements>,
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
    /// `file.on_disk`: false once a later revision's bytes replaced this file where it was.
    /// History shows such a revision's figures and says it is no longer on disk.
    pub on_disk: bool,
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
    on_disk: Option<bool>,
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
        let row: Option<CurrentRow> = sqlx::query_as(concat!(
            "SELECT p.id, r.id, f.blake3, f.storage_path, f.size_bytes, f.mtime_ns",
            current_from!(),
            "WHERE p.library_id = $1 AND p.source_path = $2"
        ))
        .bind(library.as_uuid())
        .bind(source_path)
        .fetch_optional(&self.0)
        .await?;
        row.map(current_revision).transpose()
    }

    /// Every part in `library` with its current revision, by source path: what a scan compares the files it finds
    /// against, in one read rather than one per file (A33).
    pub async fn current_by_path(
        &self,
        library: LibraryId,
    ) -> Result<HashMap<String, CurrentRevision>, DbError> {
        let rows: Vec<(
            String,
            Uuid,
            Uuid,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
        )> = sqlx::query_as(concat!(
            "SELECT p.source_path, p.id, r.id, f.blake3, f.storage_path, f.size_bytes, f.mtime_ns",
            current_from!(),
            "WHERE p.library_id = $1"
        ))
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(
                |(path, part, revision, hash, storage_path, size_bytes, mtime_ns)| {
                    Ok((
                        path,
                        current_revision((
                            part,
                            revision,
                            hash,
                            storage_path,
                            size_bytes,
                            mtime_ns,
                        ))?,
                    ))
                },
            )
            .collect()
    }

    /// Record the next revision of `req.part`, measured against `req.parent`, with its file
    /// where it is now. **Nothing on disk is touched** (M4): the owner overwrote their file, and
    /// Lapidary keeps no copy of the bytes that were there.
    ///
    /// So every earlier revision whose file was at this same place is marked `on_disk = false`
    /// in the same transaction — its figures and hash stay, and its download answers 410. An
    /// earlier file somewhere else (a pre-M4 store copy, an earlier upload under another inbox
    /// name) is still there and stays downloadable.
    ///
    /// Refused with [`DbError::RevisionConflict`] when the part is gone or another revision
    /// landed first: the job is retried and decides again from what is current then.
    pub async fn record_revision(&self, req: RevisionRequest<'_>) -> Result<RevisionId, DbError> {
        let mut tx = self.0.begin().await?;

        // A part deleted while the file was read is not revised: the conflict sends the job back
        // round, and the retry finds no part at this path and ingests the file as a new one.
        let locked: Option<i32> = sqlx::query_scalar("SELECT 1 FROM part WHERE id = $1 FOR UPDATE")
            .bind(req.part.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
        let current: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM revision WHERE part_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(req.part.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        let (Some(_), Some(current)) = (locked, current) else {
            tx.rollback().await?;
            return Err(DbError::RevisionConflict { part: req.part });
        };
        if current != req.parent.as_uuid() {
            tx.rollback().await?;
            return Err(DbError::RevisionConflict { part: req.part });
        }

        // Labels this build writes are whole numbers. The filter keeps a hand-written label
        // from failing the cast; `revision_label_unique_per_part` refuses a collision.
        let label: String = sqlx::query_scalar(
            "SELECT (coalesce(max(rev_label::bigint), 0) + 1)::text FROM revision \
             WHERE part_id = $1 AND rev_label ~ '^[0-9]+$'",
        )
        .bind(req.part.as_uuid())
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE file SET on_disk = false \
             WHERE revision_id IN (SELECT id FROM revision WHERE part_id = $1) \
               AND root = $2 AND storage_path = $3 AND on_disk",
        )
        .bind(req.part.as_uuid())
        .bind(req.root.as_str())
        .bind(req.storage_path)
        .execute(&mut *tx)
        .await?;

        // The source blob's row, as `PgIngest::record` writes it: these may be bytes no row
        // names yet, or bytes an older revision already holds. It claims no content-addressed
        // copy, because there is none (`0026`).
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, 0, $3, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(req.blob.hash.to_hex())
        .bind(req.blob.size_bytes as i64)
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
                storage_path: Some(req.storage_path),
                root: req.root,
                mtime_ns: req.mtime_ns,
                blob: req.blob,
                measurements: req.measurements,
                provenance: req.provenance,
                format: req.format,
            },
        )
        .await?;
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

    /// What a pack holds (A13, `0067`), written after its revision commits as the centre of mass is.
    pub async fn set_contents(
        &self,
        revision: RevisionId,
        contents: &serde_json::Value,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE revision SET contents = $2 WHERE id = $1")
            .bind(revision.as_uuid())
            .bind(contents)
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
                    f.format, f.blake3, f.size_bytes, f.storage_path, f.on_disk \
             FROM revision r \
             LEFT JOIN LATERAL (SELECT format, blake3, size_bytes, storage_path, on_disk FROM file \
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
                    on_disk: c.on_disk.unwrap_or(false),
                })
            })
            .collect()
    }
}
