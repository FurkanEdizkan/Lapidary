//! A density per material, per library (goal 5, stage 2): `GET /api/libraries/{id}/densities`, and
//! `PUT` and `DELETE` on `/api/libraries/{library}/densities/{material}`.
//!
//! Stored in kg/m³, whatever the page shows. A density is typed, never measured, which is why a mass
//! worked out from one is always approximate.

use crate::AppState;
use crate::folders::{internal_error, refused};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::LibraryId;
use lapidary_db::PgDensities;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Above every material a part is made of: osmium, the densest element, is 22,590 kg/m³.
const DENSITY_MAX_KG_M3: f64 = 25_000.0;
/// The longest material name, in characters, as a part's list of materials allows.
const MATERIAL_MAX: usize = 64;

/// One material's density in a library.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MaterialDensity {
    pub material: String,
    pub density_kg_m3: f64,
}

/// The `PUT` body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetDensity {
    pub density_kg_m3: f64,
}

pub async fn list(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgDensities(state.db).list(library).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| MaterialDensity {
                    material: row.material,
                    density_kg_m3: row.density_kg_m3,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "density list failed"),
    }
}

pub async fn set(
    State(state): State<AppState>,
    Path((library, material)): Path<(LibraryId, String)>,
    body: Result<Json<SetDensity>, JsonRejection>,
) -> Response {
    if !named_as_parts_hold_it(&material) {
        return bad_material();
    }
    let density = match body {
        Ok(Json(SetDensity { density_kg_m3 }))
            if density_kg_m3.is_finite()
                && density_kg_m3 > 0.0
                && density_kg_m3 < DENSITY_MAX_KG_M3 =>
        {
            density_kg_m3
        }
        _ => {
            return refused(
                StatusCode::BAD_REQUEST,
                "badDensity",
                "A density is a number of kilograms per cubic metre above 0 and below 25,000, such as \
                 7850 for steel. Type it again.",
            );
        }
    };
    match PgDensities(state.db).set(library, &material, density).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchLibrary",
            "There is no library with that id. Reload the page and choose the library again.",
        ),
        Err(err) => internal_error(&err, "density set failed"),
    }
}

pub async fn remove(
    State(state): State<AppState>,
    Path((library, material)): Path<(LibraryId, String)>,
) -> Response {
    if !named_as_parts_hold_it(&material) {
        return bad_material();
    }
    match PgDensities(state.db).remove(library, &material).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchDensity",
            &format!("{material} has no density in this library, so there is nothing to remove."),
        ),
        Err(err) => internal_error(&err, "density remove failed"),
    }
}

/// A material as parts can hold it: not blank, no space at either end, and no longer than a part's
/// material may be. Kept exactly as given otherwise, since a density is keyed by the exact name.
fn named_as_parts_hold_it(material: &str) -> bool {
    !material.is_empty() && material == material.trim() && material.chars().count() <= MATERIAL_MAX
}

fn bad_material() -> Response {
    refused(
        StatusCode::BAD_REQUEST,
        "badMaterial",
        "A material is named as parts hold it: 1 to 64 characters, with no space at either end.",
    )
}
