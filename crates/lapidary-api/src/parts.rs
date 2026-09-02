//! The grid: listing parts in a library. `GET /api/libraries/{id}/parts?after=&limit=`,
//! `api` role only. The open path's main read — this is what the grid renders from —
//! and it reads metadata and derivatives only, never a source file and never the CAD
//! kernel (structurally: this crate cannot link `lapidary-cad`, see `lib.rs`).
//!
//! `after=` with nothing after the `=` is not a client bug: it is the literal shape of
//! `` `…/parts?after=${cursor ?? ''}&limit=${n}` ``, the natural way to build this URL
//! before a cursor exists, and this handler treats it the same as `after` being absent
//! entirely rather than rejecting it as an invalid id.

use crate::AppState;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{LibraryId, PartId};
use lapidary_db::{DbError, PartRepository, PartRow, PgParts};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Page size when the query string omits `limit`.
const DEFAULT_LIMIT: u16 = 50;

/// The hard ceiling on `limit`, regardless of what the query string asks for. Trusting
/// an unbounded `limit` is a trivial way to make one request materialise an entire
/// library — thumbnails and all — into memory.
const MAX_LIMIT: u16 = 100;

/// One grid card — the wire shape, deliberately not `PartSummary`.
///
/// `PartSummary.thumbnail` is `Option<BlobHash>`: a content hash for a hash-addressed
/// thumbnail endpoint that arrives with the viewer. Slice 1 has no such endpoint — it
/// stores the WebP inline as `bytea` so a grid page costs one query, not a round trip
/// per card — so this type carries the decoded bytes themselves, as a `data:` URL a
/// browser can drop straight into an `<img src>`. Widening `PartSummary` instead would
/// put a transport concern into a domain type the viewer will reuse differently.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartCard {
    pub id: PartId,
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    /// `data:image/webp;base64,<...>`. `None` when the part's latest revision has no
    /// thumbnail derivative.
    pub thumbnail: Option<String>,
    pub triangle_count: Option<u32>,
    /// True when *any* geometric figure on this part is mesh-derived — carried
    /// verbatim from `PartSummary.approximate`, not narrowed to "every". Every figure
    /// on a slice-1 (STL-only) part is tessellated, so this reads `true` today, but the
    /// meaning must stay "any" for when analytic B-rep figures arrive alongside mesh
    /// ones on the same part.
    pub approximate: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A keyset page of the grid.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartsPage {
    pub parts: Vec<PartCard>,
    /// Pass back as `after` to fetch the next page. `None` when this page was not
    /// full — a short page proves there is nothing left, so there is no id to hand
    /// back that would not just fetch another empty page.
    pub next: Option<PartId>,
}

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    /// The previous page's last id, or absent/empty for the first page.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    after: Option<PartId>,
    /// Parsed as `i64`, not `u16`: the query string is untrusted text, and a value
    /// like `100000` must reach the `clamp` below and come out as `MAX_LIMIT`, not
    /// fail deserialization because it does not fit a 16-bit type before the clamp
    /// ever runs — `u16`'s serde impl rejects an out-of-range number outright rather
    /// than saturating.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    limit: Option<i64>,
}

/// Treats an empty query-string value the same as an absent key. `#[serde(default)]`
/// alone only covers the key being missing entirely — a key that is present with
/// nothing after the `=` still reaches the field's own deserializer as the empty
/// string, and `T::from_str("")` rejects it for every `T` this endpoint uses (`PartId`,
/// `i64`), which is what turned the documented `after=&limit=` URL shape into a 400.
fn empty_str_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = String::deserialize(deserializer)?;
    if raw.is_empty() {
        Ok(None)
    } else {
        raw.parse().map(Some).map_err(serde::de::Error::custom)
    }
}

/// `GET /api/libraries/{id}/parts?after=&limit=` — the grid's one read. Keyset paged,
/// newest first, thumbnails inline.
pub async fn page(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let PageQuery { after, limit } = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return bad_query(&rejection),
    };

    // Zero would ask PgParts::page for a LIMIT 0 query and then always report `next:
    // null` (a short page, by definition) even though more rows exist — clamp the
    // bottom as well as the top. The clamp runs on the widened `i64` so an
    // out-of-range value (too large *or* negative) lands inside [1, MAX_LIMIT] instead
    // of failing to parse; the cast back to `u16` afterward is safe because the value
    // is now guaranteed to fit.
    let limit = limit
        .unwrap_or(i64::from(DEFAULT_LIMIT))
        .clamp(1, i64::from(MAX_LIMIT)) as u16;

    match PgParts(state.db).page(library, after, limit).await {
        Ok(rows) => {
            // A page shorter than `limit` proves there is no further page. A full page
            // might or might not be the last one, so it hands back the last id and lets
            // the next request find out.
            let next = if rows.len() == usize::from(limit) {
                rows.last().map(|row| row.summary.id)
            } else {
                None
            };
            let parts = rows.into_iter().map(to_card).collect();
            Json(PartsPage { parts, next }).into_response()
        }
        Err(err) => internal_error(&err),
    }
}

/// The query string failed to parse — a malformed (non-empty) `after` or a `limit`
/// that isn't a number. axum's default rejection body is a bare, unstructured line of
/// text; wrap it so the response says what broke and what a client can send instead.
fn bad_query(rejection: &QueryRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "Could not read the query string: {rejection}. `after` must be a part id \
                 from a previous page (or omitted/empty for the first page); `limit` must \
                 be a whole number."
            )
        })),
    )
        .into_response()
}

/// The query itself failed. Every `DbError` variant already carries its own
/// operator-facing remedy via `Display` — appending fixed connectivity advice on top,
/// as this handler once did, points a `TimestampOutOfRange` (a corrupt row, nothing to
/// do with connectivity) at the wrong system. `client_message` is what decides which
/// variants' text is safe to hand back verbatim; the ones that are not (an upstream
/// `sqlx`/migration error this crate did not compose) get a generic message here while
/// the real detail still reaches the operator, through the log line below rather than
/// the response body — the same asymmetry `health::healthz` already keeps by never
/// putting a live error's text in its response at all.
fn internal_error(err: &DbError) -> Response {
    tracing::error!(error = %err, "grid page query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}

fn to_card(row: PartRow) -> PartCard {
    let summary = row.summary;
    PartCard {
        id: summary.id,
        library: summary.library,
        name: summary.name,
        part_number: summary.part_number,
        thumbnail: row
            .thumbnail_webp
            .map(|bytes| format!("data:image/webp;base64,{}", BASE64.encode(bytes))),
        triangle_count: summary.triangle_count,
        approximate: summary.approximate,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
    }
}
