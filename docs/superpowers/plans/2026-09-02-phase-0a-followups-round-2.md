# Phase 0a follow-ups, round 2 — execution plan

**Date:** 2026-09-02
**Branch:** `rust-rewrite`
**Source to-do list:** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md`
**Previous pass:** `docs/superpowers/plans/2026-09-02-phase-0a-followups-execution.md` (7 tasks,
closed 11 sub-items, CI green on run 33608501887 — the first ever)

The first pass closed the self-contained items. Three of the items it *created* during its own
final review are also self-contained, and one older item turns out to be cheaper to fix than to
keep deferring. This plan closes those four.

## Scope rulings

**In scope** — four tasks.

- **Item 12** (`lapidary-api → lapidary-cad` permitted structurally) and **item 14**
  (`publish = false` enforced by comment only) are both "make an invariant fail loudly instead
  of relying on review", both live in `xtask`, and both are proven the same way. **Ruling: one
  task, not two** — they share `xtask/src/layers.rs` and `xtask/src/main.rs`, and splitting them
  would have two agents editing the same two files back to back for no benefit.
- **Item 13** (the `api` container links `lapidary-cad`). The to-do list offers two fixes:
  separate the images, or split the binary by role. Splitting by role is item 4, which is Phase 1
  work because it needs a role concept that does not exist until the worker leases jobs.
  **Ruling: separate the images**, via a build arg. It is the boring option, it is available
  today, and it removes the coupling without inventing a role abstraction early.
- **Item 7** (the empty-state copy promises a drop interaction that does not exist). The to-do
  list defers this to Phase 1 as an acceptance item. **Ruling: fix the copy now.** The string
  ships in the UI today and is read by anyone who opens the app; it tells them to drop a folder
  and nothing happens. That is the same class of defect as the three the previous pass's Task 6
  corrected. Phase 1 restoring the promise when ingest actually ships is a separate, additive
  change — and the follow-ups entry stays open to say so.

**Out of scope, and why:**

- *Item 2's substance.* The `KernelOutput` redesign is Phase 0b work with its own spec.
- *Item 4, worker/api role switch.* Needs job leasing to exist. Task 2 below reduces the harm in
  the meantime without pre-empting the design.
- *Item 6, `Approximate<T>` unused.* Its first real consumer in Phase 3 decides its shape.
- *Item 10, compose secrets.* Waits on the fleet story.
- *Item 11's remaining two.* The stringly-typed error variants are consistent across all ten
  enums — changing house style is one Phase 1 decision, not a cleanup. `web/src/lib/api.ts`'s
  hand-written `Health` type is correct today; there is nothing wrong to fix yet.
- *Phase 0b.* A separate phase.

## Global Constraints

Binding on every task. From `CLAUDE.md`.

- **The open path never touches a source file and never invokes the CAD kernel.** Tasks 1 and 2
  both exist to protect this.
- **The application is free and complete.** No gated features in the app.
- **Measurement must not lie.** Mesh-derived measurements are labelled approximate, always.
- **We never delete user data implicitly.** Delete is soft; purge is separate and explicit.
- **Container-first.** Podman and Docker. Bundle only our own binaries — never Postgres, never OCCT.
- **Pin everything.** Exact image digests, `Cargo.lock` committed, Actions pinned to commit SHAs.
- **No SQL outside `lapidary-db`.** **`lapidary-api` is a library that builds a Router.**
- Rust: `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside tests.**
- **Errors say what broke and what to do.** Not "parse failed (3)."
- Frontend: dark only, no light mode. Motion 120/180/280ms, `cubic-bezier(0.2, 0, 0, 1)`,
  transform and opacity only, respect `prefers-reduced-motion`. **No bare user-facing strings in
  components** — everything through `web/src/lib/strings.ts`.
- Real content in all examples and fixtures. Never "Part 1 / Part 2".
- Prefer the boring option. Solo-maintained, air-gapped industrial deployments.

**Verification bar for every task — this is what CI runs, and CI now runs for real:**
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo xtask check-layers`, `cargo test --workspace --all-features`, `cargo deny check`.
Web tasks also run `npm run typecheck`, `npm test`, `npm run build` in `web/`.
Tests need a live database: `export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"`.

`cargo fmt --all --check` is listed first deliberately: the previous pass shipped a formatting
violation that survived its own review and five later verification passes, because no
verification bar named it. It is CI's first gate.

---

## Task 1 — Make two invariants fail loudly instead of relying on review

**Items 12 and 14.** Both are rules the repo states in comments and enforces by nobody.

