//! `PUT /api/parts/{id}/tags`: the tags a person gives a part.
//!
//! Set by request and never read off a file, like a part number. The body is the whole list, so
//! adding a tag and removing one are the same write. Tags are kept as written, capitals included
//! and in the order given; blanks and repeats are dropped rather than refused, because neither is
//! a tag anyone meant to keep.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::PartId;
use lapidary_db::PgParts;
use serde::Deserialize;
use ts_rs::TS;

/// Enough to describe a part several ways, and few enough to read at a glance.
const MAX_TAGS: usize = 32;
/// A word or a short phrase. A pasted sentence is refused.
const MAX_CHARS: usize = 64;

/// The `PUT` body: every tag the part has afterwards.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetTags {
    pub tags: Vec<String>,
}

pub async fn set(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<SetTags>,
) -> Response {
    let mut tags: Vec<String> = Vec::new();
    for tag in body.tags.iter().map(|tag| tag.trim()) {
        if !tag.is_empty() && !tags.iter().any(|kept| kept == tag) {
            tags.push(tag.to_owned());
        }
    }
    if let Some(long) = tags.iter().find(|tag| tag.chars().count() > MAX_CHARS) {
        return refused(
            StatusCode::BAD_REQUEST,
            "tagTooLong",
            &format!("A tag is at most {MAX_CHARS} characters. Shorten \"{long}\" and try again."),
        );
    }
    if tags.len() > MAX_TAGS {
        return refused(
            StatusCode::BAD_REQUEST,
            "tooManyTags",
            &format!(
                "A part takes at most {MAX_TAGS} tags, and this gave it {}. Remove some and try \
                 again.",
                tags.len()
            ),
        );
    }
    match PgParts(state.db).set_tags(part, &tags).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchPart",
            "There is no model with that id. It may have been deleted — reload the grid and try \
             again.",
        ),
        Err(err) => {
            tracing::error!(error = %err, "tag update failed");
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
