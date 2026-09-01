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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("LAPIDARY_LOG"))
        .init();

    let config: Config = Figment::new()
        .merge(Env::prefixed("LAPIDARY_"))
        .merge(Env::raw().only(&["DATABASE_URL"]))
        .extract()
        .context("Configuration is incomplete. LAPIDARY_DATABASE_URL or DATABASE_URL must be set; see deploy/.env.example.")?;

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
    axum::serve(listener, router(AppState { db }))
        .await
        .context("The HTTP server stopped unexpectedly")?;

    Ok(())
}
