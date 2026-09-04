//! Commit message rules, checked by the `commit-msg` hook and by CI.
//!
//! Pure functions over `&str`, unit-testable without touching git or the filesystem —
//! `main.rs` reads the message file and calls these, mirroring the `deploy.rs` / `main.rs`
//! split.
//!
//! # Why this exists
//!
//! Every one of this repository's 167 non-merge commits already followed Conventional
//! Commits before this check was written, and nothing enforced it — the convention lived
//! only in the habit of whoever was typing. A convention that holds by habit stops holding
//! on the machine where the habit lapses, which is the same portability gap
//! `.claude/settings.json` closed for plugins.
//!
//! # What is deliberately not checked
//!
//! **Subject length.** Conventional Commits sets no limit, and this repository's style is
//! long, precise subjects: the median is 63 characters and the longest is 104. A 72-column
//! cap — the traditional git advice — would reject 29 of the 167 commits written so far,
//! including `fix(xtask): check ARG SERVER_FEATURES visibility to the build line, not just
//! position vs. the first FROM`, which is a good subject rather than a bad one. Nothing
//! here would be improved by making it shorter.
//!
//! # Why attribution matching is line-anchored
//!
//! 148 of this repository's 168 commits carry `Co-Authored-By:` and `Claude-Session:`
//! trailers from before the rule existed, and recent commit messages discuss those
//! trailers, and the `claude-plugins-official` marketplace, in prose — one of them across
//! eleven lines. A substring search for a vendor name would reject those messages for
//! saying the word.
//!
//! So a line is a trailer only by its *shape*: it starts with the trailer key, or it is a
//! footer, or it is a bare session URL. Prose that mentions a vendor anywhere else passes.
//! The rules below were run over all 168 commits before being written: 148 flagged, and
//! zero false positives among the eleven commits carrying the prose.

/// The commit types this repository uses, plus `perf` and `revert` from the Conventional
/// Commits spec. Kept as a closed list rather than "any lowercase word" so that `wip:`,
/// `update:` and `misc:` — the types that turn a log into noise — are rejected by name.
const TYPES: &[&str] = &[
    "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
];

/// Vendor names that make an attribution trailer an *AI* attribution trailer. Matched only
/// inside the anchored shapes below, never as a bare substring — see the module doc.
const VENDORS: &[&str] = &[
    "claude",
    "anthropic",
    "codex",
    "openai",
    "copilot",
    "gemini",
    "antigravity",
    "cursor",
    "windsurf",
    "devin",
];

/// Message prefixes git generates itself. Their shape is not ours to dictate, and a
/// `commit-msg` hook that rejects `git merge` or `git commit --fixup` breaks those commands
/// rather than improving them.
const GENERATED_PREFIXES: &[&str] = &["merge ", "revert ", "fixup!", "squash!", "amend!"];

#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    /// The subject does not have the shape `type(scope)!: description` at all.
    NotConventional { subject: String },
    /// The shape is right but the type is not one this repository uses.
    UnknownType { got: String },
    /// `feat(api): ` with nothing after the space.
    EmptyDescription { subject: String },
    /// The line after the subject is not blank, so git and every tool that reads a commit
    /// message will run the subject and the body together.
    BodyNotSeparated { second_line: String },
    /// An AI attribution trailer. `rule` names which shape matched, so a false positive is
    /// diagnosable from the message alone rather than by reading this file.
    AiAttribution { line: String, rule: &'static str },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::NotConventional { subject } => write!(
                f,
                "subject is not a Conventional Commit: '{subject}'. Expected \
                 `type(scope): description`, where the type is lowercase, the scope is \
                 optional and may list several comma-separated parts, and an optional `!` \
                 before the colon marks a breaking change. For example \
                 `feat(jobs): add the handler seam` or `fix(web,docs): say what is true`."
            ),
            Violation::UnknownType { got } => write!(
                f,
                "'{got}' is not a commit type this repository uses. Allowed: {}. Add a new \
                 one to TYPES in xtask/src/commit.rs if the project genuinely needs it — \
                 the list is closed so that vague types like `wip` or `update` cannot turn \
                 the log into noise.",
                TYPES.join(", ")
            ),
            Violation::EmptyDescription { subject } => write!(
                f,
                "subject '{subject}' has a type and a colon but no description. Say what \
                 the commit does."
            ),
            Violation::BodyNotSeparated { second_line } => write!(
                f,
                "the line after the subject must be blank, but it is '{second_line}'. Git \
                 treats the first paragraph as the subject, so without the blank line the \
                 body is folded into it everywhere a subject is shown."
            ),
            Violation::AiAttribution { line, rule } => write!(
                f,
                "'{line}' is an AI attribution trailer ({rule}). This repository's commit \
                 messages end at their real content — remove the line. Mentioning a vendor \
                 in prose is fine and is not what this matches; only the trailer, footer \
                 and bare-session-URL shapes are."
            ),
        }
    }
}

