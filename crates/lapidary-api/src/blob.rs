//! `GET /api/blob/{blake3}` — derivative bytes by hash. `api` role only.
//!
//! Serves derivatives, never source files. That is structural rather than a check here:
//! opening the source half of the blob store demands a `WorkerRole` proof this crate
//! cannot construct, and `cargo xtask check-deploy` fails if this crate so much as names
//! that type -- which is why the sentence you are reading does not.
//!
//! Knowing a hash is not permission to read it. `PgBlobs::derivative_is_reachable`
//! carries that rule and its reasoning; this module's job is to make both failure modes
//! -- unknown hash, and bytes on disk that nothing references -- indistinguishable from
//! outside.

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::BlobHash;
use lapidary_db::{DbError, PgBlobs, PgParts};
use lapidary_storage::DerivativeStore;

/// A year, the maximum any cache should hold something. Safe to the point of being
/// uninteresting: the URL contains the hash of the content, so the bytes at a given URL
/// cannot change. `immutable` additionally tells a browser not to revalidate on reload.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

pub async fn by_hash(State(state): State<AppState>, Path(hash): Path<String>) -> Response {
    // A hash that is not 64 hex characters cannot name anything, but it must not say so:
    // a distinct 400 here would tell a caller their guess was well-formed but absent.
    let Ok(hash) = BlobHash::parse_hex(&hash) else {
        return not_found();
    };

    // Moves `state.db` out of `state`, which leaves `state.blob_root` borrowable below
    // and saves cloning a pool twice. One handle serves both the reachability check and
    // the touch that follows it.
    let blobs = PgBlobs(state.db.clone());
    // Two ways for bytes to be reachable, and both have to be asked. A derivative is the
    // original one; a gallery image is the second, and without it an image somebody had
    // just uploaded would be on disk and served to nobody, including them.
    //
    // `CLAUDE.md`: content addressing is not authorization. Knowing a hash is not a reason
    // to be given the bytes — something in this instance has to point at them.
    let reachable = match blobs.derivative_is_reachable(&hash).await {
        Ok(true) => true,
        Ok(false) => match PgParts(state.db).image_is_reachable(&hash).await {
            Ok(reachable) => reachable,
            Err(err) => return internal_error(&err),
        },
        Err(err) => return internal_error(&err),
    };
    if !reachable {
        return not_found();
    }

    match DerivativeStore::open(&state.blob_root).get(&hash) {
        Ok(bytes) => {
            // After the bytes are in hand, never before: a 404 -- unreachable, or
            // referenced but absent from disk -- is not somebody reading this blob, and
            // recording it as one would let a caller move any timestamp by guessing a
            // hash. Awaited and discarded rather than spawned, because a task racing the
            // response is a timestamp nothing can assert.
            blobs.touch_blob(&hash).await;
            (
                [
                    (header::CACHE_CONTROL, IMMUTABLE.to_owned()),
                    // Quoted per RFC 9110. Strong, not weak: these are exact bytes.
                    (header::ETAG, format!("\"{}\"", hash.to_hex())),
                    (header::CONTENT_TYPE, content_type(&bytes).to_owned()),
                ],
                bytes,
            )
                .into_response()
        }
        // Referenced but missing. A derivative is evictable by design, so this is a
        // regeneration job rather than a corruption -- but nothing regenerates yet, and
        // an operator needs to see it, which is why it logs rather than 404ing quietly.
        Err(err) => {
            tracing::error!(
                hash = %hash.to_hex(),
                error = %err,
                "a referenced derivative is missing from the blob store"
            );
            (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({
                    "message": "That file is not available. It may have been evicted; \
                                re-open the part to have it regenerated."
                })),
            )
                .into_response()
        }
    }
}

/// One body for both "no such hash" and "exists but nothing references it". They must be
/// byte-for-byte identical: a caller who can tell them apart can use this endpoint to
/// confirm that particular bytes are stored, which is the capability the reachability
/// check exists to withhold.
fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({ "message": "No such file." })),
    )
        .into_response()
}

fn internal_error(err: &DbError) -> Response {
    tracing::error!(error = %err, "blob reachability query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}

/// What these bytes are, read from the bytes.
///
/// This route serves two kinds of blob now — tessellation rungs and gallery images — and it
/// cannot tell them apart from the hash, which is the point of a hash. It can tell from the
/// first twelve bytes, which is cheaper than a second database read and cannot disagree with
/// what is actually being sent.
///
/// A wrong type here is not cosmetic: a WebP labelled `model/gltf-binary` renders in an
/// `<img>` only because browsers sniff, and sniffing is a behaviour to be grateful for
/// rather than to depend on.
///
/// glTF is the fallback, not WebP: every blob this route served before images existed is a
/// rung, and an unrecognised blob is far more likely to be a new kind of geometry than a new
/// kind of picture.
fn content_type(bytes: &[u8]) -> &'static str {
    // RIFF container with a WEBP fourcc — the WebP header, which is what `images::normalize`
    // writes and the only image format this application stores.
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "model/gltf-binary"
    }
}
