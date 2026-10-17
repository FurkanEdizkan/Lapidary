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

/// How a job finished. All eight are successes: `Skipped` means this library already
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
///
/// `Revised` is the answer to a file whose bytes changed at a path the library already indexes:
/// the change is kept as a new revision, in every library.
///
/// **`Unkept` is gone from this enum, for `Profiled`'s reason below.** It was a hobby library's
/// answer — no revisions kept — until S4 (2026-10-16) gave every library its history. Finished
/// rows still hold it and `job_outcome_known` keeps it; nothing counts it any more.
///
/// **`Profiled` is gone from this enum and must stay out of `job_outcome_known`.** R3 deleted
/// shape similarity, so nothing writes it; migration `0047` added the value and finished rows
/// still hold it, and the board's rule is that a later restatement of that constraint keeps
/// every value already in it. Finished history keeps its word. Nothing counts it — `batch_status`
/// aggregates outcomes with `count(*) FILTER` per name and never had a `profiled` column — so
/// such a row still totals correctly in the batch it belongs to.
///
/// `Rendered` is **not** dead with the renders: it is the common tail of every `derive` job and
/// the exports are still built on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    Ingested,
    Skipped,
    Rendered,
    Scanned,
    Migrated,
    Revised,
    /// A part's `metadata.json` written again from its rows.
    Described,
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
    /// A file Lapidary itself wrote into the inbox — an upload — indexed from there (M4). `path` is
    /// where it is under the inbox; `source_path` is the part it belongs to, the path the browser
    /// reported, which is what a re-upload of the same file is a revision of.
    ///
    /// Replaced `IngestBlob`, which read bytes the upload route had put into the store.
    IngestInbox {
        path: String,
        source_path: String,
    },
    /// Unpack a bundle the upload route stored (Phase 4 slice 2 spec §7): check the whole
    /// archive, then queue one `ImportPart` per part into this job's own batch. `path` is the
    /// bundle's file name, so a refused bundle is named in the batch's failure list.
    ImportBundle {
        blake3: BlobHash,
        path: String,
    },
    /// One part of a checked bundle, its revisions replayed through ingest oldest first. `path`
    /// is the part's source path, so a refused part is named in the failure list.
    ImportPart {
        bundle: BlobHash,
        part: u32,
        path: String,
    },
    /// Import a model from the page a person pasted: Printables, Thingiverse or MyMiniFactory (F3).
    /// The worker reads the model, fetches its files and files each through ordinary ingest. The
    /// row also carries the URL as `path`, which is the key the batch's failure list names a job by.
    ImportLink {
        url: String,
    },
    /// Describe a part already in the library from its model's page, downloading nothing (A2):
    /// "Fetch details". `path` is the part's source path, which is also what a failure is listed
    /// under.
    DescribePage {
        path: String,
        url: String,
    },
}

#[derive(Deserialize)]
struct DescribePagePayload {
    path: String,
    url: String,
}

#[derive(Deserialize)]
struct ImportLinkPayload {
    url: String,
}

#[derive(Deserialize)]
struct ImportBundlePayload {
    blake3: BlobHash,
    path: String,
}

#[derive(Deserialize)]
struct ImportPartPayload {
    bundle: BlobHash,
    part: u32,
    path: String,
}

/// The `ingest_inbox` payload, deserialised whole for the same reason `DerivePayload` is:
/// a malformed row reports serde's own message rather than a guess at which key was
/// wrong.
#[derive(Deserialize)]
struct IngestInboxPayload {
    path: String,
    inbox: String,
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
    pub const INGEST_INBOX: &'static str = "ingest_inbox";
    pub const IMPORT_BUNDLE: &'static str = "import_bundle";
    pub const IMPORT_PART: &'static str = "import_part";
    pub const IMPORT_LINK: &'static str = "import_link";
    pub const DESCRIBE_PAGE: &'static str = "describe_page";

