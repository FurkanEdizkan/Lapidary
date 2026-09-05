//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

mod blob;
mod derive;
mod error;
mod health;
mod jobs;
mod parts;

pub use error::ApiError;
pub use parts::{PartCard, PartsPage};

use axum::Router;
use axum::routing::{get, post};
use lapidary_db::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Where `DerivativeStore` looks. The same root the worker writes to, and it holds
    /// both halves: source blobs and derivatives share one content-addressed layout. This
    /// crate reaches the source half from the download route and nowhere else — that
    /// route hands a user the exact bytes they asked for, which is not the open path.
    /// Nothing at the type level enforces "nowhere else": the read-only handle the route
    /// uses takes no `WorkerRole` proof, deliberately (spec
    /// `2026-09-05-phase-1-slice-5-browser-design.md` §1.2), so `cargo xtask check-deploy`
    /// is what holds that line, by rejecting any other file that names it.
    pub blob_root: std::path::PathBuf,
}

/// Which process this is. `api` serves the open path and must never mount an ingest
/// route: its image deliberately does not link `lapidary-cad` (enforced by
/// `xtask/src/layers.rs`'s `FORBIDDEN_PAIRS` and `cargo xtask check-deploy`), and both
/// containers run one binary from one router, so anything mounted unconditionally is
/// served by both.
///
/// Ingest itself lives in `lapidary-ingest`, a separate crate `bin/lapidary-server`
/// merges into the worker process's router only under `Role::Worker` — this crate has no
/// route, dependency, or type that reaches a source file or the CAD kernel. That split
/// exists because Task 9 first tried putting the scan handler here, behind `Role`, and
/// found that a runtime role check cannot substitute for the dependency-graph guarantee:
/// `lapidary-api` depending on `lapidary-cad` at all — even for a route `Role::Api` never
/// mounts — makes the `api` container image link the kernel again, which is exactly what
/// `FORBIDDEN_PAIRS` and the `SERVER_FEATURES` image split exist to prevent. See
/// `docs/ARCHITECTURE.md`'s crate graph section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Api,
    Worker,
}

impl Role {
    /// Parses the `LAPIDARY_ROLE` value. Rejects anything but the two known roles rather
    /// than defaulting — a typo in `deploy/compose.yaml` that silently fell back to `api`
    /// would put the ingest route nowhere and be very hard to diagnose from the outside.
    pub fn from_env_str(s: &str) -> Result<Self, ApiError> {
        match s {
            "api" => Ok(Role::Api),
            "worker" => Ok(Role::Worker),
            other => Err(ApiError::UnknownRole {
                got: other.to_owned(),
            }),
        }
    }
}

/// Build the application router. Callers own the listener.
///
/// `role` decides which non-shared routes mount — see `Role`. `/api/healthz` mounts for
/// both roles: it's how each container proves it's alive, regardless of what it serves.
/// `Role::Worker` mounts nothing beyond that shared route here — ingest is
/// `lapidary-ingest`'s router, which `bin/lapidary-server` merges in separately.
pub fn router(state: AppState, role: Role) -> Router {
    let shared = Router::new().route("/api/healthz", get(health::healthz));
    let by_role = match role {
        Role::Api => Router::new()
            .route("/api/libraries/{id}/parts", get(parts::page))
            .route(
                "/api/libraries/{library}/jobs/{batch}",
                get(jobs::batch_status),
            )
            // The library's own settings and the two trigger routes. `Role::Api` out of
            // necessity, not preference: nothing proxies a browser to the worker, so
            // mounting these there would make them unreachable from the UI that exists to
            // call them. See `derive.rs`'s module doc and design section 3.5.
            //
            // Read and write are one `.route` on one path rather than two entries axum
            // would have to be trusted to merge, and they answer the same type.
            .route(
                "/api/libraries/{id}",
                get(derive::get_library).patch(derive::set_library),
            )
            .route("/api/parts/{id}/thumbnail", post(derive::part_thumbnail))
            .route(
                "/api/libraries/{id}/thumbnails",
                post(derive::library_thumbnails),
            )
            // Not in `shared`: the worker has no business serving bytes to anyone, and a
            // route mounted unconditionally is served by both images.
            .route("/api/blob/{blake3}", get(blob::by_hash)),
        Role::Worker => Router::new(),
    };
    shared.merge(by_role).with_state(state)
}
