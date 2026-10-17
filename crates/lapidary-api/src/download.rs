//! `GET /api/revisions/{id}/download?variant=original` — the ingested bytes, exactly as
//! they arrived. `api` role only, per `DATA.md` §5.1.
//!
//! **Since M4 the bytes are the owner's own file, read where it is** — in their folders, in the
//! inbox, or (for a part indexed before M4) in Lapidary's store — through `Roots`, which hands out
//! only `LibraryReader`, a handle with no way to write. `cargo xtask check-deploy` keeps the
//! writing handles out of this crate. That is not an exception to *the open path never touches a
//! source file*: opening — the grid, the viewer, the detail card — still reads metadata and
//! derivatives only. Handing a user the exact bytes they asked for, parsing nothing, is a
//! different path, and it is the only way `CLAUDE.md`'s other rule (*`variant=original` returns
//! byte-identical ingested bytes*) can be true at all.
//!
//! Byte-identical **as long as the file has not changed since it was indexed**. The owner may edit
//! it in place at any time, so before a byte goes out the file's size and modification time are
//! compared with the ones recorded at index, and a difference is refused with a sentence that says
//! to scan again (409). The end-of-stream hash stays, for an edit that kept both. A revision whose
//! file a later revision replaced answers 410: its figures are kept, its bytes are not.
//!
//! Four things this route refuses to do rather than do badly, and each answers with its
//! own message because each has a different person who can act on it: it will not guess a
//! `variant`, it will not serve bytes whose stored compression nobody recorded, it will
//! not serve bytes that do not hash to the digest we filed them under, and it will not
//! convert. The last one is not a limitation to lift here — converting reads the source
//! through the CAD kernel, which this crate cannot link (`xtask/src/layers.rs`), so a
//! converted download (`variant=3mf`, `variant=stl`) is a derivative the worker wrote,
//! asked for at `POST /api/parts/{id}/exports/{format}` and only served from here.
//!
//! # The body streams, and what that cost
//!
//! It used to buffer the whole file before writing a byte, because the reader
//! returned a `Vec<u8>` and `read_blob` decoded into it from a second full buffer. That was
//! not merely wasteful at Phase 1 sizes: the largest STL in the owner's corpus is 380 MB,
//! which decoded to 380 MB *plus* the ~185 MB compressed copy resident at the same time,
//! inside an `api` container `deploy/compose.yaml` caps at 512 MB. One download of one
//! real file was an OOM, and two concurrent ones would have been an OOM at any cap.
//!
//! So the body is now a stream, and one guarantee changed shape — deliberately, and it is
//! written here rather than discovered later. The route used to hash the whole file and
//! compare it to `blob.blake3` **before** sending anything, so a mismatch became a 500 and
//! the user received nothing. Streaming cannot do that: verifying before the first byte
//! *is* buffering. The bytes are still hashed, incrementally, as they go out — but the
//! answer arrives after the last chunk, so a mismatch can only abort the transfer.
//!
//! What the user gets on a mismatch is therefore a **short** response, not a wrong file:
//! `Content-Length` is sent from `blob.size_bytes`, so a client that reads fewer bytes than
//! promised has been told, and the strong `ETag` still carries the digest they can check
//! what they received against. The guarantee is no longer "we never begin sending bytes we
//! have not verified"; it is "we never finish a download whose bytes did not verify, and
//! we always say how many bytes a complete one has". That is a real weakening of a
//! promise `CLAUDE.md` cares about, bought for the ability to serve a file larger than the
//! container's memory at all.
//!
//! The hashing itself is still free: BLAKE3 at ~1 GB/s against a transfer is not the
//! bottleneck, and it remains the only thing standing between a user and a zstd frame
//! served as their file during slice 7's recompression window (spec §2.7).

use crate::AppState;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::rejection::{FormRejection, JsonRejection, QueryRejection};
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, DerivativeKind, FileRoot, LibraryId, PartId, RevisionId};
use lapidary_db::{DbError, DownloadSource, PgParts};
use lapidary_storage::{DerivativeStore, Roots, StorageError};
use lapidary_targets::{Format, Handover, Tool, bundle, negotiate};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use ts_rs::TS;

/// The ingested bytes, unconverted. Named in every message that rejects a variant, so a
/// caller is told what to send rather than what not to.
const ORIGINAL: &str = "original";

/// A cap on the synthesized filename, in bytes rather than characters and below the 255
/// every filesystem this runs on allows. `NAME_MAX` counts bytes: a Turkish `ğ` is two of
/// them, so a character cap is a byte cap only for ASCII names. The headroom is for the
/// ` (1)` a browser appends when that name is already in the download directory.
const MAX_FILENAME_BYTES: usize = 200;

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    /// Absent and unknown are different answers, so this is an `Option` the handler
    /// matches on rather than a `String` serde would reject before the handler could say
    /// anything useful.
    #[serde(default)]
    variant: Option<String>,
}

