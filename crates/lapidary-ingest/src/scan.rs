//! Kicking off a library scan: walk the read-only mounted ingest directory and enqueue
//! one job per `.stl` candidate. Nothing here reads a file's bytes, hashes anything or
//! invokes the CAD kernel — that is `handler.rs`, running later on a worker. `router()`
//! (`lib.rs`) always mounts this; see that module's doc for why this crate, rather than a
//! role check inside `lapidary-api`, is what keeps the open path from linking the kernel.
//!
//! # Why the walk stays in the request
//!
//! It would be tidier to enqueue a single "scan this directory" job and answer
//! immediately, but the walk is the one part of a scan that can fail in a way the user
//! must see *now*: a missing or unreadable `/ingest` mount is a deployment mistake, and
//! behind a job it becomes a batch that quietly fails a poll or two later, with the
//! request having already answered 202. The walk is one `read_dir` and one insert even
//! for a thousand entries, so keeping it here costs a few milliseconds and buys an
//! error the operator gets as a response.
//!
//! # What the response means
//!
//! `202 ScanAccepted` — the files have been *accepted*, not ingested. The counters that
//! slice 1 returned synchronously now live in `lapidary_core::BatchStatus`, behind
//! `GET /api/libraries/{lib}/jobs/{batch}`, and arrive as the worker commits parts.
//! `queued: 0` is a success: an empty directory scanned cleanly. It is also the one case
//! the client must not poll, because a batch with no jobs has no status resource.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{LibraryId, ScanAccepted};
use lapidary_db::{DbError, PgJobs};
use std::path::Path as FsPath;

/// Walks `state.ingest_dir` non-recursively and enqueues one `ingest_file` job per `.stl`
/// (case-insensitive) for `library`. Returns `202` with the batch id the caller polls.
pub async fn scan(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    let entries = match std::fs::read_dir(&state.ingest_dir) {
        Ok(entries) => entries,
        Err(source) => return ingest_dir_unreadable(&state.ingest_dir, &source),
    };

    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) if is_stl_candidate(&entry.path()) => {
                let path = entry.path();
                paths.push(
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                );
            }
            // Not a candidate — a README beside a library's STLs is not an error, and it
            // is counted nowhere: `queued` is the number of `*.stl` candidates, not the
            // number of directory entries.
            Ok(_) => {}
            // A directory entry the OS could not even name cannot be enqueued: there is
            // no path to put in a payload. It is logged rather than counted, because
            // `ScanAccepted` reports what was queued and this was not. See
            // `entry_read_failure` for why no live test constructs this condition.
            Err(source) => {
                let failure = entry_read_failure(&state.ingest_dir, &source);
                tracing::warn!(file = %failure.file, reason = %failure.reason, "skipped a directory entry");
            }
        }
    }

    // Deterministic order, so the job ids a scan issues are ordered the way a person
    // reading the directory would expect. `unnest` preserves array order.
    paths.sort();

    match PgJobs(state.db.clone()).enqueue_scan(library, &paths).await {
        Ok((batch_id, queued)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted { batch_id, queued }),
        )
            .into_response(),
        Err(source) => enqueue_failed(&source),
    }
}

fn is_stl_candidate(path: &FsPath) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("stl"))
}

/// The ingest directory itself could not be walked — a missing mount, a permissions
/// error, or (in a test) a nonexistent `TempDir` path. The whole request fails rather
/// than answering `202 { queued: 0 }`, which would be indistinguishable from an empty
/// directory that scanned perfectly well.
fn ingest_dir_unreadable(dir: &FsPath, source: &std::io::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "message": format!(
                "Could not read the ingest directory {}: {source}. Check that the mount is \
                 present and readable.",
                dir.display()
            )
        })),
    )
        .into_response()
}

/// The directory was walked but the batch could not be written. Nothing has been queued,
/// so retrying the same scan is safe and is what the message asks for.
fn enqueue_failed(source: &DbError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "message": format!(
                "Could not queue this scan: {source}. Nothing was queued, so it is safe to \
                 start the scan again once the database is reachable."
            )
        })),
    )
        .into_response()
}

/// What `entry_read_failure` produces. Private and log-only: the per-file failures a
/// person sees come from `lapidary_core::JobFailure`, recorded by the worker against the
/// job row. This one never reaches a job row at all, because there is no filename to
/// enqueue.
#[derive(Debug, PartialEq)]
struct EntryReadFailure {
    file: String,
    reason: String,
}

/// A directory entry failed to read mid-walk — as opposed to `ingest_dir_unreadable`,
/// which fires when the *initial* `read_dir` call fails before anything about the
/// directory's contents is known. There is no filename to report here: the OS failed to
/// produce one at all (the `DirEntry` itself is what errored), so the placeholder says
/// that plainly instead of inventing a name.
///
/// Not exercised by a live integration test: on every platform this workspace targets,
/// `ReadDir::next()` yields `Err` only for a raw OS failure on the underlying `readdir`
/// call itself (e.g. `EBADF`, `EIO`) — not for anything reachable through ordinary
/// filesystem operations like permissions, deletion, or symlinks, which was the class of
/// condition every other error path in this module *can* construct portably (see
/// `tests/scan.rs`'s unreadable-directory test). Reproducing it would need OS- or
/// hardware-level fault injection, which is neither portable across the platforms CI runs
/// nor safe to do in a shared test process. `entry_read_failure` is factored out as a
/// pure function specifically so the one part that *is* testable portably — what gets
/// reported, not how the OS condition arises — has a unit test below, and the loop's
/// behaviour at the call site (log and carry on, never abort the walk) is a one-line,
/// visually-checkable fact.
fn entry_read_failure(dir: &FsPath, source: &std::io::Error) -> EntryReadFailure {
    EntryReadFailure {
        file: "<unreadable directory entry>".to_owned(),
        reason: format!(
            "Could not read a directory entry in {}: {source}. The OS did not report which \
             file this was — check permissions on the ingest mount, and that nothing \
             removed a file while the scan was running.",
            dir.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_read_failure_names_the_directory_and_does_not_invent_a_filename() {
        // Pins the transformation this module unit-tests instead of the OS condition
        // itself — see entry_read_failure's doc for why the condition it responds to
        // cannot be constructed portably.
        let err = std::io::Error::other("synthetic failure");
        let failure = entry_read_failure(FsPath::new("/mnt/ingest"), &err);
        assert_eq!(failure.file, "<unreadable directory entry>");
        assert!(
            failure.reason.contains("/mnt/ingest"),
            "must name the directory: {}",
            failure.reason
        );
        assert!(
            failure.reason.contains("synthetic failure"),
            "must carry the underlying OS error: {}",
            failure.reason
        );
    }
}
