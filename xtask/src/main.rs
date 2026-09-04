//! Workspace automation. Run via the `cargo xtask` alias in .cargo/config.toml.

mod commit;
mod deploy;
mod layers;
mod setup;
mod strings;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("check-layers") => check_layers(),
        Some("check-deploy") => check_deploy(),
        Some("check-strings") => check_strings(),
        Some("check-commit-msg") => check_commit_msg(),
        Some("export-bindings") => export_bindings(),
        Some("export-agents-md") => export_agents_md(),
        Some("setup") => run_setup(),
        Some(other) => bail!(
            "Unknown xtask '{other}'. Available: check-layers, check-deploy, check-strings, check-commit-msg, export-bindings, export-agents-md, setup"
        ),
        None => {
            bail!(
                "Usage: cargo xtask <check-layers|check-deploy|check-strings|check-commit-msg|export-bindings|export-agents-md|setup>"
            )
        }
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

/// Make a fresh clone workable: install the commit hook, reconcile the plugins
/// `.claude/settings.json` declares, and materialize the project's skills for agents that
/// cannot install a Claude Code plugin. Idempotent — every run reports what was already
/// correct, so "already fine" is distinguishable from "did not look".
///
/// See `xtask/src/setup.rs` for what gets decided and why; this function only executes.
fn run_setup() -> Result<()> {
    let root = workspace_root()?;

    // 1. The commit hook. core.hooksPath is local to .git/config and cannot be committed,
    //    which is the entire reason this command exists rather than the hook just working.
    let hooks = root.join(".githooks");
    if !hooks.is_dir() {
        bail!("Expected the hook directory at {}", hooks.display());
    }
    run(&root, "git", &["config", "core.hooksPath", ".githooks"])?;
    println!("  {:<8} core.hooksPath -> .githooks", "hooks");

    // 2. What the project declares.
    let settings_path = root.join(".claude/settings.json");
    let settings: serde_json::Value = match std::fs::read_to_string(&settings_path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", settings_path.display()))?,
        Err(_) => {
            println!(
                "  {:<8} no .claude/settings.json; nothing declared",
                "plugins"
            );
            serde_json::Value::Object(serde_json::Map::new())
        }
    };

    // 3. What the machine has. A missing Claude CLI is not fatal: the hook is installed
    //    and the skills below come from git, so a machine whose agent is not Claude Code
    //    still gets everything it can use.
    let markets = claude_marketplaces();
    let plugins = claude_plugins();
    let claude_available = markets.is_some() && plugins.is_some();
    if !claude_available {
        println!(
            "  {:<8} the `claude` CLI is not on PATH; skipping plugin reconcile",
            "plugins"
        );
    }

    // 4. and 5. Decide, then do.
    let actions = setup::plan_actions(
        &settings,
        &markets.unwrap_or_default(),
        &plugins.unwrap_or_default(),
    );
    for action in &actions {
        match action {
            setup::Action::AddMarketplace { name, repo } if claude_available => {
                run(&root, "claude", &["plugin", "marketplace", "add", repo])?;
                println!("  {:<8} registered marketplace {name}", "plugins");
            }
            setup::Action::InstallPlugin { id } if claude_available => {
                run(&root, "claude", &["plugin", "install", id, "--yes"])?;
                println!("  {:<8} installed {id}", "plugins");
            }
            setup::Action::EnablePlugin { id } if claude_available => {
                run(&root, "claude", &["plugin", "enable", id])?;
                println!("  {:<8} enabled {id}", "plugins");
            }
            setup::Action::MaterializeSkills { name, repo } => {
                let count = materialize_skills(&root, name, repo)?;
                println!(
                    "  {:<8} .agents/skills/{name}/ <- {repo} ({count} skill(s))",
                    "skills"
                );
            }
            // Reports, and anything needing a CLI that is not here.
            other => println!("  {:<8} {other}", "note"),
        }
    }

    println!("\nSetup complete. Re-run it any time; it changes only what is not already correct.");
    Ok(())
}