/// One parsed subject line. Borrowed rather than owned because every caller is inside the
/// lifetime of the message it came from.
struct Subject<'a> {
    kind: &'a str,
    description: &'a str,
}

/// `type(scope)!: description`, parsed by hand — this workspace has no regex dependency,
/// and `deploy.rs` and `strings.rs` both parse line-wise for the same reason.
fn parse_subject(subject: &str) -> Option<Subject<'_>> {
    let bytes = subject.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_lowercase() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let kind = &subject[..i];

    if bytes.get(i) == Some(&b'(') {
        let close = i + subject[i..].find(')')?;
        let inner = &subject[i + 1..close];
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b",._/-".contains(&b);
        if inner.is_empty() || !inner.bytes().all(allowed) {
            return None;
        }
        i = close + 1;
    }

    if bytes.get(i) == Some(&b'!') {
        i += 1;
    }
    if bytes.get(i) != Some(&b':') || bytes.get(i + 1) != Some(&b' ') {
        return None;
    }
    Some(Subject {
        kind,
        description: &subject[i + 2..],
    })
}

/// Which attribution shape this line is, if any. Anchored at the start of the trimmed
/// line in every case — that anchoring is the whole reason prose survives.
fn attribution_rule(line: &str) -> Option<&'static str> {
    let lower = line.trim().to_ascii_lowercase();
    let names_vendor = |s: &str| VENDORS.iter().any(|v| s.contains(v));

    if lower.starts_with("co-authored-by:") && names_vendor(&lower) {
        return Some("co-authored-by trailer");
    }

    for vendor in VENDORS {
        for separator in ['-', ' '] {
            let prefix = format!("{vendor}{separator}session:");
            if let Some(rest) = lower.strip_prefix(&prefix)
                && !rest.trim().is_empty()
            {
                return Some("session trailer");
            }
        }
    }

    // A footer such as "🤖 Generated with [Claude Code](...)" — leading punctuation and
    // emoji are stripped so the anchor still holds, but the line must *begin* with the
    // phrase, so "...was generated with Claude" in a sentence does not match.
    let unadorned = lower.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    if unadorned.starts_with("generated with") && names_vendor(&lower) {
        return Some("generated-with footer");
    }

    if (lower.starts_with("http://") || lower.starts_with("https://"))
        && names_vendor(&lower)
        && (lower.contains("/code/") || lower.contains("/session"))
    {
        return Some("bare session url");
    }

    None
}

