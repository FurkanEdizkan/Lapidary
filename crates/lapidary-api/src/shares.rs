//! Sharing (S2a), from this installation's side: `GET` and `POST /api/libraries/{id}/shares`,
//! `GET /api/libraries/{id}/shares/preview?folderId=`, `GET /api/shares` and `DELETE /api/shares/{id}`; and asking
//! first (S4): `GET /api/shares/requests` and `PUT /api/shares/{id}/grants/{device}`.
//!
//! The api decides what is offered; the peer role serves it (`lapidary_peer::shares`), and this crate may not
//! depend on that one. Nothing here is refused for its licences: the warning is counted and shown before
//! anybody confirms, and never blocks (owner's decision, 2026-09-16).

use crate::AppState;
use crate::folders::{internal_error, refused};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::{DeviceId, FolderId, LibraryId, ShareId};
use lapidary_db::{GrantRow, PgFolders, PgShares, ShareRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A category of one library that this installation shares.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SharedCategory {
    pub id: ShareId,
    pub folder_id: FolderId,
    pub name: String,
    pub created_at: Timestamp,
    /// Whether fetching its files needs this installation's grant.
    pub asks_first: bool,
}

/// Somebody who asked for a share's files.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShareRequest {
    pub share_id: ShareId,
    pub share_name: String,
    pub device_id: String,
    /// What they call themselves, as their last hello said.
    pub name: Option<String>,
    /// `asked`, `granted` or `denied`.
    pub state: String,
    pub asked_at: Timestamp,
}

/// `PUT /api/shares/{id}/grants/{device}`'s body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecideGrant {
    pub granted: bool,
}

/// The `POST` body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShareCategory {
    pub folder_id: FolderId,
    /// Ask before anyone fetches its files. Left out, a new share is open and an existing one keeps what it had.
    #[ts(optional)]
    pub asks_first: Option<bool>,
}

/// What sharing a category would offer, counted before anybody confirms.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LicenceWarning {
    #[ts(type = "number")]
    pub parts: i64,
    /// Parts with no licence recorded.
    #[ts(type = "number")]
    pub unrecorded: i64,
    /// Parts whose licence says non-commercial.
    #[ts(type = "number")]
    pub non_commercial: i64,
}

/// Everything this installation shares, as its sharing page lists it.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShareSummary {
    pub id: ShareId,
    pub name: String,
    #[ts(type = "number")]
    pub part_count: i64,
    pub asks_first: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQuery {
    folder_id: FolderId,
}

pub async fn list(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgShares(state.db).list(library).await {
        Ok(rows) => Json(rows.into_iter().map(category).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "share list failed"),
    }
}

pub async fn preview(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Query(query): Query<PreviewQuery>,
) -> Response {
    match PgFolders(state.db.clone())
        .library_of(query.folder_id)
        .await
    {
        Ok(Some(found)) if found == library => {}
        Ok(_) => return no_such_category(),
        Err(err) => return internal_error(&err, "share preview failed"),
    }
    match PgShares(state.db).licences(query.folder_id).await {
        Ok(counts) => Json(LicenceWarning {
            parts: counts.parts,
            unrecorded: counts.unrecorded,
            non_commercial: counts.non_commercial,
        })
        .into_response(),
        Err(err) => internal_error(&err, "share preview failed"),
    }
}

pub async fn share(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    body: Result<Json<ShareCategory>, JsonRejection>,
) -> Response {
    let Ok(Json(ShareCategory {
        folder_id,
        asks_first,
    })) = body
    else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badShare",
            "Sharing needs the category to share, by its id. Choose the category in the tree and share it from there.",
        );
    };
    let shares = PgShares(state.db);
    let mut row = match shares.create(library, folder_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return no_such_category(),
        Err(err) => return internal_error(&err, "share failed"),
    };
    // Said, it is set, on a new share or one already shared; not said, the share keeps what it had.
    if let Some(asks_first) = asks_first.filter(|asks_first| *asks_first != row.asks_first) {
        match shares.set_asks_first(row.id, asks_first).await {
            Ok(_) => row.asks_first = asks_first,
            Err(err) => return internal_error(&err, "share mode failed"),
        }
    }
    Json(category(row)).into_response()
}

pub async fn all(State(state): State<AppState>) -> Response {
    match PgShares(state.db).offered().await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| ShareSummary {
                    id: row.id,
                    name: row.name,
                    part_count: row.part_count,
                    asks_first: row.asks_first,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "share summary failed"),
    }
}

pub async fn stop(State(state): State<AppState>, Path(share): Path<ShareId>) -> Response {
    match PgShares(state.db).remove(share).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "notShared",
            "That category is not shared, so there is nothing to stop. Reload the list of what this installation shares.",
        ),
        Err(err) => internal_error(&err, "stop sharing failed"),
    }
}

/// `GET /api/shares/requests` — everybody who asked for a share's files, whatever was decided, newest first.
pub async fn requests(State(state): State<AppState>) -> Response {
    match PgShares(state.db).requests().await {
        Ok(rows) => Json(rows.into_iter().map(request).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "share requests failed"),
    }
}

/// `PUT /api/shares/{id}/grants/{device}` — grant or deny somebody's request, or change the answer.
pub async fn decide(
    State(state): State<AppState>,
    Path((share, device)): Path<(ShareId, String)>,
    body: Result<Json<DecideGrant>, JsonRejection>,
) -> Response {
    let Ok(Json(DecideGrant { granted })) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badGrant",
            "Say whether the request is granted, as {\"granted\": true} or {\"granted\": false}.",
        );
    };
    let Ok(device) = device.parse::<DeviceId>() else {
        return no_such_request();
    };
    match PgShares(state.db).decide(share, device, granted).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_request(),
        Err(err) => internal_error(&err, "grant failed"),
    }
}

fn no_such_request() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchRequest",
        "Nobody with that device id asked for this share, or it is not shared any more. Reload the list of requests.",
    )
}

fn request(row: GrantRow) -> ShareRequest {
    ShareRequest {
        share_id: row.share,
        share_name: row.share_name,
        device_id: row.device.to_string(),
        name: row.name,
        state: row.state.as_str().to_owned(),
        asked_at: row.asked_at,
    }
}

fn no_such_category() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchCategory",
        "This library has no such category, so it cannot be shared. It may have been deleted: reload the tree and choose it again.",
    )
}

fn category(row: ShareRow) -> SharedCategory {
    SharedCategory {
        id: row.id,
        folder_id: row.folder,
        name: row.name,
        created_at: row.created_at,
        asks_first: row.asks_first,
    }
}
