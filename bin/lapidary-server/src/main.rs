//! Container entrypoint: the API, the job worker and the browser app, in one process.
//!
//! Until S1 (2026-10-16) the API and the worker were two containers built from this binary with
//! and without a `worker` feature, so the API image never linked the OCCT kernel. The kernel is
//! gone, ingest takes about 200 ms a file, and a hobby library does not need four containers. The

mod web;

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
    /// Gone. Read only so that a compose file from before S1 or S3, which still sets it, is refused
    /// with a sentence saying what changed rather than starting as something it no longer is.
    role: Option<String>,
    // `Option` and checked by hand when the app starts, so each missing one gets its own sentence
    // rather than figment's generic one.
    ingest_dir: Option<PathBuf>,
    blob_root: Option<PathBuf>,
    // Where a partial upload is assembled before its bytes are verified and enter the store.
    upload_dir: Option<PathBuf>,
    /// Where `blob_root` is mounted **from**, on the host.
    ///
    /// Nothing in this process reads a file through it, and nothing should: it exists to be
    /// *shown*. `blob_root` is where the store is inside the container, and under
    /// `deploy/compose.yaml` that is `/var/lib/lapidary` — a path that exists on no machine
    /// anyone is sitting at. The store's whole layout is built on being openable in a file
    /// manager (`DATA.md` §1.1), and the path a person needs for that is a fact only the
    /// deployment has. `deploy/compose.yaml` passes the `LAPIDARY_STORAGE_ROOT` the operator
    /// already sets, so this is a variable they usually do not have to think about.
    ///
    /// Ignored unless absolute. A relative value — `../storage`, which is what compose
    /// falls back to — is resolved against `deploy/`, and this process has no idea where
    /// that is; prefixing it onto a model's path would produce something that looks like an
    /// answer and is not.
    host_storage_root: Option<PathBuf>,
    /// Where the bundled example parts live, so a first run is never an empty grid.
    ///
    /// Deliberately not `ingest_dir`, even though `deploy/compose.yaml` defaults that to
    /// the same six files: `LAPIDARY_INGEST_DIR` is the *operator's*, and one who pointed
    /// it at a 320 GB corpus must not have it scanned because they restarted the server.
    /// `deploy/Containerfile` puts these at a path the image owns.
    ///
    /// Unset, or a path that does not exist, means no seeding — which is the normal case
    /// for a binary run outside the image.
    example_dir: Option<PathBuf>,
    /// Where `npm run build` put the browser app (`web/dist`). Unset serves the API alone, which is
    /// what `npm run dev` wants: Vite serves the app and proxies `/api` here.
    web_dir: Option<PathBuf>,
    // The job worker's knobs. Their defaults live in `lapidary_jobs::WorkerConfig`'s `Default`
    // impl, so one place decides them; unset is the normal case.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    worker_concurrency: Option<usize>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    job_lease_secs: Option<u64>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    job_poll_secs: Option<u64>,
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
/// Accepts a number as readily as a string, which is not optional here: figment types
/// environment values as it reads them, so `LAPIDARY_JOB_LEASE_SECS=10` arrives as an
/// unsigned int and `LAPIDARY_JOB_LEASE_SECS=` arrives as a string. Demanding one of the
/// two rejects the other — the first version of this function asked for a `String` and
/// took the worker down with
/// `invalid type: found unsigned int 10, expected a string for key "JOB_LEASE_SECS"`,
/// which is the opposite of the failure it was written to prevent.
fn empty_str_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    struct Knob<T>(std::marker::PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for Knob<T>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        type Value = Option<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a worker setting, or an empty value meaning \"use the default\"")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse().map(Some).map_err(E::custom)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            self.visit_str(&value.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            self.visit_str(&value.to_string())
        }

        /// An explicitly null value is as good as an absent one.
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(Knob(std::marker::PhantomData))
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_owned()
}

/// What this build can read, for the startup log. Asked of `lapidary-cad` rather than written out,
/// so the line is evidence of what was linked rather than a literal that would print regardless.
fn parsers_description() -> String {
    format!("{}, for stl, obj and 3mf", lapidary_cad::IMPLEMENTATION)
}

