//! Starting a library scan, and the walk it starts.
//!
//! Two halves that used to be one. [`scan`] is the worker's `POST /scan` route, now a
//! thin enqueue of a single `scan_directory` job; [`WorkerHandler::scan_directory`] is
//! the walk itself, running later on a worker, which reads the mounted ingest directory
//! and enqueues one `ingest_file` job per mesh candidate. Nothing in either half reads a
//! file's bytes, hashes anything or invokes the CAD kernel — that is `handler.rs`.
//! `router()` (`lib.rs`) always mounts the route; see that module's doc for why this
//! crate, rather than a role check inside `lapidary-api`, is what keeps the open path
//! from linking the kernel.
//!
//! # The walk used to stay in the request, and no longer does
//!
//! What this doc said until slice 5, and it was right on its own terms:
//!
//! > It would be tidier to enqueue a single "scan this directory" job and answer
//! > immediately, but the walk is the one part of a scan that can fail in a way the user
//! > must see *now*: a missing or unreadable `/ingest` mount is a deployment mistake, and
//! > behind a job it becomes a batch that quietly fails a poll or two later, with the
//! > request having already answered 202.
//!
//! Reversed deliberately, for two reasons that arrived together.
//!
//! The forcing one: the browser cannot reach this route. `deploy/web/Caddyfile` proxies
//! `/api/*` to `api:8080` and nothing else, so a scan a user can start has to be a route
//! on `lapidary-api` — and `lapidary-api` does not mount `/ingest`, by design, so it has
//! no directory to walk and no path list to enqueue. A job is the only thing an api-side
//! route can hand to a worker. (The two alternatives were weighed and rejected: a Caddy
//! route to `worker:8081` splits the browser's API surface across two backends by URL
//! pattern with no gate watching that the pattern still matches the route, and mounting
//! `/ingest` on the api re-litigates why scan lives in this crate at all.)
//!
//! The one that makes it honest rather than merely necessary: **the failure UI now
//! exists.** The objection above was that a failure goes unseen, and when it was written
//! that was true — `batch_status` returned a `failures` list carrying each job's
//! `last_error` and the grid rendered only the count. It renders the reasons now
//! (`web/src/routes/index.tsx`), so an unreadable mount arrives in the browser as the
//! message [`WorkerHandler::scan_directory`] wrote, naming the directory. Two things keep
//! that promise from being decorative: the failure is classified `Permanent`, so it lands
//! in `state = 'failed'` on the first attempt rather than three backoffs later (a
//! `Transient` classification would not appear in `failures` at all until `max_attempts`
//! ran out), and the scan job's children go into the scan job's **own** batch, so the
//! batch the browser is already polling is the one that carries the failure.
//!
//! # The batch
//!
//! `enqueue` mints a fresh `BatchId` on every call, so the walk uses
//! [`lapidary_db::PgJobs::enqueue_into`] with the batch its own job row already belongs
//! to. A scan job that enqueued its files into a new batch would leave the browser
//! polling a batch of one, seeing it settle, and reporting a finished scan while a
//! hundred and fifty files were still ingesting. `batch_status` stores no total — it
//! counts rows by `batch_id` on every read — so the total simply grows as the walk
//! inserts.
//!
//! # What the reversal cost: the walk is retryable now
//!
//! The in-request walk ran exactly once — a request that failed was a request, and there
//! was nothing to retry it. A job is retried, and this one is not idempotent: it re-walks
//! and re-enqueues every candidate. Three paths reach it — `dequeue` reclaiming an
//! expired lease, `release_leases` after `SHUTDOWN_GRACE` on a worker that was killed
//! mid-walk, and a `Transient` failure from `enqueue_into` rescheduling the whole job —
//! and `max_attempts` bounds it at three.
//!
//! Not made idempotent, deliberately. The duplicate `ingest_file` jobs hash the same
//! bytes, hit `library_holds`, and settle as `Skipped`; nothing is ingested twice and no
//! part is duplicated. What it costs is honesty in two numbers: `total` counts the
//! duplicates, and a first-ever scan can report *"Scan complete — 150 added, 150 already
//! here."* Deduplicating the walk would mean reading the batch's existing payloads before
//! enqueueing — a query, a race, and a second definition of "already queued" — to prevent
//! a wrong sentence on a path that needs a crashed worker to reach. Written down instead,
//! because the in-request walk could not do this and a reader comparing the two versions
//! deserves to know what changed.
//!
//! # What the response means
//!
//! `202 ScanAccepted` — the scan has been *accepted*, not performed. `queued` is `1`: the
//! `scan_directory` job. The file counters live in `lapidary_core::BatchStatus`, behind
//! `GET /api/libraries/{lib}/jobs/{batch}`, and `total` climbs from 1 to 1 + however many
//! candidates the walk finds.

