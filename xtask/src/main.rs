//! Workspace automation. Run via the `cargo xtask` alias in .cargo/config.toml.

mod deploy;
mod layers;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::process::Command;

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("check-layers") => check_layers(),
        Some("check-deploy") => check_deploy(),
        Some("export-bindings") => export_bindings(),
        Some(other) => {
            bail!("Unknown xtask '{other}'. Available: check-layers, check-deploy, export-bindings")
        }
        None => bail!("Usage: cargo xtask <check-layers|check-deploy|export-bindings>"),
    }
}

/// Locate the workspace root from xtask's own manifest directory — xtask must stay one
/// level below the root for this to hold. Shared by `check_deploy` and `export_bindings`.
fn workspace_root() -> Result<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live one level below the workspace root")?
        .to_path_buf();
    if !root.join("Cargo.toml").exists() {
        bail!(
            "Expected the workspace manifest at {}. xtask derives the workspace root from its \
             own location, so it must stay one level below the root; move it back, or update \
             this path.",
            root.join("Cargo.toml").display()
        );
    }
    Ok(root)
}

/// Static checks over `deploy/compose.yaml` and `deploy/Containerfile` — see
/// `xtask/src/deploy.rs` for the rules. This check is static: it verifies the
/// configuration text, not the images actually built from it. A green result means the
/// files are internally consistent with each other, not that the last-built `api` image
/// lacks the CAD kernel.
fn check_deploy() -> Result<()> {
    let root = workspace_root()?;
    let compose_path = root.join("deploy/compose.yaml");
    let containerfile_path = root.join("deploy/Containerfile");

    let compose = std::fs::read_to_string(&compose_path)
        .with_context(|| format!("Could not read {}", compose_path.display()))?;
    let containerfile = std::fs::read_to_string(&containerfile_path)
        .with_context(|| format!("Could not read {}", containerfile_path.display()))?;

    let mut violations = deploy::check(&compose, &containerfile);

    let api_sources = collect_api_sources(&root)?;
    violations.extend(deploy::check_open_path_boundary(&api_sources));

    if violations.is_empty() {
        println!(
            "deploy check OK — deploy/compose.yaml and deploy/Containerfile agree on which \
             services link the CAD kernel (static check: configuration only, not built images), \
             and lapidary-api never names SourceStore ({} source file(s) checked)",
            api_sources.len()
        );
        Ok(())
    } else {
        eprintln!(
            "Deploy configuration check failed ({} problem(s)):\n",
            violations.len()
        );
        for v in &violations {
            eprintln!("  {v}");
        }
        eprintln!(
            "\nThis check is static — it verifies deploy/compose.yaml and \
             deploy/Containerfile, not a built image. The open path (lapidary-api) must \
             never invoke the CAD kernel; only services in KERNEL_LINKED_SERVICES \
             (xtask/src/deploy.rs) may set SERVER_FEATURES. The open path also never \
             touches a source file — lapidary-api must never name SourceStore."
        );
        bail!("deploy check failed")
    }
}

/// Walk `crates/lapidary-api/src/**/*.rs` and read each file, for
/// `deploy::check_open_path_boundary`. Recursive: handler modules can nest in
/// subdirectories.
fn collect_api_sources(root: &std::path::Path) -> Result<Vec<(String, String)>> {
    let api_src = root.join("crates/lapidary-api/src");
    let mut out = Vec::new();
    collect_rs_files(&api_src, &mut out)?;
    Ok(out)
}

fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<(String, String)>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Could not read directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("Could not read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Could not read {}", path.display()))?;
            out.push((path.display().to_string(), contents));
        }
    }
    Ok(())
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
    // Workspace member name -> whether cargo metadata reports its `publish` field as `null`
    // (missing `publish = false`, i.e. publishable). Kept separate from `graph`: `Graph`
    // means "the dependency graph" and publishability is not an edge.
    let mut publish_is_null: BTreeMap<String, bool> = BTreeMap::new();
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
        publish_is_null.insert(name.to_owned(), pkg["publish"].is_null());
    }

    let mut violations = layers::check(&graph).err().unwrap_or_default();
    for (name, &is_null) in &publish_is_null {
        if let Some(v) = layers::check_publish(name, is_null) {
            violations.push(v);
        }
    }

    if violations.is_empty() {
        println!("layering OK — {} workspace crates checked", graph.len());
        Ok(())
    } else {
        eprintln!(
            "Layering rule violated ({} problem(s)):\n",
            violations.len()
        );
        for v in &violations {
            eprintln!("  {v}");
        }
        eprintln!(
            "\nThe tier rule is in docs/ARCHITECTURE.md: L2 crates may depend on L0 and L1, \
             never on each other or on L3; L3 may depend on L0-L3 but never on \
             Enterprise, the wrapper tier that holds lapidary-enterprise; and nothing — \
             not even Enterprise — may depend on a binary (lapidary-server, lapidary, \
             xtask), which sits outside the tier rule and depends on everything instead. \
             Beyond the tier rule, specific edges may also be forbidden by name — see \
             FORBIDDEN_PAIRS in xtask/src/layers.rs. And every workspace member must set \
             `publish = false`: deny.toml's `allow-wildcard-paths` depends on it."
        );
        bail!("layering check failed")
    }
}

/// Regenerate the TypeScript bindings from #[ts(export)] types in lapidary-core.
fn export_bindings() -> Result<()> {
    let root = workspace_root()?;
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
