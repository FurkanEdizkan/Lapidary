//! The queue's wire shapes. `BatchStatus` is aggregated from job rows on every read and
//! never stored, so it cannot disagree with the rows it summarises.

use crate::{BatchId, BlobHash, CoreError, DerivativeKind, LibraryId, RevisionId};
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

/// How a job finished. All five are successes: `Skipped` means this library already
/// held this exact file, which is slice 1's hash short-circuit doing its job; `Rendered`
/// means a `derive` job upserted the derivative it was asked to produce; `Scanned` means
/// a `scan_directory` job walked the ingest mount and enqueued what it found; `Migrated`
/// means a `migrate_storage` job moved a slice of an old content-addressed store into the
/// model directories that replaced it.
///
/// `Scanned` exists because the other three would each be a counter that lies. The
/// database requires a finished job to say how it finished (`job_done_has_outcome`), and
/// a scan job ingests nothing, skips nothing and renders nothing: reporting it as
/// `Skipped` tells a user a file was "already here", `Rendered` makes the grid read the
/// whole batch as a preview render, and `Ingested` claims a part that does not exist.
///
/// `Migrated` exists for the same reason one step further on: a `migrate_storage` run
/// indexes nothing new — it moves bytes a part already had from the content-addressed
/// store into that part's own directory. Reporting it as `Ingested` would put files a
/// user already had into the "added" column of a batch they are watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    Ingested,
    Skipped,
    Rendered,
    Scanned,
    Migrated,
}

/// What a job carries, without its kind.
///
/// The `job.kind` COLUMN is the discriminator, not a key inside the payload. Every row
/// written before this slice holds a bare `{"path": …}`, so an internally-tagged enum
/// would fail to deserialise all of them and the queue would stop draining on upgrade.
#[derive(Debug, Clone, PartialEq)]
pub enum JobPayload {
    IngestFile {
        path: String,
    },
    Derive {
        revision: RevisionId,
        produce: DerivativeKind,
    },
    /// Walk the worker's ingest mount and enqueue one `IngestFile` per candidate. No
    /// fields: the directory is the worker's `ingest_dir` and the library is the job
    /// row's own `library_id`, so a payload would only be a second place for either to
    /// be wrong. See `lapidary_ingest::scan`'s module doc for why the walk is a job at
    /// all.
    ScanDirectory,
    /// The same work as `IngestFile`, for bytes that are already in the blob store
    /// rather than on the ingest mount. The upload route writes the blob from the api
    /// and enqueues this; the worker reads it back out with the `SourceStore` it holds
    /// anyway.
    ///
    /// It carries the hash *and* the path because they answer different questions: the
    /// hash says which bytes, and `source_path` is the part's identity within the
    /// library (§2) and the name the browser reported for the file. Neither is
    /// derivable from the other.
    ///
    /// A separate kind rather than a flag on `IngestFile`, because the two differ in
    /// where the bytes come from, and a payload whose meaning depends on which of two
    /// optional keys is present is the shape `from_row` exists to keep out. See the
    /// slice 6a design, §4.1.
    IngestBlob {
        blake3: BlobHash,
        source_path: String,
    },
    /// Move every source blob in this library out of the content-addressed store and into
    /// its model's own directory, writing a `metadata.json` beside it.
    ///
    /// A job rather than part of migration `0009`, because sqlx runs a migration in one
    /// transaction at startup and copying a corpus is neither transactional nor fast.
    /// Resumable because the queue is: it selects the next batch of `file` rows whose
    /// `storage_path` is still null, so a killed worker resumes where it stopped.
    ///
    /// No fields, for `ScanDirectory`'s reason: the library is the job row's own column,
    /// and how much to do per run is the handler's policy rather than a number a caller
    /// gets to write into a row that outlives the build that wrote it.
    MigrateStorage,
}

/// The `ingest_blob` payload, deserialised whole for the same reason `DerivePayload` is:
/// a malformed row reports serde's own message rather than a guess at which key was
/// wrong.
#[derive(Deserialize)]
struct IngestBlobPayload {
    blake3: BlobHash,
    path: String,
}

