//! Check-out locks (Phase 4 slice 1 spec §5): one active lock per part, held by free text,
//! in a controlled library only.
//!
//! A lock id is an identifier, not a secret — there is no auth yet — so what protects a
//! check-out is the record: who took it, who released it, and whether they forced it.

use crate::DbError;
use crate::repo::detail_stamp;
use lapidary_core::{LibraryMode, LockId, PartId};
use sqlx::PgPool;
use uuid::Uuid;

/// An active check-out.
#[derive(Debug, Clone, PartialEq)]
pub struct LockRow {
    pub id: LockId,
    pub part: PartId,
    pub holder: String,
    pub taken_at: jiff::Timestamp,
}

/// What asking for a check-out did.
#[derive(Debug, Clone, PartialEq)]
pub enum Checkout {
    Taken(LockRow),
    /// Somebody already holds the part's lock.
    Held(LockRow),
    /// A hobby library keeps no revisions, so there is nothing to check out.
    HobbyLibrary,
    /// No such part, or a deleted one.
    NoSuchPart,
}

fn lock_row((id, part, holder, taken_us): (Uuid, Uuid, String, i64)) -> Result<LockRow, DbError> {
    Ok(LockRow {
        id: LockId::from_uuid(id),
        part: PartId::from_uuid(part),
        holder,
        taken_at: detail_stamp("part_lock.taken_at", taken_us)?,
    })
}

pub struct PgLocks(pub PgPool);

impl PgLocks {
    /// Check `part` out to `holder`. The part row is locked first, so two check-outs of one
    /// part at once answer one `Taken` and one `Held`, never a unique violation.
    pub async fn take(&self, part: PartId, holder: &str) -> Result<Checkout, DbError> {
        let mut tx = self.0.begin().await?;
        let mode: Option<String> = sqlx::query_scalar(
            "SELECT l.mode FROM part p JOIN library l ON l.id = p.library_id \
             WHERE p.id = $1 AND p.deleted_at IS NULL FOR UPDATE OF p",
        )
        .bind(part.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(mode) = mode else {
            tx.rollback().await?;
            return Ok(Checkout::NoSuchPart);
        };
        if mode != LibraryMode::Controlled.as_str() {
            tx.rollback().await?;
            return Ok(Checkout::HobbyLibrary);
        }

        let held: Option<(Uuid, Uuid, String, i64)> = sqlx::query_as(
            "SELECT id, part_id, holder, (extract(epoch FROM taken_at) * 1000000)::bigint \
             FROM part_lock WHERE part_id = $1 AND released_at IS NULL",
        )
        .bind(part.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(held) = held {
            tx.rollback().await?;
            return Ok(Checkout::Held(lock_row(held)?));
        }

        let taken: (Uuid, Uuid, String, i64) = sqlx::query_as(
            "INSERT INTO part_lock (id, part_id, holder) VALUES ($1, $2, $3) \
             RETURNING id, part_id, holder, (extract(epoch FROM taken_at) * 1000000)::bigint",
        )
        .bind(Uuid::now_v7())
        .bind(part.as_uuid())
        .bind(holder)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Checkout::Taken(lock_row(taken)?))
    }

    /// The part's active check-out, if anybody holds one.
    pub async fn active(&self, part: PartId) -> Result<Option<LockRow>, DbError> {
        let row: Option<(Uuid, Uuid, String, i64)> = sqlx::query_as(
            "SELECT id, part_id, holder, (extract(epoch FROM taken_at) * 1000000)::bigint \
             FROM part_lock WHERE part_id = $1 AND released_at IS NULL",
        )
        .bind(part.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        row.map(lock_row).transpose()
    }

    /// The holder checks `lock` in. `false` when it is not this part's active lock any more:
    /// already checked in, released, or never this part's.
    pub async fn check_in(&self, part: PartId, lock: LockId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE part_lock SET released_at = now(), released_by = holder \
             WHERE id = $1 AND part_id = $2 AND released_at IS NULL",
        )
        .bind(lock.as_uuid())
        .bind(part.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Somebody other than the holder releases the part's check-out, recorded as forced so
    /// the holder's next save can say who. `None` when nothing was checked out.
    pub async fn force_release(&self, part: PartId, by: &str) -> Result<Option<LockRow>, DbError> {
        let row: Option<(Uuid, Uuid, String, i64)> = sqlx::query_as(
            "UPDATE part_lock SET released_at = now(), released_by = $2, forced = true \
             WHERE part_id = $1 AND released_at IS NULL \
             RETURNING id, part_id, holder, (extract(epoch FROM taken_at) * 1000000)::bigint",
        )
        .bind(part.as_uuid())
        .bind(by)
        .fetch_optional(&self.0)
        .await?;
        row.map(lock_row).transpose()
    }
}
