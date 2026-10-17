//! The port's wire shapes (A5). The routes themselves are `bin/lapidary-server`'s: this crate builds a
//! Router and never owns a socket, so the one thing that moves the app to another port lives with the
//! listeners it moves.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The port the app answers on, and whether Settings may change it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ServerPort {
    pub port: u16,
    /// The `deploy/.env` setting that fixes the port, when one does — `LAPIDARY_PORT` or
    /// `LAPIDARY_BIND` — and Settings then shows it instead of offering a change.
    pub locked_by: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MovePort {
    pub port: u16,
}