/// The order below is the whole design and it is not interchangeable.
///
/// The row resolves first: everything after it needs the hash, and a 404 for a revision
/// that names nothing must not depend on what the caller asked for. `variant` is checked
/// before any byte is read, because reading a file to then refuse the request is work
/// nobody asked for. The compression level is checked before the read rather than after,
/// because an unrecorded level makes the read itself meaningless. The re-hash is last
/// because it is the only check that needs the bytes. And the touch comes after all of
/// them: this timestamp means *these bytes were handed to somebody*, so a request that
/// refused to serve must not move it, or a caller could warm any blob by guessing.
pub async fn original(
    State(state): State<AppState>,
    Path(revision): Path<RevisionId>,
    query: Result<Query<DownloadQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Response {
    let roots = state.roots();
    let AppState {
        db,
        blob_root,
        touches,
        ..
    } = state;
    let source = match PgParts(db.clone()).source_for_download(revision).await {
        Ok(Some(source)) => source,
        Ok(None) => return no_such_revision(),
        Err(err) => return internal_error(&err),
    };

    // Not dead even though every field of `DownloadQuery` is optional, and not for the
    // reason first written here: a malformed percent-escape does *not* reach it, because
    // `form_urlencoded` decodes lossily and hands `%ZZ` through as those three literal
    // characters, which then land on `unknown_variant`. What does reject is a repeated key
    // — `?variant=original&variant=3mf`, the shape a URL edited by hand produces — and
    // that one must not be answered by silently picking whichever came first. Verified in
    // `tests/download.rs`, since a guess is what the previous comment was.
    let variant = match query {
        Ok(Query(DownloadQuery { variant })) => variant,
        Err(rejection) => return bad_query(&rejection),
    };
    // `?variant=` with nothing after the `=` is somebody who built the URL wrong, not
    // somebody naming an unknown variant, and `` `` is not a download variant `` is an
    // answer to a question nobody asked. `parts::empty_str_as_none` reads an empty
    // query-string value the same way, for the same reason.
    match variant.as_deref().filter(|variant| !variant.is_empty()) {
        Some(ORIGINAL) => {}
        Some(other) => match built_for(other, &source.format) {
            Ok(Some(built)) => {
                return serve_built(db, &blob_root, &touches, &source.part_name, revision, built)
                    .await;
            }
            // The part's own file is already in the format asked for: served below, as
            // `original` is.
            Ok(None) => {}
            Err(message) => return refused(message),
        },
        None => return missing_variant(),
    }
    serve_original(&roots, &touches, source, &headers)
}

/// `GET /api/parts/{id}/file` — the part's **current** file, exactly as it arrived. One link per
/// part that keeps working when a new revision is recorded: paste it into a slicer, a chat or a
/// script and it always gives the newest file. The same bytes, headers and checks as
/// `variant=original`, because it is that route with the revision looked up for you.
///
/// Access is what every route has today: whoever can reach the port. A deleted part, or one with
/// no file, answers 404 as its page does.
pub async fn current_file(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    headers: HeaderMap,
) -> Response {
    let roots = state.roots();
    let AppState { db, touches, .. } = state;
    let parts = PgParts(db);
    let revision = match parts.latest_revision(part).await {
        Ok(Some(revision)) => revision,
        Ok(None) => return no_such_part_file(),
        Err(err) => return internal_error(&err),
    };
    match parts.source_for_download(revision).await {
        Ok(Some(source)) => serve_original(&roots, &touches, source, &headers),
        Ok(None) => no_such_part_file(),
        Err(err) => internal_error(&err),
    }
}

fn no_such_part_file() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No part with that id has a file here: it may have been deleted, or the link \
                        may be mistyped. Open the part from the grid and copy its link again."
        })),
    )
        .into_response()
}

/// The ingested bytes of `source`, whole or the one range asked for.
fn serve_original(
    roots: &Roots,
    touches: &lapidary_db::Touches,
    source: DownloadSource,
    headers: &HeaderMap,
) -> Response {
    // A file a later revision replaced where it was: Lapidary keeps no copy (M4). Answered
    // from the row, before the disk is looked at, so an old revision is told it is gone rather
    // than that the newer file there "changed".
    if !source.on_disk {
        return no_longer_on_disk();
    }
    // Spec §2.5.1: a NULL level is a row nobody recorded how its bytes were written.
    let Some(zstd_level) = source.zstd_level else {
        return unrecorded_level(&source.hash, source.storage_path.as_deref());
    };
    // The pre-flight (M4): a file in the owner's folders or the inbox is checked against the
    // size and modification time it was indexed with before a byte is sent. The end-of-stream
    // hash below still catches an edit that kept both.
    if let (Some(mtime_ns), Some(rel)) = (source.mtime_ns, source.storage_path.as_deref()) {
        match roots.reader(source.root).stat_at(rel) {
            Ok(now)
                if now.mtime_ns == mtime_ns
                    && i64::try_from(now.size).ok() == Some(source.size_bytes) => {}
            Ok(_) => return changed_on_disk(source.root),
            Err(StorageError::Io { source: io, .. })
                if io.kind() == std::io::ErrorKind::NotFound =>
            {
                return missing_from_folder(source.root, rel);
            }
            Err(err) => {
                return unreadable(
                    &source.hash,
                    source.root,
                    source.storage_path.as_deref(),
                    &err,
                );
            }
        }
    }

    // Opening is where a missing or unreadable file is still caught synchronously, so that
    // case is a clean answer with a message rather than an empty 200.
    let opened = roots.open(
        source.root,
        source.storage_path.as_deref(),
        &source.hash,
        Some(zstd_level),
    );
    let reader = match opened {
        Ok(reader) => reader,
        Err(err) => {
            return unreadable(
                &source.hash,
                source.root,
                source.storage_path.as_deref(),
                &err,
            );
        }
    };
    let etag = format!("\"{}\"", source.hash.to_hex());
    let size = u64::try_from(source.size_bytes).unwrap_or(0);
    let range = match requested_range(headers, &etag, size) {
        Ok(range) => range,
        Err(()) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{size}"))],
                Json(serde_json::json!({
                    "message": format!(
                        "That range starts past the end of the file, which is {size} bytes. Ask for a range inside it, or for the whole file."
                    )
                })),
            )
                .into_response();
        }
    };

    // Recorded in memory and written by the next flush (`DATA.md` §1.4), as `blob.rs` records.
    // This is the *only* warm input a
    // source blob has — a library browsed constantly and never downloaded stays cold, and
    // spec §2.6 hands slice 7 that decision rather than letting it inherit it silently.
    touches.record(&source.hash);

    let filename = download_filename(&source.part_name, &source.format);
    if let Some((start, end)) = range {
        // A range is a resumed or split download. Its bytes cannot be checked against the digest,
        // which covers the whole file — so this is the one answer that skips the outgoing hash,
        // and the strong `ETag` is how the client checks the file it has put back together.
        let len = end - start + 1;
        return (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                (header::CACHE_CONTROL, "no-cache".to_owned()),
                (header::ETAG, etag),
                (header::CONTENT_DISPOSITION, content_disposition(&filename)),
                (header::ACCEPT_RANGES, "bytes".to_owned()),
                (header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}")),
                (header::CONTENT_LENGTH, len.to_string()),
            ],
            stream_range(reader, start, len),
        )
            .into_response();
    }
    let body = stream_verified(reader, source.hash, source.root);
    (
        [
            // Always, and never `model/stl`: a browser that renders the file inline is a
            // download that did not download.
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            // `no-cache` is "revalidate before reuse", not "do not store". The blob route
            // can promise `immutable` because its URL contains the hash of what it
            // returns; this URL names a *revision*, whose source could be re-pointed, so
            // the same promise would be one this route cannot keep. Sending nothing at
            // all is worse than either: with no directive a browser falls back to
            // heuristic freshness and may hand back a stale file it never asked us about.
            // Nothing serves 304s yet — a revalidation costs a full transfer today — but
            // the strong `ETag` below is what a conditional handler would need, and
            // correctness before the round trip is the right order.
            (header::CACHE_CONTROL, "no-cache".to_owned()),
            // Quoted per RFC 9110, strong because these are exact bytes — and the same
            // digest `DATA.md` §5.1 asks the UI to show beside the button, so a user can
            // check what they got against what they were promised.
            (header::ETAG, etag),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
            (header::ACCEPT_RANGES, "bytes".to_owned()),
            // From `blob.size_bytes`, the uncompressed length. Without it the response is
            // chunked and a client cannot tell a complete download from one this route
            // abandoned mid-body on a hash mismatch — which is the whole reason the
            // mismatch is survivable as a failure rather than as silence.
            (header::CONTENT_LENGTH, source.size_bytes.to_string()),
        ],
        body,
    )
        .into_response()
}

