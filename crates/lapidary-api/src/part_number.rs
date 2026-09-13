//! `PUT /api/parts/{id}/part-number`: the number a person gives a part.
//!
//! Set by request and never read off a file. Ingest writes none, because a number made up from
//! a filename is one nobody gave the part. Search ranks it above every other match
//! (`PgParts::search`), so an empty value clears it instead of storing a blank that matches
//! nothing and still looks set.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::PartId;
use lapidary_db::PgParts;
use serde::Deserialize;

/// Generous for any numbering scheme, and short enough that a pasted paragraph is refused.
const MAX_CHARS: usize = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPartNumber {
    pub part_number: Option<String>,
}

pub async fn set(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<SetPartNumber>,
) -> Response {
    let number = body
        .part_number
        .as_deref()
        .map(str::trim)
        .filter(|number| !number.is_empty());
    if number.is_some_and(|number| number.chars().count() > MAX_CHARS) {
        return refused(
            StatusCode::BAD_REQUEST,
            "partNumberTooLong",
            &format!("A part number is at most {MAX_CHARS} characters. Shorten it and try again."),
        );
    }
    match PgParts(state.db).set_part_number(part, number).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchPart",
            "There is no model with that id. It may have been deleted — reload the grid and try \
             again.",
        ),
        Err(err) => {
            tracing::error!(error = %err, "part number update failed");
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