/// One `migrate_storage` job per library still holding source files in the old
/// content-addressed store, so an operator upgrading from before that migration existed
/// never has to hand-write `INSERT INTO job` -- the gap Task 8b closes.
///
/// Never fails startup. `libraries_needing_migration` and `enqueue_migration_if_absent`
/// each log and return on their own error rather than propagate one, because a database
/// hiccup at this one moment must cost a delayed migration -- picked up on the next
/// restart, exactly as any other resumable job would be -- and not a server that never
/// came up at all.
async fn enqueue_pending_migrations(db: &lapidary_db::PgPool) {
    let libraries = match lapidary_db::PgStorageMigration(db.clone())
        .libraries_needing_migration()
        .await
    {
        Ok(libraries) => libraries,
        Err(error) => {
            tracing::warn!(
                %error,
                "could not check for libraries needing a storage migration; skipping for \
                 this boot -- the next restart tries again"
            );
            return;
        }
    };

    let jobs = lapidary_db::PgJobs(db.clone());
    for library in libraries {
        match jobs.enqueue_migration_if_absent(library).await {
            Ok(Some(batch)) => tracing::info!(
                %library,
                %batch,
                "queued a storage migration for a library upgraded from the old \
                 content-addressed store"
            ),
            // Already pending, already running, or a concurrent caller won the race --
            // all three are the guard doing its job, not something worth a log line.
            Ok(None) => {}
            Err(error) => tracing::warn!(
                %library,
                %error,
                "could not queue a storage migration for this library; it will be \
                 retried on the next restart"
            ),
        }
    }
}

