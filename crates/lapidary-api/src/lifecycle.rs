//! Deleting a part from a library.
//!
//! Delete is delete (owner, 2026-10-17: "we don't want a restore option; if it is deleted it is
//! deleted"). `DELETE` takes the part's rows out of the library at once — revisions, files,
//! pictures, tags, sources — and its model file, when nothing else points at it, is **moved** into
//! `_removed/` in the models folder, never deleted (A2; see `retire.rs`). There is no restore in the
//! app; the confirmation is where a person gets to change their mind, and `_removed/` is where a
//! mistake can still be undone by hand.
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
use crate::auth::CurrentUser;
use crate::derive::{internal_error, no_such_part};
use crate::retire::retire;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use lapidary_core::PartId;
use lapidary_db::PgParts;

/// `DELETE /api/parts/{id}` — delete the part, for good, and move its file into `_removed/`.
///
/// `200` with where each file went ([`crate::retire::Retired`]). A second `DELETE` on the same id
/// answers `404` — there is nothing there — rather than reporting success for a delete that changed
/// nothing.
pub async fn remove(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(part): Path<PartId>,
) -> Response {
    match PgParts(state.db.clone()).delete(part).await {
        Ok(Some(files)) => Json(retire(state.roots(), user.username, files).await).into_response(),
        Ok(None) => no_such_part(),
        Err(err) => internal_error(&err, "delete failed"),
    }
}
