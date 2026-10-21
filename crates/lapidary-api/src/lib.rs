//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

// `pub` for `CurrentUser`, `prepare_setup` and `hash_password`, which the server binary and the tests use.
mod added;
pub mod auth;
mod blob;
mod dashboard;
mod densities;
mod derive;
mod detail;
mod download;
mod error;
// `pub` and its own router, unlike every module above: the hub is not a field on `AppState`, which is
// built in 88 places. `bin/lapidary-server` merges `events::router(hub)` for the api role.
pub mod events;
mod fields;
mod filters;
mod folders;
mod health;
mod images;
mod jobs;
mod lifecycle;
mod moves;
mod part_number;
mod parts;
mod people;
mod profile;
// `pub` for `Retired`, the shape a delete answers with.
pub mod retire;
mod revisions;
mod scan;
// `pub` for `bin/lapidary-server`'s port routes, which own the listeners (A5).
pub mod server_port;
mod site_settings;
mod sources;
mod tags;
mod upload;
mod wall;

pub use added::{BrowseEntry, BrowseListing, LibrarySource, NewSourceRequest};
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
    /// Lapidary's own store: pictures, exports, imported bundles, and the files of parts
    /// indexed before M4, which stay where they were and still download.
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
    /// The owner's folders, mounted read-only: where a `library` file row's path is (M4). This
    /// crate reads here, through `LibraryReader`, and never writes.
    pub ingest_dir: std::path::PathBuf,
    /// Where an upload is written once (M4), then indexed from there. Never inside `ingest_dir`.
    pub inbox_dir: std::path::PathBuf,
    /// Where `ingest_dir` is on the host, for the same reason as `host_storage_root`.
    pub host_ingest_root: Option<String>,
    /// Where `inbox_dir` is on the host, for the same reason as `host_storage_root`.
    pub host_inbox_root: Option<String>,
    /// Which blobs were read since the last flush (`docs/DATA.md` §1.4). The server flushes it
    /// every five minutes and when it stops; a test flushes it itself.
    pub touches: lapidary_db::Touches,
}

impl AppState {
    /// The three directories a `file` row's `storage_path` can be relative to.
    pub fn roots(&self) -> lapidary_storage::Roots {
        lapidary_storage::Roots {
            library: self.ingest_dir.clone(),
            inbox: self.inbox_dir.clone(),
            store: self.blob_root.clone(),
        }
    }
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
    // The only routes a browser may call without a session (A1): the wizard's two, signing in and out,
    // and the sign-in wall's pictures (A28), which carry nothing but pictures. Everything in `guarded` is
    // behind `auth::require_user`.
    let public = Router::new()
        .route("/api/setup", get(auth::setup_status).post(auth::setup))
        .route(
            "/api/session",
            get(auth::me).post(auth::sign_in).delete(auth::sign_out),
        )
        .route("/api/wall", get(wall::pictures))
        .route("/api/wall/{image}", get(wall::picture));
    let by_role = match role {
        Role::Api => public.merge(guarded(state.db.clone())),
        Role::Worker => Router::new(),
    };
    shared
        .merge(by_role)
        .layer(axum::middleware::from_fn(auth::same_origin))
        .with_state(state)
}

