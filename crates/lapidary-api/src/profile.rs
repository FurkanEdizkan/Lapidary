//! `GET`/`PUT /api/profile` — the signed-in person's display name (M2, per person since A1). What the
//! user menu and the Profile page call them.

use crate::AppState;
use crate::auth::CurrentUser;
use crate::derive::internal_error;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use lapidary_db::PgAccounts;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest display name, in characters; the column's check says the same.
const MAX_NAME: usize = 80;

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// `null` until a name is given; an empty or blank one clears it.
    pub display_name: Option<String>,
}

pub async fn get(Extension(user): Extension<CurrentUser>) -> Response {
    Json(Profile {
        display_name: user.display_name,
    })
    .into_response()
}

pub async fn set(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<Profile>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send the name as `{\"displayName\": \"…\"}`, or `null` to clear it.");
    };
    let name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if name.is_some_and(|name| name.chars().count() > MAX_NAME) {
        return refused(&format!(
            "A display name is at most {MAX_NAME} characters. Shorten it and save again."
        ));
    }
    match PgAccounts(state.db).set_display_name(user.id, name).await {
        Ok(()) => Json(Profile {
            display_name: name.map(str::to_owned),
        })
        .into_response(),
        Err(err) => internal_error(&err, "profile save failed"),
    }
}

fn refused(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}
