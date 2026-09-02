//! Kicking off a library scan (ingest): walk the read-only mounted ingest directory,
//! hash each `.stl`, and either link an already-known blob or parse, rasterize and
//! record a new one. `router()` (`lib.rs`) mounts this only under `Role::Worker`.
//!
//! This is the one file in `lapidary-api` allowed to name `SourceStore` and the one place
//! that depends on `MeshKernel` — `xtask/src/deploy.rs`'s `check_open_path_boundary`
//! exempts it by name (`OPEN_PATH_BOUNDARY_EXEMPTIONS`), and `xtask/src/layers.rs` no
//! longer forbids `lapidary-api -> lapidary-cad` as a blanket dependency-graph edge (see
//! the doc comments on `FORBIDDEN_PAIRS` there for why the role split made a crate-level
//! ban the wrong granularity). The product rule — the open path never touches a source
//! file and never invokes the kernel — is enforced by `Role::Api` never mounting this
//! route at all, not by keeping the crate ignorant of these types.
//!
//! # Ordering
//!
//! Per file, in this order — the order is the design:
//!
//! 1. read bytes
//! 2. BLAKE3 — hash first, always
//! 3. `blobs.exists(hash)`? yes -> count it `skipped`, no further work (see below)
//! 4. `kernel.ingest(bytes)` — parse + measure + rasterize
//! 5. `source.put(bytes)` — the blob is written *before* the transaction
//! 6. `ingest.record(...)` — one transaction; on error, `source.remove(hash)` reaps the
//!    blob just written, then the failure is recorded
//!
//! Step 6's reap is not optional. The Node prototype wrote its blob and then failed the
//! insert with no cleanup, leaving bytes on disk that nothing referenced and nothing
//! would ever collect — `docs/prototype-notes.md` records it.
//!
//! # Deviation from the plan's outline: the short-circuit never calls `link_existing`
//!
//! The plan's outline (and `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-
//! design.md` §5) has step 3 call `PgIngest::link_existing` on a known hash: "link a new
//! file row to the existing blob, bump ref_count". That cannot satisfy this task's own
//! acceptance test — re-scanning an unchanged directory must still leave exactly one
//! part — because `insert_part_chain` (shared by `record` and `link_existing`, landed in
//! Task 7 and out of this task's scope to change) always inserts a brand-new `part` row
//! with a fresh id; nothing in the schema or repository layer deduplicates by hash or by
//! path. Calling `link_existing` on every hash hit, including a verbatim re-scan, would
//! create a second part every time the same folder is scanned twice — worse, an
//! unbounded number of duplicate parts on every repeat scan, which is a real workflow
//! (add one file, scan again) for a product whose whole point is "hundreds of parts,
//! browsable". Task 7's own test (`linking_an_existing_blob_adds_a_part_without_touching_
//! ref_count_twice`, `crates/lapidary-db/tests/repo.rs`) models `link_existing` for a
//! *different* physical part that happens to share byte-identical content ("Bracket
//! copy, LP-1042-03"), not for the same file re-seen — that reading is consistent with
//! treating a known hash here as a true no-op instead.
//!
//! Consequence: a known hash here does zero database work, and `PgIngest::link_existing`
//! is not called from this handler. It stays correct and tested in `lapidary-db` for a
//! caller that can tell "the same file, again" apart from "a different file, same
//! bytes" — this handler cannot, since `PgBlobs::exists` is not scoped to a library or a
//! path, only a hash, and adding that distinction is out of this task's declared scope
//! (`crates/lapidary-db` is not in its file list). One known limitation follows: two
//! genuinely distinct files that happen to be byte-identical are indistinguishable from a
//! re-scan here, so only the first is ever ingested as a part. No enumerated test in this
//! task's brief exercises that case; flagging it rather than guessing further.

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

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub ingested: u32,
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
            Err(source) => return ingest_dir_unreadable(&state.ingest_dir, &source),
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

    // 3. A known hash short-circuits parse, raster and the blob write entirely — see the
    // module doc for why this is a true no-op rather than a call to `link_existing`.
    if blobs.exists(&hash).await.map_err(|e| e.to_string())? {
        return Ok(FileOutcome::Skipped);
    }

    // 4. Parse + measure + rasterize. Nothing has been written yet, so a failure here
    // needs no cleanup.
    let output = kernel.ingest(&bytes).map_err(|e| e.to_string())?;

    // 5. The blob is written before the transaction. `stored.hash` is recomputed from
    // `bytes` inside `put` and is definitionally the same as `hash` above; `hash` is used
    // below rather than `stored.hash` so there is exactly one hash variable in scope.
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
    let name = part_name(file_name);
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
            // Best-effort: the DB error is the one worth reporting either way, and a
            // failed reap here does not change what the caller needs to know about this
            // file. `SourceStore::remove` already treats a missing file as success, so
            // this only fails on a real I/O problem with the store itself.
            let _ = source.remove(&hash);
            Err(db_err.to_string())
        }
    }
}
