//! What a fresh clone needs before it is workable, decided as a pure function.
//!
//! `main.rs` runs the three read-only commands (`git config`, `claude plugin marketplace
//! list --json`, `claude plugin list --json`), hands the results here, and executes the
//! actions that come back. That split matters more here than for the other checks:
//! `setup` is the first xtask subcommand that *changes machine state* rather than
//! reporting on it, and the interesting cases — a marketplace registered from the wrong
//! repository, a plugin naming a marketplace nobody declared — are the ones least
//! convenient to reproduce live.
//!
//! # Why this exists at all
//!
//! `.claude/settings.json` declares the marketplaces and plugins this repository needs,
//! and Claude Code applies it when it opens the project. Nothing else does. There was no
//! way to ask a terminal what a machine was missing, no way to prepare a machine before
//! opening Claude Code, and no way at all to get the project's skills onto a machine whose
//! agent is not Claude Code. This reconciles toward that file and never writes to it: the
//! JSON stays the single source of truth.

/// A marketplace as `claude plugin marketplace list --json` reports it.
#[derive(Debug, PartialEq, Eq)]
pub struct Marketplace {
    pub name: String,
    pub repo: String,
}

/// A plugin as `claude plugin list --json` reports it.
#[derive(Debug, PartialEq, Eq)]
pub struct Plugin {
    pub id: String,
    pub enabled: bool,
}

/// One thing `main.rs` should do, or tell the operator about. Reports are actions too:
/// a setup command that silently does nothing when everything is correct leaves you
/// unable to tell "already fine" from "did not look".
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    AddMarketplace {
        name: String,
        repo: String,
    },
    InstallPlugin {
        id: String,
    },
    EnablePlugin {
        id: String,
    },
    /// Clone `repo` and copy its `skills/` into `.agents/skills/<name>/`.
    MaterializeSkills {
        name: String,
        repo: String,
    },
    /// The marketplace is registered, but from a different repository than this project
    /// declares. Reported and never "fixed": re-pointing a marketplace someone else's
    /// project may depend on is not this command's call to make.
    MarketplaceMismatch {
        name: String,
        declared: String,
        registered: String,
    },
    /// A plugin names a marketplace that is neither registered nor declared here.
    /// `claude-plugins-official` is the live case — it ships with Claude Code, so this
    /// repository deliberately does not declare it, and on a machine where it is somehow
    /// absent the right answer is a command to run, not a source this file invented.
    UnknownMarketplace {
        plugin: String,
        marketplace: String,
    },
    /// A marketplace whose source this command cannot clone (anything but `github`).
    UnsupportedSource {
        name: String,
        kind: String,
    },
    AlreadyCorrect {
        what: String,
    },
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::AddMarketplace { name, repo } => {
                write!(f, "register marketplace {name} from {repo}")
            }
            Action::InstallPlugin { id } => write!(f, "install {id}"),
            Action::EnablePlugin { id } => write!(f, "enable {id} (installed but off)"),
            Action::MaterializeSkills { name, repo } => {
                write!(f, "materialize {repo}'s skills into .agents/skills/{name}/")
            }
            Action::MarketplaceMismatch {
                name,
                declared,
                registered,
            } => write!(
                f,
                "marketplace {name} is registered from {registered} but this project \
                 declares {declared}. Left alone — re-pointing a marketplace another \
                 project may rely on is not this command's decision. Run `claude plugin \
                 marketplace remove {name}` first if the declared one is the one you want."
            ),
            Action::UnknownMarketplace {
                plugin,
                marketplace,
            } => write!(
                f,
                "{plugin} needs marketplace {marketplace}, which is neither registered nor \
                 declared in .claude/settings.json. If it is the official one it normally \
                 ships with Claude Code; otherwise run `claude plugin marketplace add \
                 <source>` for it."
            ),
            Action::UnsupportedSource { name, kind } => write!(
                f,
                "marketplace {name} has a '{kind}' source, and skills can only be \
                 materialized from a github source. Its plugins still install normally \
                 through the Claude CLI."
            ),
            Action::AlreadyCorrect { what } => write!(f, "{what} already correct"),
        }
    }
}

