//! Batch status: what a scan turned into. `api` role only — it reads job rows, touches
//! no source file and invokes no kernel, so it belongs on the open path.
//!
//! The route is scoped under its library rather than being a bare `/api/jobs/{id}`.
//! CLAUDE.md: content addressing is not authorization, and a batch id is no different —
//! it is a uuid a caller might hold from anywhere. Scoping the route makes the
//! reachability check structural instead of a step someone can forget, and
//! `PgJobs::batch_status` filters on `library_id` for that reason rather than for
//! performance.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{BatchId, LibraryId};
use lapidary_db::{DbError, PgJobs};

/// `GET /api/libraries/{library}/jobs/{batch}` — how a scan is going, and how it ended.
pub async fn batch_status(
    State(state): State<AppState>,
    Path((library, batch)): Path<(LibraryId, BatchId)>,
) -> Response {
    match PgJobs(state.db).batch_status(library, batch).await {
        Ok(Some(status)) => Json(status).into_response(),
        Ok(None) => no_such_batch(),
        Err(err) => internal_error(&err),
    }
}

/// No job rows for that batch in that library. Three different situations arrive here and
/// all three are honestly described by "no scan with that id has run in this library":
/// an id that was never issued, an id belonging to another library (which must not be
/// distinguishable from the first — that is the authorization point above), and a scan
/// that enqueued nothing at all. The last one is not a lost batch: `ScanAccepted.queued`
/// already told the client it was zero, and its doc says such a batch has no status
/// resource and must not be polled.
fn no_such_batch() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No scan with that id has run in this library. Check the id, or \
                        start a new scan."
        })),
    )
        .into_response()
}

/// The query itself failed. Same asymmetry `parts::internal_error` keeps: the operator
/// gets the real error through the log, the client gets whatever `client_message` decides
/// is safe to hand back.
fn internal_error(err: &DbError) -> Response {
    tracing::error!(error = %err, "batch status query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
