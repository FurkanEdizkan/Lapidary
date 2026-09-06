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

## The verification bar

One command, three tiers. The gates are the same ones `.github/workflows/ci.yml` runs;
what the tiers change is how often you pay for each.

```sh
cargo xtask verify fast    # every commit -- ~0.3s, compiles nothing
cargo xtask verify         # every task -- the whole bar, ~30s warm
cargo xtask verify slice   # before a merge -- the same, with nothing gated out
```

`fast` is the four checks that only read files: `fmt`, `check-layers`, `check-deploy`,
`check-strings`. `cargo xtask setup` installs it as the `pre-commit` hook, so it runs
without being asked.

`verify` adds clippy, the test suite, the generated-file staleness gates, and — when the
diff shows they can have something to say — `cargo deny check` and the web suite. Those two
are skipped when nothing they inspect has changed: `deny` when no manifest, `Cargo.lock` or
`deny.toml` moved, the web suite when nothing under `web/` moved. `verify slice` forces
both on. Nothing is ever dropped; set `LAPIDARY_VERIFY_BASE` if your branch forked from
something other than `main`.

**The Rust tests need a live PostgreSQL 18.** Anything the tiers do not cover — the compose
exit run in particular — is still done by hand.

Why these tiers exist, and why each gate sits where it does, is in `xtask/src/verify.rs`.

## Before opening a PR

`cargo xtask verify slice`.
