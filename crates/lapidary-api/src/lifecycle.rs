//! Deleting a part from a library.
//!
//! Delete is delete (owner, 2026-10-17: "we don't want a restore option; if it is deleted it is
//! deleted"). `DELETE` takes the part's rows out of the library at once — revisions, files,
//! pictures, tags, sources — and **never touches the model file on disk**. There is no restore and
//! no purge; the confirmation in the web app is where a person gets to change their mind.
//!
//! That is also why this module is not in `detail.rs`. The two share a URL and nothing
//! else: one answers a question about a part, and this changes what the library holds.
//!
//! # A missing part is a `404`, not a `410`
//!
//! A part this cannot act on gets the same [`no_such_part`](crate::derive) body the rest of the
//! crate uses. `410 Gone` would confirm that an id once named something real to a caller who
//! cannot see it, and a client that just deleted a part knows it deleted the part.

use crate::AppState;
use crate::derive::{internal_error, no_such_part};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::PartId;
use lapidary_db::PgParts;

/// `DELETE /api/parts/{id}` — delete the part, for good. Its model file stays where it is.
///
/// `204` because there is nothing to say: the part is gone, and the caller already knows which
/// one it deleted. A second `DELETE` on the same id answers `404` — there is nothing there —
/// rather than reporting success for a delete that changed nothing.
pub async fn remove(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).delete(part).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_part(),
        Err(err) => internal_error(&err, "delete failed"),
    }
}
