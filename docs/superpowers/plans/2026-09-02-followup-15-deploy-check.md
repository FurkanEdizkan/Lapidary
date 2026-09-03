# Item 15 — make the worker-only-links-kernel invariant fail in CI

**Date:** 2026-09-02
**Branch:** `rust-rewrite`
**Source item:** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md` item 15
**Previous rounds:** `2026-09-02-phase-0a-followups-execution.md`, `...-round-2.md`

## The problem

Item 12 converted an invariant from comment-enforced to check-enforced: `lapidary-api ->
lapidary-cad` now fails `cargo xtask check-layers`, which `ci.yml` runs on every push. Item 13
created a *sibling* invariant — only the `worker` image links the CAD kernel — and left it
enforced by a comment and by nothing. Today, all three of these would pass CI:

- adding `SERVER_FEATURES: mock-kernel` to the `api` service in `deploy/compose.yaml`
- deleting it from `worker`, silently producing a worker with no kernel
- re-hardcoding `--features mock-kernel` in `deploy/Containerfile`, undoing the split entirely

The product rule behind it is non-negotiable: **the open path never invokes the CAD kernel.**

## Scope rulings

**In scope:** a new `cargo xtask check-deploy` that asserts the deploy configuration still
expresses the invariant, wired into `ci.yml`, plus the to-do bookkeeping.

**Out of scope — `containers.yml` stays as it is.** It builds `lapidary-server:${{ github.sha }}`
with no build arg, so it now produces only the kernel-free variant and nothing in CI exercises
the `SERVER_FEATURES=mock-kernel` build path. Item 15 already records that the fuller fix — a
second `docker build --build-arg ...` — starts to pre-empt item 4 (the role split). **Ruling: do
not decide the role split as a side effect of adding a check.** That half of item 15 stays open.

**Out of scope — building images in CI.** Slow, and it is the same pre-emption. The check below
is static: it verifies the configuration that *would* produce the right images, not the images.
Say so plainly where it is documented, so nobody mistakes it for an artifact-level guarantee.

**Ruling on the mechanism: a new `xtask` subcommand, not a `grep` step in `ci.yml`.** A shell
grep is three lines, but it is untested, runs only in CI, and cannot be run locally before a
push. This session has repeatedly found checks that silently stopped checking; `xtask` is where
this repo already puts "enforced here rather than by review", and it gets unit tests.

**Ruling: no YAML dependency.** `xtask` currently depends on `serde_json` only. `serde_yaml` is
unmaintained, which `cargo deny check advisories` may flag, and pulling a parser to read two
service blocks from a file we control is not the boring option. Parse line-wise — but see the
failure-mode requirement below, which is what makes that safe.

## Global Constraints

- **The open path never touches a source file and never invokes the CAD kernel.**
- **Container-first.** Podman and Docker. Bundle only our own binaries — never Postgres, never OCCT.
- **Pin everything.** Exact digests, `Cargo.lock` committed, Actions pinned to commit SHAs.
- **The application is free and complete.**
- Rust: `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside tests.**
- **Errors say what broke and what to do.**
- Prefer the boring option. Fewer dependencies, fewer moving parts, more explicit failure modes.
- The codebase comments *why*, not *what*.

**Verification bar (what CI runs):** `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo xtask check-layers`, `cargo test --workspace --all-features`, `cargo deny check`.
Tests need `export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"`.

---

## Task 1 — `cargo xtask check-deploy`

**Files:** `xtask/src/main.rs`, a new `xtask/src/deploy.rs`, `.github/workflows/ci.yml`.

Put the rules and their tests in `deploy.rs` as pure functions over `&str` file contents, so they
are unit-testable without touching the filesystem. `main.rs` reads the two files and calls them —
the same split `layers.rs` / `main.rs` already uses.

### The rules

Given `deploy/compose.yaml` and `deploy/Containerfile`:

