//! `PUT /api/parts/{id}/tags`, and `PUT`/`DELETE` on `/api/parts/{id}/materials`: the tags and the
//! materials a person gives a part.
//!
//! Each is set by request as the whole list, so adding one and removing one are the same write. They
//! are kept as written, capitals included and in the order given; blanks and repeats are dropped
//! rather than refused, because neither is one anyone meant to keep. Tags are never read off a file,
//! like a part number. Materials are also what a CAD file states, until a person types them
//! (`PgParts::set_materials`) — including typing none, which is why materials have a `DELETE` and
//! tags do not.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::PartId;
use lapidary_db::{DbError, PgParts};
use serde::Deserialize;
use ts_rs::TS;

/// A tag is a word or a short phrase a person types. A pasted sentence is refused.
const TAG_MAX: usize = 64;

/// A material is often not typed at all — it is whatever a CAD file states, and a supplier's export
/// spells a grade out with its standard and its condition: "Stainless steel, AISI 316L, annealed,
/// cold drawn bar to ASTM A276/A276M" is 71 characters. A material longer than a tag may be is
/// therefore ordinary, and `material_density`'s key takes the same 200 (migration `0051`), so a
/// material a part can hold is always a material a density can be set for.
pub(crate) const MATERIAL_MAX: usize = 200;

/// The `PUT` body: every tag the part has afterwards.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetTags {
    pub tags: Vec<String>,
}

/// The `PUT` body: every material the part has afterwards. An empty list says the part holds no
/// material, and is kept over what its file states like any other typed list; `DELETE` on the same
/// path is what hands the part back to the file.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetMaterials {
    pub materials: Vec<String>,
}

/// What one list is called in a refusal, and how long it may be.
struct Words {
    one: &'static str,
    many: &'static str,
    /// How many values the list takes.
    max: usize,
    /// How long one value may be, in characters. Not the same for a tag and a material: see
    /// [`MATERIAL_MAX`].
    max_chars: usize,
    too_long: &'static str,
    too_many: &'static str,
}

/// Enough to describe a part several ways, and few enough to read at a glance.
const TAGS: Words = Words {
    one: "tag",
    many: "tags",
    max: 32,
    max_chars: TAG_MAX,
    too_long: "tagTooLong",
    too_many: "tooManyTags",
};

/// A part is made of a few materials; a longer list is a paste.
const MATERIALS: Words = Words {
    one: "material",
    many: "materials",
    max: 8,
    max_chars: MATERIAL_MAX,
    too_long: "materialTooLong",
    too_many: "tooManyMaterials",
};

pub async fn set(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<SetTags>,
) -> Response {
    match cleaned(&body.tags, &TAGS) {
        Ok(tags) => saved(
            PgParts(state.db).set_tags(part, &tags).await,
            "tag update failed",
        ),
        Err((reason, message)) => refused(StatusCode::BAD_REQUEST, reason, &message),
    }
}

pub async fn set_materials(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<SetMaterials>,
) -> Response {
    match cleaned(&body.materials, &MATERIALS) {
        Ok(materials) => saved(
            PgParts(state.db).set_materials(part, &materials).await,
            "material update failed",
        ),
        Err((reason, message)) => refused(StatusCode::BAD_REQUEST, reason, &message),
    }
}

/// `DELETE /api/parts/{id}/materials` — hand the part back to what its file states.
///
/// Its own verb because an empty `PUT` is now a statement of fact: this part holds no material.
/// Tags have no counterpart, because nothing but a person ever gives a part one.
pub async fn unset_materials(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    saved(
        PgParts(state.db).unset_materials(part).await,
        "material reset failed",
    )
}

/// The list as it is kept, or the refusal's reason and sentence.
fn cleaned(given: &[String], words: &Words) -> Result<Vec<String>, (&'static str, String)> {
    let mut kept: Vec<String> = Vec::new();
    for value in given.iter().map(|value| value.trim()) {
        if !value.is_empty() && !kept.iter().any(|known| known == value) {
            kept.push(value.to_owned());
        }
    }
    if let Some(long) = kept
        .iter()
        .find(|value| value.chars().count() > words.max_chars)
    {
        return Err((
            words.too_long,
            format!(
                "A {} is at most {} characters. Shorten \"{long}\" and try again.",
                words.one, words.max_chars
            ),
        ));
    }
    if kept.len() > words.max {
        return Err((
            words.too_many,
            format!(
                "A part takes at most {} {}, and this gave it {}. Remove some and try again.",
                words.max,
                words.many,
                kept.len()
            ),
        ));
    }
    Ok(kept)
}

fn saved(result: Result<bool, DbError>, failed: &'static str) -> Response {
    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchPart",
            "There is no model with that id. It may have been deleted — reload the grid and try \
             again.",
        ),
        Err(err) => {
            tracing::error!(error = %err, "{failed}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "message": err.client_message() })),
            )
                .into_response()
        }
    }
}

fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message, "reason": reason })),
    )
        .into_response()
}
