//! Files into a library from a browser.
//!
//! `DATA.md` §5.2 sets the contract — BLAKE3 on the client before transferring, a probe
//! so a re-import moves only what is new, resumable chunks because a 2 GB file over a VPN
//! as one POST fails at 90%, and a server-side re-verification of the assembled bytes
//! because the client's hash is the dedup key for the whole store and a wrong one
//! silently attaches one library's part to another library's bytes.
//!
//! # Where the bytes land
//!
//! In the blob store, written from here. The alternative was a staging volume the worker
//! also mounts, with `IngestFile` learning a second root to join against; that buys a
//! mount, a payload discriminator and the failure mode of the worker opening a file the
//! api has not finished writing, in order to move bytes this process is already sitting
//! on. `deploy/compose.yaml` mounts the blob volume here read-write already, and says in
//! a comment that this is deliberate — the open-path boundary is a type, not a mount
//! flag.
//!
//! So this route writes the blob and enqueues `JobPayload::IngestBlob`, and the worker
//! reads it back with the full source handle it holds anyway (naming that type here,
//! even in a comment, is what `check-deploy` refuses). The only file-shaped state here
//! is the partial upload, which lives in `upload_dir` and never in the blob store.
//!
//! This is the one file in `lapidary-api` allowed to name `SourceWriter`, enforced by
//! `cargo xtask check-deploy` exactly as `download.rs` is the only one allowed to name
//! the matching read handle. (Naming that type here, even in this sentence, is what
//! the check would refuse — it greps source text, deliberately.) See
//! `lapidary_storage`'s module doc and the slice 6a design §4.1.
//!
//! # The session is the file
//!
//! There is no session table, no session id, no expiry sweep and no in-memory map. The
//! client computed the hash before transferring, so both sides already share one
//! identifier; the chunks append to `<upload_dir>/<library>/<blake3>.part`, and **that
//! file's length is the session state**. Resuming is `offset = length`, and a wrong
//! offset is answered with the length the server actually has rather than guessed at. An
//! api restart costs nothing if `upload_dir` is a volume and costs a re-transfer if it is
//! not, which is what a session table would have said with a schema attached.
//!
//! The hash goes through `BlobHash::parse_hex` — 64 characters, lowercase, hex — before
//! it reaches the filesystem, so there is no client string in that path to traverse with.
//! `source_path` never reaches a filesystem here at all; it is checked because it becomes
//! `part.source_path` and from there a `Content-Disposition` filename on the download
//! route.

use crate::AppState;
use crate::derive::{accept, internal_error, no_such_library};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, JobPayload, LibraryId, path_escapes, source_format};
use lapidary_db::{PgBlobs, PgParts, StoredBlobRow};
use lapidary_storage::{Compression, SourceWriter};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// The largest a single chunk may be. Generous enough that a 2 GB file is 128 requests
/// rather than 2,000, small enough that a chunk lost to a dropped connection is a second
/// of transfer rather than a minute — and small enough that the api holding one whole
/// chunk in memory per concurrent upload is bounded well under the 512 MB the container
/// is capped at.
pub(crate) const MAX_CHUNK_BYTES: usize = 16 * 1024 * 1024;

/// The largest manifest either JSON route will read. See `lib.rs` where it is applied:
/// axum's 2 MB default is about 16,000 files, which a real parts library passes.
pub(crate) const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

/// `DATA.md` §1.2's stated upper bound for a source file. Enforced on the staged file
/// rather than on a declared size because a declared size is a claim: a client that lies
/// about it still cannot append past this.
const MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// One file the client is offering: the path it will be known by, and the hash it claims
/// the bytes have.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UploadFile {
    /// Relative to the folder that was dropped, `/`-separated — the browser's own
    /// `webkitRelativePath`. This is the `source_path` that gives the part its identity
    /// within the library, which is what makes an uploaded folder and a scanned folder
    /// the same thing in the database.
    pub path: String,
    pub blake3: BlobHash,
}

/// What the client is offering, whole. The probe and the commit take the same list — they
/// are the same question asked before and after the transfer — so they take the same
/// type rather than two that must be kept in step.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UploadManifest {
    pub files: Vec<UploadFile>,
}

