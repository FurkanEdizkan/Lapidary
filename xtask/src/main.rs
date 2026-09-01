//! Workspace automation. Run via the `cargo xtask` alias in .cargo/config.toml.

mod layers;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::process::Command;

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("check-layers") => check_layers(),
        Some("export-bindings") => export_bindings(),
        Some(other) => bail!("Unknown xtask '{other}'. Available: check-layers, export-bindings"),
        None => bail!("Usage: cargo xtask <check-layers|export-bindings>"),
    }
}

/// Read the workspace graph from `cargo metadata` and apply the layering rule.
fn check_layers() -> Result<()> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .context("Could not run `cargo metadata`. Is cargo on PATH?")?;

    if !output.status.success() {
        bail!(
            "`cargo metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("`cargo metadata` returned invalid JSON")?;

    let packages = meta["packages"]
        .as_array()
        .context("`cargo metadata` output had no packages array")?;

    let member_names: std::collections::BTreeSet<String> = packages
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();

    let mut graph: layers::Graph = BTreeMap::new();
    for pkg in packages {
        let Some(name) = pkg["name"].as_str() else {
            continue;
        };
        let deps: Vec<String> = pkg["dependencies"]
            .as_array()
            .map(|ds| {
                ds.iter()
                    .filter(|d| d["kind"].is_null()) // normal deps only; dev and build deps exempt (deliberate — allows L2 test fixtures)
                    .filter_map(|d| d["name"].as_str())
                    .filter(|d| member_names.contains(*d))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        graph.insert(name.to_owned(), deps);
    }

    match layers::check(&graph) {
        Ok(()) => {
            println!("layering OK — {} workspace crates checked", graph.len());
            Ok(())
        }
        Err(violations) => {
            eprintln!(
                "Layering rule violated ({} problem(s)):\n",
                violations.len()
            );
            for v in &violations {
                eprintln!("  {v}");
            }
            eprintln!(
                "\nThe rule is in docs/ARCHITECTURE.md: L2 crates may depend on L0 and L1, never on each other or on L3."
            );
            bail!("layering check failed")
        }
    }
}

/// Regenerate the TypeScript bindings from #[ts(export)] types in lapidary-core.
fn export_bindings() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live one level below the workspace root")?
        .to_path_buf();
    // The line above only fires if CARGO_MANIFEST_DIR is the filesystem root, which never
    // happens. Moving xtask deeper — to tools/xtask, say — would silently resolve `root` to
    // the wrong directory and write bindings somewhere nothing reads. Fail loudly instead.
    if !root.join("Cargo.toml").exists() {
        bail!(
            "Expected the workspace manifest at {}. xtask derives the workspace root from its \
             own location, so it must stay one level below the root; move it back, or update \
             this path.",
            root.join("Cargo.toml").display()
        );
    }
    let out = root.join("web/src/bindings");

    // ts-rs writes on test run; clear first so removed types do not linger.
    if out.exists() {
        std::fs::remove_dir_all(&out).context("Could not clear web/src/bindings")?;
    }
    std::fs::create_dir_all(&out).context("Could not create web/src/bindings")?;

    let status = Command::new(env!("CARGO"))
        .args(["test", "-p", "lapidary-core", "export_bindings"])
        .env("TS_RS_EXPORT_DIR", &out)
        .status()
        .context("Could not run the ts-rs export tests")?;

    if !status.success() {
        // The output directory was cleared above, so the previously committed bindings are
        // gone from the working tree. Say so — the user needs the recovery step, not just
        // the diagnosis.
        bail!(
            "ts-rs export failed, and web/src/bindings/ was cleared before the attempt, so \
             the committed bindings are missing from your working tree. Run `git checkout -- \
             web/src/bindings` to restore them, then `cargo test -p lapidary-core` to see \
             which type could not be exported."
        );
    }

    println!("bindings written to {}", out.display());
    Ok(())
}
