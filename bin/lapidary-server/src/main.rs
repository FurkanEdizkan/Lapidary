//! Container entrypoint: the API, and optionally an in-process worker.

use anyhow::{Context, Result, bail};
use figment::Figment;
use figment::providers::Env;
use lapidary_api::{AppState, Role, router};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Config {
    database_url: String,
    #[serde(default = "default_bind")]
    bind: String,
    // No default, deliberately, and left `Option` (checked by hand below) rather than a
    // required field on the struct, so a missing LAPIDARY_ROLE gets its own actionable
    // error instead of being folded into the generic "configuration is incomplete"
    // message below, which is written for the DATABASE_URL case and would be misleading
    // for this one. The two roles are not interchangeable, and that asymmetry is exactly
    // why a *default* would be wrong: if the `api` service loses this variable it stays
    // `api` and nothing looks wrong; if `worker` loses it, ingest silently stops with no
    // error anywhere — the container would start, bind its port, and pass its healthcheck
    // while never mounting /scan. Missing it is now a startup failure instead. The
    // matching CI-side guard lives in xtask/src/deploy.rs, which fails `check-deploy` if
    // deploy/compose.yaml ever stops setting this for a service that runs lapidary-server.
    role: Option<String>,
    // Same reasoning as `role`, and `Option` rather than required for the same reason:
    // only the worker role reads these, so requiring them unconditionally would make the
    // `api` service's config incomplete for no reason it would ever hit. Checked by hand
    // in `worker_router` below when `role` turns out to be `Role::Worker`. No hardcoded
    // container path either way: tests build `lapidary_ingest::AppState` directly, and
    // `deploy/compose.yaml` (Task 12) supplies the real `/ingest` mount.
    ingest_dir: Option<PathBuf>,
    blob_root: Option<PathBuf>,
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_owned()
}

/// Human-readable kernel description for the startup log. `deploy/Containerfile` takes the
/// feature list as a build arg, empty by default, and `deploy/compose.yaml` sets it only for
/// the `worker` service — so only `worker`'s image links the mock kernel and prints this
/// branch; `api` prints the `not(feature = "mock-kernel")` line below instead. This is the
/// only way an operator can tell from `podman logs` whether that feature chain actually held.
#[cfg(feature = "mock-kernel")]
fn kernel_description() -> String {
    use lapidary_cad::Kernel;
    let version = lapidary_cad::MockKernel::new().version();
    format!("{} {}", version.implementation, version.version)
}

#[cfg(not(feature = "mock-kernel"))]
fn kernel_description() -> String {
    "none (build with --features mock-kernel to compile one in)".to_owned()
}

/// Builds the worker process's router: `lapidary-api`'s (health only — `Role::Worker`
/// mounts nothing else) merged with `lapidary-ingest`'s (the scan route). Only this
/// build's feature gates whether that merge is even possible — see
/// `Cargo.toml`'s `mock-kernel` feature doc.
#[cfg(feature = "mock-kernel")]
fn worker_router(
    db: lapidary_db::PgPool,
    ingest_dir: Option<PathBuf>,
    blob_root: Option<PathBuf>,
) -> Result<axum::Router> {
    let ingest_dir =
        ingest_dir.context("Could not start as worker: LAPIDARY_INGEST_DIR is not set.")?;
    let blob_root =
        blob_root.context("Could not start as worker: LAPIDARY_BLOB_ROOT is not set.")?;
    let api = router(AppState { db: db.clone() }, Role::Worker);
    let ingest = lapidary_ingest::router(lapidary_ingest::AppState {
        db,
        ingest_dir,
        blob_root,
    });
    Ok(api.merge(ingest))
}

/// This build was not compiled with the feature that pulls `lapidary-ingest` in at all
/// (see `Cargo.toml`) — fail loudly rather than silently start a worker with no scan
/// route. `deploy/compose.yaml` always builds the worker image with the feature, so this
/// only fires when someone runs a default-featured binary manually with
/// `LAPIDARY_ROLE=worker`.
#[cfg(not(feature = "mock-kernel"))]
fn worker_router(
    _db: lapidary_db::PgPool,
    _ingest_dir: Option<PathBuf>,
    _blob_root: Option<PathBuf>,
) -> Result<axum::Router> {
    bail!(
        "Could not start as worker: this binary was built without ingest support. Rebuild \
         with `--features mock-kernel` — deploy/compose.yaml does this automatically for \
         the worker service."
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        // from_env() would default to ERROR, which silences the "listening" line below —
        // a container-first product that prints nothing on a successful start is not
        // operable. Default to INFO; LAPIDARY_LOG still overrides.
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .with_env_var("LAPIDARY_LOG")
                .from_env_lossy(),
        )
        .init();

    let config: Config = Figment::new()
        // Order matters: figment's later merge wins, so the namespaced variable is merged
        // LAST and takes precedence. sqlx projects routinely have a bare DATABASE_URL in
        // the environment for compile-time query checking, and it must not silently
        // override an operator's deliberate LAPIDARY_DATABASE_URL.
        .merge(Env::raw().only(&["DATABASE_URL"]))
        .merge(Env::prefixed("LAPIDARY_"))
        .extract()
        .context("Configuration is incomplete. Set LAPIDARY_DATABASE_URL (preferred — it wins if both are set) or DATABASE_URL; see deploy/.env.example.")?;

    let Some(role_str) = config.role.as_deref() else {
        bail!(
            "Could not start: LAPIDARY_ROLE is not set. Set it to `api` (serves the grid \
             and the open path) or `worker` (runs ingest) — deploy/compose.yaml sets it per \
             service."
        );
    };
    let role = Role::from_env_str(role_str).context("Could not start: bad LAPIDARY_ROLE.")?;

    // `lapidary_db::connect()` now classifies *why* the connection failed
    // (unreachable, wrong credentials, missing database) and that message is
    // actionable on its own — this outer line must only name the startup stage, not
    // guess a cause the classified error below it might contradict.
    let db = lapidary_db::connect(&config.database_url)
        .await
        .context("Could not start: connecting to the database failed.")?;

    lapidary_db::migrate(&db)
        .await
        .context("Could not start: the database schema could not be brought up to date.")?;

    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| {
            format!(
                "Could not bind {}. Another process may already hold that port.",
                config.bind
            )
        })?;

    tracing::info!(bind = %config.bind, "lapidary-server listening");
    // Two containers run this one binary (deploy/compose.yaml: api on 8080, worker on
    // 8081) and only the router differs between them — this line is what lets `podman
    // logs` tell an operator which one a given container actually took.
    tracing::info!(role = %role_str, "role");
    tracing::info!(kernel = %kernel_description(), "CAD kernel");
    let app_router = match role {
        Role::Api => router(AppState { db }, Role::Api),
        Role::Worker => worker_router(db, config.ingest_dir, config.blob_root)?,
    };
    axum::serve(listener, app_router)
        .await
        .context("The HTTP server stopped unexpectedly")?;

    Ok(())
}

#[cfg(all(test, feature = "mock-kernel"))]
mod tests {
    use super::*;

    #[test]
    fn kernel_description_reports_the_mock_implementation_when_the_feature_is_on() {
        assert!(
            kernel_description().starts_with("mock "),
            "expected the mock implementation name, got: {}",
            kernel_description()
        );
    }
}
