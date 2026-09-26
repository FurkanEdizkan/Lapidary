//! A part's shape profile, as the worker records it and likeness reads it (Phase 6; design in
//! `docs/goals/phase-6.md`, table in `0047_part_shape.sql`).
//!
//! One row a part: the profile of its current revision's L0 tessellation. A row is **stale** when its
//! `version` is not [`SHAPE_VERSION`] or its `l0_blake3` is not the current L0's; every reader ignores a stale
//! row and the worker computes it again.

use crate::DbError;
use lapidary_core::{
    BlobHash, DESCRIPTOR_LEN, LibraryId, PartId, RevisionId, SHAPE_VERSION, ShapeProfile,
};
use sqlx::PgPool;
use uuid::Uuid;

/// A stored profile, with what it was computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRow {
    pub revision: RevisionId,
    /// The L0 tessellation it was computed from.
    pub l0: BlobHash,
    /// The [`SHAPE_VERSION`] it was computed with.
    pub version: i16,
    pub profile: ShapeProfile,
}

pub struct PgShapes(pub PgPool);

impl PgShapes {
    /// Record a part's profile, computed from `revision`'s L0 tessellation `l0` with this build's
    /// [`SHAPE_VERSION`].
    ///
    /// Replaces the stored one only when `revision` is the same as or newer than the one it was computed from:
    /// revision ids are UUID v7 and sort by time, so a backfill that finishes after the next revision's job
    /// cannot put the older shape back. `false` when nothing was written — that, or the part is gone.
    pub async fn record(
        &self,
        part: PartId,
        revision: RevisionId,
        l0: BlobHash,
        profile: &ShapeProfile,
    ) -> Result<bool, DbError> {
        let written = sqlx::query(
            "INSERT INTO part_shape (part_id, library_id, revision_id, l0_blake3, version, size_mm, descriptor) \
             SELECT p.id, p.library_id, $2, $3, $4, $5, $6 FROM part p WHERE p.id = $1 \
             ON CONFLICT (part_id) DO UPDATE SET \
                 revision_id = EXCLUDED.revision_id, l0_blake3 = EXCLUDED.l0_blake3, \
                 version = EXCLUDED.version, size_mm = EXCLUDED.size_mm, \
                 descriptor = EXCLUDED.descriptor, computed_at = now() \
             WHERE part_shape.revision_id <= EXCLUDED.revision_id",
        )
        .bind(part.as_uuid())
        .bind(revision.as_uuid())
        .bind(l0.to_hex())
        .bind(SHAPE_VERSION)
        .bind(profile.size_mm)
        .bind(&profile.descriptor[..])
        .execute(&self.0)
        .await?;
        Ok(written.rows_affected() == 1)
    }

    /// A part's stored profile, stale or not; `None` when it has none. Whether it is stale is the caller's
    /// to judge against the current L0 and [`SHAPE_VERSION`].
    pub async fn of_part(&self, part: PartId) -> Result<Option<ShapeRow>, DbError> {
        let row: Option<(Uuid, String, i16, f64, Vec<f32>)> = sqlx::query_as(
            "SELECT revision_id, l0_blake3, version, size_mm, descriptor FROM part_shape WHERE part_id = $1",
        )
        .bind(part.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((revision, l0, version, size_mm, descriptor)) = row else {
            return Ok(None);
        };
        let l0 = BlobHash::parse_hex(&l0).map_err(|_| DbError::CorruptBlobHash {
            column: "part_shape.l0_blake3",
            value: l0,
        })?;
        // The table's check holds every descriptor at this length; one that is not is treated as no profile,
        // and computed again.
        let Ok(descriptor) = <[f32; DESCRIPTOR_LEN]>::try_from(descriptor) else {
            return Ok(None);
        };
        Ok(Some(ShapeRow {
            revision: RevisionId::from_uuid(revision),
            l0,
            version,
            profile: ShapeProfile {
                size_mm,
                descriptor,
            },
        }))
    }
}

/// A part whose profile is missing or stale, and the revision to compute it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleShape {
    pub library: LibraryId,
    pub part: PartId,
    /// The part's latest revision — the one whose L0 the profile must come from.
    pub revision: RevisionId,
}

