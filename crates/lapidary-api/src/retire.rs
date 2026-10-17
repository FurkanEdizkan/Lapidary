//! Moving a removed model's file out of the way (A2, owner 2026-10-17).
//!
//! Removing a part takes its rows out of the library at once, and its file — when no other part, of
//! anybody, still points at it — is **moved** into `_removed/<owner>/` in the models folder. Never
//! deleted: a removal made by mistake is a file a person can move back. This is the one module of the
//! crate allowed to name the mover (`cargo xtask check-deploy`).

use lapidary_core::FileRoot;
use lapidary_storage::{Roots, move_to_removed};
use serde::Serialize;
use ts_rs::TS;

/// What happened to the files of what was removed.
#[derive(Debug, Default, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Retired {
    /// Each file moved, as where it is now under the models folder.
    pub moved: Vec<String>,
    /// Each file left where it was, because it could not be moved; the server log says why.
    pub stayed: Vec<String>,
}

/// Move each of `files` into `_removed/<owner>/` under its own root. One that cannot be moved stays
/// where it is and is said in the log: the rows are already gone, and the file being left in place
/// loses nothing.
pub async fn retire(roots: Roots, owner: String, files: Vec<(FileRoot, String)>) -> Retired {
    if files.is_empty() {
        return Retired::default();
    }
    tokio::task::spawn_blocking(move || {
        let mut retired = Retired::default();
        for (root, file) in files {
            match move_to_removed(roots.dir(root), &file, &owner) {
                Ok(Some(to)) => retired.moved.push(to),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%file, %error, "could not move a removed model's file into _removed/; it stays where it is");
                    retired.stayed.push(file);
                }
            }
        }
        retired
    })
    .await
    .unwrap_or_default()
}