use crate::AppState;
use crate::handler::WorkerHandler;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{BatchId, JobPayload, LibraryId, Outcome, ScanAccepted};
use lapidary_db::{DbError, PgJobs};
use lapidary_jobs::HandlerError;
use std::path::Path as FsPath;

/// Enqueues one `scan_directory` job for `library` and returns `202` with the batch id
/// the caller polls. The walk happens in [`WorkerHandler::scan_directory`].
///
/// Deliberately identical in shape to `lapidary_api::scan`, which is the route a browser
/// reaches. This one is the worker's own `:8081` surface, kept so `README.md`'s first-run
/// `curl` keeps working — and kept as an enqueue rather than a second walk, so there is
/// one walk implementation rather than two that drift.
pub async fn scan(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgJobs(state.db.clone())
        .enqueue(library, &[JobPayload::ScanDirectory])
        .await
    {
        Ok((batch_id, queued)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted { batch_id, queued }),
        )
            .into_response(),
        Err(source) => enqueue_failed(&source),
    }
}

impl WorkerHandler {
    /// Walks `self.ingest_dir` and enqueues one `ingest_file` job per mesh candidate
    /// (case-insensitive) into `batch` — the batch this job itself is in.
    ///
    /// Recursive since slice 6a. It was one `read_dir`, and a nested corpus therefore
    /// scanned as "0 files" with no error anywhere — the operator had to flatten their
    /// library before Lapidary could see it. See
    /// `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md` §1.
    ///
    /// Returns [`Outcome::Scanned`], which exists for this and nothing else: a scan job
    /// ingests nothing, skips nothing and renders nothing, and borrowing one of those
    /// three would put a number the user reads on a line it is not true of.
    pub(crate) async fn scan_directory(
        &self,
        batch: BatchId,
        library: LibraryId,
    ) -> Result<Outcome, HandlerError> {
        let mut paths = walk(&self.ingest_dir)?;

        // Deterministic order, so the job ids a scan issues are ordered the way a person
        // reading the tree would expect. The whole relative path sorts, not the basename,
        // so a folder's files stay together. `unnest` preserves array order.
        paths.sort();

        let jobs: Vec<JobPayload> = paths
            .into_iter()
            .map(|path| JobPayload::IngestFile { path })
            .collect();
        // `enqueue_into`, never `enqueue`: the files belong to the batch the browser is
        // already polling. See this module's doc.
        PgJobs(self.db.clone())
            .enqueue_into(batch, library, &jobs)
            .await
            .map_err(|e| HandlerError::Transient {
                message: format!(
                    "Could not queue the files this scan found: {e}. Wait for the \
                     database to come back and start the scan again — anything this \
                     attempt did queue is re-scanned and reported as already here, \
                     never ingested twice."
                ),
            })?;
        Ok(Outcome::Scanned)
    }
}

/// The extension is the format, and this is the only place that decides it.
///
/// Not a byte sniff: OBJ is plain text with no magic number, so sniffing reduces to
/// guessing from the first non-comment line. The extension is also what an operator sees
/// in the directory, so a file that is skipped is skipped for a reason they can see.
fn is_mesh_candidate(path: &FsPath) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| MESH_EXTENSIONS.iter().any(|k| ext.eq_ignore_ascii_case(k)))
}

pub(crate) const MESH_EXTENSIONS: [&str; 3] = ["stl", "obj", "3mf"];

/// How deep the walk will descend before it stops and says so.
///
/// Not a cycle guard — real directories cannot cycle, and symlinked ones are not followed
/// (see `walk`), so a cycle is unreachable. This bounds pathological nesting and the one
/// case that *can* cycle without a symlink: a bind mount pointing at one of its own
/// ancestors. Sixteen is far past any real parts library and still finite.
const MAX_DEPTH: usize = 16;

