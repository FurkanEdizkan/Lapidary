# Contributing

**Not yet open for contributions.** The licence is AGPL-3.0-only, taken under the DCO
(see the licensing section in `docs/ARCHITECTURE.md`); the project is simply not yet
accepting outside work.

Issues and discussion are welcome in the meantime.

## Setting up a checkout

```sh
cargo xtask setup
```

Idempotent, and the first thing to run on a new machine. It installs the commit-message
hook (`core.hooksPath` is local to `.git/config`, so it cannot travel in a commit),
installs the plugins `.claude/settings.json` declares, and materializes this project's
skills into `.agents/skills/` with plain git — no vendor CLI needed, so an agent that is
not Claude Code gets them too.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `type(scope)!: description`.
The scope is optional and may list several parts (`fix(web,docs): …`); `!` marks a
breaking change. Types:

```
build  chore  ci  docs  feat  fix  perf  refactor  revert  style  test
```

**No length limit.** This project writes long, precise subjects; the rule is about shape,
not brevity.

**No AI attribution trailers** — no `Co-Authored-By:` naming a model or vendor, no
`…-Session:` line, no "Generated with" footer, for any tool. A message ends at its real
content. Mentioning a vendor *in prose* is fine and is not what the check matches; only
the trailer, footer and bare-session-URL shapes are.

The rules, and the reasoning behind each, live in `xtask/src/commit.rs`. `cargo xtask
check-commit-msg <file>` runs them by hand. CI applies the same check to the commits each
push adds, so `--no-verify` postpones a failure rather than avoiding it.

## Before opening a PR

Everything `.github/workflows/ci.yml` runs — the list is in
`docs/superpowers/plans/2026-09-04-phase-1-slice-2-HANDOFF.md` under "The verification
bar", and the Rust tests need a live PostgreSQL 18.
