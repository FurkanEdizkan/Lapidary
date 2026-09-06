//! The verification bar, in tiers.
//!
//! Every gate here already existed; what did not exist was a way to run fewer than all of
//! them. `docs/superpowers/plans/2026-09-05-phase-1-slice-5-browser.md` sets the rule as
//! "ten gates, the bar, every task", and slice 5 was six implementation commits, six
//! reviews and five fix rounds — each round paying all ten.
//!
//! Measured on 2026-09-06, warm, on a 12-core machine: the ten gates are **39.7 s**, of
//! which `cargo test` is 27.1 s and the four text checks are 0.56 s *combined*. The bar
//! was never slow. It was paid too often, and the part that is nearly free was welded to
//! the part that is not.
//!
//! So the tiers split on cost, not on importance:
//!
//! - [`Tier::Fast`] — the four gates that read files and compile nothing. Sub-second, so
//!   it can run on every commit without anyone deciding whether it is worth it. This is
//!   the tier that catches the class nothing else can see: `check-strings` inspects
//!   string literal *contents*, which fmt, clippy and every test are blind to.
//! - [`Tier::Task`] — the whole bar, paid once per task rather than once per commit.
//! - [`Tier::Slice`] — the same, with the change-gated gates forced on, for a merge.
//!
//! **Nothing is weakened.** No gate is dropped from `Tier::Task`; two are skipped when the
//! diff proves they cannot have anything to say. `cargo deny check` answers a question
//! about the dependency graph, so it runs when `Cargo.lock`, a manifest or `deny.toml`
//! moved. The web suite answers a question about `web/`, so it runs when `web/` moved —
//! and because `cargo xtask export-bindings` writes into `web/src/bindings`, a Rust change
//! that alters an exported type shows up as a `web/` change and pulls the suite back in.
//! That coupling is why one glob is enough, and it is the reason the web suite joined the
//! bar at slice 4: `tsc --noEmit` had been red for nine tasks with no gate noticing.
//!
//! When the diff cannot be determined at all, every gate runs. The `commits` CI job skips
//! and says so in the same situation, because there the safe direction is less work; here
//! it is more.
//!
//! `check-commit-msg` is deliberately absent. It is already per-commit, as the
//! `.githooks/commit-msg` shim, and a message is not a property of the working tree.

/// How much of the bar to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Every commit. Reads files, compiles nothing.
    Fast,
    /// Every task. The whole bar, with the two change-gated gates gated.
    Task,
    /// Before a merge. The whole bar, nothing gated.
    Slice,
}

impl Tier {
    /// Parse the subcommand argument. `None` — no argument — is [`Tier::Task`], because
    /// the unqualified request "run the bar" means the bar, not a subset of it.
    pub fn parse(arg: Option<&str>) -> Result<Self, String> {
        match arg {
            None | Some("task") => Ok(Tier::Task),
            Some("fast") => Ok(Tier::Fast),
            Some("slice") => Ok(Tier::Slice),
            Some(other) => Err(format!(
                "Unknown verify tier '{other}'. Use `fast` (every commit), `task` (the \
                 whole bar, the default) or `slice` (the whole bar with nothing gated)."
            )),
        }
    }
}

/// An xtask check that already lives in this binary. Called directly rather than shelled
/// as `cargo xtask ...`, so the tier costs one process instead of five.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Layers,
    Deploy,
    Strings,
    ExportBindings,
    ExportAgentsMd,
}

/// One thing to do, in order. The first failure stops the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Run one of this binary's own checks in-process.
    Internal { name: &'static str, check: Check },
    /// Shell out, streaming the child's output so a clippy diagnostic is readable.
    Command {
        name: &'static str,
        program: &'static str,
        args: &'static [&'static str],
    },
    /// A generated file that must already be committed and current: `git status
    /// --porcelain -- <path>` must print nothing.
    ///
    /// `git status`, not `git diff`: the export deletes and recreates the bindings
    /// directory, so a type newly given `#[ts(export)]` produces an *untracked* file, and
    /// `git diff` never reports those. The stalest possible case would pass silently.
    Generated {
        name: &'static str,
        path: &'static str,
        fix: &'static str,
    },
}