/// What the client has to do, per file, sorted into three answers.
///
/// `DATA.md` describes two lists. The third is where the larger win is: `have` only
/// catches a re-import into the *same* library at the *same* path, while `needRows`
/// catches every file whose bytes are already in the store for any reason — a second
/// library, a moved file, the same model downloaded twice — and skips its transfer
/// entirely.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UploadPlan {
    /// This library already indexes these bytes at this path. Nothing to send, nothing
    /// to commit.
    pub have: Vec<String>,
    /// The bytes are in the store; only the rows are missing. Commit without
    /// transferring.
    pub need_rows: Vec<String>,
    /// Send these.
    pub need_bytes: Vec<String>,
}

/// How much of a staged file the server holds. The answer to a chunk, and the answer to
/// a rejected offset — a resuming client reads the same field either way.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChunkAccepted {
    /// `number` on the wire, for the reason `PartSummary::source_bytes` gives: serde
    /// writes a JSON number and ts-rs would otherwise type a 64-bit integer as `bigint`,
    /// which `JSON.parse` never produces — so the client would compare an offset against
    /// a type it can never hold. The 2^53 ceiling this buys is far above the 2 GB one
    /// this route already enforces.
    #[ts(type = "number")]
    pub received: u64,
}

#[derive(Debug, Deserialize)]
pub struct ChunkQuery {
    offset: u64,
}

/// `POST /api/libraries/{id}/uploads/probe` — which of these files does this library
/// still need, and which of those need their bytes.
pub async fn probe(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(manifest): Json<UploadManifest>,
) -> Response {
    if let Err(response) = library_exists(&state, library).await {
        return response;
    }
    let blobs = PgBlobs(state.db.clone());
    let mut plan = UploadPlan {
        have: Vec::new(),
        need_rows: Vec::new(),
        need_bytes: Vec::new(),
    };
    // Two queries per file, in sequence. A folder of 1,700 files is 3,400 round trips
    // against a database in the same compose network, once, before any byte moves — the
    // transfer it decides about is several orders of magnitude longer. A single query
    // over an unnested array is the upgrade if a corpus ever makes this visible.
    for file in &manifest.files {
        match blobs.library_holds(library, &file.path, &file.blake3).await {
            Ok(true) => {
                plan.have.push(file.path.clone());
                continue;
            }
            Ok(false) => {}
            Err(err) => return internal_error(&err, "upload probe failed"),
        }
        match blobs.exists(&file.blake3).await {
            Ok(true) => plan.need_rows.push(file.path.clone()),
            Ok(false) => plan.need_bytes.push(file.path.clone()),
            Err(err) => return internal_error(&err, "upload probe failed"),
        }
    }
    Json(plan).into_response()
}

/// `PUT /api/libraries/{id}/uploads/{blake3}?offset=N` — one chunk, appended.
///
/// `offset` is checked rather than trusted, and a mismatch is a `409` carrying the length
/// the server actually holds. A client that lost track of where it was is told; a client
/// that retried a chunk that had in fact landed is told that too, and does not duplicate
/// it. Nothing here is trusted to be right about the bytes — the assembled file is
/// verified against the hash at commit, and only then does anything enter the store.
pub async fn chunk(
    State(state): State<AppState>,
    Path((library, hex)): Path<(LibraryId, String)>,
    Query(query): Query<ChunkQuery>,
    body: Bytes,
) -> Response {
    let hash = match BlobHash::parse_hex(&hex) {
        Ok(hash) => hash,
        Err(err) => return bad_request(err.to_string()),
    };
    if body.len() > MAX_CHUNK_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "message": format!(
                    "That chunk is larger than the {} MB limit. Send the file in smaller \
                     chunks.",
                    MAX_CHUNK_BYTES / 1024 / 1024
                )
            })),
        )
            .into_response();
    }

    let path = staged_path(&state.upload_dir, library, &hash);
    let offset = query.offset;
    let appended = tokio::task::spawn_blocking(move || append_chunk(&path, offset, &body)).await;
    match appended {
        Ok(Ok(received)) => Json(ChunkAccepted { received }).into_response(),
        Ok(Err(refusal)) => refusal.into_response(),
        Err(err) => {
            tracing::error!(error = %err, "the upload chunk writer panicked");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "message": "Could not store that chunk. Retry the upload; if it keeps \
                                failing, check the server's upload volume has space."
                })),
            )
                .into_response()
        }
    }
}