/// Run a command in `root`, failing with its stderr rather than a bare exit code.
fn run(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("Could not run `{program}`. Is it installed and on PATH?"))?;
    if !output.status.success() {
        bail!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `claude plugin marketplace list --json`, or `None` when the CLI is absent or unhappy.
fn claude_marketplaces() -> Option<Vec<setup::Marketplace>> {
    let output = Command::new("claude")
        .args(["plugin", "marketplace", "list", "--json"])
        .output()
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(
        parsed
            .as_array()?
            .iter()
            .map(|m| setup::Marketplace {
                name: m["name"].as_str().unwrap_or_default().to_owned(),
                repo: m["repo"].as_str().unwrap_or_default().to_owned(),
            })
            .collect(),
    )
}

/// `claude plugin list --json`, or `None` when the CLI is absent or unhappy.
fn claude_plugins() -> Option<Vec<setup::Plugin>> {
    let output = Command::new("claude")
        .args(["plugin", "list", "--json"])
        .output()
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(
        parsed
            .as_array()?
            .iter()
            .map(|p| setup::Plugin {
                id: p["id"].as_str().unwrap_or_default().to_owned(),
                enabled: p["enabled"].as_bool().unwrap_or(false),
            })
            .collect(),
    )
}

/// Clone (or refresh) a marketplace repository and copy its `skills/` into
/// `.agents/skills/<name>/`, so an agent that cannot install a Claude Code plugin can
/// still read them. Plain git on purpose: no vendor CLI is involved.
///
/// Both directories are gitignored. This is a materialized cache of another repository,
/// not a copy this one owns — committing it would fork the skills, which is exactly what
/// removing them from `.claude/skills/` was meant to stop.
fn materialize_skills(root: &Path, name: &str, repo: &str) -> Result<usize> {
    let cache = root.join(".agents/.cache").join(name);
    let url = format!("https://github.com/{repo}.git");
    if cache.join(".git").exists() {
        run(&cache, "git", &["fetch", "--depth", "1", "origin"])?;
        run(&cache, "git", &["reset", "--hard", "FETCH_HEAD"])?;
    } else {
        if let Some(parent) = cache.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let cache_arg = cache.to_string_lossy().into_owned();
        run(
            root,
            "git",
            &["clone", "--depth", "1", "--quiet", &url, &cache_arg],
        )?;
    }

    let source = cache.join("skills");
    let destination = root.join(".agents/skills").join(name);
    if destination.exists() {
        std::fs::remove_dir_all(&destination)?;
    }
    if !source.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(&destination)?;

    let mut count = 0;
    for entry in std::fs::read_dir(&source)? {
        let entry = entry?;
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &destination.join(entry.file_name()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// Recursive directory copy. Hand-rolled because this workspace adds no dependency it can
/// avoid, and the tree being copied is a handful of markdown files.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The generated wrapper around `CLAUDE.md`. Pure so it can be tested without writing to
/// the repository, and deterministic so the CI staleness gate compares like with like.
///
/// `CLAUDE.md` stays the source because Claude Code loads it automatically, and its
/// content needs no rewriting for other tools: it names no vendor anywhere in its 3.8 KB.
/// Only the filename was ever Claude-specific.
fn render_agents_md(claude_md: &str) -> String {
    format!(
        "<!-- Generated from CLAUDE.md by `cargo xtask export-agents-md`. Edit CLAUDE.md, \
         not this file — CI fails when the two disagree. -->\n\
         \n\
         {}\n\
         \n\
         ---\n\
         \n\
         ## For agents other than Claude Code\n\
         \n\
         The rules above are this project's, not any one tool's. Claude Code reads them \
         from `CLAUDE.md`; everything else reads them here.\n\
         \n\
         This project's skills — its commit and branch conventions, and its \
         module-structure guidance — are deliberately not committed to this repository. \
         They live in their own repository so a single copy stays current everywhere. Run \
         `cargo xtask setup` to materialize them into `.agents/skills/`; it uses plain git \
         and needs no vendor CLI.\n",
        claude_md.trim_end()
    )
}

/// Write `AGENTS.md` from `CLAUDE.md`, so agents that do not read `CLAUDE.md` still find
/// the project's rules. Committed and gated in CI, exactly like `web/src/bindings` — a
/// generated file that is not checked is a file that silently goes stale.
fn export_agents_md() -> Result<()> {
    let root = workspace_root()?;
    let source = root.join("CLAUDE.md");
    let claude_md = std::fs::read_to_string(&source)
        .with_context(|| format!("Could not read {}", source.display()))?;

    let out = root.join("AGENTS.md");
    let rendered = render_agents_md(&claude_md);
    std::fs::write(&out, &rendered)
        .with_context(|| format!("Could not write {}", out.display()))?;

    println!(
        "AGENTS.md written from CLAUDE.md ({} bytes)",
        rendered.len()
    );
    Ok(())
}

/// Validate one commit message file — the whole body of the `commit-msg` hook, and what
/// CI runs per commit. See `xtask/src/commit.rs` for the rules.
///
/// The only subcommand that takes arguments, because git names a different temporary file
/// each time. `--structure-only` drops the AI-attribution rule; it exists so the structure
/// claim can be checked against this repository's entire history, 148 commits of which
/// predate the attribution rule and carry the trailers it rejects.
fn check_commit_msg() -> Result<()> {
    const USAGE: &str = "Usage: cargo xtask check-commit-msg [--structure-only] <file>";

    let mut structure_only = false;
    let mut path = None;
    for arg in std::env::args().skip(2) {
        match arg.as_str() {
            "--structure-only" => structure_only = true,
            other if other.starts_with('-') => bail!("Unknown option '{other}'. {USAGE}"),
            other => path = Some(other.to_owned()),
        }
    }
    let Some(path) = path else {
        bail!("{USAGE}. Git passes the message file to the commit-msg hook as $1.")
    };

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Could not read the commit message file {path}"))?;
    let violations = commit::check(&raw, structure_only);

    if violations.is_empty() {
        let checked = if structure_only {
            "subject shape and a known type"
        } else {
            "subject shape, a known type, a blank line before the body, and no AI attribution trailer"
        };
        println!("commit message OK — {checked}");
        return Ok(());
    }

    eprintln!(
        "Commit message rejected ({} problem(s)):\n",
        violations.len()
    );
    for violation in &violations {
        eprintln!("  {violation}");
    }
    eprintln!(
        "\nThe rules live in xtask/src/commit.rs, and CI applies the same ones to the \
         commits in each push, so `git commit --no-verify` postpones this rather than \
         avoiding it. Run `cargo xtask setup` on a new machine to install the hook."
    );
    bail!("commit message check failed")
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
             every service that runs lapidary-server sets LAPIDARY_ROLE, every kernel-linked \
             service sets it to worker and something does, and lapidary-api never names \
             SourceStore ({} source file(s) checked)",
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
             (xtask/src/deploy.rs) may set SERVER_FEATURES. Every service that builds \
             deploy/Containerfile (i.e. runs lapidary-server) must set LAPIDARY_ROLE \
             explicitly — it has no default, so a compose file that loses it would start a \
             worker container that silently never mounts /scan, and a kernel-linked \
             service must set it to `worker` specifically, since any other value produces \
             that same container by a different route. The open path also never touches a \
             source file — lapidary-api must never name SourceStore."
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

/// Scans every `.rs` file under `crates/`, `xtask/` and `bin/` for a string literal
/// holding a mangled `\`-continuation — see `xtask/src/strings.rs` for what that looks
/// like and why `cargo fmt --check` cannot catch it on its own. Does not touch `web/`:
/// that tree is not Rust source and this check has nothing to say about it.
fn check_strings() -> Result<()> {
    let root = workspace_root()?;
    let mut sources = Vec::new();
    for dir in ["crates", "xtask", "bin"] {
        collect_rs_files(&root.join(dir), &mut sources)?;
    }

    let mut violations = Vec::new();
    for (path, contents) in &sources {
        // Report relative to the workspace root, matching how EXEMPT's entries and every
        // other check in this crate name a file, and so this check's own output stays
        // stable if the workspace is checked out somewhere other than /home/.../Lapidary.
        let relative = path
            .strip_prefix(&format!("{}/", root.display()))
            .unwrap_or(path);
        violations.extend(
            strings::check_source(relative, contents)
                .map_err(|e| anyhow::anyhow!(e))
                .with_context(|| format!("Could not scan {relative}"))?,
        );
    }

    if violations.is_empty() {
        println!(
            "string literal check OK — no mangled continuations found ({} source file(s) checked)",
            sources.len()
        );
        Ok(())
    } else {
        eprintln!(
            "String literal check failed ({} problem(s)):\n",
            violations.len()
        );
        for v in &violations {
            eprintln!("  {v}");
        }
        eprintln!(
            "\nEach of these is a string literal containing a run of three or more space \
             characters between two words — the shape a `\\`-continuation leaves behind when \
             something (a code-generation step, a find-and-replace, a tool that pre-processes \
             the text before Rust ever sees it) strips the backslash and newline but not the \
             following line's indentation. `cargo fmt --check` does not look inside string \
             literals, so this passes fmt, clippy and every test silently — this check is what \
             catches it. Fix the message (collapse it to one line, or use a real `\\`-continuation \
             written directly rather than generated), or if the spacing is genuinely \
             intentional (a YAML/Dockerfile fixture, reproduced external output), add a narrow, \
             commented entry to EXEMPT in xtask/src/strings.rs naming exactly this file and line."
        );
        bail!("string literal check failed")
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

/// Regenerate the TypeScript bindings from every `#[ts(export)]` type in the workspace.
///
/// ts-rs generates one `#[test] fn export_bindings_<type>()` per `#[ts(export)]` type, in
/// whichever crate defines the type. This used to run `cargo test -p lapidary-core
/// export_bindings` — hardcoded to the one crate that held every such type at the time.
/// Task 10 added `PartCard`/`PartsPage` in `lapidary-api`, and that crate's export tests
/// never ran: the hardcoded `-p lapidary-core` cannot see them. Worse, CI's staleness gate
/// (`export-bindings` then `git status --porcelain -- web/src/bindings`) diffs the output
/// of that same blind command against the committed files, so it would have reported
/// clean — a gate that cannot see new types is worse than no gate, because it looks like
/// coverage.
///
/// `cargo test --workspace export_bindings` fixes that by construction rather than by a
/// maintained list of crate names (a list is the same defect one level up — silently
/// missing a crate the next time one gains a `#[ts(export)]` type): the `export_bindings`
/// name filter runs across every workspace crate, so any crate holding such a type is
/// covered automatically, the moment the type exists. The filter also keeps this out of
/// the database: every `#[sqlx::test]` integration test in the workspace is named for what
/// it tests, never for exporting bindings, so `--workspace` picks up none of them — this
/// runs with no `DATABASE_URL` needed and no risk of failing for want of a live Postgres.
///
/// One more failure mode, found in review: `cargo test` exits 0 when its filter matches
/// zero tests — there is nothing to fail. Before this function cleared
/// `web/src/bindings/`, ran the (matched-nothing) export, and printed "bindings written
/// to ..." over an empty directory: a filter matching nothing looked identical to
/// success, and every committed binding was gone. Reachable with no code edit here at
/// all — any ts-rs upgrade that changes the generated test-name prefix (today's is
/// `export_bindings_<type>`) triggers it. So this now asks `cargo test` to *list* what
/// the filter matches, with `--list`, before touching anything on disk: if that list is
/// empty, it stops right there and the committed bindings are never cleared. After the
/// real run, it also counts the `.ts` files actually written and compares that count to
/// how many tests the list step predicted, so a type whose test passed but which failed
/// to write its file for some other reason is caught too, not just the all-or-nothing
/// case.
fn export_bindings() -> Result<()> {
    let root = workspace_root()?;
    let out = root.join("web/src/bindings");
    export_bindings_into(&out)
}

/// The real logic, taking the output directory as a parameter rather than hardcoding
/// `web/src/bindings` so the bail paths below can be exercised against a scratch
/// directory in tests instead of the committed one.
fn export_bindings_into(out: &Path) -> Result<()> {
    // Ask cargo how many tests the filter matches *before* touching anything on disk —
    // see this function's doc for why a successful run alone cannot be trusted to mean
    // "something was exported".
    let list = Command::new(env!("CARGO"))
        .args(["test", "--workspace", "export_bindings", "--", "--list"])
        .output()
        .context("Could not list the ts-rs export tests")?;
    if !list.status.success() {
        bail!(
            "Could not list the ts-rs export tests: {}",
            String::from_utf8_lossy(&list.stderr)
        );
    }
    let expected = count_listed_tests(&String::from_utf8_lossy(&list.stdout));
    if expected == 0 {
        bail!(
            "`cargo test --workspace export_bindings` matches zero tests, so nothing would be exported. This usually means a ts-rs upgrade changed the generated `export_bindings_<type>` test name pattern. Nothing on disk was touched — fix the filter (or ts-rs's generated name) before running this again."
        );
    }

    // ts-rs writes on test run; clear first so removed types do not linger.
    if out.exists() {
        std::fs::remove_dir_all(out).context(
            "Could not clear web/src/bindings. If it was partially cleared, run `git checkout -- web/src/bindings` to restore the committed files.",
        )?;
    }
    std::fs::create_dir_all(out).context(
        "Could not create web/src/bindings. Run `git checkout -- web/src/bindings` to restore the committed files if the directory was already cleared.",
    )?;

    let status = Command::new(env!("CARGO"))
        .args(["test", "--workspace", "export_bindings"])
        .env("TS_RS_EXPORT_DIR", out)
        .status()
        .context("Could not run the ts-rs export tests")?;

    if !status.success() {
        // The output directory was cleared above, so the previously committed bindings are
        // gone from the working tree. Say so — the user needs the recovery step, not just
        // the diagnosis.
        bail!(
            "ts-rs export failed, and web/src/bindings/ was cleared before the attempt, so the committed bindings are missing from your working tree. Run `git checkout -- web/src/bindings` to restore them, then `cargo test --workspace export_bindings` to see which type could not be exported."
        );
    }

    let written = count_ts_files(out)?;
    if written == 0 {
        bail!(
            "The export tests reported success but wrote no bindings, and web/src/bindings/ was cleared before the attempt, so nothing was written back. Run `git checkout -- web/src/bindings` to restore the committed files, then run `cargo test --workspace export_bindings` directly to see what happened."
        );
    }
    if written != expected {
        bail!(
            "Expected {expected} binding(s) — one per matching export test — but found {written} file(s) in web/src/bindings/ after a successful run. Some type did not write its file. Run `git checkout -- web/src/bindings` to restore the committed files, then run `cargo test --workspace export_bindings` directly to see which type is missing."
        );
    }

    println!("bindings written to {} ({written} file(s))", out.display());
    Ok(())
}

/// The prefix ts-rs gives every test it generates for a `#[ts(export)]` type:
/// `export_bindings_<lowercased type name>`. One such test writes exactly one `.ts` file,
/// which is what makes the count comparison below meaningful.
const TS_RS_TEST_PREFIX: &str = "export_bindings_";

/// How many ts-rs export tests `cargo test ... -- --list` reports as matched, summed
/// across every test binary section in its stdout. Each matching test is printed as its
/// own `<module path>::<name>: test` line; each binary's non-matching count is summarised
/// as `N tests, 0 benchmarks` and contributes nothing here.
///
/// Only the *last* path segment is checked against `TS_RS_TEST_PREFIX`, and that is the
/// whole point rather than a detail. `cargo test export_bindings` filters on a substring
/// of the full path, so a test in a MODULE named `export_bindings_something` matches the
/// filter too — this file's own tests used to live in `mod export_bindings_tests`, and
/// their four names inflated the prediction from 11 to 15. The comparison then failed on
/// a correct export, which is how a gate gets deleted. The module is renamed below so it
/// no longer matches at all; this rule is the second, independent guard, and it is the
/// one that holds for a module nobody thought to rename.
fn count_listed_tests(list_stdout: &str) -> usize {
    list_stdout
        .lines()
        .filter_map(|line| line.trim_end().strip_suffix(": test"))
        .filter(|name| {
            name.rsplit("::")
                .next()
                .is_some_and(|last| last.starts_with(TS_RS_TEST_PREFIX))
        })
        .count()
}

/// Counts `.ts` files directly inside `dir` — ts-rs writes one flat file per exported
/// type, no subdirectories.
fn count_ts_files(dir: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).context("Could not read the bindings output directory")? {
        let entry = entry.context("Could not read an entry in the bindings output directory")?;
        if entry.path().extension().is_some_and(|ext| ext == "ts") {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
// NOT named `export_bindings_*`: `cargo test --workspace export_bindings` filters on a
// substring of the full test path, so a module whose own name contains the filter makes
// its tests match too. That is exactly what happened — these four inflated the predicted
// binding count from 11 to 15, and `cargo xtask export-bindings` failed on a perfectly
// correct export, leaving CI's staleness gate red for a reason that had nothing to do
// with the bindings.
mod bindings_command_tests {
    use super::{count_listed_tests, count_ts_files};

    #[test]
    fn counts_zero_when_every_section_matches_nothing() {
        // The exact shape `cargo test <bogus-filter> -- --list` produces: every binary
        // section reports its summary line, no test-name lines anywhere. This is the
        // real-world shape reproduced against a filter matching no test in the
        // workspace — see the fix report for the literal command output.
        let listing = "     Running unittests src/lib.rs (target/debug/deps/lapidary_core-abc123)
0 tests, 0 benchmarks
     Running unittests src/lib.rs (target/debug/deps/lapidary_api-def456)
0 tests, 0 benchmarks
";
        assert_eq!(count_listed_tests(listing), 0);
    }

    #[test]
    fn sums_matched_tests_across_every_binary_section() {
        // Mirrors the real two-crate shape today: lapidary-core's 9 export tests and
        // lapidary-api's 2, interleaved with other binaries that match nothing.
        let listing = "     Running unittests src/lib.rs (target/debug/deps/lapidary_api-abc123)
parts::export_bindings_partcard: test
parts::export_bindings_partspage: test

2 tests, 0 benchmarks
     Running unittests src/lib.rs (target/debug/deps/lapidary_cad-def456)
0 tests, 0 benchmarks
     Running unittests src/lib.rs (target/debug/deps/lapidary_core-ghi789)
ids::export_bindings_blobhash: test
ids::export_bindings_libraryid: test
ids::export_bindings_partid: test
ids::export_bindings_revisionid: test
measurement::export_bindings_meshmeasurements: test
measurement::export_bindings_provenance: test
approximate::export_bindings_approximate: test
part::export_bindings_librarymode: test
part::export_bindings_partsummary: test

9 tests, 0 benchmarks
";
        assert_eq!(count_listed_tests(listing), 11);
    }

    #[test]
    fn a_test_matching_only_by_its_module_name_is_not_counted_as_a_binding() {
        // The regression this closes, in its real shape: xtask's own tests lived in
        // `mod export_bindings_tests`, so `cargo test --workspace export_bindings`
        // matched all four. The predicted count went 11 -> 15, the comparison against 11
        // written files failed, and `cargo xtask export-bindings` exited 1 on a correct
        // export — leaving CI's bindings staleness gate red for a reason that had
        // nothing to do with the bindings. Only the LAST path segment counts.
        let listing = "     Running unittests src/lib.rs (target/debug/deps/lapidary_api-abc123)
parts::export_bindings_partcard: test

1 test, 0 benchmarks
     Running unittests src/main.rs (target/debug/deps/xtask-def456)
export_bindings_tests::counts_zero_ts_files_in_an_empty_directory: test
export_bindings_tests::sums_matched_tests_across_every_binary_section: test

2 tests, 0 benchmarks
";
        assert_eq!(count_listed_tests(listing), 1);
    }

    #[test]
    fn counts_ts_files_and_ignores_everything_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("PartCard.ts"), "export type PartCard = {};")
            .expect("write");
        std::fs::write(
            dir.path().join("PartsPage.ts"),
            "export type PartsPage = {};",
        )
        .expect("write");
        // A non-.ts file in the same directory must not be counted — this is what
        // distinguishes "count .ts files" from "count directory entries".
        std::fs::write(dir.path().join("README.md"), "not a binding").expect("write");
        assert_eq!(count_ts_files(dir.path()).expect("count"), 2);
    }

    #[test]
    fn counts_zero_ts_files_in_an_empty_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(count_ts_files(dir.path()).expect("count"), 0);
    }
}
