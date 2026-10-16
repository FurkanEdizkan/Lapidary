//! The gallery's routes. Every picture comes through `lapidary_storage::images::normalize`, which
//! is where the controls on the *bytes* live (`DATA.md` §4.1); `fetch.rs` owns the ones about the
//! *network*.

use crate::AppState;
use crate::derive::internal_error;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lapidary_core::{PartId, PartImageId};
use lapidary_db::{PgBlobs, PgParts};
pub use lapidary_storage::images::{
    INLINE_LIMIT, ImageError, MAX_INPUT_BYTES, NormalizedImage, normalize,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `POST /api/parts/{id}/images` — attach a picture to a part.
///
/// The body is the file, raw. Not multipart: there is one field, and a multipart parser is
/// a second format to read from an untrusted source in a route whose whole job is being
/// careful about that. Not the chunked uploader either — that exists for 2 GB CAD files
/// over a VPN, and an image is capped at 10 MB, so a resumable protocol for it would be
/// machinery with no failure to survive.
///
/// The declared content type is ignored on purpose. [`normalize`] reads the file's own
/// header, because the header is the only part of this request the sender does not choose.
///
/// `?from=snapshot` says the bytes are the 3D viewer's own canvas, not a file somebody chose — the
/// same bytes through the same checks, recorded as `snapshot` so the gallery can tell them apart.
/// Any other value is refused rather than recorded as an upload it was not.
pub async fn upload(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Response {
    let origin = match query.get("from").map(String::as_str) {
        None => "uploaded",
        Some("snapshot") => "snapshot",
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "message": "`from` may only be `snapshot`, for a picture taken from the 3D viewer. Leave it out for a file you chose."
                })),
            )
                .into_response();
        }
    };
    match normalize(&body) {
        Ok(image) => store(state, part, image, origin, None).await,
        Err(err) => refused(&err),
    }
}

/// `POST /api/parts/{id}/images/from-url` — attach a picture by pasting its address.
///
/// **The fetch is the whole risk and it lives in [`crate::fetch`]**, which resolves, checks
/// the address against every private range, pins it, and follows redirects one hop at a time
/// re-checking each — `DATA.md` §4.1. What arrives back here is bytes from a place this
/// server was willing to ask, and nothing more: they go through exactly the same
/// [`normalize`] every uploaded file does, because bytes fetched from the internet are no
/// more trustworthy than bytes somebody hands you.
///
/// **Fetched once, then served from our own copy.** `source_url` is recorded so a person can
/// see they did not choose the picture themselves and so a broken one can be re-fetched — it
/// is never used to load the image. Hotlinking leaks a referrer on every grid scroll and
/// breaks the day the host rotates a URL (`0015_part_images_and_sources.sql`).
///
/// In an air-gapped install this route simply fails, which is correct: uploading always
/// works, and that is the path `DATA.md` §4 puts first for exactly this reason.
pub async fn from_url(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<FetchImageRequest>,
) -> Response {
    let bytes = match lapidary_sites::fetch::fetch_image(&body.url).await {
        Ok(bytes) => bytes,
        Err(err) => return not_fetched(&err),
    };
    match normalize(&bytes) {
        Ok(image) => store(state, part, image, "url_supplied", Some(body.url)).await,
        Err(err) => refused(&err),
    }
}

