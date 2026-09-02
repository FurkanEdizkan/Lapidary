//! Container entrypoint: the API, and optionally an in-process worker.

use anyhow::{Context, Result};
use figment::Figment;
use figment::providers::Env;
use lapidary_api::{AppState, router};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    database_url: String,
    #[serde(default = "default_bind")]
    bind: String,
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_owned()
}

/// Human-readable kernel description for the startup log. The `mock-kernel` feature is
/// what the `worker` compose service builds with in Phase 0a — this line is the only way
/// an operator can tell from `podman logs` whether that feature chain actually held.
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

    let db = lapidary_db::connect(&config.database_url).await.context(
        "Could not start: the database is unreachable. Check that the `db` service is running.",
    )?;

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
    tracing::info!(kernel = %kernel_description(), "CAD kernel");
    axum::serve(listener, router(AppState { db }))
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
