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
use axum::extract::rejection::{BytesRejection, FailedToBufferBody};
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
    /// The check-out these bytes were saved under: the agent's, never the browser's. A changed
    /// file for a checked-out part is kept only when it carries that part's lock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lock: Option<lapidary_core::LockId>,
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
    // Two queries for the whole manifest, however many files it names.
    let files: Vec<(&str, BlobHash)> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.blake3))
        .collect();
    let held = match blobs.library_holds_each(library, &files).await {
        Ok(held) => held,
        Err(err) => return internal_error(&err, "upload probe failed"),
    };
    let hashes: Vec<BlobHash> = manifest
        .files
        .iter()
        .zip(&held)
        .filter(|(_, held)| !**held)
        .map(|(file, _)| file.blake3)
        .collect();
    let stored = match blobs.existing(&hashes).await {
        Ok(stored) => stored,
        Err(err) => return internal_error(&err, "upload probe failed"),
    };
    let mut plan = UploadPlan {
        have: Vec::new(),
        need_rows: Vec::new(),
        need_bytes: Vec::new(),
    };
    for (file, held) in manifest.files.into_iter().zip(held) {
        if held {
            plan.have.push(file.path);
        } else if stored.contains(&file.blake3) {
            plan.need_rows.push(file.path);
        } else {
            plan.need_bytes.push(file.path);
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
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let hash = match BlobHash::parse_hex(&hex) {
        Ok(hash) => hash,
        Err(err) => return bad_request(err.to_string()),
    };
    // The size limit is `DefaultBodyLimit` on this route and not a length test here,
    // because a test here has already lost: reading the body to measure it is the memory
    // the limit exists to bound. What this arm does is replace the rejection's message.
    // axum answers "Failed to buffer the request body: length limit exceeded", which
    // names our framework rather than the caller's mistake and says nothing about what
    // to send instead.
    //
    // An earlier version kept a `body.len() > MAX_CHUNK_BYTES` check with the layer set
    // one byte above it, so that ours would fire "first". It fired for exactly one body
    // size and every genuinely oversized chunk got axum's line -- which a live 35 MB PUT
    // is what showed.
    let body = match body {
        Ok(body) => body,
        Err(rejection) => return refuse_chunk(&rejection),
    };

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

/// A chunk the server would not read at all.
///
/// Only one rejection is reachable in practice — the body over `MAX_CHUNK_BYTES` — but
/// the others (a client that hung up mid-body, an unreadable stream) are real and get a
/// sentence rather than being flattened into the size message, which would tell someone
/// whose connection dropped to send smaller chunks.
fn refuse_chunk(rejection: &BytesRejection) -> Response {
    let (status, message) = match rejection {
        BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "That chunk is larger than the {} MB limit. Send the file in smaller chunks.",
                MAX_CHUNK_BYTES / 1024 / 1024
            ),
        ),
        other => (
            StatusCode::BAD_REQUEST,
            format!("Could not read that chunk: {other}. Send it again."),
        ),
    };
    (status, Json(serde_json::json!({ "message": message }))).into_response()
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

    let hashes: Vec<BlobHash> = manifest.files.iter().map(|file| file.blake3).collect();
    // Bytes some library already holds need nothing verified and nothing written: the store is
    // content-addressed, so the bytes at that hash are already the bytes a file names.
    let mut stored = match PgBlobs(state.db.clone()).existing(&hashes).await {
        Ok(stored) => stored,
        Err(err) => return internal_error(&err, "upload commit failed"),
    };
    let mut jobs = Vec::with_capacity(manifest.files.len());
    let mut storing = Vec::new();
    for file in &manifest.files {
        // Each set of bytes the store lacks, once: two files with the same bytes stage one file, and
        // storing it twice at once would race over it.
        if stored.insert(file.blake3) {
            storing.push(file.clone());
        }
        jobs.push(JobPayload::IngestBlob {
            blake3: file.blake3,
            source_path: file.path.clone(),
            lock: file.lock,
        });
    }
    if let Err(response) = store_all(&state, library, storing).await {
        return response;
    }
    accept(state.db, library, &jobs).await
}

