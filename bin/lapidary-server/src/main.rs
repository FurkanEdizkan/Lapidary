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
    // Only the worker role reads these four, and each is `Option` for the same reason as
    // the two above. Their defaults live in `lapidary_jobs::WorkerConfig`'s `Default` impl
    // rather than here, so one place decides them: a second set of numbers in this file
    // would be free to drift from the ones the crate's own tests exercise. Unset is the
    // normal case — deploy/compose.yaml sets none of them, because the defaults are what
    // a single-machine worker wants.
    //
    // Gated, unlike `ingest_dir` and `blob_root`, because only `spawn_worker` reads them
    // and only this feature compiles it: left ungated they are dead code in the `api`
    // image, which `deploy/Containerfile` builds with no features at all. Figment ignores
    // environment variables that match no field, so setting one of these against an `api`
    // build is inert either way — and an `api` process has no worker for them to configure.
    #[cfg(feature = "mock-kernel")]
    #[serde(default, deserialize_with = "empty_str_as_none")]
    worker_concurrency: Option<usize>,
    #[cfg(feature = "mock-kernel")]
    #[serde(default, deserialize_with = "empty_str_as_none")]
    job_lease_secs: Option<u64>,
    #[cfg(feature = "mock-kernel")]
    #[serde(default, deserialize_with = "empty_str_as_none")]
    job_poll_secs: Option<u64>,
    #[cfg(feature = "mock-kernel")]
    #[serde(default, deserialize_with = "empty_str_as_none")]
    worker_id: Option<String>,
}

/// Treats an environment variable that is present but empty the same as one that is
/// absent, which is what `deploy/compose.yaml` needs to be able to write
/// `LAPIDARY_WORKER_CONCURRENCY: ${LAPIDARY_WORKER_CONCURRENCY:-}` and have an operator
/// who never sets it get the default rather than a refusal to start.
///
/// `#[serde(default)]` alone does not cover it: that handles the key being missing
/// entirely, but compose's `:-` substitution produces a key that is *present* and empty,
/// and the empty string reaches this field's deserializer. Verified by hand before this
/// was written — without it, an empty value fails startup with
/// `invalid type: found string "", expected usize for key "WORKER_CONCURRENCY"`, so a
/// worker whose operator copied `.env.example` and changed nothing would not boot.
///
/// The same problem, with the same fix, is documented at `empty_str_as_none` in
/// `crates/lapidary-api/src/parts.rs`, where it was a query string rather than an
/// environment variable that turned a documented URL shape into a 400.
#[cfg(feature = "mock-kernel")]
fn empty_str_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = String::deserialize(deserializer)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        trimmed.parse().map(Some).map_err(serde::de::Error::custom)
    }
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

/// Spawns the job worker that drains what `/scan` enqueued. Gated by the same feature as
/// `worker_router` and for the same reason: without it there is no `lapidary-ingest`, and
/// therefore no `JobHandler` to hand the loop.
///
/// Returns the loop's `JoinHandle` so `main` can wait for it. That wait is not optional —
/// `lapidary_jobs::run` releases this worker's leases as its last act, and a process that
/// exits without letting it get there leaves every in-flight job leased until it expires.
#[cfg(feature = "mock-kernel")]
fn spawn_worker(
    db: lapidary_db::PgPool,
    config: &Config,
    shutdown: tokio_util::sync::CancellationToken,
) -> Result<tokio::task::JoinHandle<()>> {
    use std::time::Duration;

    let ingest_dir = config
        .ingest_dir
        .clone()
        .context("Could not start the worker loop: LAPIDARY_INGEST_DIR is not set.")?;
    let blob_root = config
        .blob_root
        .clone()
        .context("Could not start the worker loop: LAPIDARY_BLOB_ROOT is not set.")?;

    let defaults = lapidary_jobs::WorkerConfig::default();
    let worker_config = lapidary_jobs::WorkerConfig {
        worker_id: config.worker_id.clone().unwrap_or(defaults.worker_id),
        lease: config
            .job_lease_secs
            .map(Duration::from_secs)
            .unwrap_or(defaults.lease),
        poll_interval: config
            .job_poll_secs
            .map(Duration::from_secs)
            .unwrap_or(defaults.poll_interval),
        concurrency: config.worker_concurrency.unwrap_or(defaults.concurrency),
        listen: true,
    };

    // The counterpart to the `role` and `kernel` lines above: an operator reading
    // container logs can see the worker started and with what, without attaching a
    // debugger to find out whether the loop is running at all.
    tracing::info!(
        worker = %worker_config.worker_id,
        concurrency = worker_config.concurrency,
        lease_secs = worker_config.lease.as_secs(),
        poll_secs = worker_config.poll_interval.as_secs(),
        "job worker starting"
    );

    let handler = std::sync::Arc::new(lapidary_ingest::IngestHandler {
        db: db.clone(),
        ingest_dir,
        blob_root,
    });
    Ok(tokio::spawn(async move {
        if let Err(error) =
            lapidary_jobs::run(lapidary_db::PgJobs(db), handler, worker_config, shutdown).await
        {
            tracing::error!(%error, "the job worker stopped");
        }
    }))
}

/// Unreachable in practice — `worker_router` bails on this same build before `main` gets
/// here — but it must compile, and if it ever does run it says the same thing that one
/// does rather than starting a worker that would drain nothing.
#[cfg(not(feature = "mock-kernel"))]
fn spawn_worker(
    _db: lapidary_db::PgPool,
    _config: &Config,
    _shutdown: tokio_util::sync::CancellationToken,
) -> Result<tokio::task::JoinHandle<()>> {
    bail!(
        "Could not start the worker loop: this binary was built without ingest support. \
         Rebuild with `--features mock-kernel` — deploy/compose.yaml does this \
         automatically for the worker service."
    );
}

