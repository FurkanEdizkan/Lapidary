//! Moving a model between categories.
//!
//! **A database change and nothing else** (M4). A category is the library's own grouping: the
//! file stays exactly where it is in the owner's folders, the inbox or the store, and no
//! directory is created or renamed. So there is no ordering to get right between a rename and a
//! commit any more, and no part that cannot be moved because its bytes are somewhere old.
//!
//! What a move does **not** touch is as load-bearing as what it does. `part.source_path` is
//! identity: where the file sits, immutable, and the reason a re-scan after a move recognises the
//! same file and leaves it in the category the user put it in.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::{FolderId, PartId};
use lapidary_db::{DbError, PgFolders, PgParts};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MovePart {
    /// The category to file it under, or `null` for the library root. `null` is a real
    /// target and not the absence of one.
    pub folder_id: Option<FolderId>,
    /// The client has seen the collision warning and wants it anyway. Slice 6a decided two
    /// parts called `bracket` are the truth; this is how the UI says it knows.
    #[serde(default)]
    pub acknowledge_duplicate: bool,
}

/// One row of `GET /api/parts/{id}/moves`. Serialize-only and no ts-rs export: nothing on
/// the web side reads a history yet, and an exported type with no consumer is a file the
/// bindings gate then polices for nobody.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRecord {
    pub from_folder: Option<FolderId>,
    pub to_folder: Option<FolderId>,
    pub moved_at: Timestamp,
}

/// `PATCH /api/parts/{id}` — file a model under a category, or under none.
///
/// `200` when it moved, `404` when there is no such live part, and `409` for the two things
/// that are answers rather than failures: the target is in another library, or a model of the
/// same name is already there. They share a status and are told apart by `reason` in the body.
pub async fn move_part(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<MovePart>,
) -> Response {
    let parts = PgParts(state.db.clone());

    let source = match parts.move_source(part).await {
        Ok(Some(source)) => source,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "move source lookup failed"),
    };

    // Same folder, nothing to do, and nothing written to the move trail.
    if source.folder == body.folder_id {
        return StatusCode::OK.into_response();
    }

    if let Some(target) = body.folder_id {
        match PgFolders(state.db.clone()).library_of(target).await {
            Ok(Some(owner)) if owner == source.library => {}
            // Another of this person's libraries is an answer; somebody else's is a folder that is not there (A38).
            Ok(Some(other)) => match (
                parts.library_owner(other).await,
                parts.library_owner(source.library).await,
            ) {
                (Ok(theirs), Ok(ours)) if theirs == ours => return other_library(),
                (Ok(_), Ok(_)) => return no_such_folder(),
                (Err(err), _) | (_, Err(err)) => {
                    return internal_error(&err, "move target lookup failed");
                }
            },
            Ok(None) => return no_such_folder(),
            Err(err) => return internal_error(&err, "move target lookup failed"),
        }
    }

    if !body.acknowledge_duplicate {
        match parts
            .name_taken_in_folder(source.library, body.folder_id, &source.name, part)
            .await
        {
            Ok(true) => return duplicate(&source.name),
            Ok(false) => {}
            Err(err) => return internal_error(&err, "duplicate name check failed"),
        }
    }

    match parts
        .move_to_folder(part, source.folder, body.folder_id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => internal_error(&err, "part move failed"),
    }
}

/// `GET /api/parts/{id}/moves` — where this model has been filed, newest first.
///
/// A part that has never moved and a part id that names nothing both answer `[]`. That is
/// deliberate rather than lazy about the 404: nothing acts differently on the two, and the
/// second query it would take to tell them apart would be paid by every read of a history
/// that is empty for the honest reason.
pub async fn history(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).moves(part).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| MoveRecord {
                    from_folder: row.from_folder,
                    to_folder: row.to_folder,
                    moved_at: row.moved_at,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "move history query failed"),
    }
}

/// A refusal with a machine-readable `reason` beside its prose.
///
/// `409` has two distinct causes on this route and the status alone cannot tell them apart,
/// so the client would otherwise be reading the message text — which is written for a
/// person, and gets rewritten — to decide whether to offer "move anyway".
fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message, "reason": reason })),
    )
        .into_response()
}

fn no_such_part() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchPart",
        "There is no model with that id. It may have been deleted — reload the grid and try \
         again.",
    )
}

fn no_such_folder() -> Response {
    refused(
        StatusCode::CONFLICT,
        "noSuchFolder",
        "There is no category with that id, so there is nowhere to file this model. Reload \
         the category tree and try again.",
    )
}

fn other_library() -> Response {
    refused(
        StatusCode::CONFLICT,
        "crossLibrary",
        "That category is in a different library. A model can only be filed under a \
         category in its own library — pick one from this library's tree.",
    )
}

/// Named, because the plan says name it: a warning that will not say which model it is
/// about is a warning the user has to go and check for themselves.
fn duplicate(name: &str) -> Response {
    refused(
        StatusCode::CONFLICT,
        "duplicateName",
        &format!(
            "A model called `{name}` is already in that category. Two models can share a \
             name — they are told apart by where they came from — so send the same move \
             again with acknowledgeDuplicate set if that is what you meant."
        ),
    )
}

/// Same shape and reasoning as `parts.rs`'s: the operator gets the real error through the
/// log, the caller gets whatever `client_message` has decided is safe to show.
fn internal_error(err: &DbError, what: &'static str) -> Response {
    crate::error::log_db_error(err, what);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
