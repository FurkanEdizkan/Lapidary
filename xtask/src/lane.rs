//! Several sessions on one machine, each in its own worktree (`docs/goals/PROTOCOL.md`).
//!
//! Two things make that safe, and both live here so no session has to remember them:
//!
//! - **Its own environment.** `scripts/claim-goal.sh` writes an untracked `.lane.env` at the root of a
//!   lane's worktree — its own test database, its build settings, its ports. Every `cargo xtask`
//!   command re-runs itself with that file applied, so the gate, the git hooks and an ad hoc
//!   `cargo xtask heavy -- …` all see the lane's database rather than whichever one the shell
//!   last exported. The file sets `LAPIDARY_LANE`, and its presence is what marks the environment
//!   as already applied.
//! - **One compile at a time.** 15.5 GB of RAM holds one workspace build, not two: background gate
//!   runs were killed under memory pressure before this existed. The steps that compile take an
//!   exclusive lock on a file in the repository's common git directory — the same file from every
//!   worktree — and a second session waits, saying so, rather than both being killed.

use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The file a lane's settings live in, at the root of its worktree. Untracked (`.gitignore`).
pub const LANE_ENV: &str = ".lane.env";

/// The variable that names the lane, and marks its environment as applied.
pub const LANE_VAR: &str = "LAPIDARY_LANE";

/// `KEY=VALUE` lines, `#` comments and blank lines. A value may be wrapped in one pair of
/// quotes. Anything else is refused by line number, because a lane whose database was
/// silently not set would run its tests against the lead's.
pub fn parse_env(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut vars = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "{LANE_ENV} line {}: expected KEY=VALUE, found `{line}`",
                index + 1
            ));
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "{LANE_ENV} line {}: `{key}` is not a variable name",
                index + 1
            ));
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        vars.push((key.to_owned(), value.to_owned()));
    }
    Ok(vars)
}

/// Re-run this xtask command with the lane's settings, when there is a lane file and they are not
/// applied yet. `Some(code)` is the child's exit code, for `main` to exit with; `None` means carry
/// on in this process.
pub fn rerun_with_lane_env(root: &Path) -> Result<Option<i32>> {
    if std::env::var_os(LANE_VAR).is_some() {
        return Ok(None);
    }
    let path = root.join(LANE_ENV);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("Could not read {}", path.display()));
        }
    };
    let vars = parse_env(&text).map_err(|message| anyhow::anyhow!(message))?;
    if !vars.iter().any(|(key, _)| key == LANE_VAR) {
        bail!(
            "{} does not set {LANE_VAR}. Write it with `scripts/claim-goal.sh`, which names the lane.",
            path.display()
        );
    }
    let exe = std::env::current_exe().context("Could not find this xtask binary to re-run it")?;
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .envs(vars)
        .status()
        .context("Could not re-run xtask with the lane's settings")?;
    Ok(Some(status.code().unwrap_or(1)))
}

/// Where the lock lives: the common git directory, which every worktree of this repository shares.
fn lock_path(root: &Path) -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(root)
        .output()
        .context("Could not run `git rev-parse --git-common-dir`")?;
    if !output.status.success() {
        bail!("{} is not inside a git repository", root.display());
    }
    let common = String::from_utf8_lossy(&output.stdout);
    // Relative in the main checkout (`.git`), absolute in a linked worktree: `join` handles both.
    Ok(root.join(common.trim()).join("lapidary-heavy.lock"))
}

/// Hold the machine-wide compile lock until the returned file is dropped. Waits, and says it is
/// waiting, when another session holds it.
pub fn heavy_lock(root: &Path) -> Result<File> {
    let path = lock_path(root)?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("Could not open the compile lock at {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            println!(
                "  {:<24} another session is compiling; waiting for {}",
                "lock",
                path.display()
            );
            file.lock().with_context(|| {
                format!("Could not take the compile lock at {}", path.display())
            })?;
        }
        Err(TryLockError::Error(err)) => {
            return Err(err)
                .with_context(|| format!("Could not take the compile lock at {}", path.display()));
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_file_reads_as_its_variables_in_order() {
        let text = "# lane 2\nLAPIDARY_LANE=2\n\nDATABASE_URL=\"postgres://lapidary:localdev@localhost:55434/lapidary\"\nCARGO_BUILD_JOBS = 4\n";
        assert_eq!(
            parse_env(text),
            Ok(vec![
                ("LAPIDARY_LANE".to_owned(), "2".to_owned()),
                (
                    "DATABASE_URL".to_owned(),
                    "postgres://lapidary:localdev@localhost:55434/lapidary".to_owned()
                ),
                ("CARGO_BUILD_JOBS".to_owned(), "4".to_owned()),
            ])
        );
    }

    #[test]
    fn a_line_that_is_not_an_assignment_is_refused_by_number() {
        let err = parse_env("LAPIDARY_LANE=2\nexport DATABASE_URL\n").expect_err("refused");
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn a_key_that_is_not_a_variable_name_is_refused() {
        let err = parse_env("DATABASE URL=x\n").expect_err("refused");
        assert!(err.contains("not a variable name"), "{err}");
    }

    #[test]
    fn a_value_may_hold_an_equals_sign() {
        assert_eq!(
            parse_env("RUSTFLAGS=-C opt-level=1\n"),
            Ok(vec![("RUSTFLAGS".to_owned(), "-C opt-level=1".to_owned())])
        );
    }

    #[test]
    fn a_second_holder_waits_for_the_first() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("lock");
        let first = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .expect("opens");
        first.lock().expect("locks");
        let second = OpenOptions::new().write(true).open(&path).expect("opens");
        assert!(
            matches!(second.try_lock(), Err(TryLockError::WouldBlock)),
            "a second holder must wait while the first holds it"
        );
        drop(first);
        assert!(
            second.try_lock().is_ok(),
            "and take it once the first lets go"
        );
    }
}