/// Resolves on SIGTERM or Ctrl-C, then cancels `token` so the job worker stops dequeuing
/// at the same moment the HTTP server stops accepting. A container restart must release
/// leases rather than orphan them for a lease period.
///
/// Neither branch panics if its handler cannot be installed: it logs and then never
/// resolves, leaving the other branch to decide. A server that refuses to start because
/// it could not register a signal handler would be a worse outcome than one that can only
/// be stopped by the other signal.
async fn shutdown_signal(token: tokio_util::sync::CancellationToken) {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(error) => {
                tracing::warn!(%error, "could not listen for Ctrl-C; relying on SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM; relying on Ctrl-C");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("shutdown signal received; draining");
    token.cancel();
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

    lapidary_db::migrate(&config.database_url)
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
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (app_router, worker) = match role {
        Role::Api => (router(AppState { db }, Role::Api), None),
        Role::Worker => {
            // worker_router is built first on purpose: on a build without ingest support
            // it bails, and that is the error an operator should see, rather than one
            // about the loop that was only ever a consequence of the same missing feature.
            let app = worker_router(
                db.clone(),
                config.ingest_dir.clone(),
                config.blob_root.clone(),
            )?;
            let handle = spawn_worker(db, &config, shutdown.clone())?;
            (app, Some(handle))
        }
    };

    axum::serve(listener, app_router)
        .with_graceful_shutdown(shutdown_signal(shutdown.clone()))
        .await
        .context("The HTTP server stopped unexpectedly")?;

    // The listener is closed; the worker may still be finishing a file. Cancelling here
    // as well as in `shutdown_signal` covers the case where `serve` returned for some
    // other reason, so this is never reached with a worker that was never told to stop.
    shutdown.cancel();
    if let Some(worker) = worker {
        // Waiting is the point. `lapidary_jobs::run` awaits in-flight handlers and then
        // releases this worker's leases; returning from main before it gets there is the
        // crash path, not the graceful one, and would leave every job it held leased for
        // a full lease period after a deliberate restart.
        worker
            .await
            .context("The job worker did not shut down cleanly")?;
    }

    Ok(())
}

#[cfg(all(test, feature = "mock-kernel"))]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

    #[test]
    fn an_empty_worker_variable_is_treated_as_unset_rather_than_as_a_bad_value() {
        // deploy/compose.yaml writes `${LAPIDARY_WORKER_CONCURRENCY:-}`, which produces a
        // key that is present and empty for every operator who never set one. Without
        // empty_str_as_none that is a startup failure, so a worker whose .env came
        // straight from .env.example would refuse to boot. Deserializing the field
        // directly rather than through figment keeps this off the process environment,
        // which the rest of the suite shares.
        use serde::de::IntoDeserializer;
        use serde::de::value::{Error as ValueError, StrDeserializer};

        fn parse(raw: &str) -> Result<Option<usize>, ValueError> {
            let deserializer: StrDeserializer<ValueError> = raw.into_deserializer();
            empty_str_as_none(deserializer)
        }

        assert_eq!(parse("").expect("an empty value is not an error"), None);
        assert_eq!(parse("  ").expect("nor is whitespace"), None);
        assert_eq!(parse("8").expect("a real value still parses"), Some(8));
        parse("not-a-number").expect_err(
            "a genuinely wrong value must still be rejected rather than silently defaulted",
        );
    }

    #[test]
    fn kernel_description_reports_the_mock_implementation_when_the_feature_is_on() {
        assert!(
            kernel_description().starts_with("mock "),
            "expected the mock implementation name, got: {}",
            kernel_description()
        );
    }

    // worker_router is what actually runs in the worker container, and it's the thing
    // fix round 1 introduced: lapidary-api's router (health only) merged with
    // lapidary-ingest's (scan). Each half already has its own unit tests in its own
    // crate; this is the one place the *merge* itself is exercised, so a route added to
    // one side that silently collides with, or fails to reach, the other stops being
    // provable "by inspection" and starts being caught here.
    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn the_worker_router_serves_both_health_and_scan(pool: sqlx::PgPool) {
        let ingest_dir = tempfile::tempdir().expect("temp dir");
        let blob_root = tempfile::tempdir().expect("temp dir");
        let app = worker_router(
            pool,
            Some(ingest_dir.path().to_path_buf()),
            Some(blob_root.path().to_path_buf()),
        )
        .expect("worker router builds");

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/healthz")
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        assert_eq!(health.status(), StatusCode::OK);

        let scan = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/libraries/{SEEDED_LIBRARY}/scan"))
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        // An empty ingest_dir scans cleanly — 202, not merely "not 404" — which is a
        // stronger proof the merge actually wired the route through to a working
        // handler, not just to something that answers. 202 proves it exactly as well as
        // the 200 this asserted before task 10: reaching ACCEPTED means the walk ran and
        // the batch was written, so both the ingest router and the database are wired.
        assert_eq!(scan.status(), StatusCode::ACCEPTED);
    }

    // The other half of the same regression this fix round is closing: the api role's
    // router (no merge at all) must still never serve /scan. lapidary-api's own test
    // suite already proves this crate has no route reaching ingest under any role; this
    // pins it at the composition site in this file too, where a future edit could
    // accidentally merge lapidary-ingest's router into the Api arm as well.
    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn the_api_role_router_does_not_serve_scan(pool: sqlx::PgPool) {
        let app = router(AppState { db: pool }, Role::Api);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/libraries/{SEEDED_LIBRARY}/scan"))
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
