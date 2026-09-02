//! The grid: listing parts in a library. Placeholder for Task 10 — this crate's router
//! must be role-aware (Task 8) before the real route lands, so the route is mounted now
//! with a body that says it isn't built yet rather than left unmounted, which is what
//! lets `the_worker_role_does_not_serve_the_grid` prove the route is missing *because of
//! role*, not because it doesn't exist anywhere.

use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;

/// TODO(Task 10): replace with the real listing handler.
pub async fn page(State(_state): State<AppState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