/// The one byte range a request asks for, inclusive, or `None` to send the whole file.
///
/// Only a single `bytes=` range is honoured: `start-end`, `start-` or `-suffix`. Anything else —
/// several ranges, another unit, a header that does not parse — is ignored and the whole file is
/// sent, which RFC 9110 §14.2 allows. `If-Range` that names anything but this file's `ETag` also
/// sends the whole file: the file at `/parts/{id}/file` changes when a revision is recorded, and
/// resuming a download of the old one onto the new one would splice two files together.
/// `Err` is a range that starts past the end, which is 416.
fn requested_range(headers: &HeaderMap, etag: &str, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(range) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };
    if let Some(if_range) = headers.get(header::IF_RANGE)
        && if_range.to_str().ok() != Some(etag)
    {
        return Ok(None);
    }
    let Some(spec) = range.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return Ok(None);
    };
    let (first, last) = (first.trim(), last.trim());
    let parsed = match (first.parse::<u64>(), last.parse::<u64>()) {
        (Ok(start), Ok(end)) if start <= end => (start, end.min(size.saturating_sub(1))),
        (Ok(start), Err(_)) if last.is_empty() => (start, size.saturating_sub(1)),
        (Err(_), Ok(suffix)) if first.is_empty() && suffix > 0 => {
            (size.saturating_sub(suffix), size.saturating_sub(1))
        }
        _ => return Ok(None),
    };
    if size == 0 || parsed.0 >= size {
        return Err(());
    }
    Ok(Some(parsed))
}

/// `len` bytes from `start`, streamed the way `stream_verified` streams the whole file. The
/// stored bytes may be a zstd frame, so the start is reached by reading through, not by seeking.
fn stream_range(mut reader: Box<dyn std::io::Read + Send>, start: u64, len: u64) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        let mut skipped = std::io::Read::take(&mut reader, start);
        if let Err(source) = std::io::copy(&mut skipped, &mut std::io::sink()) {
            let _ = tx.blocking_send(Err(source));
            return;
        }
        let mut wanted = std::io::Read::take(reader, len);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match std::io::Read::read(&mut wanted, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .blocking_send(Ok(Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(source) => {
                    let _ = tx.blocking_send(Err(source));
                    return;
                }
            }
        }
    });
    Body::from_stream(ReceiverStream::new(rx))
}

/// The blob's bytes as a response body, hashed on the way out.
///
/// A blocking reader on a blocking thread, feeding a bounded channel the response drains:
/// `zstd`'s decoder is `Read`, and an async decompressor would be a new dependency bought
/// to avoid an ordinary thread. The bound is what makes this stream rather than buffer —
/// a slow client stops the reader instead of letting it race ahead into memory.
///
/// The final hash is compared to the digest the blob is filed under, and a mismatch closes
/// the body early. See this module's header for what that does and does not promise; the
/// short read is deliberate, and it is why `Content-Length` above is not optional.
fn stream_verified(
    mut reader: Box<dyn std::io::Read + Send>,
    expected: BlobHash,
    root: FileRoot,
) -> Body {
    // Four 64 KiB chunks in flight. Small enough that a stalled client costs a quarter of
    // a megabyte rather than a file, large enough that the channel is not the bottleneck.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);

    tokio::task::spawn_blocking(move || {
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    hasher.update(&buf[..n]);
                    // A closed receiver means the client went away. Stop reading rather
                    // than decompressing the rest of a 380 MB file into a channel nobody
                    // is draining.
                    if tx
                        .blocking_send(Ok(Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(source) => {
                    let _ = tx.blocking_send(Err(source));
                    return;
                }
            }
        }

        let served = BlobHash::from_bytes(*hasher.finalize().as_bytes());
        if served != expected {
            // The bytes have already gone out, so all this can do is refuse to end the
            // body cleanly and leave the client short of `Content-Length`. The operator's
            // half of the old 500 survives as a log line, which is where it was always the
            // more useful of the two: nothing is wrong with the row here, and the remedy is
            // the blob rather than the record.
            let expected_hex = expected.to_hex();
            let served_hex = served.to_hex();
            tracing::error!(
                expected = %expected_hex,
                served = %served_hex,
                "stored bytes do not hash to the digest they are filed under; download truncated"
            );
            let message = match root {
                // A file edited in place in a way that kept its size and time.
                FileRoot::Library | FileRoot::Inbox => format!(
                    "{} (It hashes to {served_hex}, not {expected_hex}; the download was stopped rather than completed.)",
                    changed(root)
                ),
                FileRoot::Store => format!(
                    "The bytes stored for blob {expected_hex} hash to {served_hex}, so they are not the file that was ingested. The download was stopped rather than completed. Restore that blob from a backup, or re-ingest the source file to replace it."
                ),
            };
            let _ = tx.blocking_send(Err(std::io::Error::other(message)));
        }
    });

    Body::from_stream(ReceiverStream::new(rx))
}

