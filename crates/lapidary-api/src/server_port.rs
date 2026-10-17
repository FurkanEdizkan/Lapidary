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

/// The models folder (A6): where it is on the host, and whether the wizard and Settings may choose
/// another. Choosing one restarts Lapidary on it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelsFolder {
    /// The folder on the host, when Lapidary knows it.
    pub path: Option<String>,
    /// Where Lapidary keeps its own files (pictures and the store) on the host, when known. Shown,
    /// never chosen here: moving it would break every download.
    pub store: Option<String>,
    /// `LAPIDARY_MODELS_DIR` when `deploy/.env` fixes the folder.
    pub locked_by: Option<String>,
    /// Whether this deployment mounts a browse root to choose from.
    pub can_choose: bool,
}

/// One folder of the host, as the models-folder picker lists it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HostFolders {
    pub path: String,
    pub parent: Option<String>,
    /// The folders inside it, by name. Symbolic links are left out.
    pub folders: Vec<String>,
    /// Whether this folder can be the models folder.
    pub choosable: bool,
    /// Why it can't be opened or chosen, with what to run, when that is so.
    pub problem: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ChooseModelsFolder {
    pub path: String,
}