impl PgShapes {
    /// The L0 tessellation a `profile_shape` job must read, and the part it belongs to.
    ///
    /// Scoped through `part.library_id`, not by a check the caller has to remember: a revision id
    /// is a uuid a caller might hold from anywhere, and content addressing is not authorization
    /// (`CLAUDE.md`). `None` when this library has no such revision, when its part is deleted, or
    /// when the revision has no L0 yet — three different reasons for the same answer, because the
    /// caller does the same thing with all three: it cannot profile, and says so.
    ///
    /// "Newest L0" is `created_at DESC, id DESC`, character for character as
    /// `PgParts::derivative_hash` resolves it. Two resolutions of the same thing that can disagree
    /// are a bug waiting for a second rung to exist.
    pub async fn l0_of_revision(
        &self,
        library: LibraryId,
        revision: RevisionId,
    ) -> Result<Option<(PartId, BlobHash)>, DbError> {
        let row: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT p.id, d.blake3 FROM revision r \
             JOIN part p ON p.id = r.part_id AND p.library_id = $1 AND p.deleted_at IS NULL \
             JOIN LATERAL (SELECT blake3 FROM derivative \
                            WHERE revision_id = r.id AND kind = 'tessellation_l0' \
                              AND blake3 IS NOT NULL \
                            ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             WHERE r.id = $2",
        )
        .bind(library.as_uuid())
        .bind(revision.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        let Some((part, l0)) = row else {
            return Ok(None);
        };
        let l0 = BlobHash::parse_hex(&l0).map_err(|_| DbError::CorruptBlobHash {
            column: "derivative.blake3",
            value: l0,
        })?;
        Ok(Some((PartId::from_uuid(part), l0)))
    }

    /// Every part whose latest revision has an L0 tessellation but no current profile of it, newest
    /// part first, at most `limit`.
    ///
    /// Four ways to be stale, and they are one query because they are one question — *is the stored
    /// row the profile of this part's current L0, computed by this build?*
    ///
    /// - no row at all (a part ingested before Phase 6, or one whose row was purged);
    /// - `version <> SHAPE_VERSION` (the algorithm moved);
    /// - `l0_blake3` is not the current L0's (the rung was rebuilt by a newer kernel);
    /// - `revision_id <> ` the latest revision's (a controlled part gained a revision).
    ///
    /// `<>` and not `<` on the revision: a row pointing at a *newer* revision than the latest is
    /// impossible today, and if some future path made one, profiling the latest again is the right
    /// answer rather than leaving a row nothing can explain.
    ///
    /// Deleted parts are skipped, as `PgParts::revisions_missing` skips them: a part in the bin is
    /// not in the duplicate review queue either.
    // ponytail: a revision whose L0 will never profile — a rung that decodes but has no surface —
    // is found again on every worker start and fails again each time. One row of "we tried this
    // L0 and it has no shape" would stop that; a handful of permanently failed jobs a start is
    // cheaper than the row until somebody sees it in the log.
    pub async fn stale_revisions(&self, limit: i64) -> Result<Vec<StaleShape>, DbError> {
        let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
            "SELECT p.library_id, p.id, r.id FROM part p \
             JOIN LATERAL (SELECT id FROM revision \
                            WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r \
                       ON true \
             JOIN LATERAL (SELECT blake3 FROM derivative \
                            WHERE revision_id = r.id AND kind = 'tessellation_l0' \
                              AND blake3 IS NOT NULL \
                            ORDER BY created_at DESC, id DESC LIMIT 1) d ON true \
             LEFT JOIN part_shape s ON s.part_id = p.id \
             WHERE p.deleted_at IS NULL \
               AND (s.part_id IS NULL OR s.version <> $1 OR s.l0_blake3 <> d.blake3 \
                    OR s.revision_id <> r.id) \
             ORDER BY p.id DESC LIMIT $2",
        )
        .bind(SHAPE_VERSION)
        .bind(limit)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(library, part, revision)| StaleShape {
                library: LibraryId::from_uuid(library),
                part: PartId::from_uuid(part),
                revision: RevisionId::from_uuid(revision),
            })
            .collect())
    }
}