/// Both halves, per RFC 6266 and `DATA.md` §5.1. The ASCII `filename=` comes first
/// because a client that understands only that one must not have to skip past a parameter
/// it cannot parse; every browser in use prefers `filename*` and never reads the fallback.
///
/// One percent-encoding pass over the finished `{stem}.{ext}` rather than one per half:
/// `.` is an `attr-char` and survives encoding, so splitting the string first would be a
/// branch that changes nothing.
fn content_disposition(filename: &str) -> String {
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_fallback(filename),
        percent_encode(filename)
    )
}

/// `{part.name}.{format}`, sanitized and capped. Spec §2.4: no new column, and a renamed
/// part downloads under its new name — the byte-identity claim is about bytes, and a file
/// named after a name the user changed last month would be the surprising behaviour.
fn download_filename(part_name: &str, format: &str) -> String {
    let ext = sanitize(format);
    let stem = sanitize(part_name);
    // A name that sanitizes away entirely. Nothing writes one today — `part.name` is an
    // ingested file's stem — but `.stl` is a hidden file on Linux and an empty save
    // dialog everywhere else, so the case gets an answer instead of a surprise.
    let stem = if stem.trim().is_empty() {
        "download"
    } else {
        stem.as_str()
    };

    let budget = MAX_FILENAME_BYTES.saturating_sub(ext.len() + 1);
    let mut capped = String::new();
    for c in stem.chars() {
        // On a character boundary, not a byte one: truncating mid-`ğ` would produce a
        // header that is not UTF-8 at all.
        if capped.len() + c.len_utf8() > budget {
            break;
        }
        capped.push(c);
    }
    format!("{capped}.{ext}")
}

/// What a download name must not carry, whatever a part is called. `/` and `\` would let
/// a name reach outside the browser's download directory; `"` would close the
/// quoted-string the ASCII half of the header is; a control character would end the
/// header line itself. Dropped rather than substituted — these carry no meaning worth
/// preserving as a placeholder, unlike the letters `ascii_fallback` cannot spell.
fn sanitize(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '"'))
        .collect()
}

/// The half a client predating RFC 5987 reads. Non-ASCII becomes `_` rather than being
/// dropped: `Şaft` reduced to `aft` is a different plausible-looking word, where `_aft`
/// reads as what it is — a name this half cannot spell. The `filename*` parameter beside
/// it carries the real one.
fn ascii_fallback(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect()
}

/// RFC 5987 `ext-value`: every byte outside `attr-char`, over the UTF-8 encoding rather
/// than over characters. `attr-char` is `token` minus `*`, `'` and `%`, which is why `.`,
/// `-` and `_` survive while a space, a comma and every Turkish letter do not. Uppercase
/// hex per RFC 3986 §2.1.
fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A file Lapidary built from a part, as this route serves it.
struct Built {
    kind: DerivativeKind,
    /// How a message names it: `3mf export`.
    what: String,
    /// What follows `<part>.lapidary.` in its filename.
    extension: String,
    content_type: &'static str,
    /// The route that builds it, after `/api/parts/{id}/`.
    route: String,
}

/// What `variant` names for a part whose own file is in `source_format`: a mesh export, or
/// `None` for the part's own file when that is already the format asked for.
///
/// A format is a download of that one format, negotiated as any hand-over is (`DATA.md`
/// §5.1): never a mesh for a B-rep format, and never an export of a file already in it. A
/// refusal is the message of a 400.
fn built_for(variant: &str, source_format: &str) -> Result<Option<Built>, String> {
    // `l0`, `l1` and `l2` used to be answered here, from the rung ladder. They fall through to
    // `unknown_variant` now, and that is deliberate (R3): the links are in people's history and
    // in the e2e rig, nothing builds a rung, and a stale bookmark deserves a sentence rather
    // than a 404 naming a route that no longer exists. The sentence below no longer offers them.
    let Some(format) = Format::named(variant) else {
        return Err(unknown_variant(variant));
    };
    // Every extension ingest records is a named format, so this is a row written some other way.
    let Some(source) = Format::named(source_format) else {
        return Err(format!(
            "This part's file is recorded as `{source_format}`, a format this server cannot \
             negotiate a download for. Download it with `?variant={ORIGINAL}` instead."
        ));
    };
    let download = Tool {
        name: format!("a `{}` download", format.name()),
        accepts: vec![format],
    };
    match negotiate(&download, source) {
        Ok(Handover::Original) => Ok(None),
        Ok(Handover::Export(format)) => match format.export() {
            Some(kind) => Ok(Some(Built {
                kind,
                what: format!("{} export", format.name()),
                extension: format.name().to_owned(),
                content_type: "application/octet-stream",
                route: format!("exports/{}", format.name()),
            })),
            // `negotiate` hands over only what `EXPORTS` lists, and every one of those is written.
            None => Err(unknown_variant(variant)),
        },
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// One file Lapidary built, streamed from the derivative store under a name that says so.
/// Never the source file under another name: the part's own file is served as `original` is.
async fn serve_built(
    db: lapidary_db::PgPool,
    blob_root: &std::path::Path,
    touches: &lapidary_db::Touches,
    part_name: &str,
    revision: RevisionId,
    built: Built,
) -> Response {
    let Built {
        kind,
        what,
        extension,
        content_type,
        route,
    } = built;
    let hash = match PgParts(db.clone()).derivative_hash(revision, kind).await {
        Ok(Some(hash)) => hash,
        Ok(None) => return not_built(&what, &route),
        Err(err) => return internal_error(&err),
    };
    let bytes = match DerivativeStore::open(blob_root).get(&hash) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(error = %err, hash = %hash.to_hex(), "a recorded derivative is not readable");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "message": format!(
                        "The {what} is recorded but its bytes could not be read. \
                         Ask for it again with POST /api/parts/{{id}}/{route} to rebuild it."
                    )
                })),
            )
                .into_response();
        }
    };
    touches.record(&hash);
    let filename = download_filename(part_name, &format!("lapidary.{extension}"));
    (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (header::CACHE_CONTROL, "no-cache".to_owned()),
            (header::ETAG, format!("\"{}\"", hash.to_hex())),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
            (header::CONTENT_LENGTH, bytes.len().to_string()),
        ],
        bytes,
    )
        .into_response()
}