/// Put a normalized image where it belongs and write its gallery row.
///
/// Shared by both routes, so "inline under 64 KB, blob above it" and the order the two
/// writes happen in are decided once. `origin` and `source_url` are the only things the two
/// callers disagree about.
async fn store(
    state: AppState,
    part: PartId,
    image: NormalizedImage,
    origin: &str,
    source_url: Option<String>,
) -> Response {
    // Inline or blob, decided by size and nothing else. The blob is written before the row
    // that references it: bytes on disk with no row are collected by the sweep, where a row
    // pointing at bytes that were never written is an image that renders as a broken box.
    let blob = if image.webp.len() < INLINE_LIMIT {
        None
    } else {
        match lapidary_storage::DerivativeStore::open(&state.blob_root).put(&image.webp) {
            Ok(stored) => {
                // `StoredBlob` is the store's answer; `StoredBlobRow` is the row shape.
                let row = lapidary_db::StoredBlobRow {
                    hash: stored.hash,
                    size_bytes: stored.size_bytes,
                    stored_bytes: stored.stored_bytes,
                    zstd_level: stored.zstd_level,
                };
                if let Err(err) = PgBlobs(state.db.clone()).record_unreferenced(&row).await {
                    return internal_error(&err, "recording an image blob failed");
                }
                Some(row.hash)
            }
            Err(err) => {
                tracing::error!(error = %err, "could not write an image to the store");
                return refused(&ImageError::Unencodable);
            }
        }
    };

    match PgParts(state.db)
        .add_part_image(
            part,
            lapidary_db::NewPartImage {
                bytes: match &blob {
                    Some(hash) => lapidary_db::ImageBytes::Blob(hash),
                    None => lapidary_db::ImageBytes::Inline(&image.webp),
                },
                origin,
                source_url: source_url.as_deref(),
            },
        )
        .await
    {
        // The size is in the answer because the resize is silent otherwise. An image over
        // `MAX_EDGE_PX` is scaled down on the way in — correct, and not something to do to
        // somebody's photograph without telling them what they got.
        Ok(id) => (
            StatusCode::CREATED,
            Json(StoredImage {
                id,
                width: image.width,
                height: image.height,
            }),
        )
            .into_response(),
        Err(err) => internal_error(&err, "attaching an image failed"),
    }
}

/// `GET /api/parts/{id}/images` — the gallery, in order.
///
/// An inline image comes back as a `data:` URL, exactly as a thumbnail does on a card: it
/// is already in the row, and a second request for bytes we are holding would be a round
/// trip bought with nothing. A blob comes back as its `/api/blob/{hash}` URL, which is
/// cached immutably and shared between every part that points at it.
pub async fn list(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).part_images(part).await {
        Ok(rows) => Json(rows.into_iter().map(part_image).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "reading a part's gallery failed"),
    }
}

/// One stored row as a reader gets it, bytes or hash resolved to something an `<img src>`
/// takes.
///
/// Shared by the gallery and by the grid card (`parts::to_card`), so "inline rides as a
/// `data:` URL, a blob is addressed at `/api/blob/{hash}`" is decided once. A card showing a
/// picture a different way from the gallery showing the same picture would be two answers to
/// the same question.
pub(crate) fn part_image(row: lapidary_db::PartImageRow) -> PartImage {
    PartImage {
        id: row.id,
        src: match (row.inline_webp, row.hash) {
            (Some(bytes), _) => format!("data:image/webp;base64,{}", BASE64.encode(bytes)),
            (None, Some(hash)) => format!("/api/blob/{}", hash.to_hex()),
            // The CHECK constraint makes this unreachable; an empty string is what an
            // `<img>` renders as nothing rather than as a broken box.
            (None, None) => String::new(),
        },
        origin: row.origin,
        source_url: row.source_url,
        fit: row.fit,
        focus_x: row.focus.0,
        focus_y: row.focus.1,
    }
}

/// What an upload produced: the id to address it by, and the size it was stored at.
///
/// The size is here because the resize is otherwise invisible. A photograph over
/// `MAX_EDGE_PX` on its long edge is scaled down on the way in, and a person who attached a
/// 4000-pixel picture is owed the sentence saying what is now on the card.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StoredImage {
    pub id: PartImageId,
    pub width: u32,
    pub height: u32,
}

/// One image, ready to render. `src` is a `data:` URL or a blob route — the caller does not
/// need to know which, and the distinction is a storage decision rather than a fact about
/// the picture.
// `Clone` and `Deserialize` because `PartCard` carries one and is itself both — a card is
// deserialized in tests and cloned by the grid handler.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PartImage {
    pub id: PartImageId,
    pub src: String,
    /// `uploaded`, `url_supplied`, `embedded` (a 3MF's own thumbnail) or `snapshot` (the 3D viewer's).
    pub origin: String,
    /// Where it came from, for one that was fetched. Shown so a person can tell an image
    /// they chose from one that was pulled in for them.
    pub source_url: Option<String>,
    /// `cover` or `contain` — CSS's own `object-fit` values, because the browser is what
    /// applies them. The bytes are never re-encoded to match.
    pub fit: String,
    /// What to keep when `cover` crops, as a fraction of each edge. CSS's `object-position`.
    pub focus_x: f64,
    pub focus_y: f64,
}