/// Spawns the job worker that drains what `/scan`, uploads and imports enqueue, and the hourly
/// quarantine sweep beside it.
///
/// Returns the loop's `JoinHandle` so `main` can wait for it. That wait is not optional —
/// `lapidary_jobs::run` releases this worker's leases as its last act, and a process that
/// exits without letting it get there leaves every in-flight job leased until it expires.
fn spawn_worker(
    db: lapidary_db::PgPool,
    config: &Config,
    ingest_dir: PathBuf,
    blob_root: PathBuf,
    shutdown: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    use std::time::Duration;

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

    // An operator reading container logs can see the worker started and with what, without
    // attaching a debugger to find out whether the loop is running at all.
    tracing::info!(
        worker = %worker_config.worker_id,
        concurrency = worker_config.concurrency,
        lease_secs = worker_config.lease.as_secs(),
        poll_secs = worker_config.poll_interval.as_secs(),
        "job worker starting"
    );

    let handler = std::sync::Arc::new(lapidary_ingest::WorkerHandler {
        db: db.clone(),
        ingest_dir,
        blob_root: blob_root.clone(),
        pause: None,
        sites: lapidary_sites::Sites::public(),
    });

    // The quarantine sweep rides along with the worker rather than getting a job kind of its
    // own. It is a timer with no row to lease and nothing to report.
    //
    // Not awaited by `main` the way the job loop is: it holds no leases, so a process that
    // exits mid-sweep leaves nothing behind — the sweep's own transaction either committed
    // or rolled back, and the next start picks up where it left off an hour later.
    tracing::info!(
        quarantine_days = lapidary_ingest::reap::QUARANTINE.as_secs() / 86_400,
        "quarantine sweep starting"
    );
    tokio::spawn(lapidary_ingest::reap::run(
        db.clone(),
        blob_root,
        shutdown.clone(),
    ));

    tokio::spawn(async move {
        // Before the loop takes its first job, and never a reason not to start: see
        // `enqueue_stale_derivatives`.
        handler.enqueue_stale_derivatives().await;
        if let Err(error) =
            lapidary_jobs::run(lapidary_db::PgJobs(db), handler, worker_config, shutdown).await
        {
            tracing::error!(%error, "the job worker stopped");
        }
    })
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

    refuse_old_role(config.role.as_deref())?;

    // `lapidary_db::connect()` classifies *why* the connection failed (unreachable, wrong
    // credentials, missing database) and that message is actionable on its own — this outer
    // line must only name the startup stage, not guess a cause the classified error below it
    // might contradict.
    let db = lapidary_db::connect(&config.database_url, lapidary_db::INTERACTIVE)
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
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (app_router, worker, flusher) = {
        tracing::info!(parsers = %parsers_description(), "parsers");
        let blob_root = config
                .blob_root
                .clone()
                .context("Could not start: LAPIDARY_BLOB_ROOT is not set. It names the store every model is kept in.")?;
        let ingest_dir = config
                .ingest_dir
                .clone()
                .context("Could not start: LAPIDARY_INGEST_DIR is not set. It names the folder the scan button reads.")?;
        // Required rather than defaulted: a default would put half-transferred files somewhere
        // nobody chose, and the one wrong answer here — the blob root itself — would put
        // incomplete bytes inside a store whose whole contract is that everything in it is
        // complete.
        let upload_dir = config
                .upload_dir
                .clone()
                .context("Could not start: LAPIDARY_UPLOAD_DIR is not set. It names where an upload waits until its bytes are checked.")?;

        // The worker's own pool, with the long ceiling: nobody waits on a job, and a job that
        // runs long is not a stuck query. The API keeps the interactive one, so a slow job can
        // never hold a connection a browser is waiting for.
        let jobs_db = lapidary_db::connect(&config.database_url, lapidary_db::BACKGROUND)
            .await
            .context("Could not start: connecting the job worker to the database failed.")?;
        // Before the loop starts, so a first run has a populated grid by the time anyone
        // opens it; and the migration check after the seed, so the two do not race for the
        // same library on a first run.
        if let Some(examples) = &config.example_dir {
            lapidary_ingest::seed_examples(jobs_db.clone(), &blob_root, examples).await;
        }
        enqueue_pending_migrations(&jobs_db).await;
        let worker = spawn_worker(
            jobs_db,
            &config,
            ingest_dir,
            blob_root.clone(),
            shutdown.clone(),
        );

        // Which blobs were read, written every five minutes and once more when the server
        // stops (`docs/DATA.md` §1.4), rather than one UPDATE per read.
        let touches = lapidary_db::Touches::default();
        let flusher = {
            let (touches, db, shutdown) = (touches.clone(), db.clone(), shutdown.clone());
            tokio::spawn(async move {
                let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
                every.tick().await;
                loop {
                    tokio::select! {
                        _ = every.tick() => {}
                        () = shutdown.cancelled() => break,
                    }
                    if let Err(err) = touches.flush(&db).await {
                        tracing::warn!(error = %err, "could not record which blobs were read; they keep their older access times");
                    }
                }
            })
        };
        // The last flush is `main`'s, once `serve` has drained every request that could still
        // record a read.
        let last = (touches.clone(), db.clone());
        // One Postgres listener for this whole process, fanned out to every open tab
        // (`crates/lapidary-api/src/events.rs`). It holds one pooled connection for as long as
        // the process runs, and `shutdown` ends it, which is what ends every open stream.
        let hub = lapidary_api::events::Hub::spawn(db.clone(), shutdown.clone()).await;
        let api = router(
            AppState {
                db,
                blob_root,
                upload_dir,
                touches,
                // Absolute or nothing — see the field's doc. A relative path here is
                // compose's unset default rather than an operator's choice, and showing it as
                // though it were one is the mistake this guards.
                host_storage_root: config
                    .host_storage_root
                    .as_ref()
                    .filter(|path| path.is_absolute())
                    .map(|path| path.display().to_string()),
            },
            Role::Api,
        )
        .merge(lapidary_api::events::router(hub));
        let app = match &config.web_dir {
            Some(dist) => {
                if !dist.join("index.html").is_file() {
                    bail!(
                        "Could not start: LAPIDARY_WEB_DIR is {} and there is no index.html in \
                             it. Point it at the folder `npm --prefix web run build` writes \
                             (web/dist), or unset it to serve the API alone.",
                        dist.display()
                    );
                }
                tracing::info!(web_dir = %dist.display(), "serving the browser app");
                web::with_web(api, dist)
            }
            None => api,
        };
        (app, Some(worker), Some((flusher, last)))
    };

    let served = axum::serve(listener, app_router)
        .with_graceful_shutdown(shutdown_signal(shutdown.clone()))
        .await;

    // The listener is closed; the worker may still be finishing a file. Cancelling here
    // as well as in `shutdown_signal` covers the case where `serve` returned for some
    // other reason, so this is never reached with a worker that was never told to stop.
    shutdown.cancel();
    if let Some((flusher, (touches, db))) = flusher {
        if let Err(err) = flusher.await {
            tracing::warn!(error = %err, "the flush of which blobs were read did not finish");
        }
        // After `serve`, however it returned: no request is left to record a read after this.
        if let Err(err) = touches.flush(&db).await {
            tracing::warn!(error = %err, "could not record the last blobs read before stopping");
        }
    }
    served.context("The HTTP server stopped unexpectedly")?;
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

/// `LAPIDARY_ROLE` chose between the processes this server used to be split into. One process does
/// everything now, so any value is a compose file from before.
fn refuse_old_role(role: Option<&str>) -> Result<()> {
    match role.map(str::trim) {
        None | Some("") => Ok(()),
        Some(old) => bail!(
            "Could not start: LAPIDARY_ROLE={old} is gone. One process now serves the API, runs \
             ingest and serves the app, and sharing between installations has been removed; \
             remove LAPIDARY_ROLE from this service and delete any separate api, worker, web or \
             peer services from your compose file (see deploy/compose.yaml)."
        ),
    }
}

#[cfg(test)]
mod role_tests {
    use super::refuse_old_role;

    #[test]
    fn unset_starts_and_every_old_role_is_refused_by_name_with_what_to_do() {
        refuse_old_role(None).expect("unset");
        refuse_old_role(Some("")).expect("empty");
        for old in ["api", "worker", "peer"] {
            let message = refuse_old_role(Some(old))
                .expect_err("an old role must not start as something it no longer is")
                .to_string();
            assert!(
                message.contains(&format!("LAPIDARY_ROLE={old} is gone")),
                "{message}"
            );
            assert!(message.contains("remove LAPIDARY_ROLE"), "{message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use uuid::Uuid;

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
        use serde::de::value::{Error as ValueError, StrDeserializer, U64Deserializer};

        fn from_text(raw: &str) -> Result<Option<usize>, ValueError> {
            let deserializer: StrDeserializer<ValueError> = raw.into_deserializer();
            empty_str_as_none(deserializer)
        }

        fn from_number(raw: u64) -> Result<Option<usize>, ValueError> {
            let deserializer: U64Deserializer<ValueError> = raw.into_deserializer();
            empty_str_as_none(deserializer)
        }

        assert_eq!(from_text("").expect("an empty value is not an error"), None);
        assert_eq!(from_text("  ").expect("nor is whitespace"), None);
        assert_eq!(from_text("8").expect("a real value still parses"), Some(8));
        from_text("not-a-number").expect_err(
            "a genuinely wrong value must still be rejected rather than silently defaulted",
        );

        // Both halves matter, and only testing the first is how the empty case got fixed
        // while the set case broke. figment types environment values as it reads them, so
        // a number in the environment does NOT arrive as a string: the first version of
        // this helper asked for a String, and `LAPIDARY_JOB_LEASE_SECS=10` took the worker
        // down at startup with "invalid type: found unsigned int 10, expected a string".
        // Caught by running the stack, not by this file — hence this line.
        assert_eq!(
            from_number(10).expect("a number from the environment parses too"),
            Some(10)
        );
    }

    #[test]
    fn the_startup_line_names_the_parsers_this_build_links_and_what_they_read() {
        let description = parsers_description();
        assert!(description.starts_with("mesh"), "got: {description}");
        for format in lapidary_core::MESH_EXTENSIONS {
            assert!(
                description.contains(format),
                "names the formats a person may hand this server, got: {description}"
            );
        }
    }

    // The app mounts `lapidary-api`'s scan route, which enqueues, and not `lapidary-ingest`'s,
    // which walks the folder inside the request. The two are told apart by the one place they
    // differ: for a library id that names nothing, `lapidary_api::scan` probes first and answers a
    // clean 404, while `lapidary_ingest::scan` enqueues blind and answers 500 on the foreign key.
    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn the_api_role_router_serves_its_own_scan_and_not_ingests(pool: sqlx::PgPool) {
        let app = router(
            AppState {
                db: pool,
                // This test asks only which routes mount; it never reaches the store or
                // stages an upload.
                blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
                host_storage_root: None,
                touches: Default::default(),
                upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            },
            Role::Api,
        );

        // Mounted: the browser's scan button has a route to reach.
        let seeded = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/libraries/{SEEDED_LIBRARY}/scan"))
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        assert_eq!(seeded.status(), StatusCode::ACCEPTED);

        // And it is *this crate's* handler. `lapidary_ingest::scan` would answer 202
        // here, because it never asks whether the library exists.
        let absent = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/libraries/01931b6e-0000-7000-8000-00000000dead/scan")
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");
        assert_eq!(absent.status(), StatusCode::NOT_FOUND);
    }

    // `enqueue_pending_migrations`'s own wiring, proved end to end: the underlying
    // enumeration and guard are `lapidary-db`'s own, thoroughly tested there, but
    // nothing there proves this function actually calls them in the right order for a
    // real library holding a real un-migrated file. This is a plain, sequential test —
    // the guard's race-freedom is `lapidary-db`'s `two_workers_forced_to_start_together_
    // still_enqueue_exactly_one_migration`, which forces a real concurrent collision;
    // this only needs to show one call queues a job and a second does not duplicate it.
    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn enqueue_pending_migrations_queues_exactly_one_job_for_a_library_with_un_migrated_files(
        pool: sqlx::PgPool,
    ) {
        let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
        let hash = "a".repeat(64);
        sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, 10, 10)")
            .bind(&hash)
            .execute(&pool)
            .await
            .expect("blob inserts");
        let part = Uuid::now_v7();
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $3)")
            .bind(part)
            .bind(library)
            .bind("bracket-lp-1042-05")
            .execute(&pool)
            .await
            .expect("part inserts");
        let revision = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
        )
        .bind(revision)
        .bind(part)
        .execute(&pool)
        .await
        .expect("revision inserts");
        sqlx::query(
            "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes) \
             VALUES ($1, $2, 'source', 'stl', $3, 10)",
        )
        .bind(Uuid::now_v7())
        .bind(revision)
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("file inserts with a null storage_path -- an old, un-migrated store");

        enqueue_pending_migrations(&pool).await;

        let jobs: Vec<(String, Uuid)> =
            sqlx::query_as("SELECT kind, library_id FROM job WHERE kind = 'migrate_storage'")
                .fetch_all(&pool)
                .await
                .expect("reads back");
        assert_eq!(
            jobs,
            vec![("migrate_storage".to_owned(), library)],
            "exactly one migration job, for the library the un-migrated file belongs to"
        );

        // Calling it again -- the next worker restart, say -- must not queue a
        // duplicate: the guard underneath this wiring is what makes that true, and this
        // is what proves the wiring actually reaches it.
        enqueue_pending_migrations(&pool).await;
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'migrate_storage'")
                .fetch_one(&pool)
                .await
                .expect("counts");
        assert_eq!(total, 1, "a second call must not queue a duplicate");
    }

    // The converse: a library with nothing un-migrated must not get a job at all. The
    // seeded library has no parts of its own in a fresh migrated database, so this is
    // the "nothing to do" case with no setup beyond the migrations themselves.
    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn enqueue_pending_migrations_queues_nothing_for_a_library_with_nothing_to_migrate(
        pool: sqlx::PgPool,
    ) {
        enqueue_pending_migrations(&pool).await;

        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'migrate_storage'")
                .fetch_one(&pool)
                .await
                .expect("counts");
        assert_eq!(total, 0, "nothing needs a migration, so nothing is queued");
    }
}