/// The `derive` payload's shape, deserialised as a whole rather than field by field so a
/// malformed row reports serde's own message through `CoreError::MalformedJobPayload`.
#[derive(Deserialize)]
struct DerivePayload {
    revision: RevisionId,
    produce: DerivativeKind,
}

impl JobPayload {
    pub const INGEST_FILE: &'static str = "ingest_file";
    pub const DERIVE: &'static str = "derive";
    pub const SCAN_DIRECTORY: &'static str = "scan_directory";
    pub const INGEST_BLOB: &'static str = "ingest_blob";
    pub const MIGRATE_STORAGE: &'static str = "migrate_storage";

    pub fn kind(&self) -> &'static str {
        match self {
            JobPayload::IngestFile { .. } => Self::INGEST_FILE,
            JobPayload::Derive { .. } => Self::DERIVE,
            JobPayload::ScanDirectory => Self::SCAN_DIRECTORY,
            JobPayload::IngestBlob { .. } => Self::INGEST_BLOB,
            JobPayload::MigrateStorage => Self::MIGRATE_STORAGE,
        }
    }

    /// The `payload` column's value. `IngestFile` emits exactly what `enqueue_scan` has
    /// always written, so old and new rows are indistinguishable.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            JobPayload::IngestFile { path } => serde_json::json!({ "path": path }),
            JobPayload::Derive { revision, produce } => {
                serde_json::json!({ "revision": revision, "produce": produce })
            }
            JobPayload::ScanDirectory | JobPayload::MigrateStorage => serde_json::json!({}),
            // `path` and not `sourcePath`: it means what `IngestFile`'s `path`
            // means, and one key spelled two ways across two kinds is a key
            // somebody reads out of the wrong one.
            JobPayload::IngestBlob {
                blake3,
                source_path,
            } => {
                serde_json::json!({ "blake3": blake3, "path": source_path })
            }
        }
    }

    /// Rebuild from a row. `kind` comes from the column.
    pub fn from_row(kind: &str, payload: &serde_json::Value) -> Result<Self, CoreError> {
        match kind {
            Self::INGEST_FILE => payload
                .get("path")
                .and_then(|p| p.as_str())
                .map(|path| JobPayload::IngestFile {
                    path: path.to_owned(),
                })
                .ok_or_else(|| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: "it has no file path".to_owned(),
                }),
            Self::DERIVE => serde_json::from_value::<DerivePayload>(payload.clone())
                .map(|p| JobPayload::Derive {
                    revision: p.revision,
                    produce: p.produce,
                })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            // Nothing is read out of the payload, so nothing in it can be malformed:
            // a row written by an older or newer Lapidary carrying extra keys still
            // names a directory walk, and refusing it would strand a scan over a key
            // this build does not use.
            Self::SCAN_DIRECTORY => Ok(JobPayload::ScanDirectory),
            // Empty for the same reason, and read the same way: a `migrate_storage` row
            // carries no payload at all, so there is nothing in it to be malformed.
            Self::MIGRATE_STORAGE => Ok(JobPayload::MigrateStorage),
            Self::INGEST_BLOB => serde_json::from_value::<IngestBlobPayload>(payload.clone())
                .map(|p| JobPayload::IngestBlob {
                    blake3: p.blake3,
                    source_path: p.path,
                })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            other => Err(CoreError::UnknownJobKind {
                kind: other.to_owned(),
            }),
        }
    }
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
    pub rendered: u32,
    /// How many `scan_directory` jobs in this batch have finished their walk — in
    /// practice 0 or 1, since a scan enqueues one and its children join the same batch.
    ///
    /// Exposed so the progress line can say *files*. `total` counts jobs, and the walk is
    /// a job; reporting it as a file made a three-file directory read "Scanning — 1 of 4
    /// files", which `CLAUDE.md`'s measurement rule forbids. Subtracting this from both
    /// halves is exact, where subtracting a hardcoded 1 would encode "every batch has a
    /// walk" in the frontend — untrue of a render sweep.
    pub scanned: u32,
    /// How many `migrate_storage` jobs in this batch have finished moving a slice of
    /// the old content-addressed store into its parts' own directories.
    ///
    /// A migration chains itself the same way a scan chains its walk (`total` grows as
    /// each run re-enqueues the next slice), and it ingests nothing and skips nothing —
    /// borrowing `ingested` for it would put moved-not-added files in the grid's "added"
    /// column, and `scanned` already means something else.
    ///
    /// This counts settled OUTCOMES, not rows, so it is 0 for every migration at the
    /// instant it begins — and the worker's startup enqueue is the only way a
    /// `migrate_storage` batch can exist, so every migration a browser can watch starts
    /// inside that window. `migrating` below is what the batch-kind guess actually reads
    /// to tell a fresh migration apart from a fresh scan; this field is left for
    /// whatever eventually reports how much of one migration run has settled.
    pub migrated: u32,
    /// How many `migrate_storage` job ROWS exist in this batch, settled or not —
    /// `count(*) FILTER (WHERE kind = 'migrate_storage')`, not `... WHERE outcome =
    /// 'migrated'`. Non-zero from the moment the first `migrate_storage` row is
    /// inserted, unlike `migrated` above, which stays 0 until one finishes — and since
    /// `HASHES_PER_RUN` (200) makes the first run the slowest on a large corpus, that is
    /// exactly the batch where the gap between "exists" and "has settled one" is widest.
    /// This is the field the batch-kind guess reads.
    pub migrating: u32,
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
    /// How many jobs were enqueued. Zero is a success, not an error — and a batch with
    /// zero jobs has no status resource, so the client must not poll. Not a file count:
    /// the thumbnail routes answer with revisions, and a scan answers with `1`, the one
    /// `scan_directory` job whose own walk grows the batch as it finds candidates.
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
            total: 7,
            pending: 0,
            running: 0,
            ingested: 5,
            skipped: 0,
            rendered: 0,
            // The walk that found the six files, one of which failed. Counted in `total`
            // as the job it is, and subtracted out wherever the number is called `files`.
            scanned: 1,
            migrated: 0,
            migrating: 0,
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

    #[test]
    fn an_ingest_blob_payload_round_trips_through_its_row() {
        let payload = JobPayload::IngestBlob {
            blake3: BlobHash::from_bytes([0x5c; 32]),
            source_path: "brackets/steel/LP-1042-03.stl".to_owned(),
        };
        let json = payload.to_json();
        assert_eq!(payload.kind(), "ingest_blob");
        // The hash goes over as hex, like everywhere else a `BlobHash` is written, and
        // the path key is spelled the way `ingest_file` spells it.
        assert_eq!(
            json["blake3"],
            "5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c"
        );
        assert_eq!(json["path"], "brackets/steel/LP-1042-03.stl");
        assert_eq!(
            JobPayload::from_row("ingest_blob", &json).expect("round trips"),
            payload
        );
    }

    #[test]
    fn an_ingest_blob_row_missing_its_hash_names_the_kind_and_the_problem() {
        // The kind column says `ingest_blob`, so the row is not an unknown kind and must
        // not be reported as one: it is this kind, malformed, and the message has to say
        // which key serde could not find or a reader is left guessing at a payload.
        let err = JobPayload::from_row("ingest_blob", &serde_json::json!({ "path": "a.stl" }))
            .expect_err("a payload with no hash is malformed");
        let message = err.to_string();
        assert!(
            message.contains("ingest_blob") && message.contains("blake3"),
            "must name the kind and the missing key, got: {message}"
        );
    }

    #[test]
    fn an_existing_ingest_row_still_deserialises() {
        // The exact shape every row in the database holds today: no `kind` key, because
        // the kind is a column. This is the test that fails if someone reaches for
        // #[serde(tag = "kind")].
        let payload = serde_json::json!({ "path": "bracket-lp-1042-03.stl" });
        let got = JobPayload::from_row("ingest_file", &payload).expect("parses");
        assert_eq!(
            got,
            JobPayload::IngestFile {
                path: "bracket-lp-1042-03.stl".to_owned()
            }
        );
    }

    #[test]
    fn an_ingest_payload_round_trips_byte_identically() {
        let p = JobPayload::IngestFile {
            path: "x.stl".to_owned(),
        };
        assert_eq!(p.to_json(), serde_json::json!({ "path": "x.stl" }));
        assert_eq!(p.kind(), "ingest_file");
    }

    #[test]
    fn a_derive_payload_carries_a_revision_and_one_kind() {
        let rev = RevisionId::new();
        let p = JobPayload::Derive {
            revision: rev,
            produce: DerivativeKind::TessellationL2,
        };
        assert_eq!(p.kind(), "derive");
        assert_eq!(
            JobPayload::from_row("derive", &p.to_json()).expect("round trips"),
            p
        );
        assert_eq!(p.to_json()["produce"], "tessellation_l2");
    }

    /// The payload is empty by design, and `from_row` must not start caring what is in
    /// it: the library comes from the job row's own column and the directory from the
    /// worker's mount, so a row carrying extra keys is still a directory walk.
    #[test]
    fn a_scan_directory_payload_is_empty_and_round_trips() {
        let p = JobPayload::ScanDirectory;
        assert_eq!(p.kind(), "scan_directory");
        assert_eq!(p.to_json(), serde_json::json!({}));
        assert_eq!(
            JobPayload::from_row("scan_directory", &p.to_json()).expect("round trips"),
            p
        );
        assert_eq!(
            JobPayload::from_row("scan_directory", &serde_json::json!({ "path": "unused" }))
                .expect("an unread key is not a malformed payload"),
            p
        );
    }

    /// Same shape as the `scan_directory` case above, and pinned for the same two
    /// reasons: the kind is the COLUMN, and `from_row` must not start reading a payload it
    /// does not use. A `migrate_storage` row written by a build that later learns to carry
    /// a batch size in its payload must still name a storage migration to this one.
    #[test]
    fn a_migrate_storage_payload_is_empty_and_round_trips() {
        let p = JobPayload::MigrateStorage;
        assert_eq!(p.kind(), "migrate_storage");
        assert_eq!(p.to_json(), serde_json::json!({}));
        assert_eq!(
            JobPayload::from_row("migrate_storage", &p.to_json()).expect("round trips"),
            p
        );
        assert_eq!(
            JobPayload::from_row("migrate_storage", &serde_json::json!({ "limit": 200 }))
                .expect("an unread key is not a malformed payload"),
            p
        );
    }

    #[test]
    fn an_unknown_kind_names_itself() {
        let err = JobPayload::from_row("polish_the_brass", &serde_json::json!({}))
            .expect_err("must fail");
        assert!(err.to_string().contains("polish_the_brass"), "{err}");
    }

    /// The message must not claim Lapidary did not write the row. `from_row`'s whole
    /// reason for existing is that an older Lapidary wrote rows a newer one has to read,
    /// so version skew is the *expected* way to reach this error and "It was not written
    /// by Lapidary" was a false statement pointing the operator at the wrong cause.
    /// Nothing outside `src/` pinned the wording, which is how it survived.
    #[test]
    fn a_malformed_payload_does_not_assert_who_wrote_the_row() {
        let err = JobPayload::from_row(JobPayload::DERIVE, &serde_json::json!({}))
            .expect_err("a derive payload with no revision is malformed");
        let message = err.to_string();
        assert!(
            message.contains("`derive`"),
            "the kind is quoted like every other offending value, got: {message}"
        );
        assert!(
            message.contains("written by an older Lapidary")
                && message.contains("something other than Lapidary"),
            "the message must name both causes and say what to check (CLAUDE.md), \
             got: {message}"
        );
        assert!(
            !message.contains("It was not written by Lapidary"),
            "a row an older Lapidary wrote is malformed AND written by Lapidary, \
             got: {message}"
        );
    }
}
