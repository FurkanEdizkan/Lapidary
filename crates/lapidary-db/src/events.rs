//! What changed, told by the database itself. The other half of migration `0049`.
//!
//! There is no `pg_notify` call in this file and there is not meant to be one: the trigger the migration
//! installs is the writer, so every statement in this crate notifies without knowing it does. What is
//! here is the reading end -- the channel's name, and the one listener that hears it.

use crate::DbError;
use sqlx::PgPool;
use sqlx::postgres::PgListener;

/// The `LISTEN`/`NOTIFY` channel migration `0049`'s trigger notifies on. Each notification's payload is
/// one library id, as text.
///
/// Unlike [`JOB_CHANNEL`](crate::JOB_CHANNEL), this one is not an optimization over a poll that would
/// work without it: `GET /api/events` has no timer behind it, and a notification that is lost is a page
/// that stays stale until somebody navigates. That is why the api answers a lost listener with
/// `AppEvent::Resync` rather than with nothing.
pub const CHANGE_CHANNEL: &str = "lapidary_events";

/// Listen for what changes, for as long as the returned listener lives.
///
/// **One of these per api process, never one per browser tab.** A `PgListener` occupies a connection for
/// as long as it is listening (`PgJobs::listener` says the same), so a listener per tab is a Postgres
/// backend per tab against a pool of eight -- which is exactly why `crates/lapidary-api/src/jobs.rs`
/// refused to wire NOTIFY into the per-batch stream. The fan-out to the tabs is a `broadcast` in
/// `crates/lapidary-api/src/events.rs`, above this.
///
/// Reconnection is `try_recv`'s, not this function's: sqlx re-establishes the connection and re-issues
/// the `LISTEN` itself, and reports having done so as `Ok(None)` so the caller can say that a gap
/// happened. Anything that calls this in a loop is handling a listener that could not be opened at all.
pub async fn listen_for_changes(db: &PgPool) -> Result<PgListener, DbError> {
    let mut listener = PgListener::connect_with(db).await?;
    listener.listen(CHANGE_CHANNEL).await?;
    Ok(listener)
}
