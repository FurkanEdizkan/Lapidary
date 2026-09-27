//! `PUT /api/parts/{id}/tags`, and `PUT`/`DELETE` on `/api/parts/{id}/materials`: the tags and the
//! materials a person gives a part. And the two reads that make a library's tags a place to browse
//! rather than only a filter — `GET /api/libraries/{id}/tags` and `GET /api/libraries/{id}/tags/related`
//! (`docs/goals/P3.md`).
//!
//! Each is set by request as the whole list, so adding one and removing one are the same write. They
//! are kept as written, capitals included and in the order given; blanks and repeats are dropped
//! rather than refused, because neither is one anyone meant to keep. Tags are never read off a file,
//! like a part number. Materials are also what a CAD file states, until a person types them
//! (`PgParts::set_materials`) — including typing none, which is why materials have a `DELETE` and
//! tags do not.

use crate::AppState;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{LibraryId, PartId};
use lapidary_db::{DbError, PgParts, PgTags};
use serde::{Deserialize, Serialize};
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

/// One tag, and how many of a library's live parts carry it.
///
/// Not [`crate::parts::FacetValue`], whose `count` is nullable: the facet panel withholds counts past
/// `EXACT_FACET_ROWS`, and these two reads never do, because the count is what the index is ordered by.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TagCount {
    pub value: String,
    /// Live parts carrying it, always exact. `u64` would export as `bigint`.
    #[ts(type = "number")]
    pub count: u64,
}

/// `GET /api/libraries/{id}/tags` — every tag in the library, most-carried first then alphabetically.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TagIndex {
    pub tags: Vec<TagCount>,
}

/// `GET /api/libraries/{id}/tags/related?tag=` — one tag, and the tags it shares parts with.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelatedTags {
    /// The tag asked about, as it was asked for.
    pub tag: String,
    /// Live parts carrying it. `0` means no live part does: the tag is gone, not merely lonely.
    #[ts(type = "number")]
    pub parts: u64,
    /// The fewest parts a tag must share to be listed below. On the wire so the sentence naming it
    /// cannot drift from the rule.
    #[ts(type = "number")]
    pub floor: u64,
    /// Other tags on those same parts, most shared first then alphabetically. Each `count` is the
    /// number of parts shared with `tag`, not the tag's own total.
    pub related: Vec<TagCount>,
}

/// A tag sharing one part with another is a coincidence, not a relation: two is the fewest that says
/// anything. Stated to the reader through [`RelatedTags::floor`].
const RELATED_FLOOR: i64 = 2;

/// Enough related tags to suggest somewhere to go next, few enough to read without scrolling a panel.
const RELATED_LIMIT: i64 = 24;

/// The tag `related` is about. A query parameter and not a path segment: a tag may hold a slash, and a
/// slash inside a path segment is a fight with every proxy between the browser and here.
#[derive(Debug, Deserialize)]
pub struct RelatedQuery {
    pub tag: String,
}

/// The two reads that make a library's tags a place. Their own router, so this goal adds one line to
/// the chain in `lib.rs` rather than two — `likeness::routes()` and `dashboard::routes()` are there
/// for the same reason. The part writes above stay in that chain: they were already in it.
pub(crate) fn reads() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/libraries/{id}/tags", axum::routing::get(index))
        .route(
            "/api/libraries/{id}/tags/related",
            axum::routing::get(related),
        )
}

/// `GET /api/libraries/{id}/tags` — the whole library's tags, with exact counts.
///
/// A library that does not exist answers an empty list, as the facets read does: an empty library and
/// an id naming nothing look alike to somebody browsing, and no count here is a number to believe.
pub async fn index(State(app): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgTags(app.db).index(library).await {
        Ok(rows) => Json(TagIndex {
            tags: counted(rows),
        })
        .into_response(),
        Err(err) => failed(&err, "tag index query failed"),
    }
}

/// `GET /api/libraries/{id}/tags/related?tag=` — how many parts carry the tag, and what else is on them.
pub async fn related(
    State(app): State<AppState>,
    Path(library): Path<LibraryId>,
    query: Result<Query<RelatedQuery>, QueryRejection>,
) -> Response {
    // A rejection here is the parameter missing or unreadable, which is the same refusal as a blank
    // one: there is no tag to be about. `Result` rather than `Option` so the shape matches `facets`.
    let asked = query.map(|Query(query)| query.tag).unwrap_or_default();
    let tag = asked.trim();
    if tag.is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "noTag",
            "This needs a tag to be about. Add `?tag=` and try again.",
        );
    }
    match PgTags(app.db)
        .related(library, tag, RELATED_FLOOR, RELATED_LIMIT)
        .await
    {
        Ok((parts, rows)) => Json(RelatedTags {
            tag: tag.to_owned(),
            parts: u64::try_from(parts).unwrap_or(0),
            floor: u64::try_from(RELATED_FLOOR).unwrap_or(0),
            related: counted(rows),
        })
        .into_response(),
        Err(err) => failed(&err, "related tags query failed"),
    }
}

/// `(value, count)` rows as the wire carries them. A negative count is impossible from `count()`, so
/// the conversion cannot fail; `0` rather than a panic is what a refusal would cost a whole page.
fn counted(rows: Vec<(String, i64)>) -> Vec<TagCount> {
    rows.into_iter()
        .map(|(value, count)| TagCount {
            value,
            count: u64::try_from(count).unwrap_or(0),
        })
        .collect()
}

fn failed(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
