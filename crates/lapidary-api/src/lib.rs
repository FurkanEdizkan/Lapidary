#![deny(clippy::unwrap_used)]
//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

mod health;

use axum::Router;
use axum::routing::get;
use lapidary_db::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

/// Build the application router. Callers own the listener.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/healthz", get(health::healthz))
        .with_state(state)
}
