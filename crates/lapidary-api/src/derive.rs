//! The libraries, and asking the worker to write an export.
//!
//! **What R3 took out of here was most of the file**: the ingest-time thumbnail setting
//! (`GET`/`PATCH /api/libraries/{id}`, with its `LibrarySettings` type and the
//! `auto_thumbnail` column), `POST /api/parts/{id}/thumbnail`, the whole-library sweep
//! `POST /api/libraries/{id}/thumbnails`, and `POST /api/parts/{id}/rungs/{level}` — which
//! answered `202` with a batch id for work that would never land, so the viewer polled it
//! for ever. `POST /api/parts/{id}/exports/{format}` is what is left: the two exports are the
//! only thing a worker still builds on demand.
//!
//! These are on `Role::Api`, and that is necessity rather than preference (design
//! §3.5). `deploy/web/Caddyfile` proxies `/api/*` to `api:8080` and nothing else, and
//! `web/vite.config.ts` does the same in development — there is no route from a browser
//! to the worker at all. A trigger route mounted under `Role::Worker` would be
//! unreachable from the UI it exists for. `scan.rs` is the other route to reach that
//! conclusion, and the one that had to change the shape of a scan to act on it.
//!
//! Enqueueing is a database write, so none of this needs the kernel, `ingest_dir` or a
//! source file: the writing still happens in the worker, where `lapidary-cad` is linked
//! and `xtask check-layers` keeps it. These handlers only write `job` rows.

use crate::AppState;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{BlobHash, DerivativeKind, JobPayload, LibraryId, PartId, ScanAccepted};
use lapidary_db::{DbError, PgJobs, PgParts, PgPool};
use lapidary_targets::Format;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One library in the switcher.
///
/// `partCount` rides along so the control can say which library has anything in it without
/// a request per row — the same reason `FolderNode` carries one.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummary {
    pub id: LibraryId,
    pub name: String,
    #[ts(type = "number")]
    pub part_count: i64,
}

/// `GET /api/libraries` — every library, oldest first.
///
/// No pagination. A deployment has a handful of libraries, not a page of them, and a
/// switcher that paged would be a control nobody could scan.
pub async fn list_libraries(State(state): State<AppState>) -> Response {
    match PgParts(state.db).libraries().await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| LibrarySummary {
                    id: row.id,
                    name: row.name,
                    part_count: row.part_count,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "library list query failed"),
    }
}

/// What `POST /api/libraries` takes.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewLibrary {
    pub name: String,
}

/// `POST /api/libraries` — make one.
///
/// The route `0002_parts.sql` said would arrive: *"Whichever slice adds a second library
/// replaces this seed rather than building beside it."* The seed stays — it is the library
/// an existing deployment has been using, and deleting it would take their models with it —
/// but it is no longer the only one there can be.
pub async fn create_library(
    State(state): State<AppState>,
    body: Result<Json<NewLibrary>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badBody",
            "A library needs a name.",
        );
    };
    let name = body.name.trim();
    if name.is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "emptyName",
            "A library needs a name. Type one and try again.",
        );
    }

    match PgParts(state.db).create_library(name).await {
        Ok(id) => (
            StatusCode::CREATED,
            Json(LibrarySummary {
                id,
                name: name.to_owned(),
                // Brand new, so empty. Stated rather than re-queried.
                part_count: 0,
            }),
        )
            .into_response(),
        Err(err @ DbError::LibraryNameTaken { .. }) => {
            refused(StatusCode::CONFLICT, "nameTaken", &err.to_string())
        }
        // The pair that looks different on screen and is not on disk. Its own reason, not
        // folded into `nameTaken`: a client told "that name is taken" about a name nothing
        // on screen uses would be told something it can check and find false.
        Err(err @ DbError::LibrarySlugTaken { .. }) => {
            refused(StatusCode::CONFLICT, "slugTaken", &err.to_string())
        }
        Err(err) => internal_error(&err, "library create failed"),
    }
}

