//! One part, in full — the page a card links to.
//!
//! The grid answers "which of these is the one I want"; this answers "is this one right".
//! That is a different question and it wants different figures: the bounding box a part
//! has to fit in, whether the mesh is closed, what the file actually is, and where it came
//! from. None of that fits under a thumbnail, and putting it on `PartCard` would pay for
//! it fifty times per grid page to show it once.
//!
//! Still the open path. It reads `part`, `revision`, `derivative` and `file` rows and
//! never a source file, so nothing here needs a source-bytes handle at all and nothing
//! here parses geometry. (Naming that handle's type in this sentence is what
//! `check-deploy` refuses — it greps source text, deliberately.) Describing a source
//! file is not opening one: the hash and the sizes below come off `file` and `blob`
//! rows, which is the same thing a card already does.
//!
//! # Measurements go over the wire with their provenance attached
//!
//! `CLAUDE.md` is not negotiable about this: *"Mesh-derived measurements are labelled
//! 'approximate' in the UI, always."* So every figure that has a provenance travels as
//! `Approximate<f64>` — value and flag in one object — rather than as a bare number
//! beside a page-level boolean the renderer is trusted to consult.
//!
//! The difference is not stylistic and it arrives in Phase 2. A STEP part carries an
//! analytic volume and a tessellated triangle count on the *same revision*; a single
//! `approximate: true` on the page is then wrong about one figure whichever way it is
//! set. `PartCard.approximate` stays what it is — "any figure here is mesh-derived",
//! which is the honest thing a card has room for — and this page is where the per-figure
//! truth lives.

use crate::AppState;
use crate::derive::internal_error;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{Approximate, BlobHash, LibraryId, PartId, Provenance, RevisionId};
use lapidary_db::{PartDetailRow, PgParts};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Everything one part's page shows.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartDetail {
    pub id: PartId,
    pub library: LibraryId,
    /// The revision every figure below describes, and the one the download link names.
    /// Carried rather than re-resolved for the reason `PartCard` gives: resolving
    /// "latest" a second time could name a different revision than these numbers.
    pub revision: RevisionId,
    /// The revision's label — `1` for everything ingest writes today, because a second
    /// revision is Phase 4's. Shown rather than hidden so a part that *does* carry one
    /// does not silently look like a part that does not.
    pub rev_label: String,
    pub name: String,
    pub part_number: Option<String>,
    /// The part's identity within its library since slice 6a: the path a scan found it
    /// at, or the path the browser reported when it was dropped. Two parts named
    /// `bracket` in two folders are told apart by this and by nothing else, which is
    /// exactly why the page shows it.
    pub source_path: String,
    /// `data:image/webp;base64,<...>`, as on the card.
    pub thumbnail: Option<String>,
    /// Mesh facts. `None` on a revision whose measurements were never written.
    pub triangle_count: Option<u32>,
    /// Whether the mesh is closed. This is the reason `volume` may be absent on a part
    /// that plainly has one, so it is shown next to it rather than buried.
    pub is_watertight: Option<bool>,
    /// Extent in millimetres, all three axes or none — see `PartDetailRow::bbox_mm`.
    pub bbox_mm: Option<Approximate<[f64; 3]>>,
    /// **Absent for an open mesh, deliberately.** Signed-volume integration over a
    /// non-watertight mesh produces a plausible-looking number that means nothing, and
    /// `MeshMeasurements::volume_approximate` already refuses to answer for one.
    /// "Measurement must not lie" includes declining to measure.
    pub volume_mm3: Option<Approximate<f64>>,
    pub surface_area_mm2: Option<Approximate<f64>>,
    /// What produced the derivatives — the kernel and its version, as ingest recorded it.
    /// Two parts with different values here were measured by different code, which is the
    /// first thing to look at when two figures disagree.
    pub kernel_version: Option<String>,
    pub source_hash: Option<BlobHash>,
    /// `stl`, `3mf`, `obj` — what the ingested file actually is, not what its name says.
    pub source_format: Option<String>,
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub stored_bytes: Option<u64>,
    pub compressed: Option<bool>,
    /// The L0 tessellation, and what it occupies. The first honest caller of
    /// `GET /api/blob/{blake3}`: until this page, every ingest wrote a rung nothing could
    /// address. Showing its hash and size is what tells a user their part has a usable
    /// derivative at all.
    pub tessellation_l0: Option<BlobHash>,
    #[ts(type = "number | null")]
    pub tessellation_l0_bytes: Option<u64>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// `GET /api/parts/{id}` — one part, in full.
///
/// A part that does not exist and a part that has been deleted answer the same `404`,
/// undistinguished. That is the same answer `derive.rs`'s `no_such_part` gives and for the
/// same reason: distinguishing them would confirm that a part exists to someone who
/// cannot see it.
pub async fn detail(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).detail(part).await {
        Ok(Some(row)) => Json(to_detail(row)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "message": "No part with that id exists here, or it has been deleted. \
                            Go back to the grid and open the part from there."
            })),
        )
            .into_response(),
        Err(err) => internal_error(&err, "part detail query failed"),
    }
}

/// A row into the wire shape.
///
/// The three measurement fields are each a value and a provenance that are *independently*
/// nullable in the schema, and this is where that is collapsed into one object or nothing.
/// A value with no provenance is dropped rather than guessed at, for the reason
/// `detail_provenance` refuses an unknown word: there is no safe default, because one
/// default hedges and the other lies.
fn to_detail(row: PartDetailRow) -> PartDetail {
    PartDetail {
        id: row.id,
        library: row.library,
        revision: row.revision,
        rev_label: row.rev_label,
        name: row.name,
        part_number: row.part_number,
        source_path: row.source_path,
        thumbnail: row
            .thumbnail_webp
            .map(|bytes| format!("data:image/webp;base64,{}", BASE64.encode(bytes))),
        triangle_count: row.triangle_count,
        is_watertight: row.is_watertight,
        // The box has no provenance column of its own: it is derived from the same
        // geometry as the surface area, so it carries that figure's provenance. Falling
        // back to tessellated when even that is absent is safe in the one direction that
        // matters — it can only over-label, never present a mesh figure as exact.
        bbox_mm: row.bbox_mm.map(|bbox| {
            wrap(
                bbox,
                row.surface_area_source.unwrap_or(Provenance::Tessellated),
            )
        }),
        volume_mm3: pair(row.volume_mm3, row.volume_source),
        surface_area_mm2: pair(row.surface_area_mm2, row.surface_area_source),
        kernel_version: row.kernel_version,
        source_hash: row.source_hash,
        source_format: row.source_format,
        source_bytes: row.source_bytes,
        stored_bytes: row.stored_bytes,
        compressed: row.compressed,
        tessellation_l0: row.tessellation_l0,
        tessellation_l0_bytes: row.tessellation_l0_bytes,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// A figure and its provenance, or nothing. Both halves or neither: a number whose
/// provenance was never recorded is a number nobody can say is exact, and the UI has no
/// third rendering for it.
fn pair(value: Option<f64>, source: Option<Provenance>) -> Option<Approximate<f64>> {
    Some(wrap(value?, source?))
}

fn wrap<T>(value: T, source: Provenance) -> Approximate<T> {
    match source {
        Provenance::Analytic => Approximate::analytic(value),
        Provenance::Tessellated => Approximate::tessellated(value),
    }
}
