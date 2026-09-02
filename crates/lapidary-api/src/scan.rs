//! Kicking off a library scan (ingest). Placeholder for Task 9 — see `parts.rs` for why
//! this route is mounted now with a stub body instead of waiting for the real handler.
//!
//! This module must never depend on `lapidary-cad` directly or transitively in a way that
//! would make the `api` role's process link the kernel — `xtask/src/layers.rs`'s
//! `FORBIDDEN_PAIRS` enforces that lapidary-api never depends on lapidary-cad at all, and
//! that stays true here: this handler mounts only under `Role::Worker`.

use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;

/// TODO(Task 9): replace with the real scan-kickoff handler.
pub async fn scan(State(_state): State<AppState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
