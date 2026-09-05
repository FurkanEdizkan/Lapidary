//! Asking for a derivative without a terminal: the library's ingest-time thumbnail
//! setting, one part's thumbnail, and a whole library's missing ones.
//!
//! All three are on `Role::Api`, and that is necessity rather than preference (design
//! §3.5). `deploy/web/Caddyfile` proxies `/api/*` to `api:8080` and nothing else, and
//! `web/vite.config.ts` does the same in development — there is no route from a browser
//! to the worker at all, which is why the existing `POST /scan` is documented as a `curl`
//! from the host. A trigger route mounted under `Role::Worker` would be unreachable from
//! the UI it exists for.
//!
//! Enqueueing is a database write, so none of this needs the kernel, `ingest_dir` or a
//! source file: the rendering still happens in the worker, where `lapidary-cad` is linked
//! and `xtask check-layers` keeps it. These handlers only write `job` rows.
//!
//! Every enqueue answers `202` with [`ScanAccepted`] — the same shape the scan route
//! returns, deliberately not a second one — so `GET /api/libraries/{lib}/jobs/{batch}`
//! polls a thumbnail sweep with no change at all.

use crate::AppState;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{DerivativeKind, JobPayload, LibraryId, PartId, ScanAccepted};
use lapidary_db::{DbError, PgJobs, PgParts, PgPool};
use serde::Deserialize;

/// The `PATCH` body. Deliberately not `#[ts(export)]`: nothing in `web/` sends it yet,
/// and an exported type no caller has is a binding to keep in step for no one.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySettings {
    auto_thumbnail: bool,
}

/// `PATCH /api/libraries/{id}` — whether ingest renders a thumbnail for this library.
///
/// Turning it off is not a data-loss action and reads as none: existing thumbnails stay,
/// and `POST /api/libraries/{id}/thumbnails` renders the ones a later ingest skips.
pub async fn set_library(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    body: Result<Json<LibrarySettings>, JsonRejection>,
) -> Response {
    let settings = match body {
        Ok(Json(settings)) => settings,
        Err(rejection) => return bad_body(&rejection),
    };
    match PgParts(state.db)
        .set_auto_thumbnail(library, settings.auto_thumbnail)
        .await
    {
        // Echoes the value that landed rather than answering with an empty 200, so a
        // client sees what it now holds without a second request.
        Ok(true) => {
            Json(serde_json::json!({ "autoThumbnail": settings.auto_thumbnail })).into_response()
        }
        Ok(false) => no_such_library(),
        Err(err) => internal_error(&err, "library setting update failed"),
    }
}

/// `POST /api/parts/{id}/thumbnail` — render this part's preview now. A batch of one, so
/// the client polls it exactly as it polls a scan.
///
/// The library comes off the part, never from the caller: see `PgParts::library_of`. The
/// revision comes from `PgParts::latest_revision`, which is the same resolution the
/// grid's own LATERAL performs — a second, differently-ordered resolve here would render
/// a picture of a revision nobody is looking at and report success doing it. The payload
/// then names that revision rather than the part, so a revision landing while the job
/// queues cannot redirect it (§3.7).
pub async fn part_thumbnail(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    let parts = PgParts(state.db.clone());
    let library = match parts.library_of(part).await {
        Ok(Some(library)) => library,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "part lookup failed"),
    };
    // A part with no revision has no bytes to re-read, and answers exactly as a part that
    // does not exist does. Nothing in this phase writes such a row, but the schema permits
    // one and a third status code for it would be a distinction without a difference.
    let revision = match parts.latest_revision(part).await {
        Ok(Some(revision)) => revision,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "revision lookup failed"),
    };
    accept(
        state.db,
        library,
        &[JobPayload::Derive {
            revision,
            produce: DerivativeKind::Thumbnail,
        }],
    )
    .await
}

/// `POST /api/libraries/{id}/thumbnails` — render every preview this library is missing.
///
/// `queued: 0` is a success and not an error: a library whose parts all have a thumbnail
/// is the normal state, and `ScanAccepted`'s doc already tells the client such a batch
/// has no status resource and must not be polled.
///
/// A library that does not exist is a `404`, exactly as `PATCH` on the same id is. The
/// asymmetry this route used to keep was defended as non-disclosure — not telling a caller
/// whether another tenant's library id exists — and that property is not obtained: `PATCH`
/// discloses the same fact about the same id, so an enumerator would simply use `PATCH`.
/// What the asymmetry did cost was real, because `revisions_missing` finds nothing for a
/// library that does not exist and nothing for one with every preview already rendered:
/// someone who mistyped an id got `202 queued: 0` and read it as "nothing was missing"
/// when they had named nothing at all. Non-disclosure becomes worth having when there is
/// an auth model to hang it on (Phase 5, per `FEATURES.md`), and then it belongs on both
/// routes at once rather than on one.
pub async fn library_thumbnails(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
) -> Response {
    let parts = PgParts(state.db.clone());
    // The existence probe is `auto_thumbnail`'s `None` rather than a query written for it:
    // reading the library's own setting is the one question this crate already knows how
    // to ask of a `library` row, and the sweep needs the answer before `revisions_missing`
    // collapses "does not exist" and "nothing to do" into the same empty vector.
    match parts.auto_thumbnail(library).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_library(),
        Err(err) => return internal_error(&err, "library lookup failed"),
    }
    let missing = match parts
        .revisions_missing(library, DerivativeKind::Thumbnail)
        .await
    {
        Ok(missing) => missing,
        Err(err) => return internal_error(&err, "missing-thumbnail sweep failed"),
    };
    let jobs: Vec<JobPayload> = missing
        .into_iter()
        .map(|revision| JobPayload::Derive {
            revision,
            produce: DerivativeKind::Thumbnail,
        })
        .collect();
    accept(state.db, library, &jobs).await
}

/// One batch, `202`, and the id to poll it with. Shared by both enqueue routes so the two
/// cannot drift into answering differently.
async fn accept(db: PgPool, library: LibraryId, jobs: &[JobPayload]) -> Response {
    match PgJobs(db).enqueue(library, jobs).await {
        Ok((batch_id, queued)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted { batch_id, queued }),
        )
            .into_response(),
        Err(err) => internal_error(&err, "enqueue failed"),
    }
}

/// The body did not parse. axum's own rejection is an unstructured line of text; wrap it
/// the way `parts::bad_query` wraps a bad query string, and say what a valid body is.
fn bad_body(rejection: &JsonRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "Could not read the request body: {rejection}. Send a JSON object with an \
                 `autoThumbnail` boolean, and a `Content-Type: application/json` header."
            )
        })),
    )
        .into_response()
}

/// Shared by `PATCH` and the sweep. "Nothing was changed" is true of both — an update that
/// matched no row, and a sweep that enqueued nothing — and a second message saying the same
/// thing differently would be a string to keep in step for no reason.
fn no_such_library() -> Response {
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
fn no_such_part() -> Response {
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
fn internal_error(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