fn not_built(what: &str, route: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": format!(
                "This revision has no {what} yet. Ask for it with \
                 POST /api/parts/{{id}}/{route}, then download it once that batch finishes."
            )
        })),
    )
        .into_response()
}

fn refused(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}

/// No such revision, or its part was deleted. A URL held from before the delete stops
/// serving bytes rather than outliving it (spec §2.1).
fn no_such_revision() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No revision with that id has a file to download, or its part has \
                        been deleted. Reload the grid and try the part again."
        })),
    )
        .into_response()
}

/// Spec §2.2: never a silent fallback. A download that quietly returns something other
/// than what was asked for is the failure `DATA.md` §5.1 exists to forbid, and "no
/// variant" is a URL somebody built by hand, so the answer is the URL that works.
fn missing_variant() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "This download needs a variant, and defaulting to one would risk handing \
                 you a file you did not ask for. Add `?variant={ORIGINAL}` to the URL to \
                 get the bytes exactly as they were ingested."
            )
        })),
    )
        .into_response()
}

/// Deliberately not [`missing_variant`]'s body. Someone who sent `variant=dxf` asked a
/// real question — which downloads there are — and telling them to add a parameter they
/// already sent answers a question they did not ask.
fn unknown_variant(got: &str) -> String {
    // **It must not name a rung.** `l0`, `l1` and `l2` were variants until R3, and this is the
    // sentence they now land on: offering them back would answer "`l0` is not a download
    // variant — this server serves … `l0`", which is worse than the 404 it replaced.
    format!(
        "`{got}` is not a download variant. This server serves `variant={ORIGINAL}`, the \
         ingested bytes unconverted, and a format — `3mf` or `stl` is a mesh Lapidary writes \
         from any part, and the part's own format is its original file. Everything Lapidary \
         built is named `*.lapidary.*`."
    )
}

/// Spec §2.5.1, and the one refusal that still happens *before* any bytes go out, which
/// is why it can still be a 500 with a body: it is answered from the row, not from the
/// file. Its message says an operator has a `file` row whose compression nobody recorded,
/// which is something they can go and look at — `storage_path` names where, or says the
/// bytes are still content-addressed. The hash mismatch — the other thing that can be
/// wrong with a source file — moved into `stream_verified` when the body began streaming,
/// because by the time it is known the status line has been sent.
fn unrecorded_level(hash: &BlobHash, storage_path: Option<&str>) -> Response {
    let hex = hash.to_hex();
    tracing::error!(
        hash = %hex,
        storage_path = storage_path.unwrap_or("(content-addressed)"),
        "a source blob has no recorded compression level"
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "message": format!(
                "Blob {hex} has no recorded compression level, so there is no way to know \
                 whether reading it would produce the file that was ingested. Nothing was \
                 served. Ingest records one for every source blob it writes, and leaves the \
                 row alone for bytes it already held — check this row against the file on \
                 disk before serving it."
            )
        })),
    )
        .into_response()
}

/// Referenced and not readable. A 500 rather than the 404 `blob.rs` answers for the same
/// shape, because the two shapes are not the same: a derivative is evictable by design
/// and regenerable, where a source blob is never removed while a part references it, so
/// its absence is a broken deployment or a lost volume and nothing regenerates it.
///
/// The store's own error names a filesystem path, which is an operator's business and not
/// a caller's — it goes to the log (with `storage_path`, when there is one), and the
/// response says what to check. `storage_path` also decides *what* it says: "the file for
/// that hash" is wrong advice for a part whose bytes were never written there at all, and
/// sending an operator to look at `blobs/ab/cd/<hash>` for a part that lives at
/// `libraries/…` wastes the one thing this message exists to save them.
fn unreadable(
    hash: &BlobHash,
    root: FileRoot,
    storage_path: Option<&str>,
    err: &StorageError,
) -> Response {
    let hex = hash.to_hex();
    tracing::error!(
        hash = %hex,
        root = root.as_str(),
        storage_path = storage_path.unwrap_or("(content-addressed)"),
        error = %err,
        "a referenced source file could not be read"
    );
    let message = match (root, storage_path) {
        (FileRoot::Library, Some(rel)) => format!(
            "Could not read {rel} in your folders, so nothing was served. Check that the folder is mounted and readable, then try again."
        ),
        (FileRoot::Inbox, Some(rel)) => format!(
            "Could not read {rel} in Lapidary's inbox, so nothing was served. Check that the inbox volume is mounted and readable, then try again."
        ),
        (_, Some(_)) => format!(
            "Blob {hex} could not be read from the blob store, so nothing was served. Check that the blob volume is mounted and that this part's file is still present in its model directory."
        ),
        (_, None) => format!(
            "Blob {hex} could not be read from the blob store, so nothing was served. Check that the blob volume is mounted and that the file for that hash is present."
        ),
    };
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}

/// A revision whose file a later revision replaced where it was (M4). `410 Gone`: the row
/// and its figures are still here, and the bytes are not anywhere Lapidary can reach.
fn no_longer_on_disk() -> Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "reason": "noLongerOnDisk",
            "message": "This revision's file is no longer on disk: a later revision replaced it in your folder, and Lapidary keeps no copy. Its figures are still in the history; download the current revision instead."
        })),
    )
        .into_response()
}

/// The pre-flight's refusal (M4): the file's size or modification time is not what was indexed.
fn changed_on_disk(root: FileRoot) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "reason": "changedOnDisk",
            "message": changed(root),
        })),
    )
        .into_response()
}

/// What to do about a changed file depends on where it is: a scan reads the owner's folders, and
/// never the inbox, so a changed upload is uploaded again.
fn changed(root: FileRoot) -> &'static str {
    match root {
        FileRoot::Inbox => {
            "This file changed in Lapidary's inbox since it was indexed — upload or import it again."
        }
        FileRoot::Library | FileRoot::Store => CHANGED_ON_DISK,
    }
}

