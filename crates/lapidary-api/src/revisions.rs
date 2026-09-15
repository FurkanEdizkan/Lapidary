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
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{
    Approximate, BlobHash, PartId, Provenance, RevisionDiff, RevisionId, RevisionOrigin,
};
use lapidary_db::{DbError, PgDensities, PgParts, PgRevisions, RevisionRow};
use lapidary_vcs::diff::{RevisionFigures, diff};
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
    /// The B-rep's faces and edges, counted exactly by the CAD kernel. `None` for a mesh.
    pub face_count: Option<u32>,
    pub edge_count: Option<u32>,
    pub bbox_mm: Option<Approximate<[f64; 3]>>,
    pub volume_mm3: Option<Approximate<f64>>,
    pub surface_area_mm2: Option<Approximate<f64>>,
    /// This revision's volume times the density of the part's one material as it is today, worked out
    /// when read and never stored. Always approximate: a density is typed, not measured. `None` without
    /// a volume, or unless the part holds exactly one material and its library has a density for it.
    pub mass_g: Option<Approximate<f64>>,
    pub source_hash: Option<BlobHash>,
    pub source_format: Option<String>,
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    /// What changed from the revision this one was recorded on top of. `None` for a part's
    /// first revision.
    pub delta_from_parent: Option<RevisionDiff>,
    /// This revision's own rungs, which the overlay draws as a ghost behind the current part.
    /// Ingest writes L0; an L1 exists only if somebody opened the part while this revision was
    /// current, so an earlier revision usually has L0 alone.
    pub tessellation_l0: Option<BlobHash>,
    pub tessellation_l1: Option<BlobHash>,
}

/// A part that is not there — never existed, or deleted — answers the `404` its page does:
/// a deleted part's history is not served beside a page that says it is gone.
pub async fn list(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match history(&state, part).await {
        Ok(revisions) => Json(revisions).into_response(),
        Err(response) => *response,
    }
}

/// `?from=&to=`, each a revision id.
#[derive(Debug, Deserialize)]
pub struct CompareQuery {
    from: RevisionId,
    to: RevisionId,
}

/// `GET /api/parts/{id}/diff?from=&to=` — any two revisions of one part, `to` against `from`.
///
/// Both must be this part's own. A revision id is not a key to another part's figures:
/// content addressing is not authorization (`CLAUDE.md`), and an id is less than that.
pub async fn compare(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    query: Result<Query<CompareQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "message": "Say which two revisions to compare: `?from=` and `?to=`, each the id \
                            of a revision of this part."
            })),
        )
            .into_response();
    };
    let revisions = match history(&state, part).await {
        Ok(revisions) => revisions,
        Err(response) => return *response,
    };
    let find = |id: RevisionId| revisions.iter().find(|revision| revision.id == id);
    let (Some(from), Some(to)) = (find(query.from), find(query.to)) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "message": "One of those is not a revision of this part, so there is nothing to \
                            compare. Pick both from this part's history."
            })),
        )
            .into_response();
    };
    Json(diff(&figures(from), &figures(to))).into_response()
}

/// The part's revisions, newest first, each with its change from its parent — or the
/// response that says why there are none to give.
async fn history(state: &AppState, part: PartId) -> Result<Vec<PartRevision>, Box<Response>> {
    match PgParts(state.db.clone()).library_of(part).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(Box::new(no_such_part())),
        Err(err) => {
            return Err(Box::new(internal_error(
                &err,
                "revision history part lookup failed",
            )));
        }
    }
    let rows = PgRevisions(state.db.clone())
        .history(part)
        .await
        .map_err(|err| Box::new(internal_error(&err, "revision history query failed")))?;
    let mut revisions = rows
        .into_iter()
        .map(to_revision)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| Box::new(internal_error(&err, "revision history row refused")))?;
    // One density for every revision: today's material and today's density, so two revisions differ in
    // mass only as their volumes do.
    let density = PgDensities(state.db.clone())
        .of_part(part)
        .await
        .map_err(|err| Box::new(internal_error(&err, "part density lookup failed")))?;
    for revision in &mut revisions {
        revision.mass_g = density
            .zip(revision.volume_mm3)
            .map(|(density, volume)| Approximate::tessellated(volume.value() * density * 1e-6));
    }

    let recorded: Vec<(RevisionId, RevisionFigures)> = revisions
        .iter()
        .map(|revision| (revision.id, figures(revision)))
        .collect();
    for revision in &mut revisions {
        let parent = recorded.iter().find(|(id, _)| Some(*id) == revision.parent);
        revision.delta_from_parent = parent.map(|(_, from)| diff(from, &figures(revision)));
    }
    Ok(revisions)
}

/// What a diff reads, off the wire shape that already carries each figure's provenance.
fn figures(revision: &PartRevision) -> RevisionFigures {
    RevisionFigures {
        volume_mm3: revision.volume_mm3,
        surface_area_mm2: revision.surface_area_mm2,
        bbox_mm: revision.bbox_mm,
        triangle_count: revision.triangle_count,
        face_count: revision.face_count,
        edge_count: revision.edge_count,
        mass_g: revision.mass_g,
    }
}

/// A count column into the wire shape, refused when negative for `NegativeTriangleCount`'s reason.
fn count(column: &'static str, value: Option<i32>) -> Result<Option<u32>, DbError> {
    value
        .map(|count| {
            u32::try_from(count).map_err(|_| DbError::NegativeTriangleCount {
                column,
                value: count,
            })
        })
        .transpose()
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
        triangle_count: count("revision.triangle_count", row.triangle_count)?,
        face_count: count("revision.face_count", row.face_count)?,
        edge_count: count("revision.edge_count", row.edge_count)?,
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
        // Filled in once the part's density is read, as the parent's delta is.
        mass_g: None,
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
        // Filled in once the whole history is read: a parent is another row.
        delta_from_parent: None,
        tessellation_l0: row.tessellation_l0,
        tessellation_l1: row.tessellation_l1,
    })
}
