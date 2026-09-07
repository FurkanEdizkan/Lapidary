//! Asking for a derivative without a terminal: the library's ingest-time thumbnail
//! setting, one part's thumbnail, and a whole library's missing ones.
//!
//! All four are on `Role::Api`, and that is necessity rather than preference (design
//! §3.5). `deploy/web/Caddyfile` proxies `/api/*` to `api:8080` and nothing else, and
//! `web/vite.config.ts` does the same in development — there is no route from a browser
//! to the worker at all. A trigger route mounted under `Role::Worker` would be
//! unreachable from the UI it exists for. `scan.rs` is the fourth route to reach that
//! conclusion, and the one that had to change the shape of a scan to act on it.
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
use lapidary_core::{DerivativeKind, JobPayload, LibraryId, LibraryMode, PartId, ScanAccepted};
use lapidary_db::{DbError, PgJobs, PgParts, PgPool};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The `PATCH` body, and the `200` echo of it.
///
/// Exported now that `web/` sends it. It was deliberately not `#[ts(export)]` while no
/// caller existed — an exported type nobody imports is a binding to keep in step for no
/// one — and the grid's action bar is that caller, so the reason has expired. Hand-writing
/// `{ autoThumbnail: boolean }` in `web/src/lib/api.ts` instead would be a shape that
/// keeps compiling after this field is renamed, which is the one thing `web/src/lib/types.ts`
/// exists to make impossible.
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibrarySettings {
    auto_thumbnail: bool,
}

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
    /// `hobby` or `controlled`. **Nothing reads it yet** — governance is Phase 8 — and it is
    /// on the wire because a switcher that shows which libraries are controlled is the point
    /// at which the column stops being decorative. Until then it is a label.
    pub mode: LibraryMode,
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
                    // Parsed rather than passed through: an unknown value in that column is
                    // a row this application does not understand, and answering it as though
                    // it were `hobby` would be quietly deciding it is one.
                    mode: match row.mode.as_str() {
                        "controlled" => LibraryMode::Controlled,
                        _ => LibraryMode::Hobby,
                    },
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
    /// Chosen at creation and not asked about later, because later means asking about a
    /// library somebody has already filled. Defaults to `hobby`, which is what a library
    /// with no governance is.
    #[serde(default)]
    pub mode: LibraryMode,
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
            "A library needs a name, and `mode` must be `hobby` or `controlled` if you send it.",
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

    match PgParts(state.db)
        .create_library(name, body.mode.as_str())
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(LibrarySummary {
                id,
                name: name.to_owned(),
                mode: body.mode,
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

/// `GET /api/libraries/{id}` — what `PATCH` on the same id would change.
///
/// Answers the very type `PATCH` takes and echoes, so a client that reads a setting and
/// one that writes it cannot disagree about the shape by construction rather than by
/// coincidence — and so no second binding exists for the frontend to keep in step.
///
/// The read is `PgParts::auto_thumbnail`, the same one the sweep already probes existence
/// with. A query written for this route would answer the same question twice and be the
/// thing that drifts.
///
/// A library that does not exist is a `404`, as it is on `PATCH` and on the sweep: a
/// documented default returned for an id that names nothing is exactly the plausible
/// answer that gets believed.
pub async fn get_library(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
) -> Response {
    match PgParts(state.db).auto_thumbnail(library).await {
        Ok(Some(auto_thumbnail)) => Json(LibrarySettings { auto_thumbnail }).into_response(),
        Ok(None) => no_such_library_read(),
        Err(err) => internal_error(&err, "library lookup failed"),
    }
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
        // client sees what it now holds without a second request. Serialized from the
        // request type itself, not a hand-built object, so the echo cannot drift from the
        // shape the exported binding promises.
        Ok(true) => Json(settings).into_response(),
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

/// One batch, `202`, and the id to poll it with. Shared by every enqueue route in this
/// crate — the two thumbnail routes here and `scan.rs` — so they cannot drift into
/// answering differently.
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

/// Shared by `PATCH`, the sweep and `scan.rs`. "Nothing was changed" is true of all three
/// — an update that matched no row, a sweep that enqueued nothing, and a scan that queued
/// no walk — and a second message saying the same thing differently would be a string to
/// keep in step for no reason.
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

/// The read half's 404. Deliberately not [`no_such_library`]: "nothing was changed" is
/// reassurance a writer needs and an answer a reader did not ask for, and telling someone
/// who asked a question that their edit did not land is its own small confusion.
fn no_such_library_read() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No library with that id exists, so it has no settings to show. \
                        Check the id against the library list."
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
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