/// Strip what git adds and the author never typed: comment lines, and everything from a
/// `--verbose` scissors line onward. Done here rather than assumed, because whether git
/// has already cleaned the file when `commit-msg` runs depends on the `--cleanup` mode,
/// and a hook that rejects every commit because of git's own comments is worse than no
/// hook.
fn strip_git_furniture(raw: &str) -> String {
    let mut kept = Vec::new();
    for line in raw.lines() {
        if line.starts_with("# ------------------------ >8 ------------------------") {
            break;
        }
        if line.starts_with('#') {
            continue;
        }
        kept.push(line);
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

/// Every rule. `structure_only` drops the attribution rule, which is what lets the
/// structure claim be checked against this repository's full history — 148 of those
/// commits predate the attribution rule and would otherwise bury the result.
pub fn check(raw: &str, structure_only: bool) -> Vec<Violation> {
    let message = strip_git_furniture(raw);
    let mut violations = Vec::new();

    // Parsed untrimmed, reported trimmed. `feat(api): ` is a subject whose description is
    // empty, and trimming first would erase the space that makes it parse at all, turning
    // a precise "no description" into a vague "not a Conventional Commit".
    let subject_raw = message.lines().next().unwrap_or("");
    let subject = subject_raw.trim_end();

    let lower_subject = subject.to_ascii_lowercase();
    let generated = GENERATED_PREFIXES
        .iter()
        .any(|prefix| lower_subject.starts_with(prefix));

    if !generated {
        match parse_subject(subject_raw) {
            None => violations.push(Violation::NotConventional {
                subject: subject.to_owned(),
            }),
            Some(parsed) => {
                if !TYPES.contains(&parsed.kind) {
                    violations.push(Violation::UnknownType {
                        got: parsed.kind.to_owned(),
                    });
                }
                if parsed.description.trim().is_empty() {
                    violations.push(Violation::EmptyDescription {
                        subject: subject.to_owned(),
                    });
                }
            }
        }

        if let Some(second) = message.lines().nth(1)
            && !second.trim().is_empty()
        {
            violations.push(Violation::BodyNotSeparated {
                second_line: second.trim_end().to_owned(),
            });
        }
    }

    // Attribution applies to generated messages too: a merge commit is as good a place to
    // smuggle a trailer as any, and git writes none of them itself.
    if !structure_only {
        for line in message.lines() {
            if let Some(rule) = attribution_rule(line) {
                violations.push(Violation::AiAttribution {
                    line: line.trim().to_owned(),
                    rule,
                });
            }
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_all(message: &str) -> Vec<Violation> {
        check(message, false)
    }

    #[test]
    fn a_plain_subject_passes() {
        assert_eq!(check_all("feat(jobs): add the handler seam"), vec![]);
    }

    #[test]
    fn a_subject_without_a_scope_passes() {
        assert_eq!(check_all("docs: correct the doc map"), vec![]);
    }

    #[test]
    fn a_breaking_change_marker_passes() {
        assert_eq!(check_all("feat!: delete the Node prototype"), vec![]);
    }

    #[test]
    fn a_breaking_change_marker_with_a_scope_passes() {
        assert_eq!(check_all("feat(api)!: drop the v1 route"), vec![]);
    }

    #[test]
    fn a_comma_separated_scope_passes() {
        // This repository really writes these: fix(web,docs), fix(api,db,xtask).
        assert_eq!(check_all("fix(api,db,xtask): grid endpoint review"), vec![]);
    }

    #[test]
    fn a_hyphenated_scope_passes() {
        assert_eq!(check_all("docs(slice-2): record the exit run"), vec![]);
    }

    #[test]
    fn a_long_subject_passes_because_there_is_no_length_rule() {
        let subject = "fix(xtask): check ARG SERVER_FEATURES visibility to the build line, \
                       not just position vs. the first FROM";
        assert!(subject.len() > 100);
        assert_eq!(check_all(subject), vec![]);
    }

    #[test]
    fn a_subject_with_no_type_is_rejected() {
        assert_eq!(
            check_all("Add new feature"),
            vec![Violation::NotConventional {
                subject: "Add new feature".to_owned()
            }]
        );
    }

    #[test]
    fn an_uppercase_type_is_rejected() {
        // Parsing stops at the first non-lowercase byte, so `Feat` never forms a type.
        assert_eq!(
            check_all("Feat(api): thing"),
            vec![Violation::NotConventional {
                subject: "Feat(api): thing".to_owned()
            }]
        );
    }

    #[test]
    fn a_missing_space_after_the_colon_is_rejected() {
        assert_eq!(
            check_all("feat(api):thing"),
            vec![Violation::NotConventional {
                subject: "feat(api):thing".to_owned()
            }]
        );
    }

    #[test]
    fn an_unclosed_scope_is_rejected() {
        assert_eq!(
            check_all("feat(api: thing"),
            vec![Violation::NotConventional {
                subject: "feat(api: thing".to_owned()
            }]
        );
    }

    #[test]
    fn an_uppercase_scope_is_rejected() {
        assert_eq!(
            check_all("feat(API): thing"),
            vec![Violation::NotConventional {
                subject: "feat(API): thing".to_owned()
            }]
        );
    }

    #[test]
    fn an_unknown_type_is_rejected_and_names_the_allowed_set() {
        let violations = check_all("wip: something");
        assert_eq!(
            violations,
            vec![Violation::UnknownType {
                got: "wip".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("refactor"));
    }

    #[test]
    fn an_empty_description_is_rejected() {
        assert_eq!(
            check_all("feat(api): "),
            vec![Violation::EmptyDescription {
                subject: "feat(api):".to_owned()
            }]
        );
    }

    #[test]
    fn a_body_separated_by_a_blank_line_passes() {
        assert_eq!(
            check_all("fix(db): guard the write\n\nBecause it raced."),
            vec![]
        );
    }

    #[test]
    fn a_body_run_into_the_subject_is_rejected() {
        assert_eq!(
            check_all("fix(db): guard the write\nBecause it raced."),
            vec![Violation::BodyNotSeparated {
                second_line: "Because it raced.".to_owned()
            }]
        );
    }

    #[test]
    fn merge_commits_are_exempt_from_the_structure_rules() {
        assert_eq!(check_all("Merge branch 'rust-rewrite' into main"), vec![]);
    }

    #[test]
    fn fixup_and_squash_commits_are_exempt() {
        assert_eq!(check_all("fixup! feat(jobs): add the seam"), vec![]);
        assert_eq!(check_all("squash! feat(jobs): add the seam"), vec![]);
    }

    #[test]
    fn git_comment_lines_are_not_part_of_the_message() {
        // What the commit-msg hook actually receives.
        let raw = "feat(jobs): add the seam\n\nA real body.\n\n# Please enter the commit \
                   message for your changes. Lines starting\n# with '#' will be ignored.\n";
        assert_eq!(check_all(raw), vec![]);
    }

    #[test]
    fn everything_after_a_scissors_line_is_ignored() {
        // `git commit --verbose` appends the whole diff, which routinely contains lines
        // that would otherwise trip the rules.
        let raw = "feat(jobs): add the seam\n\n# ------------------------ >8 \
                   ------------------------\n+Co-Authored-By: Claude <noreply@anthropic.com>\n";
        assert_eq!(check_all(raw), vec![]);
    }

    #[test]
    fn a_co_authored_by_trailer_is_rejected() {
        let violations = check_all(
            "feat(jobs): add the seam\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>",
        );
        assert_eq!(
            violations,
            vec![Violation::AiAttribution {
                line: "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>".to_owned(),
                rule: "co-authored-by trailer",
            }]
        );
    }

    #[test]
    fn a_session_trailer_is_rejected() {
        let violations = check_all(
            "feat(jobs): add the seam\n\nClaude-Session: https://claude.ai/code/session_018rCQ",
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].to_string().contains("session trailer"));
    }

    #[test]
    fn a_generated_with_footer_is_rejected() {
        let violations = check_all(
            "feat(web): add the page\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)",
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].to_string().contains("generated-with footer"));
    }

    #[test]
    fn every_vendor_is_covered_not_just_claude() {
        for trailer in [
            "Co-Authored-By: Codex <noreply@openai.com>",
            "Co-authored-by: Copilot <copilot@github.com>",
            "Co-Authored-By: Gemini <noreply@google.com>",
            "Co-Authored-By: Antigravity <noreply@antigravity.google>",
            "Co-Authored-By: Cursor <noreply@cursor.com>",
        ] {
            let message = format!("feat(api): thing\n\n{trailer}");
            assert_eq!(
                check_all(&message).len(),
                1,
                "expected {trailer} to be rejected"
            );
        }
    }

    #[test]
    fn a_human_co_author_is_not_an_ai_trailer() {
        assert_eq!(
            check_all("feat(api): thing\n\nCo-Authored-By: A Teammate <them@example.com>"),
            vec![]
        );
    }

    // The corpus that decides whether this check is usable at all. Three of the eleven
    // commits written the day this rule was added discuss attribution trailers and the
    // claude-plugins-official marketplace in prose; a substring match rejects all three.
    #[test]
    fn prose_discussing_a_trailer_is_not_a_trailer() {
        let message = "chore(claude): declare the plugins this repo needs\n\n\
             The three skills were byte-identical copies of My-Skills. Both handoffs \
             hardcoded ~/.claude/plugins/cache/claude-plugins-official/superpowers/6.3.0, \
             which points into one machine's cache.\n\n\
             The commit trailer this repository forbids is the Co-Authored-By one naming \
             Claude, along with the Claude-Session line and the robot footer. None of them \
             belongs in a message.";
        assert_eq!(check_all(message), vec![]);
    }

    #[test]
    fn a_url_in_prose_is_not_a_bare_session_url() {
        assert_eq!(
            check_all(
                "docs: link the marketplace\n\nSee https://github.com/anthropics/claude-plugins-official for the source."
            ),
            vec![]
        );
    }

    #[test]
    fn structure_only_ignores_attribution_but_still_checks_shape() {
        let with_trailer =
            "feat(jobs): add the seam\n\nCo-Authored-By: Claude <noreply@anthropic.com>";
        assert_eq!(check(with_trailer, true), vec![]);
        assert_eq!(
            check("nope", true),
            vec![Violation::NotConventional {
                subject: "nope".to_owned()
            }]
        );
    }

    #[test]
    fn a_trailer_hidden_in_a_merge_commit_is_still_rejected() {
        // Structure is exempt for merges; attribution is not, because git writes none of
        // these itself.
        let violations = check_all(
            "Merge branch 'x' into main\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
        );
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn an_empty_message_is_rejected_rather_than_silently_accepted() {
        assert_eq!(
            check_all(""),
            vec![Violation::NotConventional {
                subject: String::new()
            }]
        );
    }
}
