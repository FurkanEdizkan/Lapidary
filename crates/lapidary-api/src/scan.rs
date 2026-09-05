//! Starting a scan from the browser.
//!
//! `Role::Api` out of necessity, not preference, and for exactly the reason `derive.rs`'s
//! module doc gives for the three trigger routes beside it: `deploy/web/Caddyfile`
//! proxies `/api/*` to `api:8080` and `web/vite.config.ts` does the same in development,
//! so there is no route from a browser to the worker at all. A scan route mounted under
//! `Role::Worker` is a scan button that cannot work.
//!
//! This route walks nothing. It writes one `scan_directory` job row, and the worker —
//! which is the process that mounts `/ingest` and links the CAD kernel — does the walk
//! and enqueues the per-file jobs into the same batch. That is what lets a scan start
//! here without `lapidary-api` growing a mount it has no business holding, or a
//! dependency `xtask check-layers` forbids. See `lapidary_ingest::scan`'s module doc for
//! why the walk moved into a job, and what had to be true first.
//!
//! `lapidary-ingest` keeps its own `POST /scan` on the worker's `:8081`, enqueueing the
//! same job kind, so `README.md`'s first-run `curl` still works and there is one walk
//! implementation rather than two.

use crate::AppState;
use crate::derive::{accept, internal_error, no_such_library};
use axum::extract::{Path, State};
use axum::response::Response;
use lapidary_core::{JobPayload, LibraryId};
use lapidary_db::PgParts;

/// `POST /api/libraries/{id}/scan` — walk the worker's ingest directory into this
/// library.
///
/// Answers `202` with `ScanAccepted { queued: 1 }`: the one `scan_directory` job. The
/// batch's `total` then climbs past 1 as the walk enqueues what it found, which is the
/// whole reason the walk uses `enqueue_into` rather than minting a batch of its own.
///
/// A library that does not exist is a `404`, as it is on the thumbnail sweep next door
/// and for the same reason: the walk would otherwise run, find files, enqueue them, and
/// fail every one of them against a library id that names nothing — a hundred and fifty
/// identical failures for one mistyped id. The existence probe is `auto_thumbnail`'s
/// `None`, which is the one question this crate already knows how to ask of a `library`
/// row.
pub async fn scan(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgParts(state.db.clone()).auto_thumbnail(library).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_library(),
        Err(err) => return internal_error(&err, "library lookup failed"),
    }
    accept(state.db, library, &[JobPayload::ScanDirectory]).await
}
