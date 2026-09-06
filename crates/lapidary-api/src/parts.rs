//! The grid: listing parts in a library. `GET /api/libraries/{id}/parts?after=&limit=`,
//! `api` role only. The open path's main read — this is what the grid renders from —
//! and it reads metadata and derivatives only, never a source file and never the CAD
//! kernel (structurally: this crate cannot link `lapidary-cad`, see `lib.rs`). The
//! storage figures on a card are `file` and `blob` rows — how large a source file is
//! and how it was stored — and reading a row about a file is not opening one; nothing
//! here ever asks the blob store for bytes.
//!
//! `GET /api/libraries/{id}/storage` is the same figures summed over the library, and it
//! lives here rather than in `derive.rs` because it is the per-card storage line's total,
//! not a trigger or a setting. It reads rows too, and no bytes.
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
use lapidary_core::{BlobHash, LibraryId, PartId, RevisionId};
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
    /// The revision this card's numbers describe, and the one its download link names.
    /// Carried verbatim from `PartSummary.revision` — see there for why the frontend
    /// must not resolve "latest" a second time of its own.
    pub revision: RevisionId,
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
    /// The source file's hash, `None` when the revision has no source row. Rendered as
    /// a short hash beside the download link so a user can check what they got against
    /// what the card claimed (`DATA.md` §5.1) — holding it is not authorization to read
    /// it, exactly as `PartSummary.thumbnail` is not.
    pub source_hash: Option<BlobHash>,
    /// The L0 tessellation's hash, carried verbatim from `PartSummary` for the reason
    /// `source_hash` is: it is the only thing that makes those bytes addressable, and
    /// `GET /api/blob/{blake3}` had no possible caller without it. Holding it is not
    /// authorization — that route checks reachability before serving.
    pub tessellation_l0: Option<BlobHash>,
    /// Ingested size and size on disk, from `PartSummary`. `number | null`, not
    /// `bigint`, for the reason given there: it is what serde puts on the wire.
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub stored_bytes: Option<u64>,
    /// Whether the stored bytes are a zstd frame. `Some(false)` covers both "stored
    /// raw" and "level unrecorded" — see `PartSummary.compressed`.
    pub compressed: Option<bool>,
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

/// What a library costs, the library-level half of the figures on every card.
///
/// Both totals are bytes on disk after compression, deduplicated — see
/// `PgParts::storage_totals`, which is where the accounting is written down. `number`,
/// not `bigint`: serde puts a JSON number on the wire, and ts-rs 12 would otherwise
/// promise the frontend something `JSON.parse` never produces.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryStorage {
    #[ts(type = "number")]
    pub source_bytes: u64,
    #[ts(type = "number")]
    pub derivative_bytes: u64,
    /// Derivative bytes ÷ source bytes. `None` for a library holding no source bytes,
    /// where the division has no answer — a library with nothing in it, and a `0` there
    /// would read as "derivatives cost nothing", which is a different claim.
    ///
    /// Sent rather than left to the client because the direction is the whole meaning:
    /// this is the figure that made slice 4's 92.5% drop legible (spec §4), and a second
    /// consumer dividing the other way would report the same library twice, differently.
    pub derivative_ratio: Option<f64>,
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
        Err(err) => internal_error(&err, "grid page query failed"),
    }
}

/// `GET /api/libraries/{id}/storage` — source total, derivative total, and the ratio.
///
/// A library that does not exist is a `404`, as it is on every other library route. The
/// grid deliberately answers an unknown id with an empty page — an empty library and an
/// id that names nothing look alike to someone browsing — but a storage panel reporting
/// `0 B` for a mistyped id is a number a person would believe.
pub async fn storage(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgParts(state.db).storage_totals(library).await {
        Ok(Some(totals)) => Json(LibraryStorage {
            source_bytes: totals.source_bytes,
            derivative_bytes: totals.derivative_bytes,
            // Both casts are lossless below 2^53 bytes, which is 9 petabytes in one
            // library; a ratio is a display figure and does not need more than that.
            derivative_ratio: (totals.source_bytes > 0)
                .then(|| totals.derivative_bytes as f64 / totals.source_bytes as f64),
        })
        .into_response(),
        Ok(None) => no_such_library(),
        Err(err) => internal_error(&err, "library storage query failed"),
    }
}

/// The storage route's `404`. Its own message rather than `derive.rs`'s: that one tells a
/// writer nothing was changed, which is an answer to a question a reader did not ask.
fn no_such_library() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No library with that id exists, so there is nothing stored under \
                        it. Check the id against the library list."
        })),
    )
        .into_response()
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
fn internal_error(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
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
        revision: summary.revision,
        name: summary.name,
        part_number: summary.part_number,
        thumbnail: row
            .thumbnail_webp
            .map(|bytes| format!("data:image/webp;base64,{}", BASE64.encode(bytes))),
        triangle_count: summary.triangle_count,
        approximate: summary.approximate,
        source_hash: summary.source_hash,
        tessellation_l0: summary.tessellation_l0,
        source_bytes: summary.source_bytes,
        stored_bytes: summary.stored_bytes,
        compressed: summary.compressed,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
    }
}