/// Every mesh candidate under `root`, as paths relative to it with `/` separators.
///
/// A worklist rather than recursion: the depth cap makes recursion safe, but a flat loop
/// is easier to read and cannot be made unsafe by someone raising the cap later.
///
/// Only `root` being unreadable fails the job. A single unreadable subdirectory deep in a
/// corpus is logged and skipped, because failing four thousand files over one folder is
/// not the trade an operator wants — and the failure they need to see, a missing mount, is
/// exactly the case where `root` itself cannot be read.
fn walk(root: &FsPath) -> Result<Vec<String>, HandlerError> {
    // Read the root eagerly, so an unreadable mount is Permanent before anything else.
    let mut queue = vec![(root.to_path_buf(), 0usize)];
    std::fs::read_dir(root).map_err(|e| ingest_dir_unreadable(root, &e))?;

    let mut found = Vec::new();
    while let Some((dir, depth)) = queue.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source) => {
                tracing::warn!(dir = %dir.display(), reason = %source, "skipped an unreadable directory");
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                // A directory entry the OS could not even name cannot be enqueued: there
                // is no path to put in a payload. Logged rather than counted, and it does
                // not fail the scan — the other candidates are still real work. See
                // `entry_read_failure` for why no live test constructs this condition.
                Err(source) => {
                    let failure = entry_read_failure(&dir, &source);
                    tracing::warn!(file = %failure.file, reason = %failure.reason, "skipped a directory entry");
                    continue;
                }
            };

            // A `.git`, `.Trash` or `.DS_Store` inside someone's parts folder is not part
            // of their library, and walking a `.git` on a large corpus is pure waste.
            // Skipped as silently as any other non-candidate.
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }

            let path = entry.path();
            // `DirEntry::file_type` does not traverse a symlink, so this is false for a
            // symlinked directory and following one is impossible — which is what makes
            // cycles unreachable. A symlinked *file* still reaches `is_mesh_candidate`
            // below, which uses `Path::is_file` and does follow: an operator who symlinks
            // an STL into their library meant it.
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => {
                    if depth + 1 > MAX_DEPTH {
                        tracing::warn!(
                            dir = %path.display(),
                            max_depth = MAX_DEPTH,
                            "stopped descending; the files above this depth were still scanned"
                        );
                        continue;
                    }
                    queue.push((path, depth + 1));
                }
                Ok(_) if is_mesh_candidate(&path) => {
                    if let Some(relative) = relative_to(root, &path) {
                        found.push(relative);
                    }
                }
                // Not a candidate — a README beside a library's STLs is not an error, and
                // it is counted nowhere: the batch total grows by the number of mesh
                // candidates, not the number of directory entries.
                Ok(_) => {}
                Err(source) => {
                    tracing::warn!(file = %path.display(), reason = %source, "could not type a directory entry");
                }
            }
        }
    }
    Ok(found)
}

/// `path` relative to `root`, `/`-separated, or `None` if it is somehow not below it.
///
/// `/` rather than the platform separator because this string becomes `part.source_path`,
/// which is compared across scans and, from slice 6a's upload route, against a path a
/// browser reported. One separator, or a library ingested on Windows and re-scanned on
/// Linux is two libraries.
fn relative_to(root: &FsPath, path: &FsPath) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

/// The ingest directory itself could not be walked — a missing mount, a permissions
/// error, or (in a test) a nonexistent `TempDir` path. The whole job fails rather than
/// succeeding with nothing queued, which would be indistinguishable from an empty
/// directory that scanned perfectly well.
///
/// `Permanent`, and that is the classification the reversal in this module's doc rests
/// on. A `Transient` failure is rescheduled with its message on a `pending` row, and
/// `batch_status` reports `failures` for `state = 'failed'` only — so the operator would
/// see nothing at all until `max_attempts` ran out, which is the "quietly fails a poll or
/// two later" this design is supposed to have stopped being. A mount that is absent when
/// a person clicks Scan is a deployment fact, not a race worth three retries, and the
/// remedy is to fix the mount and click Scan again.
fn ingest_dir_unreadable(dir: &FsPath, source: &std::io::Error) -> HandlerError {
    HandlerError::Permanent {
        message: format!(
            "Could not read the ingest directory {}: {source}. Check that the mount is \
             present and readable on the worker, then start the scan again.",
            dir.display()
        ),
    }
}

/// The scan could not be queued at all. Nothing has been written, so retrying the same
/// request is safe and is what the message asks for.
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
/// `tests/handler.rs`'s unreadable-directory test). Reproducing it would need OS- or
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

    /// The reversal recorded in this module's doc rests on the operator seeing this
    /// message on the first poll. `batch_status` lists `failures` for `state = 'failed'`
    /// rows only, and a `Transient` error is rescheduled as `pending` — so if this ever
    /// reads `Transient` again, a bad mount goes unreported until `max_attempts` runs
    /// out and the argument for moving the walk into a job stops holding.
    #[test]
    fn an_unreadable_ingest_directory_is_permanent_so_the_browser_sees_it_at_once() {
        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        match ingest_dir_unreadable(FsPath::new("/ingest"), &err) {
            HandlerError::Permanent { message } => {
                assert!(
                    message.contains("/ingest"),
                    "must name the mount: {message}"
                );
                assert!(
                    message.contains("Check that the mount"),
                    "must say what to check (CLAUDE.md): {message}"
                );
            }
            other => panic!("a bad mount must not be retried into invisibility, got {other:?}"),
        }
    }

    #[test]
    fn a_3mf_is_a_mesh_candidate_and_a_readme_is_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in ["carrier.3mf", "carrier.3MF", "bracket.stl", "README.md"] {
            std::fs::write(dir.path().join(name), b"x").expect("write");
        }
        let mut found: Vec<String> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter(|e| is_mesh_candidate(&e.path()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, vec!["bracket.stl", "carrier.3MF", "carrier.3mf"]);
    }
}
