//! The grid: listing parts in a library. `GET /api/libraries/{id}/parts?after=&limit=`,
//! `api` role only. The open path's main read — this is what the grid renders from —
//! and it reads metadata and derivatives only, never a source file and never the CAD
//! kernel (structurally: this crate cannot link `lapidary-cad`, see `lib.rs`).

use crate::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{LibraryId, PartId};
use lapidary_db::{PartRepository, PartRow, PgParts};
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
    after: Option<PartId>,
    limit: Option<u16>,
}

/// `GET /api/libraries/{id}/parts?after=&limit=` — the grid's one read. Keyset paged,
/// newest first, thumbnails inline.
pub async fn page(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Query(query): Query<PageQuery>,
) -> Response {
    // Zero would ask PgParts::page for a LIMIT 0 query and then always report `next:
    // null` (a short page, by definition) even though more rows exist — clamp the
    // bottom as well as the top.
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    match PgParts(state.db).page(library, query.after, limit).await {
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
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "message": format!(
                    "Could not load parts for this library: {err}. Check that the `db` \
                     service is running and that DATABASE_URL matches it."
                )
            })),
        )
            .into_response(),
    }
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