/// The sentence a changed file is refused with, before a byte is sent.
pub const CHANGED_ON_DISK: &str =
    "This file changed in your folder since Lapidary last read it — scan again.";

/// The pre-flight found nothing at the recorded path: moved, renamed or deleted in the folder.
fn missing_from_folder(root: FileRoot, rel: &str) -> Response {
    let (place, remedy) = match root {
        FileRoot::Inbox => ("Lapidary's inbox", "upload or import it again"),
        _ => ("your folders", "put it back, or scan again"),
    };
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "reason": "missingFromFolder",
            "message": format!(
                "There is no file at {rel} in {place} any more: it was moved, renamed or deleted there since Lapidary last read it. To download it, {remedy}."
            ),
        })),
    )
        .into_response()
}

/// The query string did not decode. Wrapped the way `parts::bad_query` wraps its own, so
/// the response says what a client can send instead of handing back axum's bare line.
fn bad_query(rejection: &QueryRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "Could not read the query string: {rejection}. Send exactly \
                 `?variant={ORIGINAL}`."
            )
        })),
    )
        .into_response()
}

/// Same asymmetry the other handlers keep: the operator gets the real error through the
/// log, the client gets whatever `client_message` decides is safe to hand back.
fn internal_error(err: &DbError) -> Response {
    crate::error::log_db_error(err, "download source lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------------------
// Bundles (Phase 4 slice 2 spec §6): the selected parts, every revision, one ZIP.
// ---------------------------------------------------------------------------------------

/// At most this many parts in one bundle: a selection's worth, and an export that finishes.
const MAX_BUNDLE_PARTS: usize = 500;

/// `POST /api/libraries/{id}/bundle`'s form: the selected parts' ids, comma-separated. A form
/// rather than JSON, so the browser posts it itself and saves the stream under the name the
/// response gives, instead of a page holding the whole archive in memory.
#[derive(Debug, Deserialize)]
pub struct BundleForm {
    parts: String,
}

/// `POST /api/libraries/{id}/bundle/plan`'s body.
#[derive(Debug, Deserialize)]
pub struct BundlePlanRequest {
    parts: Vec<PartId>,
}

/// What a bundle of the selected parts holds, said before the download starts.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BundlePlan {
    pub parts: u32,
    pub revisions: u32,
    /// The archive's exact length: the download's `Content-Length`.
    #[ts(type = "number")]
    pub bytes: u64,
}

struct BundleEntry {
    path: String,
    size: u64,
    source: DownloadSource,
}

struct PlannedBundle {
    library_name: String,
    entries: Vec<BundleEntry>,
    manifest: Vec<u8>,
    parts: usize,
    bytes: u64,
}

/// `POST /api/libraries/{id}/bundle/plan` — every check the download makes, answered before the
/// browser commits to a download, so a refusal is a message on the grid rather than a page of JSON
/// the browser navigates to.
pub async fn bundle_plan(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    body: Result<Json<BundlePlanRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return bundle_refusal(
            StatusCode::BAD_REQUEST,
            "Send the parts to bundle as {\"parts\": [part ids]}, then plan again.",
        );
    };
    match plan_bundle(&state, library, &request.parts).await {
        Ok(planned) => Json(BundlePlan {
            parts: u32::try_from(planned.parts).unwrap_or(u32::MAX),
            revisions: u32::try_from(planned.entries.len()).unwrap_or(u32::MAX),
            bytes: planned.bytes,
        })
        .into_response(),
        Err(refused) => *refused,
    }
}

/// `POST /api/libraries/{id}/bundle` — the selected parts as one ZIP: every revision's original
/// bytes and a `manifest.json`, streamed. `lapidary-targets`' `bundle` says why the ZIP is
/// written by hand.
pub async fn bundle(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    form: Result<Form<BundleForm>, FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return bundle_refusal(
            StatusCode::BAD_REQUEST,
            "Send the parts to bundle as the form field `parts`: part ids separated by commas.",
        );
    };
    let mut parts = Vec::new();
    for id in form
        .parts
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        match id.parse::<PartId>() {
            Ok(part) => parts.push(part),
            Err(_) => {
                return bundle_refusal(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "{id} is not a part id. Select the parts in the grid, then export again."
                    ),
                );
            }
        }
    }
    let planned = match plan_bundle(&state, library, &parts).await {
        Ok(planned) => planned,
        Err(refused) => return *refused,
    };
    // These bytes are about to be handed to somebody, which is what `last_accessed_at` means.
    for entry in &planned.entries {
        state.touches.record(&entry.source.hash);
    }

    let filename = download_filename(
        &format!(
            "{}-bundle.lapidary",
            lapidary_core::slug::slugify(&planned.library_name)
        ),
        "zip",
    );
    let length = planned.bytes;
    let body = stream_bundle(state.roots(), planned.entries, planned.manifest);
    (
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
            // Exact, from `archive_len`: a bundle that stops short of it did not finish.
            (header::CONTENT_LENGTH, length.to_string()),
        ],
        body,
    )
        .into_response()
}