**Files:** `xtask/src/layers.rs`, `xtask/src/main.rs`.

### 1a. `lapidary-api` may not depend on `lapidary-cad`

`edge_allowed(Layer::L3, Layer::L2)` returns `true` for every L3→L2 pair, `lapidary-cad`
included. A contributor who adds `lapidary-cad.workspace = true` back to
`crates/lapidary-api/Cargo.toml` passes `cargo xtask check-layers` cleanly, and the only thing
standing in the way is a comment. The rule it would break is non-negotiable: the open path lives
in `lapidary-api` and must never invoke the CAD kernel.

A tier rule cannot express this — it is a named-pair exception, not a layer relation.

**Ruling on the mechanism: a forbidden-pairs list, not an allow-list of `lapidary-api`'s
permitted L2 deps.** An allow-list needs editing every time `lapidary-api` legitimately gains an
L2 dependency, and a list that must be edited to permit ordinary work gets widened carelessly.
A forbidden-pairs list is edited only to add another prohibition.

- Add a module-level constant of `(from, to, why)` triples, with exactly one entry today:
  `("lapidary-api", "lapidary-cad", <the product reason>)`. The third field is the message —
  keep the reason next to the rule, not in a match arm far away.
- Check it in `check()` alongside the tier rule, so one run reports both kinds.
- Add a `Violation` variant for it. Its `Display` must name both crates and state the rule:
  the open path lives in `lapidary-api` and must never invoke the kernel.

### 1b. Every workspace member must be `publish = false`

`deny.toml`'s `allow-wildcard-paths = true` is workspace-wide and is only sound because every
member is unpublishable. The pair is documented in two places and enforced in none: a new crate
added without `publish = false` silently inherits the exemption.

`cargo xtask check-layers` already walks every member and already fails on one missing from
`layer_of`, so it is the natural home.

- `cargo metadata --format-version 1 --no-deps` reports `publish` as `[]` for
  `publish = false` and `null` for a publishable crate. **Verified: all 14 members currently
  report `[]`.**
- Put the rule in `layers.rs` as a pure function over member-name → is-publishable, so it is
  unit-testable without invoking cargo. `main.rs` extracts the field and calls it.
- Add a `Violation` variant. Its `Display` must say what to add (`publish = false` in
  `[package]`) **and why it matters** — that `deny.toml`'s `allow-wildcard-paths` depends on it.
  A message that only says "add publish = false" invites someone to satisfy the check without
  understanding what it protects.

### Both

**`xtask/src/main.rs`'s footer** currently explains the tier rule and the Enterprise rule. It is
printed under every violation list. Extend it to cover the two new rules — the previous pass had
to fix this same footer twice for the same reason, so check it says something true for *every*
violation the checker can now emit.

**Tests** (`xtask/src/layers.rs`):

- `lapidary-api -> lapidary-cad` is rejected, and the message names both crates and states the
  open-path rule. Assert on a distinctive substring of the remedy, so deleting the advice fails
  the test.
- A different L3→L2 edge is still allowed (`lapidary-api -> lapidary-index`), so the rule is a
  named pair and not a blanket L3→L2 ban.
- A member with `publish` unset is rejected, and the message mentions `allow-wildcard-paths`.
- All 14 members publishable-false passes.
- Keep every existing test green.

**Prove both rules bite** — do not just assert the tests pass:

1. Temporarily add `lapidary-cad.workspace = true` to `crates/lapidary-api/Cargo.toml`, run
   `cargo xtask check-layers`, confirm it exits non-zero naming that pair, revert, confirm clean.
2. Temporarily remove `publish = false` from one manifest, run it, confirm it exits non-zero
   naming that crate, revert, confirm clean.
3. For each new test, make the corresponding rule wrong in the source, confirm the test fails,
   revert.

Paste all outputs. Confirm `git status` is clean after reverting, and that `Cargo.lock` did not
drift (step 1 will rewrite it — restore it).

---

## Task 2 — The `api` image must not link the CAD kernel

**Item 13.** Both compose services build from `deploy/Containerfile` with `--features
mock-kernel` hardcoded, so the binary serving the open path links `lapidary-cad` even though
`lapidary-api` does not depend on it. Today nothing in the `api` role calls into it — but the
open-path binary and the worker binary are currently the same artifact, and Phase 1 puts real
code behind that boundary.

**Files:** `deploy/Containerfile`, `deploy/compose.yaml`.

**Change:** make the feature list a build argument that defaults to empty.

- `ARG` in the build stage, defaulting to no features, threaded into the existing
  `cargo build --release --locked -p lapidary-server` line. Use a form that adds no
  `--features` flag at all when the arg is empty — an empty `--features ""` is not the same
  thing and may not behave as you expect; check it.
