//! A shared file's bytes (sharing S3): `GET /peer/v1/shares/{share}/blob/{blake3}`, with `Range: bytes=N-` to resume.
//!
//! The one module in this crate that reads source bytes, and it may name `SourceReader` and nothing wider
//! (`cargo xtask check-deploy`). Access is asked first, as on every share route; then reachability — the hash must be
//! the source file of a live part inside the share, by the catalogue's own walk. Stored bytes may be compressed, so a
//! resumed request cannot seek: the route reads and discards the first N decompressed bytes, and says how many in its
//! log, since that is what a resume costs this machine's disk.

use crate::PeerDevice;
use crate::shares::{asked_owner, failed, may_read, refused};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, DeviceId, PeerShareId, ShareId};
use lapidary_db::{Grant, PgMirror, PgPool, PgShares, PgSharing, Serving};
use lapidary_storage::SourceReader;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// At most this many files go to one installation at once.
pub const STREAMS_PER_DEVICE: usize = 2;
/// At most this many files go out at once, to everybody.
pub const STREAMS_IN_ALL: usize = 8;

/// The files being sent, a count per installation and in all. A request past either limit is told to try again: a pull
/// fetches one file at a time, so only somebody pulling in parallel, or many people at once, meets them.
#[derive(Clone, Default)]
pub struct Streams(Arc<Mutex<(HashMap<DeviceId, usize>, usize)>>);

/// One file being sent. Its count is given back when this is dropped, which is when the file's body has been sent or
/// the puller went away.
pub struct Stream {
    streams: Streams,
    device: DeviceId,
}

impl Streams {
    /// A stream for `device`, or `None` past a limit.
    pub fn take(&self, device: DeviceId) -> Option<Stream> {
        let mut counts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (per_device, in_all) = &mut *counts;
        let mine = per_device.entry(device).or_default();
        if *mine >= STREAMS_PER_DEVICE || *in_all >= STREAMS_IN_ALL {
            return None;
        }
        *mine += 1;
        *in_all += 1;
        Some(Stream {
            streams: self.clone(),
            device,
        })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let mut counts = self
            .streams
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (per_device, in_all) = &mut *counts;
        if let Some(mine) = per_device.get_mut(&self.device) {
            *mine = mine.saturating_sub(1);
            if *mine == 0 {
                per_device.remove(&self.device);
            }
        }
        *in_all = in_all.saturating_sub(1);
    }
}

/// Whose folder a file is being asked for, when it is not this installation's (S8).
#[derive(Debug, Deserialize)]
pub struct BlobQuery {
    owner: Option<String>,
}

/// Which file of which folder is being asked about (S9).
#[derive(Debug, Deserialize)]
pub struct HaveQuery {
    owner: Option<String>,
    blake3: String,
}

/// What the blob route reads: the database, for access and reachability, and the store the bytes are in.
#[derive(Clone)]
pub struct BlobState {
    pub db: PgPool,
    pub blob_root: PathBuf,
    pub streams: Streams,
}

/// The blob route, over the store the peer role mounts.
pub fn blob_router(db: PgPool, blob_root: PathBuf) -> axum::Router {
    blob_router_with(db, blob_root, Streams::default())
}

/// [`blob_router`], counting its streams in `streams`.
pub fn blob_router_with(db: PgPool, blob_root: PathBuf, streams: Streams) -> axum::Router {
    axum::Router::new()
        .route(
            "/peer/v1/shares/{share}/blob/{blake3}",
            axum::routing::get(blob),
        )
        .route("/peer/v1/shares/{share}/have", axum::routing::get(have))
        .with_state(BlobState {
            db,
            blob_root,
            streams,
        })
}

