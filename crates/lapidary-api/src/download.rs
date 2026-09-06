//! `GET /api/revisions/{id}/download?variant=original` — the ingested bytes, exactly as
//! they arrived. `api` role only, per `DATA.md` §5.1.
//!
//! The one file in this crate allowed to name `SourceReader`, and `cargo xtask
//! check-deploy` (rule 5, `xtask/src/deploy.rs`) fails on any other that does. That is
//! not an exception to *the open path never touches a source file*: opening — the grid,
//! the viewer, the detail card, the interactive path whose failure mode is parsing a
//! STEP file to draw a thumbnail — still reads metadata and derivatives only. Handing a
//! user the exact bytes they asked for, parsing nothing, is a different path, and it is
//! the only way `CLAUDE.md`'s other rule (*`variant=original` returns byte-identical
//! ingested bytes*) can be true at all. Spec §1.3 argues both halves.
//!
//! Four things this route refuses to do rather than do badly, and each answers with its
//! own message because each has a different person who can act on it: it will not guess a
//! `variant`, it will not serve bytes whose stored compression nobody recorded, it will
//! not serve bytes that do not hash to the digest we filed them under, and it will not
//! convert. The last one is not a limitation to lift here — converting invokes the CAD
//! kernel, which this crate cannot link (`xtask/src/layers.rs`), so a converted download
//! is a worker-side derivative and a different route entirely.
//!
//! The whole file is buffered before a byte is written, because `SourceReader::get`
//! returns a `Vec<u8>`. Fine at Phase 1 sizes and wrong for a 2 GB STEP; streaming is its
//! own slice, and the re-hash below costs nothing extra while the bytes are already
//! resident (spec §2.5).

use crate::AppState;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, RevisionId};
use lapidary_db::{DbError, PgBlobs, PgParts};
use lapidary_storage::{SourceReader, StorageError};
use serde::Deserialize;

/// The only `variant` this slice serves. Named in every message that rejects another
/// one, so a caller is told what to send rather than what not to.
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
/// because it is the only check that needs the bytes. And `touch_blob` comes after all of
/// them: this timestamp means *these bytes were handed to somebody*, so a request that
/// refused to serve must not move it, or a caller could warm any blob by guessing.
pub async fn original(
    State(state): State<AppState>,
    Path(revision): Path<RevisionId>,
    query: Result<Query<DownloadQuery>, QueryRejection>,
) -> Response {
    let AppState { db, blob_root } = state;
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
        Some(other) => return unknown_variant(other),
        None => return missing_variant(),
    }

    // Spec §2.5.1. Ingest records a concrete level for every source blob it writes and
    // NULL for every derivative — which does not make a NULL reached here a row from
    // outside. `link_existing` leaves an existing `blob` row alone, so bytes
    // byte-identical to a tessellation rung land as a source file over a NULL-level blob,
    // and lapidary-db's `a_source_blob_whose_level_nobody_recorded_reads_as_uncompressed`
    // builds exactly that. Either way nobody recorded how these bytes were written:
    // reading them raw would serve something and hope, and refusing names the one thing
    // an operator can go and look at.
    let Some(zstd_level) = source.zstd_level else {
        return unrecorded_level(&source.hash);
    };

    // Opened per request from the root, as `blob.rs` opens its own store: the handle is a
    // `PathBuf` and a decode flag, so holding one in `AppState` would buy nothing and put
    // source-byte access in a struct every other route shares.
    let reader = SourceReader::open(&blob_root);

    // `storage_path` is null while `migrate_storage` is still draining a library — a live
    // state for as long as that job takes, hours on a real corpus, and not an edge case to
    // special-case away (migration `0008`, `CLAUDE.md`). `Some` names where the bytes
    // actually sit and reads through `get_at`; `None` means they are still at the old
    // content-addressed path and reads exactly as this route always has. Either way the
    // `zstd_level` above came off the same row, so both branches follow the same recorded
    // compression rather than one of them re-deriving it from which branch it is.
    let bytes = match source.storage_path.as_deref() {
        Some(rel) => reader.get_at(rel, Some(zstd_level)),
        None => reader.get(&source.hash, Some(zstd_level)),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(err) => return unreadable(&source.hash, source.storage_path.as_deref(), &err),
    };

    // Spec §2.5. BLAKE3 at ~1 GB/s against single-digit-megabyte files is free next to
    // the transfer that follows, and it turns the product claim — byte-identical,
    // verifiable against the stored digest — into something checked rather than asserted.
    // It is also the only thing standing between a user and a zstd frame served as their
    // file during slice 7's recompression window (spec §2.7): decode with the wrong level
    // and the digest cannot match.
    let served = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
    if served != source.hash {
        return hash_mismatch(&source.hash, &served);
    }

    // Awaited and discarded rather than spawned, exactly as `blob.rs` does: a task racing
    // the response is a timestamp nothing can assert. This is the *only* warm input a
    // source blob has — a library browsed constantly and never downloaded stays cold, and
    // spec §2.6 hands slice 7 that decision rather than letting it inherit it silently.
    PgBlobs(db).touch_blob(&source.hash).await;

    let filename = download_filename(&source.part_name, &source.format);
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
            (header::ETAG, format!("\"{}\"", source.hash.to_hex())),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
        ],
        bytes,
    )
        .into_response()
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