/// The `extraKnownMarketplaces` entries, as `(name, source-kind, repo)`.
///
/// `additionalMarketplaces` is read as a fallback because the settings schema documents it
/// as an exact alias, and a file written by a newer Claude Code may use it.
fn declared_marketplaces(settings: &serde_json::Value) -> Vec<(String, String, String)> {
    let block = settings
        .get("extraKnownMarketplaces")
        .or_else(|| settings.get("additionalMarketplaces"));
    let Some(object) = block.and_then(|b| b.as_object()) else {
        return Vec::new();
    };
    object
        .iter()
        .map(|(name, entry)| {
            let source = entry.get("source");
            let kind = source
                .and_then(|s| s.get("source"))
                .and_then(|k| k.as_str())
                .unwrap_or("unknown");
            let repo = source
                .and_then(|s| s.get("repo"))
                .and_then(|r| r.as_str())
                .unwrap_or_default();
            (name.clone(), kind.to_owned(), repo.to_owned())
        })
        .collect()
}

/// The plugin ids set to `true` in `enabledPlugins`. A `false` is a deliberate opt-out and
/// is left alone.
fn declared_plugins(settings: &serde_json::Value) -> Vec<String> {
    let Some(object) = settings.get("enabledPlugins").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    object
        .iter()
        .filter(|(_, want)| want.as_bool() == Some(true))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Everything `setup` should do, given what the project declares and what the machine has.
pub fn plan_actions(
    settings: &serde_json::Value,
    markets: &[Marketplace],
    plugins: &[Plugin],
) -> Vec<Action> {
    let mut actions = Vec::new();
    let declared = declared_marketplaces(settings);

    for (name, kind, repo) in &declared {
        match markets.iter().find(|m| &m.name == name) {
            None => actions.push(Action::AddMarketplace {
                name: name.clone(),
                repo: repo.clone(),
            }),
            Some(registered) if &registered.repo != repo && !repo.is_empty() => {
                actions.push(Action::MarketplaceMismatch {
                    name: name.clone(),
                    declared: repo.clone(),
                    registered: registered.repo.clone(),
                });
            }
            Some(_) => actions.push(Action::AlreadyCorrect {
                what: format!("marketplace {name}"),
            }),
        }

        if kind == "github" && !repo.is_empty() {
            actions.push(Action::MaterializeSkills {
                name: name.clone(),
                repo: repo.clone(),
            });
        } else {
            actions.push(Action::UnsupportedSource {
                name: name.clone(),
                kind: kind.clone(),
            });
        }
    }

    for id in declared_plugins(settings) {
        let marketplace = id.split_once('@').map(|(_, m)| m).unwrap_or_default();
        let known = markets.iter().any(|m| m.name == marketplace)
            || declared.iter().any(|(name, _, _)| name == marketplace);
        if !known {
            actions.push(Action::UnknownMarketplace {
                plugin: id.clone(),
                marketplace: marketplace.to_owned(),
            });
            continue;
        }
        match plugins.iter().find(|p| p.id == id) {
            None => actions.push(Action::InstallPlugin { id }),
            Some(installed) if !installed.enabled => {
                actions.push(Action::EnablePlugin { id });
            }
            Some(_) => actions.push(Action::AlreadyCorrect { what: id }),
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("test settings parse")
    }

    /// The real `.claude/settings.json`, trimmed to what this function reads.
    fn lapidary_settings() -> serde_json::Value {
        // One line, no interior runs of spaces: a pretty-printed fixture would need
        // an EXEMPT entry, and those are pinned by line number and go stale on every
        // edit above them.
        settings(
            r#"{"extraKnownMarketplaces":{"furkan-skills":{"source":{"source":"github","repo":"FurkanEdizkan/My-Skills"}}},"enabledPlugins":{"my-skills@furkan-skills":true,"superpowers@claude-plugins-official":true}}"#,
        )
    }

    fn market(name: &str, repo: &str) -> Marketplace {
        Marketplace {
            name: name.to_owned(),
            repo: repo.to_owned(),
        }
    }

    fn plugin(id: &str, enabled: bool) -> Plugin {
        Plugin {
            id: id.to_owned(),
            enabled,
        }
    }

    #[test]
    fn a_fully_set_up_machine_only_reports() {
        let actions = plan_actions(
            &lapidary_settings(),
            &[
                market("furkan-skills", "FurkanEdizkan/My-Skills"),
                market(
                    "claude-plugins-official",
                    "anthropics/claude-plugins-official",
                ),
            ],
            &[
                plugin("my-skills@furkan-skills", true),
                plugin("superpowers@claude-plugins-official", true),
            ],
        );
        // Skills are re-materialized every run; that is a refresh, not a repair.
        assert!(actions.contains(&Action::MaterializeSkills {
            name: "furkan-skills".to_owned(),
            repo: "FurkanEdizkan/My-Skills".to_owned(),
        }));
        assert!(
            actions.iter().all(|a| matches!(
                a,
                Action::AlreadyCorrect { .. } | Action::MaterializeSkills { .. }
            )),
            "nothing should need installing: {actions:?}"
        );
    }

    #[test]
    fn a_bare_machine_registers_and_installs_everything() {
        let actions = plan_actions(&lapidary_settings(), &[], &[]);
        assert!(actions.contains(&Action::AddMarketplace {
            name: "furkan-skills".to_owned(),
            repo: "FurkanEdizkan/My-Skills".to_owned(),
        }));
        assert!(actions.contains(&Action::InstallPlugin {
            id: "my-skills@furkan-skills".to_owned()
        }));
    }

    #[test]
    fn a_plugin_naming_an_undeclared_unregistered_marketplace_is_reported_not_guessed() {
        // The live case: claude-plugins-official ships with Claude Code, so this project
        // deliberately does not declare it. On a machine without it, inventing a source
        // would be worse than saying so.
        let actions = plan_actions(&lapidary_settings(), &[], &[]);
        assert!(actions.contains(&Action::UnknownMarketplace {
            plugin: "superpowers@claude-plugins-official".to_owned(),
            marketplace: "claude-plugins-official".to_owned(),
        }));
        assert!(
            !actions.iter().any(|a| matches!(
                a,
                Action::InstallPlugin { id } if id.ends_with("@claude-plugins-official")
            )),
            "must not try to install from a marketplace it cannot resolve"
        );
    }

    #[test]
    fn a_marketplace_registered_from_a_different_repo_is_reported_and_left_alone() {
        let actions = plan_actions(
            &lapidary_settings(),
            &[market("furkan-skills", "SomeoneElse/Other-Skills")],
            &[],
        );
        assert!(actions.contains(&Action::MarketplaceMismatch {
            name: "furkan-skills".to_owned(),
            declared: "FurkanEdizkan/My-Skills".to_owned(),
            registered: "SomeoneElse/Other-Skills".to_owned(),
        }));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::AddMarketplace { .. })),
            "a mismatch must never be silently re-registered"
        );
    }

    #[test]
    fn an_installed_but_disabled_plugin_is_enabled_rather_than_reinstalled() {
        let actions = plan_actions(
            &lapidary_settings(),
            &[market("furkan-skills", "FurkanEdizkan/My-Skills")],
            &[plugin("my-skills@furkan-skills", false)],
        );
        assert!(actions.contains(&Action::EnablePlugin {
            id: "my-skills@furkan-skills".to_owned()
        }));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::InstallPlugin { .. }))
        );
    }

    #[test]
    fn a_plugin_set_to_false_is_a_deliberate_opt_out_and_is_skipped() {
        let json = settings(r#"{ "enabledPlugins": { "ponytail@ponytail": false } }"#);
        assert_eq!(plan_actions(&json, &[], &[]), vec![]);
    }

    #[test]
    fn a_non_github_marketplace_cannot_have_its_skills_materialized() {
        let json = settings(
            r#"{ "extraKnownMarketplaces": { "local": { "source": { "source": "directory", "path": "/opt/m" } } } }"#,
        );
        let actions = plan_actions(&json, &[], &[]);
        assert!(actions.contains(&Action::UnsupportedSource {
            name: "local".to_owned(),
            kind: "directory".to_owned(),
        }));
    }

    #[test]
    fn the_additional_marketplaces_alias_is_read_too() {
        let json = settings(
            r#"{ "additionalMarketplaces": { "x": { "source": { "source": "github", "repo": "o/r" } } } }"#,
        );
        assert!(
            plan_actions(&json, &[], &[]).contains(&Action::AddMarketplace {
                name: "x".to_owned(),
                repo: "o/r".to_owned(),
            })
        );
    }

    #[test]
    fn settings_with_neither_key_plans_nothing_rather_than_failing() {
        assert_eq!(plan_actions(&settings("{}"), &[], &[]), vec![]);
    }
}