/// Everything a bundle needs, or the refusal saying why there is none: parts of this library and
/// not removed, each revision's source file, no two files at one path, and under 4 GiB. The whole
/// selection is read in one query, [`PgParts::bundle_parts`].
async fn plan_bundle(
    state: &AppState,
    library: LibraryId,
    requested: &[PartId],
) -> Result<PlannedBundle, Box<Response>> {
    let refuse = |status: StatusCode, message: String| Box::new(bundle_refusal(status, message));
    let failed = |err: DbError| Box::new(internal_error(&err));
    if requested.is_empty() {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "Select at least one part to export.".to_owned(),
        ));
    }
    // Before de-duplicating, which is quadratic: a body of fifty thousand ids is refused, not
    // searched.
    if requested.len() > MAX_BUNDLE_PARTS {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            format!(
                "A bundle holds at most {MAX_BUNDLE_PARTS} parts, and {} were selected. Export them in several bundles.",
                requested.len()
            ),
        ));
    }
    let mut parts: Vec<PartId> = Vec::with_capacity(requested.len());
    for part in requested {
        if !parts.contains(part) {
            parts.push(*part);
        }
    }

    let repo = PgParts(state.db.clone());
    let Some(owner) = repo
        .libraries()
        .await
        .map_err(failed)?
        .into_iter()
        .find(|row| row.id == library)
    else {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "No such library. Reload the page, then export again.".to_owned(),
        ));
    };

    let found = repo.bundle_parts(library, &parts).await.map_err(failed)?;
    if found.len() != parts.len() {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "One of the selected parts is not in this library, or was removed since the grid was loaded. Reload the grid, select again, then export.".to_owned(),
        ));
    }
    let mut paths = std::collections::HashSet::new();
    let mut entries = Vec::new();
    let mut manifest_parts = Vec::with_capacity(found.len());
    for mut part in found {
        // A revision whose file a later one replaced in place has no bytes left to carry (M4): the
        // bundle holds the revisions still on disk, with their own labels and figures.
        part.revisions
            .retain(|row| row.source.as_ref().is_none_or(|source| source.on_disk));
        let labels: Vec<(RevisionId, String)> = part
            .revisions
            .iter()
            .map(|row| (row.id, row.rev_label.clone()))
            .collect();
        let last = part.revisions.len().saturating_sub(1);

        let mut manifest_revisions = Vec::with_capacity(part.revisions.len());
        for (index, row) in part.revisions.into_iter().enumerate() {
            let Some(source) = row.source else {
                return Err(refuse(
                    StatusCode::CONFLICT,
                    format!(
                        "Revision {} of {} has no source file to put in a bundle. Re-scan its library to attach one, then export again.",
                        row.rev_label, part.name
                    ),
                ));
            };
            if let (Some(mtime_ns), Some(rel)) = (source.mtime_ns, source.storage_path.as_deref()) {
                let unchanged = state
                    .roots()
                    .reader(source.root)
                    .stat_at(rel)
                    .is_ok_and(|now| {
                        now.mtime_ns == mtime_ns
                            && i64::try_from(now.size).ok() == Some(source.size_bytes)
                    });
                if !unchanged {
                    return Err(refuse(
                        StatusCode::CONFLICT,
                        format!("{} ({}): {}", part.name, rel, changed(source.root)),
                    ));
                }
            }
            if source.zstd_level.is_none() {
                return Err(refuse(
                    StatusCode::CONFLICT,
                    format!(
                        "Nobody recorded how revision {} of {} was stored, so its bytes cannot be read back. Re-scan its library, then export again.",
                        row.rev_label, part.name
                    ),
                ));
            }
            let size = u64::try_from(source.size_bytes).map_err(|_| {
                failed(DbError::NegativeByteCount {
                    column: "file.size_bytes",
                    value: source.size_bytes,
                })
            })?;
            let path = bundle::entry_path(&part.source_path, &row.rev_label, index == last);
            if path == bundle::MANIFEST || !paths.insert(path.clone()) {
                return Err(refuse(
                    StatusCode::CONFLICT,
                    format!(
                        "Two files would sit at {path} in this bundle. Export those parts in separate bundles."
                    ),
                ));
            }
            manifest_revisions.push(bundle::ManifestRevision {
                parent_label: labels
                    .iter()
                    .find(|(id, _)| Some(*id) == row.parent)
                    .map(|(_, label)| label.clone()),
                rev_label: row.rev_label,
                origin: row.origin.as_str().to_owned(),
                created_at: row.created_at.to_string(),
                blake3: source.hash.to_hex(),
                size_bytes: size,
                format: source.format.clone(),
                path: path.clone(),
            });
            entries.push(BundleEntry { path, size, source });
        }
        manifest_parts.push(bundle::ManifestPart {
            name: part.name,
            part_number: part.part_number,
            source_path: part.source_path,
            tags: part.tags,
            sources: part
                .sources
                .into_iter()
                .map(|source| bundle::ManifestSource {
                    url: source.url,
                    vendor: source.vendor,
                    external_id: source.external_id,
                    title: source.title,
                    license: source.license,
                })
                .collect(),
            revisions: manifest_revisions,
        });
    }

    let manifest = bundle::Manifest {
        format: bundle::FORMAT.to_owned(),
        version: bundle::VERSION,
        library: bundle::ManifestLibrary {
            name: owner.name.clone(),
        },
        parts: manifest_parts,
    };
    let manifest = match serde_json::to_vec_pretty(&manifest) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(error = %err, "a bundle manifest did not serialise");
            return Err(refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not write this bundle's manifest. Try again, and if it keeps failing, check the api log.".to_owned(),
            ));
        }
    };
    // An import's limits, not the ZIP writer's: a bundle that plans and downloads must also be
    // one Lapidary can import again.
    if entries.len() + 1 > bundle::MAX_IMPORT_ENTRIES {
        return Err(refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "These parts hold {} files, and a bundle holds at most {} so that it can be imported. Export them in several bundles.",
                entries.len(),
                bundle::MAX_IMPORT_ENTRIES - 1
            ),
        ));
    }
    let bytes = bundle::archive_len(
        entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry.size))
            .chain(std::iter::once((bundle::MANIFEST, manifest.len() as u64))),
    );
    if bytes > bundle::MAX_IMPORT_BYTES {
        return Err(refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "These parts come to {bytes} bytes as a bundle, and a bundle stops at 2 GiB, the most an import reads. Export them in several smaller bundles."
            ),
        ));
    }
    Ok(PlannedBundle {
        library_name: owner.name,
        entries,
        manifest,
        parts: parts.len(),
        bytes,
    })
}

