//! The sign-in wall (A28, owner 2026-10-18): the library's own pictures behind the sign-in form.
//!
//! **The only data a signed-out page shows**, and only pictures: the first picture of a part shared with everyone
//! that its owner has not hidden from the wall, while the admin has the wall on. No part id, name, count or figure goes
//! with them. Each is addressed by its own picture id (A36), which says nothing about the part but when the picture was
//! added. `lapidary_db::PgWall` carries the condition; this module adds nothing to it.

use crate::AppState;
use crate::auth::CurrentUser;
use crate::derive::internal_error;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use lapidary_core::{PartId, PartImageId};
use lapidary_db::{PgSettings, PgWall};
use lapidary_storage::DerivativeStore;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Enough to fill the wall's columns at any width; fewer repeat.
const PICTURES: u32 = 48;
/// A part hidden or unshared a moment ago must not come back from a cache.
const NO_STORE: (header::HeaderName, &str) = (header::CACHE_CONTROL, "no-store");

/// One picture on the wall, and nothing about the part it is of.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WallPicture {
    pub src: String,
    /// `cover` or `contain`, and where to keep when `cover` crops: CSS's `object-fit` and `object-position`.
    pub fit: String,
    pub focus_x: f64,
    pub focus_y: f64,
}

/// A switch: the part's own on the wall, or the whole wall's.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WallSwitch {
    pub on: bool,
}

const SETTING: &str = "sign_in_wall";

/// `GET /api/wall` — signed out: up to 48 pictures in a new random order, each by its address (A36): a page anyone can
/// open carries no picture's bytes. Empty when there is nothing to show.
pub async fn pictures(State(state): State<AppState>) -> Response {
    match PgWall(state.db).pictures(PICTURES).await {
        Ok(rows) => {
            let pictures: Vec<WallPicture> = rows
                .into_iter()
                .map(|row| WallPicture {
                    src: format!("/api/wall/{}", row.image),
                    fit: row.fit,
                    focus_x: row.focus.0,
                    focus_y: row.focus.1,
                })
                .collect();
            ([NO_STORE], Json(pictures)).into_response()
        }
        Err(err) => internal_error(&err, "wall pictures failed"),
    }
}

/// `GET /api/wall/{image}` — signed out: one picture, only while the wall shows it. Anything else is `blob.rs`'s 404,
/// so a guess learns nothing about what is stored.
pub async fn picture(State(state): State<AppState>, Path(image): Path<String>) -> Response {
    let Ok(image) = image.parse::<PartImageId>() else {
        return crate::blob::not_found();
    };
    let bytes = match PgWall(state.db).picture(image).await {
        Ok(Some(lapidary_db::WallBytes {
            inline_webp: Some(bytes),
            ..
        })) => bytes,
        Ok(Some(lapidary_db::WallBytes {
            hash: Some(hash), ..
        })) => match DerivativeStore::open(&state.blob_root).get(&hash) {
            Ok(bytes) => bytes,
            Err(_) => return crate::blob::not_found(),
        },
        Ok(_) => return crate::blob::not_found(),
        Err(err) => return internal_error(&err, "wall picture failed"),
    };
    (
        [
            NO_STORE,
            (header::CONTENT_TYPE, crate::blob::content_type(&bytes)),
        ],
        bytes,
    )
        .into_response()
}

/// `GET /api/parts/{id}/wall` — whether this part may show on the wall. `auth::scope` has checked the caller sees it.
pub async fn part(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgWall(state.db).on_wall(part).await {
        Ok(on) => Json(WallSwitch { on }).into_response(),
        Err(err) => internal_error(&err, "wall switch failed"),
    }
}

/// `PUT /api/parts/{id}/wall` — the owner hides a part from the wall or shows it again. `auth::scope` has checked it
/// is the caller's.
pub async fn set_part(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    body: Result<Json<WallSwitch>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send `{\"on\": true}` or `{\"on\": false}`.");
    };
    match PgWall(state.db).set_on_wall(part, body.on).await {
        Ok(()) => Json(body).into_response(),
        Err(err) => internal_error(&err, "wall switch failed"),
    }
}

/// `GET /api/settings/wall` — whether the sign-in page shows the wall's pictures at all. Missing is on.
pub async fn setting(State(state): State<AppState>) -> Response {
    match PgSettings(state.db).get(SETTING).await {
        Ok(value) => Json(WallSwitch {
            on: value.and_then(|v| v.as_bool()).unwrap_or(true),
        })
        .into_response(),
        Err(err) => internal_error(&err, "wall setting failed"),
    }
}

/// `PUT /api/settings/wall` — admin only: the wall on or off for everybody.
pub async fn set_setting(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<WallSwitch>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "message": "Only an admin can change the sign-in page." })),
        )
            .into_response();
    }
    let Ok(Json(body)) = body else {
        return refused("Send `{\"on\": true}` or `{\"on\": false}`.");
    };
    match PgSettings(state.db)
        .set(SETTING, &serde_json::Value::Bool(body.on))
        .await
    {
        Ok(()) => Json(body).into_response(),
        Err(err) => internal_error(&err, "wall setting failed"),
    }
}

fn refused(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}