/// `GET /peer/v1/shares/{share}/have?owner=&blake3=` — whether this installation can serve that file of that
/// folder right now (S9).
///
/// Behind the very gates the blob route is behind, so it tells nobody anything the catalogue does not: a
/// folder they are not in, or one this installation does not serve, answers exactly as it does there. What it
/// saves is a fetch that would have been refused, which is what makes asking several holders cheap.
async fn have(
    State(state): State<BlobState>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
    Query(query): Query<HaveQuery>,
) -> Response {
    let Ok(owner) = asked_owner(query.owner.as_deref()) else {
        return not_in_share();
    };
    let Some(device) = device else {
        return not_in_share();
    };
    let held = match relayed(&state, device, owner, share).await {
        Ok(held) => held,
        Err(refusal) => return refusal,
    };
    if held.is_none() {
        if let Err(refusal) = may_read(&state.db, Some(device), share).await {
            return refusal;
        }
        match PgShares(state.db.clone()).grant(device, share).await {
            Ok(grant) if grant.allows_files() => {}
            Ok(Grant::NotShared) => return not_in_share(),
            // A folder that asks first and has not answered yet is not a folder to fetch from, and saying so
            // here saves the asker a request they would only be refused.
            Ok(_) => return Json(serde_json::json!({ "have": false })).into_response(),
            Err(err) => return failed(&err),
        }
    }
    let Ok(hash) = BlobHash::parse_hex(&query.blake3) else {
        return not_in_share();
    };
    let found = match held {
        Some(held) => PgMirror(state.db.clone()).held(held, &hash.to_hex()).await,
        None => PgShares(state.db.clone()).blob(share, &hash.to_hex()).await,
    };
    match found {
        Ok(location) => Json(serde_json::json!({ "have": location.is_some() })).into_response(),
        Err(err) => failed(&err),
    }
}

async fn blob(
    State(state): State<BlobState>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path((share, blake3)): Path<(ShareId, String)>,
    Query(query): Query<BlobQuery>,
    headers: HeaderMap,
) -> Response {
    let Ok(owner) = asked_owner(query.owner.as_deref()) else {
        return not_in_share();
    };
    let Some(device) = device else {
        return not_in_share();
    };
    // Whose folder this file belongs to decides which of two ways it may be served, and there is **no fallback
    // between them** (S8). A file of a folder held here but not listed by it would otherwise be served for
    // knowing its hash, which is exactly what content addressing must never mean.
    let held = match relayed(&state, device, owner, share).await {
        Ok(held) => held,
        Err(refusal) => return refusal,
    };
    if held.is_none() {
        if let Err(refusal) = may_read(&state.db, Some(device), share).await {
            return refusal;
        }
        // Browsing needs no grant; a file of a share that asks first does.
        match PgShares(state.db.clone()).grant(device, share).await {
            Ok(grant) if grant.allows_files() => {}
            Ok(Grant::Denied) => {
                return refused(
                    StatusCode::FORBIDDEN,
                    "denied",
                    "The owner of this share declined your request to pull it.",
                );
            }
            Ok(Grant::NotShared) => return not_in_share(),
            Ok(_) => {
                return refused(
                    StatusCode::FORBIDDEN,
                    "askFirst",
                    "The owner of this share asks to be asked before anyone pulls it. Ask, then wait for them to grant it.",
                );
            }
            Err(err) => return failed(&err),
        }
    }
    let Some(stream) = state.streams.take(device) else {
        return refused(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "This installation is sending as many files as it sends at once. Try again in a moment.",
        );
    };
    let Ok(hash) = BlobHash::parse_hex(&blake3) else {
        return not_in_share();
    };
    let found = match held {
        Some(held) => PgMirror(state.db.clone()).held(held, &hash.to_hex()).await,
        None => PgShares(state.db.clone()).blob(share, &hash.to_hex()).await,
    };
    let location = match found {
        Ok(Some(location)) => location,
        Ok(None) => return not_in_share(),
        Err(err) => return failed(&err),
    };
    let size = u64::try_from(location.size_bytes).unwrap_or_default();
    let start = match resume_from(&headers) {
        Some(start) if start == 0 || start < size => start,
        _ => return range_refused(size),
    };
    // What a resume costs this machine: the skipped bytes are still read and decompressed, then thrown away.
    tracing::info!(
        share = %share.as_uuid(),
        blake3 = %hash.to_hex(),
        skipped_bytes = start,
        sent_bytes = size - start,
        "serving a shared file"
    );
    let store = SourceReader::open(&state.blob_root);
    let opened = match location.storage_path.as_deref() {
        Some(rel) => store.stream_at(rel, location.zstd_level),
        None => store.stream(&hash, location.zstd_level),
    };
    let reader = match opened {
        Ok(reader) => reader,
        Err(err) => {
            tracing::error!(error = %err, blake3 = %hash.to_hex(), "a shared file could not be opened");
            return refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unreadable",
                "This installation could not read that file from its store. Its owner should check the store; try the pull again later.",
            );
        }
    };
    let body = stream_from(reader, start, stream);
    let length = (header::CONTENT_LENGTH, (size - start).to_string());
    let kind = (header::CONTENT_TYPE, "application/octet-stream".to_owned());
    if start == 0 {
        (StatusCode::OK, [kind, length], body).into_response()
    } else {
        let range = (
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{size}", size - 1),
        );
        (StatusCode::PARTIAL_CONTENT, [kind, length, range], body).into_response()
    }
}

