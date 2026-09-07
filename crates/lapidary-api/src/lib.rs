//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

mod blob;
mod derive;
mod detail;
mod download;
mod error;
mod folders;
mod health;
mod images;
mod jobs;
mod lifecycle;
mod moves;
mod parts;
mod scan;
mod upload;

pub use detail::PartDetail;
pub use error::ApiError;
pub use folders::FolderNode;
pub use moves::MovePart;
pub use parts::{LibraryStorage, PartCard, PartsPage};
pub use upload::{ChunkAccepted, UploadFile, UploadManifest, UploadPlan};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post, put};
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
    /// Where a partial upload is assembled before it is verified and moved into the blob
    /// store. Never the blob root: everything under that root is content-addressed and
    /// complete, and a half-transferred file is neither.
    ///
    /// The `api` role's alone — the worker mounts nothing here and never reads it. See
    /// `upload.rs`'s module doc for why the staged file, and not a session table, is the
    /// upload's state.
    pub upload_dir: std::path::PathBuf,
    /// Where the storage root is **on the host**, if the deployment has said so.
    ///
    /// This process cannot work it out. `blob_root` above is where the store is mounted
    /// *inside the container* — `/var/lib/lapidary` under `deploy/compose.yaml` — and that
    /// path exists nowhere on the machine the user is sitting at. So the one thing the UI
    /// needs to turn `libraries/default/Rocks/cliff/cliff.stl` into something a person can
    /// paste into a file manager is a fact only the operator has.
    ///
    /// `None` unless `LAPIDARY_HOST_STORAGE_ROOT` is set to an absolute path, and the UI
    /// then shows the store-relative path exactly as it did before. Never a guess: a
    /// confidently wrong absolute path is worse than an honest relative one, which is the
    /// same rule `ShowInFolder` already follows about not joining a path out of slugs.
    pub host_storage_root: Option<String>,
}

/// Which process this is. `api` serves the open path and must never link the CAD kernel:
/// its image deliberately does not link `lapidary-cad` (enforced by
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
            // What that page of cards costs, summed. `Role::Api` with the grid it totals
            // — see `parts.rs`.
            .route("/api/libraries/{id}/storage", get(parts::storage))
            // What the whole store holds, and where it is. Not under `/api/libraries/{id}`
            // because two of its figures belong to no library and its derivative total is
            // deliberately not what adding the libraries up gives.
            .route("/api/storage", get(parts::instance_storage))
            .route(
                "/api/libraries/{library}/jobs/{batch}",
                get(jobs::batch_status),
            )
            // The same status, streamed, so the progress line keeps moving on a hidden
            // tab — which is the one thing a poll cannot do. The route above stays as the
            // client's fallback; see `jobs.rs`.
            .route(
                "/api/libraries/{library}/jobs/{batch}/events",
                get(jobs::batch_events),
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
            // The scan trigger, on `Role::Api` for the same reason the three below it
            // are: nothing proxies a browser to the worker, so a scan button needs a
            // route the api serves. It enqueues a `scan_directory` job and walks
            // nothing — see `scan.rs`.
            .route("/api/libraries/{id}/scan", post(scan::scan))
            // Upload, in three. `Role::Api` for the same reason the scan trigger above
            // is — nothing proxies a browser to the worker — and additionally because
            // this is the process that mounts the blob volume read-write. See
            // `upload.rs`.
            // Both take a manifest of every file in the drop, and axum's default body
            // limit is 2 MB — about 16,000 entries, which a real parts library passes.
            // 8 MiB is roughly 65,000 files, and it is a buffered JSON body inside a
            // container capped at 512 MB, so it is a ceiling rather than an absence of
            // one. A drop past it needs the manifest split, which is a change to make
            // when someone actually has one.
            .route(
                "/api/libraries/{id}/uploads/probe",
                post(upload::probe).layer(DefaultBodyLimit::max(upload::MAX_MANIFEST_BYTES)),
            )
            .route(
                "/api/libraries/{id}/uploads/commit",
                post(upload::commit).layer(DefaultBodyLimit::max(upload::MAX_MANIFEST_BYTES)),
            )
            // Below `probe` and `commit` so those two literal segments win over the
            // `{blake3}` capture. axum's router prefers a static segment over a dynamic
            // one regardless of order, but reading them in this order should not require
            // knowing that.
            // The default 2 MB limit would reject every chunk the client sends. This
            // layer is the *only* size guard on the route — the handler takes the
            // rejection rather than measuring a body it has already buffered — and
            // rewrites its message. See `upload::refuse_chunk`.
            .route(
                "/api/libraries/{id}/uploads/{blake3}",
                put(upload::chunk).layer(DefaultBodyLimit::max(upload::MAX_CHUNK_BYTES)),
            )
            // The page a card links to, and the two steps that take the card away and
            // bring it back. One route entry rather than two: `DELETE` on the thing
            // `GET` returns is the same resource, and giving the removal a verb of its
            // own in the path would invite a second one that forgets to be soft.
            .route(
                "/api/parts/{id}",
                get(detail::detail).delete(lifecycle::remove),
            )
            .route("/api/parts/{id}/restore", post(lifecycle::restore))
            .route("/api/parts/{id}/purge", post(lifecycle::purge))
            .route("/api/parts/{id}/thumbnail", post(derive::part_thumbnail))
            // The category tree and the two ways it changes. `Role::Api` for the reason
            // everything else a browser calls is: nothing proxies a browser to the worker.
            // None of these four reaches a file — see `folders.rs`, including why a
            // renamed category does not rename its directory.
            .route(
                "/api/libraries/{id}/folders",
                get(folders::tree).post(folders::create),
            )
            .route(
                "/api/folders/{id}",
                axum::routing::patch(folders::patch).delete(folders::delete),
            )
            // Moving a model, which is the one route here that does touch the store — a
            // directory rename, no content access. `moves.rs` is the only file in this
            // crate allowed to hold that rename handle, enforced by `cargo xtask
            // check-deploy`'s `RELOCATE_MODULE`.
            .route("/api/parts/{id}", axum::routing::patch(moves::move_part))
            .route("/api/parts/{id}/moves", get(moves::history))
            .route(
                "/api/libraries/{id}/thumbnails",
                post(derive::library_thumbnails),
            )
            // Not in `shared`: the worker has no business serving bytes to anyone, and a
            // route mounted unconditionally is served by both images.
            .route("/api/blob/{blake3}", get(blob::by_hash))
            // The only route in this crate that reads a source file, and the only one
            // that may — see `download.rs`. `Role::Api` for the same reason the blob
            // route is: nothing proxies a browser to the worker, and this URL is one a
            // user clicks.
            .route("/api/revisions/{id}/download", get(download::original)),
        Role::Worker => Router::new(),
    };
    shared.merge(by_role).with_state(state)
}
