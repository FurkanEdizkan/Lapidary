//! What the dashboard asks the database that nothing else already asks: a library's queue, counted.
//!
//! One read, for the `queue` widget (`docs/goals/phase-6.md` § Dashboard). Everything else a widget
//! needs — storage totals, the grid page, the facets, the saved filters, the near-duplicates — is a
//! read that already exists, and the dashboard calls those rather than growing copies of them here.

use crate::DbError;
use lapidary_core::LibraryId;
use sqlx::PgPool;

/// A library's jobs that have not finished, and the ones that failed.
///
/// `u32` because this is a figure on a tile: a library with more than four billion queued jobs has a
/// different problem, and a saturating count is a better answer than a failed widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueCounts {
    pub pending: u32,
    pub running: u32,
    pub failed: u32,
}

pub struct PgDashboard(pub PgPool);

impl PgDashboard {
    /// One library's queue in three numbers. A library that does not exist counts zero of each — the
    /// caller checks existence, because a widget showing an empty queue for a mistyped id is a number
    /// a person would believe.
    ///
    /// ponytail: one pass over the library's `job` rows, `done` ones included, because `job_active_idx`
    /// is partial on `pending`/`running` and nothing indexes `failed`. Measured at about 1.4 ms over
    /// 10,001 rows when migration `0036` timed the same shape. A partial index on `state = 'failed'`
    /// is the upgrade, and it needs a migration this goal was not given one for.
    pub async fn queue(&self, library: LibraryId) -> Result<QueueCounts, DbError> {
        let (pending, running, failed): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE state = 'pending'), \
                    count(*) FILTER (WHERE state = 'running'), \
                    count(*) FILTER (WHERE state = 'failed') \
             FROM job WHERE library_id = $1",
        )
        .bind(library.as_uuid())
        .fetch_one(&self.0)
        .await?;
        Ok(QueueCounts {
            pending: saturate(pending),
            running: saturate(running),
            failed: saturate(failed),
        })
    }
}

/// A `count(*)` as the tile's `u32`. `count(*)` is never negative and never past `u32::MAX` in any
/// library anybody has, so both arms are unreachable in practice and neither is a reason to fail a read.
fn saturate(count: i64) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
