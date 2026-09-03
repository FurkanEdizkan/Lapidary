//! Ingest: the worker-only scan route.
//!
//! A separate crate from `lapidary-api`, not a module inside it. Task 9 first tried
//! putting `scan.rs` in `lapidary-api` behind `Role::Worker`, on the reasoning that a
//! runtime role check keeps the open path from ever *invoking* the kernel. That is true
//! at the route level, but the open path also must never *link* the kernel — the whole
//! reason `deploy/Containerfile`'s `SERVER_FEATURES` build arg splits the `api` and
//! `worker` images in the first place — and a runtime check cannot express that:
//! `lapidary-api` depending on `lapidary-cad` at all makes the `api` image link it,
//! whether or not `Role::Api` ever mounts the route that uses it. Splitting the crate is
//! what lets `xtask/src/layers.rs`'s `FORBIDDEN_PAIRS` forbid `lapidary-api ->
//! lapidary-cad` again, unconditionally, while this crate depends on it freely.
//!
//! `bin/lapidary-server` links this crate only behind its `mock-kernel` Cargo feature
//! (see that crate's `Cargo.toml` for why that feature now does double duty), and only
//! merges its router into the worker process's router when `LAPIDARY_ROLE=worker` — the
//! `api` image is built without the feature, so it does not link `lapidary-cad`, this
//! crate, or anything reachable through either.
//!
//! No `Role` type here, and (per `docs/ARCHITECTURE.md`) no dependency on `lapidary-api`
//! either: this crate has exactly one router, always fully mounted, and the caller
//! decides whether to include it at all.

mod handler;
mod scan;

pub use handler::IngestHandler;

use axum::Router;
use axum::routing::post;
use lapidary_db::PgPool;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// The read-only mounted ingest directory `scan` walks. Never a hardcoded container
    /// path: tests point it at a `TempDir`, and `deploy/compose.yaml` (Task 12) supplies
    /// the real `/ingest` mount.
    pub ingest_dir: PathBuf,
    /// Root of the blob store. `SourceStore` and `DerivativeStore` both open under it;
    /// this crate is the one place in the workspace allowed to construct the former.
    pub blob_root: PathBuf,
}

/// Build the ingest router. Always mounts `/api/libraries/{id}/scan` — there is no
/// internal role gate to bypass, because mounting this router at all is the caller's
/// decision. `bin/lapidary-server` merges it into the worker process's router only.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/libraries/{id}/scan", post(scan::scan))
        .with_state(state)
}
