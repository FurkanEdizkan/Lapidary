//! Kicking off a library scan (ingest): walk the read-only mounted ingest directory,
//! hash each `.stl`, and either link an already-known blob or parse, rasterize and
//! record a new one. `router()` (`lib.rs`) always mounts this — see that module's doc
//! for why this crate, rather than a role check inside `lapidary-api`, is what keeps the
//! open path from linking the CAD kernel.
//!
//! # Ordering
//!
//! Per file, in this order — the order is the design:
//!
//! 1. read bytes
//! 2. BLAKE3 — hash first, always
//! 3. `blobs.library_holds(library, name, hash)`? yes -> count it `skipped`, no further
//!    work at all: not a parse, not a raster, not a query beyond this one
//! 4. `kernel.ingest(bytes)` — parse + measure + rasterize
//! 5. does any library already hold these bytes (`blobs.exists(hash)`)?
//!    - yes -> `ingest.link_existing(...)`: the blob stays exactly where it is, and this
//!      library gets its own part pointing at it. No write, so nothing to reap.
//!    - no  -> `source.put(bytes)` writes the blob *before* the transaction, then
//!      `ingest.record(...)`; on error, `source.remove(hash)` reaps the blob just
//!      written and the failure is recorded
//!
//! Step 5's reap is not optional. The Node prototype wrote its blob and then failed the
//! insert with no cleanup, leaving bytes on disk that nothing referenced and nothing
//! would ever collect — `docs/prototype-notes.md` records it. It is equally not optional
//! that the reap runs only on the branch that *wrote* something: reaping a blob another
//! library's part references would be silent data loss, which is why the two branches
//! are separate rather than one call with a flag.
//!
//! # Why the short-circuit is scoped to the library, and to the name
//!
//! It was not, and the consequence was live: `PgBlobs::exists` is keyed on the hash
//! alone, so scanning six real STLs into a brand-new empty library answered
//! `{"ingested":0,"skipped":6,"failed":[]}` and left the library empty. Three things
//! were wrong at once — a content hash decided a per-library write (CLAUDE.md: content
//! addressing is not authorization), the counter said a file row had been linked when
//! none had, and the user got an empty grid with no error anywhere to explain it.
//!
//! `blobs.library_holds(library, name, hash)` is the question this handler actually
//! needs: *is this the same file, seen again?* The bytes are still reused — that is the
//! whole point of content addressing, and step 5 reuses them without a second write —
//! but a library that does not have this part gets one.
//!
//! Keying on the part name as well as the hash settles the other half, which the earlier
//! rounds recorded as an open question: two differently-named files with identical bytes
//! are two parts sharing one blob (`ref_count` 2), not one part and one silent omission.
//! A directory of files is a set of files, and a scan that quietly indexes only the first
//! of two is the same shape of lie as the empty second library. Only "same library, same
//! name, same bytes" is a re-scan.
//!
//! Known limitation, scheduled rather than guessed at: nothing here records the *path* a
//! part came from, so renaming a file and re-scanning yields a new part beside the old
//! one rather than a rename. Closing that needs a source-path column and the slice that
//! owns incremental directory sync; a duplicate the user can see is the right failure
//! mode to have in the meantime, against a silent one.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_cad::MeshKernel;
use lapidary_core::{BlobHash, LibraryId};
use lapidary_db::{IngestRequest, PgBlobs, PgIngest, StoredBlobRow};
use lapidary_storage::{SourceStore, WorkerRole};
use serde::Serialize;
use std::path::Path as FsPath;

/// The three counters are disjoint and sum to the number of `*.stl` candidates walked.
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    /// A part row was created for this file in this library. Its bytes may have been
    /// reused from a blob another library already held — reuse is invisible here,
    /// because from this library's point of view a part appeared either way.
    pub ingested: u32,
    /// This library already holds a part with this name and these exact bytes, so
    /// nothing was done. Never reachable for a library that does not hold the file:
    /// answering `skipped` while a library stays empty is the failure this counter had.
    pub skipped: u32,
    pub failed: Vec<ScanFailure>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanFailure {
    pub file: String,
    pub reason: String,
}