/// Why a chunk was not appended. Separate from the handler so the blocking half can be
/// written as ordinary synchronous code and still say precisely what went wrong.
enum ChunkRefusal {
    WrongOffset { expected: u64, got: u64 },
    TooLarge,
    Io(std::io::Error),
}

impl IntoResponse for ChunkRefusal {
    fn into_response(self) -> Response {
        match self {
            // The length is in the body as `received`, the same field a successful chunk
            // answers with, so a resuming client reads one field rather than parsing a
            // sentence.
            ChunkRefusal::WrongOffset { expected, got } => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "received": expected,
                    "message": format!(
                        "This upload is at byte {expected}, not {got}. Resume from \
                         {expected} — the `received` field is where to continue."
                    )
                })),
            )
                .into_response(),
            ChunkRefusal::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "message": format!(
                        "That file is larger than the {} GB limit for a single source \
                         file.",
                        MAX_UPLOAD_BYTES / 1024 / 1024 / 1024
                    )
                })),
            )
                .into_response(),
            ChunkRefusal::Io(err) => {
                tracing::error!(error = %err, "could not append to a staged upload");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "message": "Could not store that chunk. Retry the upload; if it \
                                    keeps failing, check the server's upload volume has \
                                    space."
                    })),
                )
                    .into_response()
            }
        }
    }
}

/// Append one chunk to the staged file, refusing an offset that is not its current end.
///
/// Blocking, and called from `spawn_blocking` for the reason `download.rs` streams that
/// way: a 16 MB write is not something to do on a runtime thread that is also serving the
/// grid.
fn append_chunk(path: &std::path::Path, offset: u64, body: &[u8]) -> Result<u64, ChunkRefusal> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ChunkRefusal::Io)?;
    }
    let held = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(ChunkRefusal::Io(e)),
    };
    if offset != held {
        return Err(ChunkRefusal::WrongOffset {
            expected: held,
            got: offset,
        });
    }
    if held + body.len() as u64 > MAX_UPLOAD_BYTES {
        return Err(ChunkRefusal::TooLarge);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(ChunkRefusal::Io)?;
    std::io::Write::write_all(&mut file, body).map_err(ChunkRefusal::Io)?;
    Ok(held + body.len() as u64)
}

/// `POST /api/libraries/{id}/uploads/commit` — verify what was staged, store it, and
/// queue the meshing.
///
/// One batch for the whole manifest, not one per file: a folder of 500 parts committed
/// file by file would give the grid 500 progress bars. That is also why the transfer has
/// to finish before anything is committed — the batch appears when the bytes are in, and
/// tracks the work that is left.
///
/// Every file is enqueued, including one whose bytes and path this library already holds.
/// The worker's `library_holds` short-circuit answers that with `Outcome::Skipped`
/// without parsing anything, which is the same answer a re-scan gives and keeps the
/// batch's total equal to the number of files the user actually dropped.
pub async fn commit(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(manifest): Json<UploadManifest>,
) -> Response {
    if let Err(response) = library_exists(&state, library).await {
        return response;
    }
    // Every path first, before a single blob is written. A manifest with one bad path in
    // it is a mistake about the whole drop, and answering it after storing the first
    // four hundred files leaves the user to work out which ones landed.
    if let Some(bad) = manifest.files.iter().find(|f| path_escapes(&f.path)) {
        return bad_request(format!(
            "Refused the file path {:?}: a file's path must be relative to the folder you \
             dropped, and may not be absolute or contain `..`.",
            bad.path
        ));
    }

    let blobs = PgBlobs(state.db.clone());
    let writer = SourceWriter::open(&state.blob_root);
    let mut jobs = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        match blobs.exists(&file.blake3).await {
            // Some library already holds these bytes. There is nothing to verify and
            // nothing to write — the store is content-addressed, so the bytes at that
            // hash are already the bytes this file names.
            Ok(true) => {}
            Ok(false) => {
                if let Err(response) = store_staged(&state, &writer, &blobs, library, file).await {
                    return response;
                }
            }
            Err(err) => return internal_error(&err, "upload commit failed"),
        }
        jobs.push(JobPayload::IngestBlob {
            blake3: file.blake3,
            source_path: file.path.clone(),
        });
    }
    accept(state.db, library, &jobs).await
}

