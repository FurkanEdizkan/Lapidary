//! A shared file's bytes (sharing S3): `GET /peer/v1/shares/{share}/blob/{blake3}`, with `Range: bytes=N-` to resume.
//!
//! The one module in this crate that reads source bytes, and it may name `SourceReader` and nothing wider
//! (`cargo xtask check-deploy`). Access is asked first, as on every share route; then reachability — the hash must be
//! the source file of a live part inside the share, by the catalogue's own walk. Stored bytes may be compressed, so a
//! resumed request cannot seek: the route reads and discards the first N decompressed bytes, and says how many in its
//! log, since that is what a resume costs this machine's disk.

use crate::PeerDevice;
use crate::shares::{failed, may_read, refused};
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, ShareId};
use lapidary_db::{PgPool, PgShares};
use lapidary_storage::SourceReader;
use std::path::PathBuf;

/// What the blob route reads: the database, for access and reachability, and the store the bytes are in.
#[derive(Clone)]
pub struct BlobState {
    pub db: PgPool,
    pub blob_root: PathBuf,
}

/// The blob route, over the store the peer role mounts.
pub fn blob_router(db: PgPool, blob_root: PathBuf) -> axum::Router {
    axum::Router::new()
        .route(
            "/peer/v1/shares/{share}/blob/{blake3}",
            axum::routing::get(blob),
        )
        .with_state(BlobState { db, blob_root })
}

async fn blob(
    State(state): State<BlobState>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path((share, blake3)): Path<(ShareId, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(refusal) = may_read(&state.db, device, share).await {
        return refusal;
    }
    let Ok(hash) = BlobHash::parse_hex(&blake3) else {
        return not_in_share();
    };
    let location = match PgShares(state.db.clone()).blob(share, &hash.to_hex()).await {
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
    let body = stream_from(reader, start);
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
fn stream_from(mut reader: Box<dyn std::io::Read + Send>, skip: u64) -> Body {
    use std::io::Read as _;
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
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
