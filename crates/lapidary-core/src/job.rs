//! The queue's wire shapes. `BatchStatus` is aggregated from job rows on every read and
//! never stored, so it cannot disagree with the rows it summarises.

use crate::{BatchId, LibraryId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum JobState {
    Pending,
    Running,
    Done,
    Failed,
}

/// How a job finished. Both are successes: `Skipped` means this library already held
/// this exact file, which is slice 1's hash short-circuit doing its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    Ingested,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobFailure {
    pub path: String,
    /// The handler's message, verbatim. A person reads this in the UI, so it says what
    /// broke and what to do about it.
    pub reason: String,
    pub attempts: u32,
}

/// What a scan turned into.
///
/// `ingested`, `skipped` and the per-file failures are slice 1's `ScanReport` counters,
/// relocated from a response body that vanished with the connection to rows that do not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BatchStatus {
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub total: u32,
    pub pending: u32,
    pub running: u32,
    pub ingested: u32,
    pub skipped: u32,
    pub failed_total: u32,
    /// The first 100 failures, ordered by creation, so the list is stable across polls
    /// rather than reshuffling under the reader. `failed_total` is the real count.
    pub failed: Vec<JobFailure>,
    /// RFC 3339 on the wire, exactly like `PartCard.created_at` — ts-rs renders a
    /// `jiff::Timestamp` as `string`. The microsecond hop is a *database-read* workaround
    /// (sqlx 0.9 ships `chrono` and `time`, not `jiff`), never a wire format:
    /// `lapidary-db` selects microseconds and rebuilds with `Timestamp::from_microsecond`
    /// before this type is ever constructed, which is what `PgParts::page` already does.
    pub started_at: Timestamp,
    /// Set only once no job in the batch is pending or running.
    pub finished_at: Option<Timestamp>,
}

/// The scan route's response. The work has been accepted, not done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanAccepted {
    pub batch_id: BatchId,
    /// How many `*.stl` candidates were enqueued. Zero is a success, not an error —
    /// and a batch with zero jobs has no status resource, so the client must not poll.
    pub queued: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_serialises_camel_case_so_the_wire_matches_the_generated_type() {
        let json = serde_json::to_string(&JobState::Running).expect("serialises");
        assert_eq!(json, "\"running\"");
    }

    /// One sample `BatchStatus`, shared by the round-trip test and the wire-shape test
    /// below so neither drifts from the other's fixture.
    fn sample_status() -> BatchStatus {
        BatchStatus {
            batch_id: BatchId::new(),
            library_id: LibraryId::new(),
            total: 6,
            pending: 0,
            running: 0,
            ingested: 5,
            skipped: 0,
            failed_total: 1,
            failed: vec![JobFailure {
                path: "spacer-lp-2001-00.stl".to_owned(),
                reason: "Could not read this STL - it declares 24 facets but the file \
                         ends after 11. Re-export from your CAD tool and retry."
                    .to_owned(),
                attempts: 1,
            }],
            started_at: "2026-09-03T12:00:00Z".parse().expect("a valid timestamp"),
            finished_at: Some("2026-09-03T12:00:04Z".parse().expect("a valid timestamp")),
        }
    }

    #[test]
    fn a_batch_status_round_trips() {
        let status = sample_status();

        let json = serde_json::to_string(&status).expect("serialises");
        let back: BatchStatus = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(status, back);
    }

    /// A round trip cannot catch a wrong or missing `rename_all`: serialising and
    /// deserialising consult the same attribute, so they agree with each other even
    /// when both disagree with the wire contract. The wire contract is the KEY NAMES,
    /// so those are what this asserts directly.
    #[test]
    fn batch_status_serialises_camel_case_keys_so_the_generated_type_matches() {
        let json: serde_json::Value = serde_json::to_value(sample_status()).expect("serialises");
        let keys: Vec<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.contains(&"batchId"),
            "expected camelCase keys, got: {keys:?}"
        );
        assert!(
            keys.contains(&"failedTotal"),
            "expected camelCase keys, got: {keys:?}"
        );
        assert!(
            keys.contains(&"startedAt"),
            "expected camelCase keys, got: {keys:?}"
        );
    }

    /// Same rationale as `batch_status_serialises_camel_case_keys...`: a round trip
    /// cannot distinguish a correct `rename_all` from a missing one.
    #[test]
    fn scan_accepted_serialises_camel_case_keys_so_the_generated_type_matches() {
        let accepted = ScanAccepted {
            batch_id: BatchId::new(),
            queued: 3,
        };
        let json: serde_json::Value = serde_json::to_value(accepted).expect("serialises");
        let keys: Vec<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.contains(&"batchId"),
            "expected camelCase keys, got: {keys:?}"
        );
    }

    // `JobFailure` has no wire-shape key test: its fields (`path`, `reason`,
    // `attempts`) are all single words, so `camelCase` and the default renaming
    // coincide for every one of them. A key-name assertion here could not fail if
    // `#[serde(rename_all = "camelCase")]` were wrong or absent, so it would be a
    // test that cannot test anything — skipped rather than faked. `Outcome` and
    // `JobState` are skipped for the same reason: their variants are single words,
    // so `JobState`'s existing literal-match test above is already sufficient.
}