/// What happened to one candidate file. `Ingested` and `Skipped` map straight onto
/// `ScanReport`'s disjoint counters; a failure carries its own reason and is handled by
/// the caller rather than folded in here, so `ingest_one`'s `Err` type stays a plain
/// `String` describing what broke.
enum FileOutcome {
    Ingested,
    Skipped,
}

/// Walks `state.ingest_dir` non-recursively, ingesting every `.stl` (case-insensitive)
/// into `library`. One file's failure is recorded in `failed` and the walk continues — a
/// scan is not transactional across files, so a malformed STL must not abort the ones
/// after it. Always 200: a non-empty `failed` list is a partial success, not an error.
pub async fn scan(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    let entries = match std::fs::read_dir(&state.ingest_dir) {
        Ok(entries) => entries,
        Err(source) => return ingest_dir_unreadable(&state.ingest_dir, &source),
    };

    let kernel = MeshKernel;
    let version = kernel.version();
    let kernel_version = format!("{} {}", version.implementation, version.version);
    let source = SourceStore::open(&state.blob_root, &WorkerRole::assume());
    let blobs = PgBlobs(state.db.clone());
    let ingest = PgIngest(state.db.clone());

    let mut report = ScanReport::default();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // Distinct from the initial read_dir failure above: by the time this fires,
            // some entries may already have been walked and committed. That partial
            // progress belongs in `report`, not discarded — this is a per-file failure
            // to record and continue past, exactly like a malformed STL is, not a reason
            // to throw away everything seen so far and answer with a bare 500. See
            // `entry_read_failure`'s doc for why no live test constructs this condition.
            Err(source) => {
                report
                    .failed
                    .push(entry_read_failure(&state.ingest_dir, &source));
                continue;
            }
        };
        let path = entry.path();
        if !is_stl_candidate(&path) {
            // Not a candidate — a README beside a library's STLs is not an error, and it
            // is counted nowhere: ingested + skipped + failed.len() must sum to the
            // number of *.stl candidates walked, not to every directory entry.
            continue;
        }
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        match ingest_one(
            &path,
            &file_name,
            library,
            &kernel,
            &kernel_version,
            &source,
            &blobs,
            &ingest,
        )
        .await
        {
            Ok(FileOutcome::Ingested) => report.ingested += 1,
            Ok(FileOutcome::Skipped) => report.skipped += 1,
            Err(reason) => report.failed.push(ScanFailure {
                file: file_name,
                reason,
            }),
        }
    }

    Json(report).into_response()
}

fn is_stl_candidate(path: &FsPath) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("stl"))
}

/// The ingest directory itself could not be walked — a missing mount, a permissions
/// error, or (in a test) a nonexistent `TempDir` path. Distinct from a per-file failure:
/// nothing about which files exist is known yet, so there is no per-file `failed` entry
/// to produce, and the whole request fails rather than reporting an empty, misleadingly
/// clean `ScanReport`.
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
/// `tests/scan.rs`'s truncated-STL and nonexistent-library tests). Reproducing it would
/// need OS- or hardware-level fault injection, which is neither portable across the
/// platforms CI runs nor safe to do in a shared test process. `entry_read_failure` is
/// factored out as a pure function specifically so the one part that *is* testable
/// portably — what gets reported, not how the OS condition arises — has a unit test
/// below, and the loop's `continue` (not `return`) is a one-line, visually-checkable
/// fact at the call site.
fn entry_read_failure(dir: &FsPath, source: &std::io::Error) -> ScanFailure {
    ScanFailure {
        file: "<unreadable directory entry>".to_owned(),
        reason: format!(
            "Could not read a directory entry in {}: {source}. The OS did not report which \
             file this was — check permissions on the ingest mount, and that nothing \
             removed a file while the scan was running.",
            dir.display()
        ),
    }
}

/// The part name shown in the grid. Slice 1 has no part-numbering convention to draw on,
/// so the file's stem (its name without the `.stl` extension) is the whole story; falls
/// back to the full file name on the pathological case where a candidate file (already
/// proven to have a `.stl` extension by `is_stl_candidate`) somehow has no stem.
fn part_name(file_name: &str) -> &str {
    FsPath::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name)
}

