//! Which blobs were read, batched (`docs/DATA.md` §1.4).
//!
//! A read used to be an `UPDATE blob SET last_accessed_at = now()` of its own, which turns a
//! read-mostly workload into a write-heavy one: a dead tuple per read, for a column only an age
//! rule consults. A read now records its hash in memory, and one statement writes the lot, only for
//! rows more than a day stale. Day precision is plenty for a 90-day rule.

use crate::DbError;
use lapidary_core::BlobHash;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// The reads since the last flush, each with when it happened, in epoch microseconds. One per api
/// process, cloned into the state every handler gets; the server flushes it every five minutes and
/// once more when it stops.
#[derive(Debug, Clone, Default)]
pub struct Touches(Arc<Mutex<HashMap<BlobHash, i64>>>);

impl Touches {
    /// Somebody was just handed these bytes. No database round trip.
    pub fn record(&self, hash: &BlobHash) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| i64::try_from(since.as_micros()).ok())
            .unwrap_or(0);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(*hash, now);
    }

    /// Every recorded read in one statement, for the rows more than a day stale. Returns how many
    /// rows moved.
    ///
    /// Drained before the write, and not put back on failure: a missed touch costs a blob its place
    /// in an age ordering by a day at most, where a queue that grew on every failed flush would cost
    /// the process its memory.
    pub async fn flush(&self, pool: &PgPool) -> Result<u64, DbError> {
        let reads = std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner));
        if reads.is_empty() {
            return Ok(0);
        }
        let (hashes, at): (Vec<String>, Vec<i64>) = reads
            .into_iter()
            .map(|(hash, at)| (hash.to_hex(), at))
            .unzip();
        let written = sqlx::query(
            "UPDATE blob b SET last_accessed_at = to_timestamp(t.at / 1000000.0) \
             FROM unnest($1::text[], $2::bigint[]) AS t(blake3, at) \
             WHERE b.blake3 = t.blake3 \
               AND (b.last_accessed_at IS NULL \
                    OR b.last_accessed_at < to_timestamp(t.at / 1000000.0) - interval '1 day')",
        )
        .bind(&hashes)
        .bind(&at)
        .execute(pool)
        .await?;
        Ok(written.rows_affected())
    }
}