/// The folder held here whose file this request is about, when it named an owner other than this installation
/// (S8). `Ok(None)` is a folder of this installation's own, which the ordinary path answers.
///
/// Whatever this answers is final. A folder held here that this installation does not serve is refused here
/// and never tried the other way, and neither is one whose roster does not name the caller.
async fn relayed(
    state: &BlobState,
    device: DeviceId,
    owner: Option<DeviceId>,
    share: ShareId,
) -> Result<Option<PeerShareId>, Response> {
    let Some(owner) = owner else {
        return Ok(None);
    };
    if PgSharing(state.db.clone())
        .identity()
        .await
        .map_err(|err| failed(&err))?
        .is_some_and(|identity| identity.device_id == owner)
    {
        return Ok(None);
    }
    match PgMirror(state.db.clone())
        .serves(owner, share, device)
        .await
    {
        Ok(Serving::Yes(held)) => Ok(Some(held)),
        Ok(Serving::NotShared) => Err(not_in_share()),
        Ok(Serving::NotSeeding) => Err(refused(
            StatusCode::FORBIDDEN,
            "notSeeding",
            "This installation holds that folder and has been set not to pass its files on. Ask its owner, or another of the people it goes to.",
        )),
        Err(err) => Err(failed(&err)),
    }
}

/// Where a request resumes: `Range: bytes=N-`, or nothing for the whole file. `None` for any range this route does not
/// serve — only the open-ended kind a staged file's length gives.
fn resume_from(headers: &HeaderMap) -> Option<u64> {
    let Some(value) = headers.get(header::RANGE) else {
        return Some(0);
    };
    value
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?
        .strip_suffix('-')?
        .parse()
        .ok()
}

/// The bytes after `skip`, from a blocking reader, streamed as `lapidary-api`'s download streams them: a bounded
/// channel, so a slow puller stops the reader rather than letting it race ahead into memory.
fn stream_from(mut reader: Box<dyn std::io::Read + Send>, skip: u64, stream: Stream) -> Body {
    use std::io::Read as _;
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        // Held until this task ends: the file is sent, or the puller went away and the channel closed.
        let _stream = stream;
        if skip > 0
            && let Err(error) = std::io::copy(&mut (&mut reader).take(skip), &mut std::io::sink())
        {
            let _ = tx.blocking_send(Err(error));
            return;
        }
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => {
                    if tx
                        .blocking_send(Ok(Bytes::copy_from_slice(&buffer[..read])))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let _ = tx.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx))
}

fn not_in_share() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "notInShare",
        "That file is not part of this share: its part may have been removed or moved out of the category. Reload what they share.",
    )
}

fn range_refused(size: u64) -> Response {
    (
        StatusCode::RANGE_NOT_SATISFIABLE,
        [(header::CONTENT_RANGE, format!("bytes */{size}"))],
    )
        .into_response()
}
