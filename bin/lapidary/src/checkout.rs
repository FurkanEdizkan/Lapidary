//! A checkout on disk (Phase 4 slice 1 spec §6): the folder a file is handed out in, and the
//! `.lapidary-checkout.json` beside it that says what it is a checkout of.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = ".lapidary-checkout.json";

/// What `checkin` renames [`FILE`] to: the record stays in the folder, and the agent stops
/// watching it. Nothing in the folder is deleted.
pub const CHECKED_IN: &str = ".lapidary-checked-in.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkout {
    pub server: String,
    pub library: String,
    pub part: String,
    /// The part's identity in its library, and the path a save is sent back under.
    pub source_path: String,
    pub lock: String,
    pub holder: String,
    /// The revision the file in this folder is of, moved on after each save the server keeps.
    pub revision: String,
    pub rev_label: String,
    pub file_name: String,
    /// BLAKE3 of the bytes that revision holds.
    pub blake3: String,
}

impl Checkout {
    pub fn read(folder: &Path) -> Result<Self> {
        let path = folder.join(FILE);
        let bytes =
            std::fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not a Lapidary checkout file", path.display()))
    }

    /// Written whole under a temporary name and renamed over, so the agent never reads half.
    pub fn write(&self, folder: &Path) -> Result<()> {
        let path = folder.join(FILE);
        let temporary = folder.join(format!("{FILE}.part"));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, &path)
            .with_context(|| format!("could not write {}", path.display()))
    }
}

/// `<part number, or else name>_<revision>`: flat and readable (`docs/DATA.md` §6.2), with
/// anything a folder name cannot hold replaced, by the rule the store's own directories use.
pub fn folder_name(part_number: Option<&str>, name: &str, rev_label: &str) -> String {
    let base = part_number
        .map(str::trim)
        .filter(|number| !number.is_empty())
        .unwrap_or(name);
    lapidary_core::slug::slugify(&format!("{base}_{rev_label}"))
}

/// Every folder in the workspace that holds a checkout file.
pub fn folders(workspace: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(workspace)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|folder| folder.join(FILE).is_file())
        .collect()
}

/// This workspace's checkout of `part` from `server`, if there is one: what `lapidary open`
/// reuses rather than asking for a second lock on a part this computer already holds.
pub fn find(workspace: &Path, server: &str, part: &str) -> Option<(PathBuf, Checkout)> {
    folders(workspace).into_iter().find_map(|folder| {
        let checkout = Checkout::read(&folder).ok()?;
        (checkout.part == part && checkout.server == server).then_some((folder, checkout))
    })
}

/// Where checkouts go: `$LAPIDARY_WORKSPACE`, else `~/Lapidary/workspace`.
pub fn workspace() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("LAPIDARY_WORKSPACE") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").context(
        "neither LAPIDARY_WORKSPACE nor HOME is set, so there is nowhere to put a checkout; \
         set LAPIDARY_WORKSPACE to a folder",
    )?;
    Ok(PathBuf::from(home).join("Lapidary").join("workspace"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_folder_is_named_for_the_part_number_and_its_revision() {
        assert_eq!(
            folder_name(Some("LP-3310-02"), "flange-dn40-lp-3310-02", "2"),
            "LP-3310-02_2"
        );
        assert_eq!(
            folder_name(Some("   "), "flange-dn40-lp-3310-02", "1"),
            folder_name(None, "flange-dn40-lp-3310-02", "1"),
            "a blank part number is no part number"
        );
        assert!(folder_name(None, "flange-dn40-lp-3310-02", "1").ends_with("_1"));
        assert!(
            !folder_name(None, "brackets/steel", "1").contains('/'),
            "a name cannot make a nested folder"
        );
    }

    #[test]
    fn open_reuses_this_workspaces_checkout_of_the_part_from_the_same_server_only() {
        let workspace = tempfile::tempdir().expect("temp dir");
        let flange = Checkout {
            server: "http://127.0.0.1:8080".to_owned(),
            library: "01931b6e-0000-7000-8000-000000000001".to_owned(),
            part: "01931b6e-0000-7000-8000-00000000aaaa".to_owned(),
            source_path: "flange-dn40-lp-3310-02.stl".to_owned(),
            lock: "01931b6e-0000-7000-8000-00000000eeee".to_owned(),
            holder: "mira@workshop-pc".to_owned(),
            revision: "01931b6e-0000-7000-8000-00000000bbbb".to_owned(),
            rev_label: "1".to_owned(),
            file_name: "flange-dn40-lp-3310-02.stl".to_owned(),
            blake3: "5a".repeat(32),
        };
        let at = |name: &str, checkout: &Checkout| {
            let folder = workspace.path().join(name);
            std::fs::create_dir_all(&folder).expect("folder");
            checkout.write(&folder).expect("writes");
            folder
        };
        let here = at("LP-3310-02_1", &flange);
        at(
            "LP-3310-02_1-other-server",
            &Checkout {
                server: "http://lapidary.example:8080".to_owned(),
                ..flange.clone()
            },
        );
        let done = at(
            "LP-3310-02_1-checked-in",
            &Checkout {
                part: "01931b6e-0000-7000-8000-00000000cccc".to_owned(),
                ..flange.clone()
            },
        );
        std::fs::rename(done.join(FILE), done.join(CHECKED_IN)).expect("checked in");

        assert_eq!(
            find(workspace.path(), &flange.server, &flange.part),
            Some((here, flange.clone()))
        );
        assert_eq!(
            find(
                workspace.path(),
                &flange.server,
                "01931b6e-0000-7000-8000-00000000cccc"
            ),
            None,
            "a checked-in folder is not a checkout any more"
        );
        assert_eq!(
            find(workspace.path(), "http://127.0.0.1:9999", &flange.part),
            None
        );
    }

    #[test]
    fn a_checkout_file_reads_back_what_was_written() {
        let folder = tempfile::tempdir().expect("temp dir");
        let checkout = Checkout {
            server: "http://127.0.0.1:8080".to_owned(),
            library: "01931b6e-0000-7000-8000-000000000001".to_owned(),
            part: "01931b6e-0000-7000-8000-00000000aaaa".to_owned(),
            source_path: "flange-dn40-lp-3310-02.stl".to_owned(),
            lock: "01931b6e-0000-7000-8000-00000000eeee".to_owned(),
            holder: "mira@workshop-pc".to_owned(),
            revision: "01931b6e-0000-7000-8000-00000000bbbb".to_owned(),
            rev_label: "1".to_owned(),
            file_name: "flange-dn40-lp-3310-02.stl".to_owned(),
            blake3: "5a".repeat(32),
        };
        checkout.write(folder.path()).expect("writes");
        assert_eq!(Checkout::read(folder.path()).expect("reads"), checkout);
        assert!(
            !folder.path().join(format!("{FILE}.part")).exists(),
            "no half-written file is left behind"
        );
    }
}
