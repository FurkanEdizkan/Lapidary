//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

mod error;
mod health;
mod parts;
mod scan;

pub use error::ApiError;

use axum::Router;
use axum::routing::{get, post};
use lapidary_db::PgPool;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// The read-only mounted ingest directory `scan` walks. Only meaningful under
    /// `Role::Worker`, but held here rather than behind an `Option` — both roles build
    /// the same `AppState`, and a field the `api` role simply never reads is a smaller
    /// surface than an `Option` every future worker-only field must unwrap. Never a
    /// hardcoded container path: tests point it at a `TempDir`, and `deploy/compose.yaml`
    /// (Task 12) supplies the real `/ingest` mount.
    pub ingest_dir: PathBuf,
    /// Root of the blob store — `lapidary_storage`'s two content-addressed handles both
    /// open under it. Same reasoning as `ingest_dir` — both roles construct one
    /// `AppState`; only the worker's `scan` handler ever opens the source-bytes handle
    /// there (see that module's doc for which one, and why naming it directly is
    /// confined to that one file).
    pub blob_root: PathBuf,
}

/// Which process this is. `api` serves the open path and must never mount an ingest
/// route. Both containers run one binary from one router, so anything mounted
/// unconditionally is served by both — `role` is what keeps `scan` off the `api` process.
///
/// This crate now depends on `lapidary-cad` unconditionally (Task 9's `scan` handler
/// needs `MeshKernel`), so — unlike before that handler existed — the `api` image does
/// compile that crate's code in. What `api` still never does is *invoke* it: `Role::Api`
/// never mounts `scan::scan`, the one place `MeshKernel` and the source-bytes store are
/// named (see that module's doc). `xtask/src/layers.rs` no longer forbids the
/// `lapidary-api -> lapidary-cad` dependency edge for exactly this reason;
/// `cargo xtask check-deploy` keeps the type that actually reaches a source file out of
/// every file in this crate except `scan.rs` (named directly in that module's doc, and
/// deliberately not spelled out here — this file is not on the exemption list either).
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
pub fn router(state: AppState, role: Role) -> Router {
    let shared = Router::new().route("/api/healthz", get(health::healthz));
    let by_role = match role {
        Role::Api => Router::new().route("/api/libraries/{id}/parts", get(parts::page)),
        Role::Worker => Router::new().route("/api/libraries/{id}/scan", post(scan::scan)),
    };
    shared.merge(by_role).with_state(state)
}
