//! `GET /api/parts/{id}/revisions` — one part's history, newest first.
//!
//! Still the open path: `revision`, `derivative` and `file` rows, and a thumbnail that
//! travels inline out of its row exactly as `detail.rs` serves one. No source file is read
//! and no geometry parsed. Every figure keeps its provenance, for the reason `detail.rs`'s
//! module doc gives — a history that dropped the ≈ would be the one place a mesh figure
//! passed for an exact one.

use crate::AppState;
use crate::derive::{internal_error, no_such_part};
use crate::detail::{pair, wrap};
use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{Approximate, BlobHash, PartId, Provenance, RevisionId, RevisionOrigin};
use lapidary_db::{DbError, PgParts, PgRevisions, RevisionRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One revision in a part's history.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartRevision {
    pub id: RevisionId,
    /// The revision this one was recorded on top of. `None` for a part's first.
    pub parent: Option<RevisionId>,
    pub rev_label: String,
    pub origin: RevisionOrigin,
    pub created_at: Timestamp,
    /// `data:image/webp;base64,…`, as `PartDetail.thumbnail` is.
    pub thumbnail: Option<String>,
    pub triangle_count: Option<u32>,
    pub bbox_mm: Option<Approximate<[f64; 3]>>,
    pub volume_mm3: Option<Approximate<f64>>,
    pub surface_area_mm2: Option<Approximate<f64>>,
    pub source_hash: Option<BlobHash>,
    pub source_format: Option<String>,
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
}

/// A part that is not there — never existed, or deleted — answers the `404` its page does:
/// a deleted part's history is not served beside a page that says it is gone.
pub async fn list(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db.clone()).library_of(part).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "revision history part lookup failed"),
    }
    let rows = match PgRevisions(state.db).history(part).await {
        Ok(rows) => rows,
        Err(err) => return internal_error(&err, "revision history query failed"),
    };
    match rows
        .into_iter()
        .map(to_revision)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(revisions) => Json(revisions).into_response(),
        Err(err) => internal_error(&err, "revision history row refused"),
    }
}

/// A row into the wire shape, refusing what `detail.rs` refuses: a provenance word this
/// build does not know, and a count or size that is negative.
fn to_revision(row: RevisionRow) -> Result<PartRevision, DbError> {
    let provenance = |text: Option<String>| {
        text.map(|t| {
            t.parse::<Provenance>()
                .map_err(|_| DbError::UnknownProvenance { value: t })
        })
        .transpose()
    };
    let volume_source = provenance(row.volume_source)?;
    let surface_area_source = provenance(row.surface_area_source)?;
    let bbox_source = provenance(row.bbox_source)?;
    Ok(PartRevision {
        id: row.id,
        parent: row.parent,
        rev_label: row.rev_label,
        origin: row.origin,
        created_at: row.created_at,
        thumbnail: row
            .thumbnail
            .map(|bytes| format!("data:image/webp;base64,{}", BASE64.encode(bytes))),
        triangle_count: row
            .triangle_count
            .map(|count| {
                u32::try_from(count).map_err(|_| DbError::NegativeTriangleCount {
                    column: "revision.triangle_count",
                    value: count,
                })
            })
            .transpose()?,
        // The box's own provenance where ingest recorded one, else the surface area's, as
        // `detail.rs` reasons; tessellated when neither, which can only over-label.
        bbox_mm: row.bbox_mm.map(|bbox| {
            wrap(
                bbox,
                bbox_source
                    .or(surface_area_source)
                    .unwrap_or(Provenance::Tessellated),
            )
        }),
        volume_mm3: pair(row.volume_mm3, volume_source),
        surface_area_mm2: pair(row.surface_area_mm2, surface_area_source),
        source_hash: row.source_hash,
        source_format: row.format,
        source_bytes: row
            .size_bytes
            .map(|bytes| {
                u64::try_from(bytes).map_err(|_| DbError::NegativeByteCount {
                    column: "file.size_bytes",
                    value: bytes,
                })
            })
            .transpose()?,
    })
}
