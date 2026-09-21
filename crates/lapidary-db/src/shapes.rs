//! A part's shape profile, as the worker records it and likeness reads it (Phase 6; design in
//! `docs/goals/phase-6.md`, table in `0047_part_shape.sql`).
//!
//! One row a part: the profile of its current revision's L0 tessellation. A row is **stale** when its
//! `version` is not [`SHAPE_VERSION`] or its `l0_blake3` is not the current L0's; every reader ignores a stale
//! row and the worker computes it again.

use crate::DbError;
use lapidary_core::{BlobHash, DESCRIPTOR_LEN, PartId, RevisionId, SHAPE_VERSION, ShapeProfile};
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
