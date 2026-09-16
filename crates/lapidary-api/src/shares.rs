//! Sharing (S2a), from this installation's side: `GET` and `POST /api/libraries/{id}/shares`,
//! `GET /api/libraries/{id}/shares/preview?folderId=`, `GET /api/shares` and `DELETE /api/shares/{id}`.
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
use lapidary_core::{FolderId, LibraryId, ShareId};
use lapidary_db::{PgFolders, PgShares, ShareRow};
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
}

/// The `POST` body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShareCategory {
    pub folder_id: FolderId,
}

/// What sharing a category would offer, counted before anybody confirms.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LicenceWarning {
    pub parts: i64,
    /// Parts with no licence recorded.
    pub unrecorded: i64,
    /// Parts whose licence says non-commercial.
    pub non_commercial: i64,
}

/// Everything this installation shares, as its sharing page lists it.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShareSummary {
    pub id: ShareId,
    pub name: String,
    pub part_count: i64,
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
    let Ok(Json(ShareCategory { folder_id })) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badShare",
            "Sharing needs the category to share, by its id. Choose the category in the tree and share it from there.",
        );
    };
    match PgShares(state.db).create(library, folder_id).await {
        Ok(Some(row)) => Json(category(row)).into_response(),
        Ok(None) => no_such_category(),
        Err(err) => internal_error(&err, "share failed"),
    }
}

pub async fn all(State(state): State<AppState>) -> Response {
    match PgShares(state.db).offered().await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| ShareSummary {
                    id: row.id,
                    name: row.name,
                    part_count: row.part_count,
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
    }
}