/// Every route a person reaches only signed in. `route_layer`, so a path that matches nothing is still
/// a plain 404 rather than a demand to sign in.
fn guarded(db: PgPool) -> Router<AppState> {
    Router::new()
        // Every library, and the route that makes one. Not under `{id}` — these are
        // about the set of libraries rather than about any of them.
        .route(
            "/api/libraries",
            get(derive::list_libraries).post(derive::create_library),
        )
        .route("/api/libraries/{id}/parts", get(parts::page))
        .route("/api/libraries/{id}/facets", get(parts::facets))
        // What that page of cards costs, summed. `Role::Api` with the grid it totals
        // — see `parts.rs`.
        .route("/api/libraries/{id}/storage", get(parts::storage))
        // Bundles (Phase 4 slice 2): planned first, so a refusal is said before a
        // download starts, then streamed. In `download.rs`, the one file allowed to read
        // source bytes.
        .route(
            "/api/libraries/{id}/bundle/plan",
            post(download::bundle_plan),
        )
        .route("/api/libraries/{id}/bundle", post(download::bundle))
        .route("/api/libraries/{id}/imports", post(upload::import_bundle))
        // What the whole store holds, and where it is. Not under `/api/libraries/{id}`
        // because two of its figures belong to no library and its derivative total is
        // deliberately not what adding the libraries up gives.
        .route("/api/storage", get(parts::instance_storage))
        // "Free cache space": rungs Lapidary can rebuild, into quarantine (DATA §1.5).
        .route("/api/storage/render-cache", post(parts::free_render_cache))
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
        // Every failure past the status's sample, and the retry. Both under the batch
        // and its library, so the library check is the status route's own.
        .route(
            "/api/libraries/{library}/jobs/{batch}/failed",
            get(jobs::failed),
        )
        .route(
            "/api/libraries/{library}/jobs/{batch}/retry",
            post(jobs::retry),
        )
        // The scan trigger, on `Role::Api` for the same reason the three below it
        // are: nothing proxies a browser to the worker, so a scan button needs a
        // route the api serves. It enqueues a `scan_directory` job and walks
        // nothing — see `scan.rs`.
        .route("/api/libraries/{id}/scan", post(scan::scan))
        // What the library holds (A2): the Import browser over the models folder, the files and
        // folders added from it, and the pictures a person chose for one. See `added.rs`.
        .route("/api/storage/browse", get(added::browse))
        .route(
            "/api/libraries/{id}/sources",
            get(added::list_sources).post(added::add_source),
        )
        .route(
            "/api/libraries/{library}/sources/{source}",
            axum::routing::delete(added::remove_source),
        )
        .route(
            "/api/libraries/{library}/sources/{source}/pictures",
            post(added::add_source_picture).layer(DefaultBodyLimit::max(
                lapidary_storage::images::MAX_INPUT_BYTES,
            )),
        )
        // "Fetch details": a part described from its page, nothing downloaded (A2).
        .route("/api/parts/{id}/describe", post(site_settings::describe))
        // Importing a model from its page, and the Settings page's keys for the sites it
        // reads (F3). See `site_settings.rs`.
        .route(
            "/api/libraries/{id}/imports/link",
            post(site_settings::import),
        )
        // This install's display name (M2). See `profile.rs`.
        .route("/api/profile", get(profile::get).put(profile::set))
        .route(
            "/api/settings/wall",
            get(wall::setting).put(wall::set_setting),
        )
        .route("/api/parts/{id}/wall", get(wall::part).put(wall::set_part))
        .route(
            "/api/settings/sites",
            get(site_settings::list).put(site_settings::switch),
        )
        // The people on the install, the admin's account management, and sharing (A4). See
        // `people.rs`.
        .route("/api/people", get(people::list).post(people::create))
        .route("/api/people/{id}", axum::routing::delete(people::delete))
        .route("/api/people/{id}/password", put(people::reset_password))
        .route("/api/profile/password", put(people::change_password))
        .route(
            "/api/parts/{id}/shares",
            get(people::shares).put(people::set_shares),
        )
        .route("/api/shared", get(people::shared_with_me))
        .route(
            "/api/settings/import",
            get(profile::import_settings).put(profile::set_import_settings),
        )
        .route(
            "/api/settings/sites/{site}",
            put(site_settings::set).delete(site_settings::clear),
        )
        .route(
            "/api/settings/sites/{site}/check",
            post(site_settings::check),
        )
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
        // The page a card links to, and the delete that takes the part out of the
        // library for good (its model file stays on disk). One route entry: `DELETE`
        // on the thing `GET` returns is the same resource.
        .route(
            "/api/parts/{id}",
            get(detail::detail).delete(lifecycle::remove),
        )
        // A part's mesh as a 3MF or an STL, for a slicer, built when first asked for.
        // The only thing a worker still builds on demand. `Role::Api` out of necessity
        // rather than preference: nothing proxies a browser to the worker, so a route
        // mounted there would be unreachable from the UI that exists to call it. See
        // `derive.rs`'s module doc and design section 3.5.
        .route(
            "/api/parts/{id}/exports/{format}",
            post(derive::part_export),
        )
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
        // Saved filters: a name for a set of the grid's filters, per library. See
        // `filters.rs`.
        .route(
            "/api/libraries/{id}/filters",
            get(filters::list).post(filters::create),
        )
        .route(
            "/api/libraries/{library}/filters/{filter}",
            axum::routing::delete(filters::remove).patch(filters::rename),
        )
        .route(
            "/api/libraries/{library}/filters/{filter}/move",
            post(filters::move_filter),
        )
        // Custom fields: a library's own named values on its parts. See `fields.rs`.
        .route(
            "/api/libraries/{id}/fields",
            get(fields::list).post(fields::create),
        )
        .route(
            "/api/libraries/{library}/fields/{key}",
            axum::routing::patch(fields::update).delete(fields::remove),
        )
        // A density per material, per library: what mass is worked out from. See `densities.rs`.
        .route("/api/libraries/{id}/densities", get(densities::list))
        .route(
            "/api/libraries/{library}/densities/{material}",
            axum::routing::put(densities::set).delete(densities::remove),
        )
        // Moving a model, which is the one route here that does touch the store — a
        // directory rename, no content access. `moves.rs` is the only file in this
        // crate allowed to hold that rename handle, enforced by `cargo xtask
        // check-deploy`'s `RELOCATE_MODULE`.
        .route("/api/parts/{id}", axum::routing::patch(moves::move_part))
        .route("/api/parts/{id}/moves", get(moves::history))
        // Every revision of a part, newest first: rows and inline thumbnails, so the
        // open path. See `revisions.rs`.
        .route("/api/parts/{id}/revisions", get(revisions::list))
        // The number a person gives a part. Its own resource rather than a field on the
        // move `PATCH` above, where a `null` folder already means "to the top level".
        .route(
            "/api/parts/{id}/part-number",
            axum::routing::put(part_number::set),
        )
        // The tags a person gives a part, the whole list in one write.
        .route("/api/parts/{id}/tags", axum::routing::put(tags::set))
        // What a part is made of, typed over what its file states. `PUT` with an empty list
        // says the part holds none, which is a different thing from never having been told:
        // `DELETE` is what hands the part back to its file. See `tags.rs`.
        .route(
            "/api/parts/{id}/materials",
            axum::routing::put(tags::set_materials).delete(tags::unset_materials),
        )
        // One part's value for one of its library's custom fields. See `fields.rs`.
        .route(
            "/api/parts/{id}/fields/{key}",
            axum::routing::put(fields::set_value),
        )
        // A part's gallery. The upload body is the file itself, capped at the same
        // 10 MB `images::MAX_INPUT_BYTES` refuses past — set here as well because a
        // limit checked after the body is buffered is a limit that has already cost
        // what it was meant to save.
        .route(
            "/api/parts/{id}/images",
            get(images::list)
                .post(images::upload)
                .layer(axum::extract::DefaultBodyLimit::max(
                    images::MAX_INPUT_BYTES,
                )),
        )
        // The same gallery, filled from an address instead of a file. A separate
        // route rather than a mode on the one above because the bodies are different
        // shapes — raw bytes there, a JSON object here — and because the risk is
        // different: this is the only route in the application that makes an
        // outbound request, and it should be legible as that from the route table.
        .route("/api/parts/{id}/images/from-url", post(images::from_url))
        // The gallery's order, first picture first (A15).
        .route(
            "/api/parts/{id}/images/order",
            axum::routing::put(images::reorder),
        )
        // Re-framing carries both ids because an image id on its own is a bare
        // handle to a row, and the repo checks the pair — see `set_image_framing`.
        .route(
            "/api/parts/{id}/images/{imageId}",
            get(images::bytes).patch(images::set_framing),
        )
        // Where a part came from. Deliberately not near `images::from_url`: this
        // route stores a link and never follows one, and the application has exactly
        // one place that makes an outbound request.
        .route(
            "/api/parts/{id}/sources",
            get(sources::list).post(sources::create),
        )
        // Not in `shared`: the worker has no business serving bytes to anyone, and a
        // route mounted unconditionally is served by both images.
        .route("/api/blob/{blake3}", get(blob::by_hash))
        // The only route in this crate that reads a source file, and the only one
        // that may — see `download.rs`. `Role::Api` for the same reason the blob
        // route is: nothing proxies a browser to the worker, and this URL is one a
        // user clicks.
        .route("/api/revisions/{id}/download", get(download::original))
        // The part's current file, one link that survives new revisions. See
        // `download::current_file`.
        .route("/api/parts/{id}/file", get(download::current_file))
        // A library's tags as an index, and one tag's neighbours (P3). Its own router for the
        // reason Phase 6's two below give. Not the facet panel: these count over the whole
        // library and never withhold a count. See `tags.rs`.
        .merge(tags::reads())
        // Its own file so that the dashboard goal never edits this chain. Phase 6's other
        // router was `likeness`, and it went with shape similarity (R3).
        .merge(dashboard::routes())
        .route_layer(axum::middleware::from_fn_with_state(db, auth::require_user))
}