    pub fn kind(&self) -> &'static str {
        match self {
            JobPayload::IngestFile { .. } => Self::INGEST_FILE,
            JobPayload::Derive { .. } => Self::DERIVE,
            JobPayload::ScanDirectory => Self::SCAN_DIRECTORY,
            JobPayload::IngestInbox { .. } => Self::INGEST_INBOX,
            JobPayload::ImportBundle { .. } => Self::IMPORT_BUNDLE,
            JobPayload::ImportPart { .. } => Self::IMPORT_PART,
            JobPayload::ImportLink { .. } => Self::IMPORT_LINK,
            JobPayload::DescribePage { .. } => Self::DESCRIBE_PAGE,
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
            JobPayload::ScanDirectory => serde_json::json!({}),
            // `path` is the part's source path, as `IngestFile`'s is: it is what a failure is
            // listed under, and one key meaning two things across two kinds is a key somebody
            // reads out of the wrong one. Where the bytes are in the inbox is `inbox`.
            JobPayload::IngestInbox { path, source_path } => {
                serde_json::json!({ "path": source_path, "inbox": path })
            }
            JobPayload::ImportBundle { blake3, path } => {
                serde_json::json!({ "blake3": blake3, "path": path })
            }
            JobPayload::ImportPart { bundle, part, path } => {
                serde_json::json!({ "bundle": bundle, "part": part, "path": path })
            }
            JobPayload::ImportLink { url } => serde_json::json!({ "url": url, "path": url }),
            JobPayload::DescribePage { path, url } => {
                serde_json::json!({ "path": path, "url": url })
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
            Self::INGEST_INBOX => serde_json::from_value::<IngestInboxPayload>(payload.clone())
                .map(|p| JobPayload::IngestInbox {
                    path: p.inbox,
                    source_path: p.path,
                })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            Self::IMPORT_BUNDLE => serde_json::from_value::<ImportBundlePayload>(payload.clone())
                .map(|p| JobPayload::ImportBundle {
                    blake3: p.blake3,
                    path: p.path,
                })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            Self::IMPORT_PART => serde_json::from_value::<ImportPartPayload>(payload.clone())
                .map(|p| JobPayload::ImportPart {
                    bundle: p.bundle,
                    part: p.part,
                    path: p.path,
                })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            Self::IMPORT_LINK => serde_json::from_value::<ImportLinkPayload>(payload.clone())
                .map(|p| JobPayload::ImportLink { url: p.url })
                .map_err(|source| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: source.to_string(),
                }),
            Self::DESCRIBE_PAGE => serde_json::from_value::<DescribePagePayload>(payload.clone())
                .map(|p| JobPayload::DescribePage {
                    path: p.path,
                    url: p.url,
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
    /// The job that failed, so one file can be retried by itself.
    pub job: crate::JobId,
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
    /// Files whose changed bytes were kept as a new revision.
    pub revised: u32,
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
            revised: 0,
            // The walk that found the six files, one of which failed. Counted in `total`
            // as the job it is, and subtracted out wherever the number is called `files`.
            scanned: 1,
            migrated: 0,
            migrating: 0,
            failed_total: 1,
            failed: vec![JobFailure {
                job: crate::JobId::new(),
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
    fn an_import_link_payload_round_trips_and_names_its_url_as_its_path() {
        let payload = JobPayload::ImportLink {
            url: "https://www.printables.com/model/482910-motorcycle-throttle-stop".to_owned(),
        };
        let json = payload.to_json();
        assert_eq!(
            json["path"], json["url"],
            "the failure list names a job by `path`"
        );
        assert_eq!(
            JobPayload::from_row(payload.kind(), &json).expect("parses"),
            payload
        );
    }

    #[test]
    fn an_ingest_inbox_payload_lists_its_part_by_source_path_and_round_trips() {
        let payload = JobPayload::IngestInbox {
            path: "uploads/brackets/steel/LP-1042-03 (2).stl".to_owned(),
            source_path: "brackets/steel/LP-1042-03.stl".to_owned(),
        };
        let json = payload.to_json();
        assert_eq!(payload.kind(), "ingest_inbox");
        assert_eq!(
            json["path"], "brackets/steel/LP-1042-03.stl",
            "a failure is listed under `path`, which is the part's own path"
        );
        assert_eq!(
            JobPayload::from_row("ingest_inbox", &json).expect("round trips"),
            payload
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
            produce: DerivativeKind::Export3mf,
        };
        assert_eq!(p.kind(), "derive");
        assert_eq!(
            JobPayload::from_row("derive", &p.to_json()).expect("round trips"),
            p
        );
        assert_eq!(p.to_json()["produce"], "export_3mf");
        // A `derive` row naming a kind nothing builds any more is malformed, not silently
        // taken as something else: those rows exist from before the pivot, and migration
        // `0054` deletes the pending and running ones precisely so none is ever read here.
        let dead = serde_json::json!({ "revision": rev, "produce": "tessellation_l0" });
        JobPayload::from_row("derive", &dead)
            .expect_err("a rung is not a derivative this build can produce");
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

#[cfg(test)]
mod import_payload_tests {
    use super::*;

    #[test]
    fn an_import_job_round_trips_through_its_row_and_names_its_path() {
        for (payload, path) in [
            (
                JobPayload::ImportBundle {
                    blake3: BlobHash::from_bytes([7; 32]),
                    path: "workshop-bundle.lapidary.zip".to_owned(),
                },
                "workshop-bundle.lapidary.zip",
            ),
            (
                JobPayload::ImportPart {
                    bundle: BlobHash::from_bytes([7; 32]),
                    part: 3,
                    path: "brackets/bracket-lp-1042-03.stl".to_owned(),
                },
                "brackets/bracket-lp-1042-03.stl",
            ),
        ] {
            assert_eq!(
                JobPayload::from_row(payload.kind(), &payload.to_json()).expect("parses"),
                payload
            );
            assert_eq!(payload.to_json()["path"], path, "the failure list names it");
        }
    }
}