/// A refusal that names itself, the shape `folders.rs` already uses so a client reads both
/// the same way.
fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "reason": reason, "message": message })),
    )
        .into_response()
}

/// One batch, `202`, and the id to poll it with. Shared by every enqueue route in this
/// crate — the two thumbnail routes here and `scan.rs` — so they cannot drift into
/// answering differently.
/// What an export request answers when that file already exists: its hash, ready for
/// `GET /api/blob/{blake3}`.
///
/// Was `RungReady`, and the rung route it was named for is gone (R3).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExportReady {
    pub hash: BlobHash,
}

/// `POST /api/parts/{id}/exports/{format}` — ask for a part's mesh written as a 3MF or an STL,
/// the formats a slicer reads.
///
/// An export that already exists answers `200` with its hash, so asking again after it has
/// landed never queues a second build. The answer otherwise is the scan's `202`, so the
/// existing batch poll says when the file is ready; asking while a build is still queued
/// answers with that build's batch rather than queuing another. The file downloads from
/// `GET /api/revisions/{id}/download?variant={format}`, named `*.lapidary.{format}`.
pub async fn part_export(
    State(state): State<AppState>,
    Path((part, format)): Path<(PartId, String)>,
) -> Response {
    let Some(kind) = Format::named(&format).and_then(Format::export) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "message": format!(
                    "`{format}` is not a format this route exports. Ask for `3mf` or `stl`; a \
                     part's own format downloads as its original file."
                )
            })),
        )
            .into_response();
    };
    build(state, part, kind).await
}

/// A derivative of the part's newest revision: its hash when it exists, else the batch building it.
async fn build(state: AppState, part: PartId, kind: DerivativeKind) -> Response {
    let parts = PgParts(state.db.clone());
    let library = match parts.library_of(part).await {
        Ok(Some(library)) => library,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "part lookup failed"),
    };
    let revision = match parts.latest_revision(part).await {
        Ok(Some(revision)) => revision,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "revision lookup failed"),
    };
    match parts.derivative_hash(revision, kind).await {
        Ok(Some(hash)) => Json(ExportReady { hash }).into_response(),
        Ok(None) => match PgJobs(state.db)
            .enqueue_derive_if_absent(library, revision, kind)
            .await
        {
            // `queued: 1` whichever call queued it: one job is building this file, and its
            // batch is the one to poll.
            Ok((batch_id, _)) => (
                StatusCode::ACCEPTED,
                Json(ScanAccepted {
                    batch_id,
                    queued: 1,
                }),
            )
                .into_response(),
            Err(err) => internal_error(&err, "enqueue failed"),
        },
        Err(err) => internal_error(&err, "derivative lookup failed"),
    }
}

pub(crate) async fn accept(db: PgPool, library: LibraryId, jobs: &[JobPayload]) -> Response {
    match PgJobs(db).enqueue(library, jobs).await {
        Ok((batch_id, queued)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted { batch_id, queued }),
        )
            .into_response(),
        Err(err) => internal_error(&err, "enqueue failed"),
    }
}

/// "Nothing was changed": a scan that queued no walk, for a library that is not there.
pub(crate) fn no_such_library() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No library with that id exists, so nothing was changed. Check the \
                        id against the library list."
        })),
    )
        .into_response()
}

/// A part id that resolves to nothing. A deleted part answers this too, and honestly: it
/// is gone from every view the grid offers, and re-rendering it would produce a preview
/// nothing displays.
pub(crate) fn no_such_part() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No part with that id exists here, or it has been deleted. Reload \
                        the grid and try the part again."
        })),
    )
        .into_response()
}

/// The query itself failed. Same asymmetry `jobs::internal_error` keeps: the operator gets
/// the real error through the log, the client gets whatever `client_message` decides is
/// safe to hand back.
pub(crate) fn internal_error(err: &DbError, what: &'static str) -> Response {
    crate::error::log_db_error(err, what);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