- `deploy/compose.yaml`: the **worker** service passes the arg enabling `mock-kernel`. The
  **api** service passes nothing and therefore builds without the kernel.
- Do not disturb the rustc-version assertion, the pinned base-image digests, or `EXPOSE 8080 8081`.
  All three were added by the previous pass and all three are deliberate.
- The `ARG` must be declared inside the stage that uses it. A build arg declared before the
  first `FROM` is a different scope and will not be visible where you need it — verify rather
  than assume.

**Comments to update:** `bin/lapidary-server/Cargo.toml` and `bin/lapidary-server/src/main.rs`
were corrected in the previous pass to say both services share one image built with
`mock-kernel`. **That stops being true.** Update both to describe what is now the case.
Getting this wrong reintroduces exactly the defect the previous pass fixed here.

**Verify by building and running both images, not by reading the file:**

- Build the api image with no build arg and the worker image with it.
- Run each far enough to print its startup kernel line. The **api** image must report no kernel
  compiled in; the **worker** image must report the mock kernel.
- Paste both startup lines.

If the two builds are impractically slow, say so plainly and show what you did run instead — an
honest "I did not run this" is worth more than a plausible claim. Do not claim a build you did
not run.

---

## Task 3 — The empty state stops promising an interaction that does not exist

**Item 7.** `web/src/lib/strings.ts:14` reads:

    body: 'Drop a folder of STL or STEP files to begin.'

Nothing implements dropping a folder. A user who opens the app is told to perform an action that
does nothing. It is rendered at `web/src/routes/index.tsx:14`.

**Files:** `web/src/lib/strings.ts`, and a test if one is warranted.

**Change:** reword `emptyLibrary.body` so it is true today.

Constraints on the new copy:

- It must not promise any interaction the app does not implement — no dropping, no uploading, no
  "click here", unless you have verified that affordance exists.
- It must not read as an error or a defect. An empty library is a normal state.
- Keep it one short sentence, in the voice of the neighbouring strings (read them first).
- It stays in `strings.ts`. No bare user-facing strings in components — that rule is not relaxed
  for a one-word change.
- `emptyLibrary.title` ("No parts yet") is already true. Leave it unless the pair reads oddly
  once `body` changes; if you touch it, say why.

**Do not** implement a drop affordance. That is Phase 1 work and out of scope here.

**Test:** add or extend a test asserting the empty state renders the string from `strings.ts`.
If such a test already exists, verify it still passes and say so rather than adding a second.
A test that merely asserts the literal new sentence is weak — prefer one that asserts the
component renders `strings.emptyLibrary.body`, so the copy can change without breaking it.

---

## Task 4 — Bring the to-do list back in line with reality

**Bookkeeping, documentation only.** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md`
is the standing open list, and two things in it are now false.

**Files:** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md` only.

1. **The push decision is no longer open.** It reads "**Still open.** Still unanswered by the
   owner, and still the reason `.github/workflows/ci.yml` and `containers.yml` have never
   executed." The owner answered, the branch was pushed (`16b60b3..15494c3`, 63 commits), and CI
   ran green on its first execution — run `33608501887`,
   `https://github.com/FurkanEdizkan/Lapidary/actions/runs/33608501887`, all four jobs (`rust`,
   `deny`, `web`, `bindings`) successful.

   Close it — and record the part that is still true: **`containers.yml` has still never run.**
   It triggers only on `workflow_dispatch` and `v*` tags, so the push did not exercise it. That
   is not a defect, but "CI is proven" would be an overstatement, and this file is where someone
   will look to find out.

2. **Close what this plan's Tasks 1-3 fixed** — items 12, 14, 13 and 7 — using the same
   `**Closed — Task N (commit), date.**` convention the file already uses, with enough of the
   original text kept that a reader learns what the problem *was*.

   For **item 7**, be precise: the copy no longer lies, but the app still cannot ingest anything.
   Phase 1 shipping ingest and restoring a truthful call to action remains an acceptance item.
   Do not mark that half closed.

   For **item 13**, note that separating the images is not the same as splitting the binary by
   role — **item 4 stays open** and is still the real fix.

3. **Leave every other open item exactly as it is**, reasoning intact: items 2 (substance), 4, 6,
   10, both remaining sub-items of 11, and the whole Phase 0b section.

**Accuracy:** every "closed" claim must be verifiable from `git log` or the working tree. If you
cannot confirm something landed, do not mark it closed — say so in your report.
