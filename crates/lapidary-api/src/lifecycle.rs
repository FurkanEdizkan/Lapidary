//! Removing things from a library, in steps that can be undone.
//!
//! `CLAUDE.md` is not negotiable here: *"We never delete user data implicitly. Delete is
//! soft. Purge is separate and explicit. Blobs quarantine 30 days before removal."* So
//! `DELETE` on a part is not a deletion. It sets a column. Nothing on disk changes, no
//! `blob` row is touched, and the part comes back whole from
//! [`restore`](self::restore) — indefinitely, not for a window.
//!
//! That is also why this module is not in `detail.rs`. The two share a URL and nothing
//! else: one answers a question about a part, and these change what the library holds.
//! Purge and the reaper land here beside them.
//!
//! # A deleted part is a `404`, not a `410`
//!
//! Both handlers below answer a part they cannot act on with the same
//! [`no_such_part`](crate::derive) body the rest of the crate uses, which says "exists
//! here, or it has been deleted" without saying which. Telling the two apart would confirm
//! that an id names something real to a caller who cannot see it, and `410 Gone` would
//! confirm it in the status line. The distinction costs nothing to give up: a client that
//! just deleted a part knows it deleted the part.

use crate::AppState;
use crate::derive::{internal_error, no_such_part};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::PartId;
use lapidary_db::PgParts;

/// `DELETE /api/parts/{id}` — step one of three. Hide the part; touch nothing.
///
/// `204` because there is nothing to say: the part is gone from every view, and the
/// caller already knows which one it removed. A second `DELETE` on the same id answers
/// `404`, not `204` — [`PgParts::soft_delete`] matches only a live row, so a repeat is
/// indistinguishable here from an id that never existed, and both are honestly "not
/// there". The alternative — reporting success for a delete that changed nothing — would
/// have a client believe it removed a part it did not.
pub async fn remove(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).soft_delete(part).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_part(),
        Err(err) => internal_error(&err, "soft delete failed"),
    }
}

/// `POST /api/parts/{id}/restore` — the undo, and a person has to ask for it.
///
/// Explicit for the reason delete is: a scan that finds the file again does *not* do this
/// (see [`PgParts::soft_delete`]), because un-deleting implicitly is the same class of
/// surprise as deleting implicitly. Nothing is rebuilt — the revisions, files, derivatives
/// and blobs never moved.
pub async fn restore(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).restore(part).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_part(),
        Err(err) => internal_error(&err, "restore failed"),
    }
}
