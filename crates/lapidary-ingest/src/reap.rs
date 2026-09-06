//! The thirty-day sweep: the only place in Lapidary where user data actually leaves.
//!
//! It lives in this crate rather than in `lapidary-db` or `lapidary-storage` because it
//! needs both — the decision is a query, the removal is an unlink — and this crate is
//! already the one place in the workspace allowed to construct a `SourceStore`. It is not
//! a job: there is no row to lease, nothing to retry per-item, and no progress a client
//! would poll. It is a timer, and [`run`] is that timer.

use lapidary_db::{DbError, PgBlobs, PgPool, ReapReport};
use lapidary_storage::{SourceStore, WorkerRole};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// `DATA.md` §1.6, and it is a promise rather than a tunable: a blob is reachable by hash
/// and restorable for thirty days after the last part naming it is purged. Nothing reads
/// this from configuration, because an operator who could shorten it could shorten it to
/// zero and turn purge into deletion.
pub const QUARANTINE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How often to ask. Hourly, not because anything needs hourly precision — the answer
/// changes at most once a day per blob — but because it is often enough that a sweep
/// missed to a restart costs nothing, and rare enough to be free. `0008_quarantine.sql`'s
/// partial index is what makes a sweep that finds nothing cost nothing.
const EVERY: Duration = Duration::from_secs(60 * 60);

/// One sweep. Separated from [`run`] so a test can drive it with a retention of its own
/// choosing rather than waiting thirty days, which is the only way this code path is
/// testable at all.
pub async fn sweep(
    db: &PgPool,
    blob_root: &Path,
    retention: Duration,
) -> Result<ReapReport, DbError> {
    let store = SourceStore::open(blob_root, &WorkerRole::assume());
    PgBlobs(db.clone())
        .reap(retention, |hash| {
            // Source blobs and derivative rungs share one sharded tree, so one handle
            // removes either. `SourceStore` and not `DerivativeStore` because this runs in
            // the worker, which is the role that holds the proof — and because a rung
            // whose row this sweep just deleted is no more evictable-cache than a source
            // file is: by this point nothing points at either.
            store.remove(hash).map_err(|error| error.to_string())
        })
        .await
}

/// The timer. Ticks hourly until cancelled, and never fails the process: a sweep that
/// errors is logged and retried next hour, because the failure mode it protects against
/// (a full disk, a permissions change) is one an operator fixes without a restart.
///
/// On a new installation this removes nothing for thirty days, and that is the correct
/// behaviour rather than a gap — there is nothing whose clock has run out. What it does do
/// from the first tick is clear the quarantine flag off any blob something points at
/// again, so a person who deleted and re-scanned does not wait a month for the column to
/// catch up.
pub async fn run(db: PgPool, blob_root: PathBuf, shutdown: CancellationToken) {
    loop {
        // Sweep first, sleep after. The other order would make the doc comment above a
        // lie by an hour: a blob whose part was restored moments before a restart would
        // stay flagged for a full interval with a running process that had already decided
        // it should not be. The sweep is a lookup against a partial index that is almost
        // always empty, so doing it at startup costs nothing worth deferring.
        match sweep(&db, &blob_root, QUARANTINE).await {
            Ok(report) if report.removed.is_empty() && report.un_quarantined == 0 => {}
            Ok(report) => tracing::info!(
                removed = report.removed.len(),
                bytes = report.bytes,
                un_quarantined = report.un_quarantined,
                hashes = ?report.removed,
                "quarantine sweep",
            ),
            // The whole sweep rolled back, so nothing was removed and nothing was left
            // half-removed. Next hour tries again.
            Err(error) => tracing::error!(%error, "quarantine sweep failed"),
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(EVERY) => {}
        }
    }
}