/// No such revision, or its part is soft-deleted. One body for both, and honestly so: a
/// deleted part is gone from every view the grid offers, and a URL held from before the
/// delete must stop serving bytes rather than outlive it (spec §2.1).
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

/// Deliberately not [`missing_variant`]'s body. Someone who sent `variant=3mf` asked a
/// real question — where converted downloads are — and telling them to add a parameter
/// they already sent answers a question they did not ask.
fn unknown_variant(got: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "`{got}` is not a download variant. This server serves \
                 `variant={ORIGINAL}` — the ingested bytes, unconverted. A converted \
                 download is produced as a derivative by the worker, not by this route."
            )
        })),
    )
        .into_response()
}

/// Spec §2.5.1. Same status and same refusal as [`hash_mismatch`], and a different
/// message on purpose: this one says an operator has a `blob` row whose compression
/// nobody recorded, which is something they can go and look at. Collapsing the two would
/// leave them holding "the bytes were wrong" about bytes that were never the problem.
fn unrecorded_level(hash: &BlobHash) -> Response {
    let hex = hash.to_hex();
    tracing::error!(hash = %hex, "a source blob has no recorded compression level");
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

/// The bytes on disk are not the bytes we filed under that digest. Distinct from
/// [`unrecorded_level`] in wording as well as in cause: nothing is wrong with the row
/// here, and the remedy is the blob, not the record.
fn hash_mismatch(expected: &BlobHash, served: &BlobHash) -> Response {
    let expected = expected.to_hex();
    let served = served.to_hex();
    tracing::error!(
        expected = %expected,
        served = %served,
        "stored bytes do not hash to the digest they are filed under"
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "message": format!(
                "The bytes stored for blob {expected} hash to {served}, so they are not \
                 the file that was ingested. Nothing was served. Restore that blob from a \
                 backup, or re-ingest the source file to replace it."
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
fn unreadable(hash: &BlobHash, storage_path: Option<&str>, err: &StorageError) -> Response {
    let hex = hash.to_hex();
    tracing::error!(
        hash = %hex,
        storage_path = storage_path.unwrap_or("(content-addressed)"),
        error = %err,
        "a referenced source blob could not be read"
    );
    let check = match storage_path {
        Some(_) => "that this part's file is still present in its model directory",
        None => "that the file for that hash is present",
    };
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "message": format!(
                "Blob {hex} could not be read from the blob store, so nothing was served. \
                 Check that the blob volume is mounted and {check} — a source file is \
                 never removed while a part still references it."
            )
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
    tracing::error!(error = %err, "download source lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
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
