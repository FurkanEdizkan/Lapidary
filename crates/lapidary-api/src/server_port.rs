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

/// The `instance_setting` key for how many imports run at once (A41), and the range offered: one is the
/// lightest, eight is past what the default 2 CPUs can use.
pub const IMPORT_WORKERS: &str = "import_workers";
pub const IMPORT_WORKERS_RANGE: std::ops::RangeInclusive<u16> = 1..=8;

/// How many files the job worker reads at once (A41), and whether Settings may change it. A change
/// restarts Lapidary, as a models folder change does.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportWorkers {
    pub workers: u16,
    /// `LAPIDARY_WORKER_CONCURRENCY` when `deploy/.env` fixes it.
    pub locked_by: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetImportWorkers {
    pub workers: u16,
}

/// The models folder (A6): where it is on the host, and whether the wizard and Settings may choose
/// another. Choosing one restarts Lapidary on it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelsFolder {
    /// The folder on the host, when Lapidary knows it.
    pub path: Option<String>,
    /// Where Lapidary keeps its own files (pictures and the store) on the host, when known. Chosen in
    /// the wizard (A8): the store is copied there whole first.
    pub store: Option<String>,
    /// `LAPIDARY_MODELS_DIR` when `deploy/.env` fixes the folder.
    pub locked_by: Option<String>,
    /// `LAPIDARY_STORAGE_ROOT` when `deploy/.env` fixes where Lapidary's own files are (A8).
    pub store_locked_by: Option<String>,
    /// Whether this deployment mounts a browse root to choose from.
    pub can_choose: bool,
}

/// One folder of the host, as the models-folder picker lists it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HostFolders {
    pub path: String,
    /// The browse root: the top of what can be opened (A9).
    pub root: String,
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
    /// The models folder, or `null` to keep it.
    #[serde(default)]
    pub path: Option<String>,
    /// Lapidary's own files (A8), or `null` to keep them where they are.
    #[serde(default)]
    pub store: Option<String>,
}

/// New folder in the folder picker (A9): one folder named `name` inside `path`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MakeFolder {
    pub path: String,
    pub name: String,
}