/// How a picture should sit in its frame.
///
/// A whole framing rather than a patch of one: there are three fields, a person adjusting
/// one is looking at the other two, and "omitted means unchanged" is a distinction worth
/// having only when the fields are independent — `FolderPatch` needed it because a rename
/// and a move are different intents, and these are one.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetFraming {
    pub fit: String,
    pub focus_x: f64,
    pub focus_y: f64,
}

/// `PATCH /api/parts/{id}/images/{imageId}` — re-frame a picture.
///
/// **Nothing is re-encoded and no bytes move.** The framing is three columns applied by the
/// browser at display time, so this can be done as often as somebody likes, undone, and
/// never costs the picture a generation of quality.
///
/// Both ids are in the path and both are checked, because an image id on its own is a bare
/// handle to a row — the same shape of mistake `CLAUDE.md` names for blob hashes. An image
/// that is not on that part is a 404 rather than a 403, which is also what a part that does
/// not exist gets: telling them apart would confirm a row exists to somebody who cannot see
/// it.
pub async fn set_framing(
    State(state): State<AppState>,
    Path((part, image)): Path<(PartId, PartImageId)>,
    Json(body): Json<SetFraming>,
) -> Response {
    // Checked here rather than left to the CHECK constraint, so a bad value is a sentence
    // instead of a 500 with a constraint name in the log. `0.0..=1.0` also catches NaN,
    // which no comparison against a bound would.
    if !matches!(body.fit.as_str(), "cover" | "contain")
        || !(0.0..=1.0).contains(&body.focus_x)
        || !(0.0..=1.0).contains(&body.focus_y)
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "message": BAD_FRAMING })),
        )
            .into_response();
    }

    match PgParts(state.db)
        .set_image_framing(
            part,
            image,
            lapidary_db::Framing {
                fit: &body.fit,
                focus: (body.focus_x, body.focus_y),
            },
        )
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => internal_error(&err, "re-framing an image failed"),
    }
}

/// The one refusal this route has, and it is a programming error rather than something a
/// person can do from the interface — the controls only ever send a value from the set.
const BAD_FRAMING: &str = "That framing is not one Lapidary can apply. `fit` must be `cover` or `contain`, and the focal point must be between 0 and 1 on each axis.";

/// The address to fetch from. An object rather than a bare string so that the crop and fit
/// fields, which live on the row and not in the bytes, have somewhere to go later.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FetchImageRequest {
    pub url: String,
}

/// A fetch that did not happen, with the status that says whose problem it is.
///
/// **`422` for the refusals and `502` for the failures**, and the split is the useful one: a
/// `422` means the address will never work and pasting it again will not help, while a `502`
/// means the other end did not answer this time. `NotPublic` is a `422` rather than a `403`
/// — nothing was forbidden to *this* caller, the address is simply not one we fetch from.
fn not_fetched(err: &lapidary_sites::fetch::FetchError) -> Response {
    use lapidary_sites::fetch::FetchError as F;
    let status = match err {
        F::NotFetchable | F::NotPublic | F::TooManyRedirects | F::BadRedirect => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        F::NotAnImage { .. } => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        F::TooLarge | F::AnswerTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        F::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        F::Unresolvable | F::NotOk { .. } | F::Unreachable { .. } => StatusCode::BAD_GATEWAY,
    };
    (
        status,
        Json(serde_json::json!({ "message": err.to_string() })),
    )
        .into_response()
}

/// A refusal a person can act on, with the status that matches which kind it is.
///
/// `413` for too large and `415` for the wrong kind of file, because those are what those
/// statuses mean; `422` for an image that is a real image and still not one we will store.
/// A caller reading only the status still learns something true.
fn refused(err: &ImageError) -> Response {
    let status = match err {
        ImageError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        ImageError::NotAnImage => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ImageError::Undecodable | ImageError::TooSmall { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        ImageError::Unencodable => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "message": err.to_string() })),
    )
        .into_response()
}