#[allow(clippy::too_many_arguments)]
async fn ingest_one(
    path: &FsPath,
    file_name: &str,
    library: LibraryId,
    kernel: &MeshKernel,
    kernel_version: &str,
    source: &SourceStore,
    blobs: &PgBlobs,
    ingest: &PgIngest,
) -> Result<FileOutcome, String> {
    // 1. Read bytes.
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {file_name}: {e}"))?;

    // 2. BLAKE3 — hash first, always. Everything below branches on this.
    let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
    let name = part_name(file_name);

    // 3. The same file, seen again — same library, same name, same bytes — short-circuits
    // parse, raster and every write entirely. Scoped to the library on purpose: a hash
    // this library has never seen is a part it does not have, whatever some other library
    // holds. See the module doc.
    if blobs
        .library_holds(library, name, &hash)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(FileOutcome::Skipped);
    }

    // 4. Parse + measure + rasterize. Nothing has been written yet, so a failure here
    // needs no cleanup. This runs even when the bytes are already in the blob store,
    // because the new part needs its own measurements and its own thumbnail; only the
    // bytes are shared, and they are already in memory from step 1.
    let output = kernel.ingest(&bytes).map_err(|e| e.to_string())?;

    // 5a. Some library already holds these bytes. Reuse them exactly as they are: no
    // second copy on disk, no second `blob` row, and — the part that matters — no reap
    // on failure, because those bytes are referenced by a part this scan did not create.
    if blobs.exists(&hash).await.map_err(|e| e.to_string())? {
        let blob = StoredBlobRow {
            hash,
            // `link_existing` reads only `hash` and `size_bytes` (the `file` row); the
            // `blob` row, and with it the stored size and compression level, already
            // exists and is not rewritten.
            size_bytes: bytes.len() as u64,
            stored_bytes: bytes.len() as u64,
            zstd_level: 0,
        };
        return ingest
            .link_existing(IngestRequest {
                library,
                name,
                blob: &blob,
                measurements: &output.measurements,
                thumbnail_webp: &output.thumbnail_webp,
                kernel_version,
            })
            .await
            .map(|_| FileOutcome::Ingested)
            .map_err(|e| e.to_string());
    }

    // 5b. New bytes. The blob is written before the transaction. `stored.hash` is
    // recomputed from `bytes` inside `put` and is definitionally the same as `hash`
    // above; `hash` is used below rather than `stored.hash` so there is exactly one hash
    // variable in scope.
    let stored = source.put(&bytes).map_err(|e| e.to_string())?;
    let blob = StoredBlobRow {
        hash,
        size_bytes: stored.size_bytes,
        stored_bytes: stored.stored_bytes,
        zstd_level: stored.zstd_level,
    };

    // 6. One transaction. On failure, reap the blob `put` just wrote — nothing
    // references it, and nothing else ever will, so it must not be left on disk. The
    // Node prototype's exact miss (docs/prototype-notes.md): a successful blob write
    // followed by a failed DB insert, with no cleanup.
    match ingest
        .record(IngestRequest {
            library,
            name,
            blob: &blob,
            measurements: &output.measurements,
            thumbnail_webp: &output.thumbnail_webp,
            kernel_version,
        })
        .await
    {
        Ok(_) => Ok(FileOutcome::Ingested),
        Err(db_err) => {
            // Best-effort: the DB error is the one worth reporting to the caller either
            // way, and a failed reap does not change what they need to know about this
            // file. But a failed reap is not nothing — it leaves bytes behind that
            // nothing will ever reference and nothing will ever collect, which is
            // exactly the leak this whole reap exists to close. `SourceStore::remove`
            // already treats a missing file as success, so this only fires on a real
            // I/O problem with the store itself — the one place in the pipeline that
            // knowingly leaves bytes behind, so it is the one place that says so.
            if let Err(reap_err) = source.remove(&hash) {
                tracing::warn!(
                    hash = %hash.to_hex(),
                    error = %reap_err,
                    "failed to reap a blob after a failed ingest write; it may now be an orphan on disk"
                );
            }
            Err(db_err.to_string())
        }
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