/// Paths whose change can make `cargo deny check` say something new.
fn touches_dependencies(path: &str) -> bool {
    path == "Cargo.lock"
        || path == "deny.toml"
        || path == "Cargo.toml"
        || path.ends_with("/Cargo.toml")
}

/// The frontend, including `web/src/bindings` — which is how a Rust change to an exported
/// type reaches this gate.
fn touches_web(path: &str) -> bool {
    path.starts_with("web/")
}

/// The steps for `tier`, in the order they should run.
///
/// `changed` is the set of paths this working tree and branch have touched, or `None` when
/// that could not be determined — in which case nothing is gated out.
///
/// Ordering is not cosmetic. `clippy` cannot share artifacts with `cargo test` (the
/// wrapper changes the fingerprint), so that pair costs two workspace builds no matter
/// what. `export-bindings` shells `cargo test --workspace export_bindings -- --list`,
/// which compiles and links every test target — placing it *after* `cargo test` makes both
/// of its invocations cache hits instead of a third build.
pub fn steps(tier: Tier, changed: Option<&[String]>) -> Vec<Step> {
    let mut steps = vec![
        Step::Command {
            name: "fmt",
            program: "cargo",
            args: &["fmt", "--all", "--check"],
        },
        Step::Internal {
            name: "check-layers",
            check: Check::Layers,
        },
        Step::Internal {
            name: "check-deploy",
            check: Check::Deploy,
        },
        Step::Internal {
            name: "check-strings",
            check: Check::Strings,
        },
    ];

    if tier == Tier::Fast {
        return steps;
    }

    steps.push(Step::Command {
        name: "clippy",
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    });
    steps.push(Step::Command {
        name: "test",
        program: "cargo",
        args: &["test", "--workspace", "--all-features"],
    });
    steps.push(Step::Internal {
        name: "export-bindings",
        check: Check::ExportBindings,
    });
    steps.push(Step::Generated {
        name: "bindings are current",
        path: "web/src/bindings",
        fix: "cargo xtask export-bindings",
    });
    steps.push(Step::Internal {
        name: "export-agents-md",
        check: Check::ExportAgentsMd,
    });
    steps.push(Step::Generated {
        name: "AGENTS.md is current",
        path: "AGENTS.md",
        fix: "cargo xtask export-agents-md",
    });

    // Unknown changes means run it: the gate is an optimization, and an optimization that
    // guesses wrong here hides a real answer.
    let runs_deny = tier == Tier::Slice
        || changed.is_none_or(|paths| paths.iter().any(|p| touches_dependencies(p)));
    let runs_web =
        tier == Tier::Slice || changed.is_none_or(|paths| paths.iter().any(|p| touches_web(p)));

    if runs_deny {
        steps.push(Step::Command {
            name: "deny",
            program: "cargo",
            args: &["deny", "check"],
        });
    }

    if runs_web {
        // `npm run build` is `tsc --noEmit && vite build`, so it already is the typecheck
        // script; running both would compile the TypeScript twice for one answer.
        steps.push(Step::Command {
            name: "web tests",
            program: "npm",
            args: &["--prefix", "web", "test"],
        });
        steps.push(Step::Command {
            name: "web build",
            program: "npm",
            args: &["--prefix", "web", "run", "build"],
        });
        steps.push(Step::Generated {
            name: "route tree is current",
            path: "web/src/routeTree.gen.ts",
            fix: "npm --prefix web run build",
        });
    }

    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(steps: &[Step]) -> Vec<&'static str> {
        steps
            .iter()
            .map(|s| match s {
                Step::Internal { name, .. }
                | Step::Command { name, .. }
                | Step::Generated { name, .. } => *name,
            })
            .collect()
    }

    #[test]
    fn the_fast_tier_compiles_nothing() {
        let steps = steps(Tier::Fast, Some(&[]));
        assert_eq!(
            names(&steps),
            ["fmt", "check-layers", "check-deploy", "check-strings"]
        );
    }

    #[test]
    fn the_fast_tier_ignores_the_diff_entirely() {
        let touched_everything = ["Cargo.lock".to_owned(), "web/src/main.tsx".to_owned()];
        assert_eq!(
            steps(Tier::Fast, Some(&touched_everything)),
            steps(Tier::Fast, Some(&[]))
        );
    }

    #[test]
    fn a_rust_only_change_skips_deny_and_the_web_suite() {
        let changed = ["crates/lapidary-cad/src/raster.rs".to_owned()];
        let got = names(&steps(Tier::Task, Some(&changed)));
        assert!(!got.contains(&"deny"), "deny ran for a source-only change");
        assert!(
            !got.contains(&"web tests"),
            "web ran for a Rust-only change"
        );
        // The gates that always run are still all there.
        assert!(got.contains(&"clippy") && got.contains(&"test") && got.contains(&"fmt"));
    }

    #[test]
    fn a_lockfile_change_brings_deny_back() {
        let changed = ["Cargo.lock".to_owned()];
        assert!(names(&steps(Tier::Task, Some(&changed))).contains(&"deny"));
    }

    #[test]
    fn a_member_manifest_change_brings_deny_back() {
        let changed = ["crates/lapidary-cad/Cargo.toml".to_owned()];
        assert!(names(&steps(Tier::Task, Some(&changed))).contains(&"deny"));
    }

    #[test]
    fn a_web_change_brings_the_web_suite_back() {
        let changed = ["web/src/lib/strings.ts".to_owned()];
        assert!(names(&steps(Tier::Task, Some(&changed))).contains(&"web tests"));
    }

    /// The coupling that makes one glob enough: a Rust type gaining `#[ts(export)]` shows
    /// up as a change under `web/src/bindings`, which is under `web/`.
    #[test]
    fn a_regenerated_binding_counts_as_a_web_change() {
        let changed = ["web/src/bindings/PartCard.ts".to_owned()];
        assert!(names(&steps(Tier::Task, Some(&changed))).contains(&"web tests"));
    }

    #[test]
    fn an_undeterminable_diff_runs_every_gate() {
        let got = names(&steps(Tier::Task, None));
        assert!(got.contains(&"deny"));
        assert!(got.contains(&"web tests"));
    }

    #[test]
    fn the_slice_tier_gates_nothing_out() {
        let changed = ["README.md".to_owned()];
        assert_eq!(
            names(&steps(Tier::Slice, Some(&changed))),
            names(&steps(Tier::Task, None))
        );
    }

    /// `export-bindings` shells `cargo test ... -- --list`, which links every test target.
    /// After `cargo test` that is a cache hit; before it, a third workspace build.
    ///
    /// Named around the filter rather than into it: `export_bindings` counts the tests
    /// that `cargo test --workspace export_bindings -- --list` matches and asserts one
    /// binding file per match, so any test in this workspace whose *name* contains
    /// `export_bindings` inflates that count and fails the gate. This one did, and the
    /// gate caught it.
    #[test]
    fn the_bindings_step_runs_after_the_test_suite() {
        let got = names(&steps(Tier::Task, None));
        let test = got.iter().position(|n| *n == "test").expect("test runs");
        let bindings = got
            .iter()
            .position(|n| *n == "export-bindings")
            .expect("bindings export runs");
        assert!(test < bindings, "export-bindings must follow cargo test");
    }

    #[test]
    fn every_generated_file_is_checked_after_the_command_that_writes_it() {
        let got = names(&steps(Tier::Task, None));
        for (writer, gate) in [
            ("export-bindings", "bindings are current"),
            ("export-agents-md", "AGENTS.md is current"),
            ("web build", "route tree is current"),
        ] {
            let w = got.iter().position(|n| *n == writer).expect("writer runs");
            let g = got.iter().position(|n| *n == gate).expect("gate runs");
            assert!(w < g, "{gate} must follow {writer}");
        }
    }

    #[test]
    fn an_unknown_tier_is_refused_by_name() {
        let err = Tier::parse(Some("everything")).expect_err("unknown tier is refused");
        assert!(
            err.contains("everything"),
            "the message names what was typed"
        );
        assert!(err.contains("fast"), "the message lists what is valid");
    }

    #[test]
    fn no_argument_means_the_whole_bar_not_the_cheap_one() {
        assert_eq!(Tier::parse(None), Ok(Tier::Task));
    }

    /// The commit message is not a property of the working tree, and the hook already runs
    /// it on every commit. A second copy here would drift from `.githooks/commit-msg`.
    #[test]
    fn check_commit_msg_is_not_one_of_the_steps() {
        let got = names(&steps(Tier::Slice, None));
        assert!(!got.iter().any(|n| n.contains("commit")));
    }
}