/// The bundle as a response body. One blocking thread reads each file, hashes it on the way into
/// the ZIP, and hands the archive to the response through a bounded channel, as `stream_verified`
/// does for one file. A file that will not open, will not read, or no longer hashes to its digest
/// ends the body short of its `Content-Length`, rather than finishing a bundle with a wrong file.
fn stream_bundle(roots: Roots, entries: Vec<BundleEntry>, manifest: Vec<u8>) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        let stop = tx.clone();
        let fail = |message: String| {
            tracing::error!(%message, "a bundle stopped short");
            let _ = stop.blocking_send(Err(std::io::Error::other(message)));
        };
        let mut zip =
            bundle::StoreZip::new(std::io::BufWriter::with_capacity(64 * 1024, Channel(tx)));
        for entry in entries {
            let opened = roots.open(
                entry.source.root,
                entry.source.storage_path.as_deref(),
                &entry.source.hash,
                entry.source.zstd_level,
            );
            let reader = match opened {
                Ok(reader) => reader,
                Err(err) => {
                    return fail(format!(
                        "Could not open {} for the bundle: {err}. Re-scan the part's library, then export again.",
                        entry.path
                    ));
                }
            };
            let mut hashed = Hashed {
                inner: reader,
                hasher: blake3::Hasher::new(),
            };
            if let Err(err) = zip.add(&entry.path, &mut hashed) {
                return fail(format!(
                    "Could not put {} in the bundle: {err}.",
                    entry.path
                ));
            }
            let served = BlobHash::from_bytes(*hashed.hasher.finalize().as_bytes());
            if served != entry.source.hash {
                return fail(format!(
                    "The bytes stored for {} hash to {}, not {}, so they are not the file that was ingested. The bundle was stopped rather than finished. Restore that file from a backup, or re-ingest it.",
                    entry.path,
                    served.to_hex(),
                    entry.source.hash.to_hex()
                ));
            }
        }
        if let Err(err) = zip.add(bundle::MANIFEST, &mut manifest.as_slice()) {
            return fail(format!("Could not put the manifest in the bundle: {err}."));
        }
        if let Err(err) = zip.finish() {
            fail(format!("Could not finish the bundle: {err}."));
        }
    });
    Body::from_stream(ReceiverStream::new(rx))
}

/// A `Write` over the response's channel. A closed channel is a client that stopped reading.
struct Channel(tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>);

impl std::io::Write for Channel {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .blocking_send(Ok(Bytes::copy_from_slice(bytes)))
            .map_err(|_| std::io::Error::other("the client stopped reading the bundle"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A file's bytes, hashed as the bundle reads them.
struct Hashed {
    inner: Box<dyn std::io::Read + Send>,
    hasher: blake3::Hasher,
}

impl std::io::Read for Hashed {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

fn bundle_refusal(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message.into() })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header shape `DATA.md` §5.1 exists for. Asserted on the exact string rather
    /// than on "contains a percent sign", because the two halves have to agree about the
    /// same name and a test that reads one half cannot see them disagree.
    #[test]
    fn a_turkish_name_survives_in_one_half_and_degrades_in_the_other() {
        let filename = download_filename("Şaft yatak kapağı, LP-3120-05", "stl");
        assert_eq!(filename, "Şaft yatak kapağı, LP-3120-05.stl");
        assert_eq!(
            content_disposition(&filename),
            "attachment; filename=\"_aft yatak kapa__, LP-3120-05.stl\"; \
             filename*=UTF-8''%C5%9Eaft%20yatak%20kapa%C4%9F%C4%B1%2C%20LP-3120-05.stl"
        );
    }

    #[test]
    fn a_name_cannot_carry_a_path_a_quote_or_a_control_character() {
        assert_eq!(
            download_filename("../../etc/passwd", "step"),
            "....etcpasswd.step",
            "a separator is dropped, so the dots that remain cannot traverse anywhere"
        );
        assert_eq!(
            download_filename("Kapak\"; rm -rf /\r\n", "stl"),
            "Kapak; rm -rf .stl",
            "the quote that would close the ASCII half, and the CRLF that would end the \
             header, are both gone"
        );
        assert_eq!(download_filename("   ", "3mf"), "download.3mf");
    }

    /// The three characters `attr-char` excludes, and the three the fixture above cannot
    /// see: `Şaft yatak kapağı, LP-3120-05` carries none of them, so an allowlist that
    /// grew to admit `%`, `'` or `*` would leave every other assertion in this file green.
    /// Each one breaks something different. A raw `'` closes the language tag in
    /// `filename*=UTF-8''…` early, so a client reads the rest of the name as a charset it
    /// does not know. A raw `*` is excluded from `attr-char` outright, so the parameter is
    /// no longer well-formed and a client is entitled to ignore it. A raw `%` is the worst
    /// of the three, because nothing looks wrong: RFC 8187 has the client percent-decode
    /// this value, so `%2F` in a name arrives at the client as `/` — the separator
    /// [`sanitize`] exists to strip, put back after it ran.
    ///
    /// Asserted against the whole header, like the Turkish case: the ASCII half keeps all
    /// three raw on purpose, because a quoted-string is not percent-decoded, and only the
    /// two halves side by side show that the difference between them is deliberate.
    #[test]
    fn the_characters_attr_char_excludes_are_escaped_rather_than_passed_through() {
        assert_eq!(
            content_disposition(&download_filename(
                "M8'lik flanş, %20 dolgu *taslak*, LP-4415-02",
                "stl"
            )),
            "attachment; filename=\"M8'lik flan_, %20 dolgu *taslak*, LP-4415-02.stl\"; \
             filename*=UTF-8''M8%27lik%20flan%C5%9F%2C%20%2520%20dolgu%20%2Ataslak%2A\
             %2C%20LP-4415-02.stl"
        );

        // The same escape as the security property rather than the grammar one. A
        // supplier export that percent-encoded the separator in its own catalogue path
        // leaves that text sitting in the part name; encoding the `%` again is the whole
        // reason the client's decode hands back those six characters instead of a path.
        assert_eq!(
            content_disposition(&download_filename("Rulman%2FLP-4415-02", "step")),
            "attachment; filename=\"Rulman%2FLP-4415-02.step\"; \
             filename*=UTF-8''Rulman%252FLP-4415-02.step"
        );
    }

    /// `NAME_MAX` is a byte count, so the cap has to be one too — and it has to land on a
    /// character boundary, since a header truncated mid-`ğ` is not UTF-8 at all.
    #[test]
    fn a_long_turkish_name_is_capped_in_bytes_without_splitting_a_character() {
        let name = "ğ".repeat(300);
        let filename = download_filename(&name, "step");
        assert!(
            filename.len() <= MAX_FILENAME_BYTES,
            "capped in bytes, got {} for {filename}",
            filename.len()
        );
        assert!(
            filename.ends_with("ğ.step"),
            "and not mid-character: {filename}"
        );
    }
}