/// Move one staged file into the blob store, and record it.
///
/// The `blob` row is the point of the second half. Between this commit and the worker's
/// job there are bytes on disk that no `part` references, and if that job fails
/// permanently nothing would ever reap them — the worker did not write them and must not
/// assume it may delete them. A row with `ref_count = 0` makes those bytes *known*: slice
/// 7's reference-counted reaper is defined over exactly that row, and an orphan with no
/// row at all is invisible to it forever. See the slice 6a design §4.2.
///
/// Order matters and is the same order ingest uses: bytes to disk, then the row. A
/// filesystem write cannot be rolled back by Postgres, so the bytes must be there before
/// anything is allowed to point at them. If the row fails the staged file is left alone,
/// so the client can commit again rather than re-transfer.
async fn store_staged(
    state: &AppState,
    writer: &SourceWriter,
    blobs: &PgBlobs,
    library: LibraryId,
    file: &UploadFile,
) -> Result<(), Response> {
    let staged = staged_path(&state.upload_dir, library, &file.blake3);
    if !staged.exists() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "message": format!(
                    "The bytes for {:?} have not been uploaded yet. Send the file's \
                     chunks before committing it.",
                    file.path
                )
            })),
        )
            .into_response());
    }

    let compression = Compression::for_source_format(&source_format(&file.path));
    let stored = match writer.put_file(&staged, &file.blake3, compression) {
        Ok(stored) => stored,
        Err(err) => {
            // A hash mismatch is the client's problem and the client's message: the
            // transfer corrupted, or the file changed under it. Everything else is ours.
            // Either way the staged file is removed — it is known-bad under this hash,
            // and leaving it means the retry resumes from the end of bytes that will
            // never verify.
            let _ = std::fs::remove_file(&staged);
            return Err(match err {
                lapidary_storage::StorageError::HashMismatch { .. } => bad_request(err.to_string()),
                other => {
                    tracing::error!(error = %other, "could not store an uploaded blob");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "message": other.to_string() })),
                    )
                        .into_response()
                }
            });
        }
    };

    if let Err(err) = blobs
        .record_unreferenced(&StoredBlobRow {
            hash: stored.hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        })
        .await
    {
        return Err(internal_error(&err, "recording an uploaded blob failed"));
    }

    // Best-effort, and warn-only for the reason ingest's reaps are: the bytes are safely
    // in the store and the user's file is committed, so a staged copy left behind is
    // wasted disk rather than a failure worth refusing the upload over. It is still worth
    // a line, because nothing else will ever look at that file.
    if let Err(err) = std::fs::remove_file(&staged) {
        tracing::warn!(
            path = %staged.display(),
            error = %err,
            "failed to remove a staged upload after committing it; it is now wasted disk"
        );
    }
    Ok(())
}

/// Where a library's partial upload of these bytes lives.
///
/// The hash has already been through `BlobHash::parse_hex`, so the file name is 64
/// characters of lowercase hex and cannot traverse. The library scopes the directory,
/// which is not an authorization check — Phase 1 has no principal — but removes the
/// question of two libraries uploading the same bytes interleaving into one partial file
/// and failing each other's verification.
fn staged_path(upload_dir: &std::path::Path, library: LibraryId, hash: &BlobHash) -> PathBuf {
    upload_dir
        .join(library.to_string())
        .join(format!("{}.part", hash.to_hex()))
}

/// The same existence probe `scan.rs` makes, for the same reason: without it a manifest
/// against a mistyped library id writes blobs, fails the enqueue on a foreign key, and
/// reports a database error for what is one wrong character in a URL.
async fn library_exists(state: &AppState, library: LibraryId) -> Result<(), Response> {
    match PgParts(state.db.clone()).auto_thumbnail(library).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(no_such_library()),
        Err(err) => Err(internal_error(&err, "library lookup failed")),
    }
}

/// The client sent something this route cannot act on, and the message says what to send
/// instead. Every caller here already has a full sentence, so this only puts it in the
/// envelope the rest of the api uses.
fn bad_request(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}