1. **Exactly the services that should link the kernel do.** Keep a named list — today
   `["worker"]` — in the same spirit as `FORBIDDEN_PAIRS`: a list edited only to permit a new
   kernel-linked service, never to make an ordinary edit pass. Then:
   - any service setting `SERVER_FEATURES` that is **not** in the list is a violation;
   - any service in the list that does **not** set it is a violation.

   Write it generically over all service blocks rather than naming `api`. A future `api-replica`
   is then covered without anyone remembering to add it.

2. **The Containerfile still routes through the arg.** Its `cargo build` line must contain the
   `${SERVER_FEATURES:+--features "$SERVER_FEATURES"}` expansion, and must not contain a second,
   hardcoded `--features` — counting occurrences is enough and is the boring check.

3. **`ARG SERVER_FEATURES` is declared after the first `FROM`.** An `ARG` before the first `FROM`
   is a different scope and is invisible to the build stage; the arg would silently do nothing and
   the worker would ship without a kernel. This trap is already documented in a comment there —
   this makes it fail instead.

### The failure mode is the point

A line-wise parser that cannot find what it expects must **fail loudly, not pass silently.** If
the `services:` key is absent, if no service blocks parse, if the `cargo build` line is not found,
or if `ARG SERVER_FEATURES` is absent entirely — each is its own violation with its own message
saying *the check's parsing is stale, not the config*, and pointing at what to update. The
distinction matters: those two failures need opposite fixes, and a check that reports the wrong
one sends the reader to the wrong file.

This requirement exists because a previous round shipped an assertion whose two halves could both
degrade to an empty string, comparing equal and passing having verified nothing. Do not repeat it.

### Messages

Every violation names the file, what is wrong, and what to do. Follow `layers.rs`'s `Violation`
`Display` impl — it is the local model. Where a violation touches the product rule, say so: the
open path must never invoke the kernel.

### Wiring

Add `check-deploy` to `main.rs`'s subcommand match **and to both the unknown-subcommand and usage
messages** — they currently enumerate `check-layers, export-bindings` and would otherwise lie.
Add a `cargo xtask check-deploy` step to `.github/workflows/ci.yml` beside `check-layers`.

### Documentation

The check is static: it verifies the configuration, not the built artifact. Note that where the
subcommand is documented, so nobody reads a green `check-deploy` as proof that the shipped `api`
image lacks the kernel.

### Tests

In `deploy.rs`, over fixture strings — real ones, shaped like the actual files, never
`service1 / service2`:

- the current, correct configuration passes;
- `SERVER_FEATURES` added to a non-listed service fails, and the message names that service;
- `worker` missing `SERVER_FEATURES` fails;
- a hardcoded `--features mock-kernel` in the build line fails;
- `ARG SERVER_FEATURES` before the first `FROM` fails;
- a compose file with no `services:` key fails **as a parse-stale violation**, not as "no
  violations found";
- a Containerfile with no `cargo build` line fails the same way.

**Prove each rule bites.** For every rule, make the real repository file wrong, run
`cargo xtask check-deploy`, confirm it exits non-zero naming that specific problem, then revert
and confirm it passes. Paste every output. `git status` must be clean afterwards.

---

## Task 2 — Update item 15

**File:** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md` only.

Mark item 15 **half closed**, in the file's existing convention, precisely:

- **Closed:** the deploy configuration expressing the invariant is now checked by
  `cargo xtask check-deploy`, which `ci.yml` runs on every push. Name what it checks.
- **Still open:** `containers.yml` still builds a single variant with no build arg, so no CI build
  exercises the `SERVER_FEATURES=mock-kernel` path, and **nothing verifies the built artifacts** —
  the new check is static, over configuration. Keep the note that the fuller fix pre-empts item 4.

Do not touch any other item. Do not disturb existing `**Closed —**` markers. Do not renumber.
Every claim must be verifiable from `git log` or the tree.