/// The most files one commit verifies and stores at once. Hashing and compressing take a core each, and
/// the process storing them is the one serving the grid, so a commit on a many-core host leaves it the
/// rest. Under compose's one-CPU api, `available_parallelism` is 1 and a commit stores one at a time.
const STORING_AT_ONCE: usize = 4;

/// Every staged file the store lacks, verified and stored as many at a time as there are cores, up to
/// [`STORING_AT_ONCE`]. Hashing and compressing are the commit's cost, and a drop's files are independent
/// of each other.
///
/// The first refusal is the answer, and no file is started after it. Files already stored stay, as
/// unreferenced blobs the reaper collects, as they did when a commit was refused partway.
async fn store_all(
    state: &AppState,
    library: LibraryId,
    files: Vec<UploadFile>,
) -> Result<(), Response> {
    let at_once = std::thread::available_parallelism()
        .map_or(1, |cores| cores.get())
        .min(STORING_AT_ONCE);
    let mut running = tokio::task::JoinSet::new();
    let mut refused = None;
    for file in files {
        while running.len() >= at_once {
            if let Some(response) = running.join_next().await.and_then(refusal_of) {
                refused.get_or_insert(response);
            }
        }
        if refused.is_some() {
            break;
        }
        let state = state.clone();
        running.spawn(async move { store_staged(&state, library, &file).await });
    }
    while let Some(result) = running.join_next().await {
        if let Some(response) = refusal_of(result) {
            refused.get_or_insert(response);
        }
    }
    refused.map_or(Ok(()), Err)
}

/// One store's refusal, if it made one, with a store that panicked answered as the server's own failure.
fn refusal_of(result: Result<Result<(), Response>, tokio::task::JoinError>) -> Option<Response> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(response)) => Some(response),
        Err(error) => {
            tracing::error!(%error, "storing an uploaded file stopped");
            Some(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "message": "Storing an uploaded file stopped unexpectedly. Commit the upload again; the server log has the cause."
                    })),
                )
                    .into_response(),
            )
        }
    }
}

/// `POST /api/libraries/{id}/imports`'s body: a bundle already sent through the chunked upload.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub blake3: BlobHash,
    /// The file's own name, which the batch names a refused bundle by.
    pub name: String,
}

/// `POST /api/libraries/{id}/imports` — store an uploaded bundle and queue the job that checks
/// and unpacks it (Phase 4 slice 2 spec §7).
///
/// The api does not open the archive: reading stored source bytes is the worker's, and
/// `download.rs` is this crate's one exception. So a file that is not a bundle is refused by its
/// job, in the batch the browser follows, before any part of it is written.
pub async fn import_bundle(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(request): Json<ImportRequest>,
) -> Response {
    if let Err(response) = library_exists(&state, library).await {
        return response;
    }
    if request.name.is_empty() || request.name.contains('/') || path_escapes(&request.name) {
        return bad_request(format!(
            "Refused the bundle name {:?}: send the file's own name, with no folder in it.",
            request.name
        ));
    }
    let file = UploadFile {
        path: request.name.clone(),
        blake3: request.blake3,
        lock: None,
    };
    // Always from this library's own staged upload, never from bytes the store already holds:
    // content addressing is not authorization, and a bundle's hash alone must not import another
    // library's export here.
    if let Err(response) = store_staged(&state, library, &file).await {
        return response;
    }
    accept(
        state.db,
        library,
        &[JobPayload::ImportBundle {
            blake3: request.blake3,
            path: request.name,
        }],
    )
    .await
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
    // Hashing and compressing block, so they run on the blocking pool instead of holding one of the
    // runtime's threads for the length of the file.
    let (root, path, expect) = (state.blob_root.clone(), staged.clone(), file.blake3);
    let put = tokio::task::spawn_blocking(move || {
        SourceWriter::open(&root).put_file(&path, &expect, compression)
    })
    .await
    .unwrap_or_else(|error| {
        Err(lapidary_storage::StorageError::Io {
            path: staged.display().to_string(),
            source: std::io::Error::other(error),
        })
    });
    let stored = match put {
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

    if let Err(err) = PgBlobs(state.db.clone())
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
