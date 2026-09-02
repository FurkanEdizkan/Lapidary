# Phase 0a follow-ups — execution plan

**Date:** 2026-09-02
**Branch:** `rust-rewrite`
**Source to-do list:** `docs/superpowers/plans/2026-09-01-phase-0a-followups.md`
**Spec:** `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
**Phase 0a itself is complete** — 8/8 exit criteria verified; see
`2026-09-01-phase-0a-verification.md`.

This plan executes the subset of the follow-ups list that is self-contained and
verifiable today. Everything it does not cover stays in the to-do list.

## Scope rulings

**In scope** — seven tasks below. Each is bounded, testable, and needs no new design.

**Out of scope, and why:**

- *Pushing the branch.* Outward-facing, and the controller does not decide it. Stays a
  decision for the owner.
- *Follow-up 4, worker/api role switch.* Needs a role concept that does not exist until
  Phase 1 leases jobs. Premature.
- *Follow-up 6, `Approximate<T>` unused.* Its first real consumer in Phase 3 decides the
  shape. Guessing now bakes in the wrong one.
- *Follow-up 7, empty-state copy.* Becomes true exactly when Phase 1 ships ingest; it is
  a Phase 1 acceptance item, not a fix.
- *Follow-up 10, compose secrets.* Waits on the fleet story.
- *Follow-up 11, stringly-typed error variants.* Consistent across all ten enums, so it
  is house style. Changing it is a Phase 1 decision made once, not a cleanup.
- *Follow-up 11, `web/src/lib/api.ts` hand-written `Health` type.* Correct today.
- *`KernelOutput`'s redesign.* Phase 0b work. Task 6 fixes only the README's false
  stability claim, which is a lie today regardless of when the redesign lands.
- *Phase 0b.* A separate phase with its own spec.

## Global Constraints

Binding on every task. Copied from `CLAUDE.md`; the spec is the authority where this
plan and the spec disagree.

- **Measurement must not lie.** Analytic values from B-rep entities where available.
  Mesh-derived measurements are labelled "approximate" in the UI, always. A kernel
  returning no analytic entities is what tells callers a number is tessellated — that
  invariant survives every change here.
- **The application is free and complete.** No gated features in the app. Revenue is the
  server, fleet, support and cloud.
- **The open path never touches a source file and never invokes the CAD kernel.**
- **No SQL outside `lapidary-db`.** Everything goes through repository traits.
- **`lapidary-api` is a library that builds a Router.** Never a binary.
- **Container-first.** Bundle only our own binaries.
- **Pin everything.** Exact image digests, `Cargo.lock` committed, Actions pinned to SHAs.
- Rust: `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside
  tests** — enforced by `[workspace.lints]`, which covers every target including
  integration tests.
- **Errors say what broke and what to do.** "Could not read this STEP file — it may use
  an unsupported AP schema. Re-export from your CAD tool and retry." Not "parse failed (3)."
- No bare user-facing strings in components; everything through `web/src/lib/strings.ts`.
- Real content in all examples and fixtures. Plausible part numbers, real dimensions.
  Never "Part 1 / Part 2".
- Prefer the boring option. Solo-maintained, air-gapped deployments.

**Verification bar for every task:** `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo test --workspace` pass, and `cargo xtask check-layers` passes. Tasks touching
`web/` also run its typecheck and tests. Tasks touching manifests also run
`cargo deny check`.

---

## Task 1 — Structurally forbid `lapidary-api -> lapidary-enterprise`

**Follow-up:** the second owner decision. **Ruling: do it.** The rule as written permits
the one edge that would break "the application is free and complete", and it is enforced
today only by review — by me, and I am not always the reviewer. Both crates are empty, so
the change costs nothing now and gets more expensive with every file added to either.

**Files:** `xtask/src/layers.rs`, `docs/ARCHITECTURE.md`.

**Change:** add a layer variant above `L3` holding `lapidary-enterprise` alone.

- Name the variant `Enterprise`, not `L4`. `docs/ARCHITECTURE.md` documents a four-layer
  L0-L3 scheme and this is not a fifth peer of those — it is a wrapper tier that exists
  for one product reason. The name should say the reason.
- `layer_of`: `"lapidary-enterprise" => Layer::Enterprise`; `"lapidary-api"` stays `L3`.
- `edge_allowed`: `L3` may depend on `L0 | L1 | L2 | L3` and **not** on `Enterprise` or
  `Bin`. `Enterprise` may depend on anything but `Bin`.
- Keep `L3 -> L3` permitted. It is not the risky direction and future L3 crates will want it.
- `Violation::ForbiddenEdge`'s `Display` currently explains only the L2 rule ("L2 crates
  may depend only on L0 and L1..."). That message is wrong for this edge. Give the
  `L3 -> Enterprise` case its own remedy text naming the product rule it protects:
  the free application must not depend on the enterprise crate.
- Replace the module doc's final paragraph — the one beginning "Note the gap this leaves"
  — with a statement that the gap is now closed structurally, keeping the explanation of
  *why* `enterprise -> api` must stay legal.

**Tests** (`xtask/src/layers.rs`, `#[cfg(test)] mod tests`):

- Amend `allows_l3_to_depend_on_another_l3_so_enterprise_can_wrap_the_api` — its comment
  currently says the reverse edge is permitted, which stops being true. Keep the assertion
  that `lapidary-enterprise -> lapidary-api` passes.
- Add `rejects_api_depending_on_enterprise`: assert `check` returns exactly one
  `ForbiddenEdge { from_layer: L3, to_layer: Enterprise }`.
- Add a test that the violation message for that edge names `lapidary-enterprise` **and**
  states the remedy — assert on a distinctive substring of the new remedy text, so
  deleting the advice and leaving a bare "is forbidden" fails.
- Add `rejects_enterprise_depending_on_a_binary`.
- Keep every existing test passing.

**`docs/ARCHITECTURE.md`:** update the crate table (line ~82) so `lapidary-enterprise` no
longer reads `L3`, and amend the layering-rule paragraph (line ~91) to state the new
constraint and its product reason.

**Verify:** `cargo test -p xtask` and `cargo xtask check-layers` both pass. Then prove the
rule bites: temporarily add `lapidary-enterprise.workspace = true` to
`crates/lapidary-api/Cargo.toml`, confirm `cargo xtask check-layers` **fails** naming that
edge, and revert. Paste both outputs into the report.

---

## Task 2 — Id newtypes constructible from stored values

**Follow-up 1.** `LibraryId`, `PartId` and `RevisionId` can be generated and displayed but
never rebuilt from a stored value, and the tuple field is private.
`PartRepository::page` already returns them, so the first real query cannot compile.

**Files:** `crates/lapidary-core/src/ids.rs`, `crates/lapidary-core/src/error.rs`,
`crates/lapidary-core/src/lib.rs` (tests).

**Change:** inside the `uuid_newtype!` macro, add to each type:

- `pub fn from_uuid(uuid: Uuid) -> Self` — the inverse of the existing `as_uuid`.
- `impl std::str::FromStr` with `Err = CoreError`, parsing via `Uuid::parse_str`.

Add one `CoreError` variant for an unparseable id string. Follow the existing variants'
style: say what broke and what to do, and name the offending value.

**Do not** add `impl From<Uuid>`. Three distinct id types converting implicitly from one
`Uuid` is exactly how a `PartId` ends up where a `LibraryId` belongs. `from_uuid` is
explicit and symmetric with `as_uuid`.

**`lapidary-core` may not depend on `sqlx`** — `deny.toml` enforces it. Any
`Uuid <-> newtype` glue for query binding belongs in `lapidary-db` and is not this task.
`from_uuid` needs only `uuid`, which `lapidary-core` already has.

**Tests** (in `crates/lapidary-core/src/lib.rs`, beside the existing ones):

- Round-trip: `PartId::from_uuid(id.as_uuid()) == id` for a generated id.
- `FromStr` round-trips through `Display` for all three types.
- `FromStr` rejects a non-UUID string, and the error message contains the rejected input.
- The three types do not silently interconvert — a compile-fail test is overkill, so
  instead assert that two ids built `from_uuid` on the *same* `Uuid` are equal, which
  documents that identity comes from the uuid alone.

---

## Task 3 — Reach the mock kernel from the binary; drop the unused `api -> cad` edge

**Follow-ups 3 and 5, together** — the to-do list notes they are coupled, and both touch
crate manifests, so one task avoids two agents editing the same files.

**The problem:** the spec says the compose `worker` runs with `mock-kernel` enabled in 0a.
It does not. `deploy/Containerfile` passes no `--features`, and no crate exposes a
passthrough, so the mock kernel is compiled out of the container entirely. Separately,
`lapidary-api` depends on `lapidary-cad` without using it — and the open path lives in
`lapidary-api`, so "the open path never invokes the CAD kernel" is one `use` away with
nothing mechanical stopping it.

**Ruling on which wiring:** the worker depends on `lapidary-cad` **directly**, not through
a feature chain via `lapidary-api`. The to-do list already prefers this; it is also the
only option compatible with dropping the `api -> cad` edge, and it keeps the kernel out of
the crate that serves the open path.

**Files:** `crates/lapidary-api/Cargo.toml`, `bin/lapidary-server/Cargo.toml`,
`bin/lapidary-server/src/main.rs`, `deploy/Containerfile`, `docs/ARCHITECTURE.md`.

**Change:**

1. Remove `lapidary-cad.workspace = true` from `crates/lapidary-api/Cargo.toml`. Confirm
   nothing in that crate references it.
2. `docs/ARCHITECTURE.md` line ~81 says `lapidary-api` "Depends on all L2". That stops
   being true. Amend it to say it depends on the L2 crates it uses, and state why `cad` is
   excluded: the open path lives here and must never invoke the kernel.
3. `bin/lapidary-server/Cargo.toml`: add an **optional** `lapidary-cad` dependency and a
   `mock-kernel` feature enabling `lapidary-cad/mock-kernel`. Default features stay empty,
   so a default build is unchanged.
4. `bin/lapidary-server/src/main.rs`: add a small function returning a human-readable
   kernel description, with two cfg'd bodies — under `mock-kernel` it reports the
   `MockKernel`'s implementation and version; without it, a line saying no kernel is
   compiled in. Log it once at startup beside the existing "listening" line.

   This is a deliberate, minimal *real* use. An unused optional dependency would repeat
   the mistake this task removes from `lapidary-api`. A startup line also makes the
   feature chain visible in `podman logs`, which is the only place an operator could
   otherwise discover it silently broke.
5. `deploy/Containerfile`: build with `--features mock-kernel`. Keep the existing
   `--release --locked -p lapidary-server`.
6. Same file: `EXPOSE 8080` is inherited by the `worker` service, which binds `8081`
   (`deploy/compose.yaml:48`). Document both ports, or drop the line — `EXPOSE` is
   documentation, and documentation that names the wrong port is worse than none.
   *(This is follow-up 11's `EXPOSE` item, folded in so only one task edits this file.)*

**Tests:**

- A unit test in `bin/lapidary-server` asserting the kernel-description function reports
  the mock implementation when the feature is on. Run it with
  `cargo test -p lapidary-server --features mock-kernel`.
- Assert the default build still compiles with the feature off:
  `cargo build -p lapidary-server`.
- `cargo xtask check-layers` still passes.

**Verify the container actually gets it:** build the image and run the binary with
`--help`-equivalent startup far enough to print the kernel line, or run
`podman run --rm <image>` and capture the startup output showing the mock kernel. If the
image build is too slow to be practical, say so in the report and show the
`cargo build --features mock-kernel` output instead — do not claim a container check you
did not run.

---

## Task 4 — `publish = false`, and drop the internal version pins

**Follow-up 9.** Every internal path dependency in `Cargo.toml` carries
`version = "0.1.0"` purely to satisfy `wildcards = "deny"` in `deny.toml`. That couples
eleven lines to the workspace version: a bump breaks `cargo deny` until all eleven are
edited. `publish = false` on the application crates removes the coupling *and* prevents an
accidental `cargo publish` of an AGPL app crate.

**Files:** all 13 manifests without `publish = false` (11 under `crates/`, 2 under `bin/`;
`xtask/Cargo.toml` already has it), plus root `Cargo.toml` and `deny.toml`.

**Change:**

1. Add `publish = false` to each of the 13 `[package]` sections, matching `xtask`'s
   placement.
2. Remove `version = "0.1.0"` from all 11 `lapidary-*` entries in
   `[workspace.dependencies]`, leaving bare `{ path = "..." }`.
3. `deny.toml` `[bans]`: add `allow-wildcard-paths = true`, with a comment saying it is
   safe precisely because every internal crate is `publish = false` — the two changes are
   a pair and must not be separated.

**Verify:** `cargo deny check` is fully green (all of advisories, bans, licenses,
sources), `cargo build --workspace` succeeds, and `Cargo.lock` is unchanged or its diff is
explained. Paste the `cargo deny check` output.

**Prove the guard works:** temporarily restore `wildcards = "deny"` without
`allow-wildcard-paths` and confirm `cargo deny check bans` fails, then revert — this gate
had never been executed before Phase 0a's Task 11, so demonstrate it runs.

---

## Task 5 — `lapidary-db` tells authentication failure apart from unreachable

**Follow-up 11, first item.** `connect()` maps *every* failure to
`DbError::Unreachable`, so a wrong password reads "Could not reach the database at
postgres://db:5432/lapidary. Check that the `db` service is running" — which sends the
operator to look at a service that is running fine. This breaks the project rule that
errors say what broke and what to do.

**Files:** `crates/lapidary-db/src/lib.rs`.

**Change:** replace the `.map_err(|_| DbError::Unreachable { .. })` closure with a
classifier that inspects the `sqlx::Error` and returns the right variant.

Add variants, each with an actionable message and the **redacted** target only — never the
raw URL. `redact_credentials` already exists and is the only thing that may put a URL in a
message:

- `AuthenticationFailed { target }` — SQLSTATE `28P01` (invalid password) and `28000`
  (invalid authorization specification). Message points at `POSTGRES_PASSWORD` /
  `DATABASE_URL` in `.env`, not at the service.
- `DatabaseMissing { target, database }` — SQLSTATE `3D000` (invalid catalog name).
- `Unreachable { target }` — keep, for genuine IO/DNS/timeout failures. It stays the
  fallback for anything unrecognised.

Route the classification through a private function taking `&sqlx::Error` so it is
unit-testable without a live server.

**Tests** (`crates/lapidary-db`, `#[cfg(test)] mod tests`):

Constructing a `sqlx::Error::Database` needs a type implementing
`sqlx::error::DatabaseError`. Write a small test-only double that returns a configurable
SQLSTATE, and assert:

- `28P01` classifies as `AuthenticationFailed`, and the rendered message mentions the
  password/credentials — not "is the service running".
- `3D000` classifies as `DatabaseMissing` and names the database.
- An unrecognised SQLSTATE falls back to `Unreachable`.
- `sqlx::Error::PoolTimedOut` (or another non-`Database` variant) classifies as
  `Unreachable`.
- **No variant leaks credentials:** classify an error with the URL
  `postgres://lapidary:sup3rs3cret@db:5432/lapidary` and assert no rendered message
  contains `sup3rs3cret`. This is the regression test for the Phase 0a credential leak;
  every new variant must be covered by it, not just the ones that existed then.

Keep every existing `redact_credentials` test passing untouched.

---

## Task 6 — Three statements that are currently false

**Batch of small independent fixes**, each one correcting something the repo asserts that
is not true. No shared files.

**6a. `sidecar/occt-bridge/README.md:12` — "The trait does not change."**
It will. Phase 0b needs blob references for the LOD ladder, and measurement cannot snap to
opaque strings like `"CYLINDRICAL_SURFACE:22.000"` — it needs axes, radii and normals.
`docs/ARCHITECTURE.md:106` already specifies the richer shape. Amend the claim to say what
is actually stable: the `Kernel` trait's *shape* — one async `process` call returning
derivatives — and the invariant that **mesh input yields no analytic entities**, which is
what stops tessellated numbers being presented as exact. State plainly that `KernelOutput`'s
fields will change in Phase 0b.

**6b. `web/tsconfig.json` — `"include": ["src"]` leaves the config files untypechecked.**
`vite.config.ts` and `vitest.config.ts` are excluded, so an error in either is invisible
until the build breaks. Add them to `include`. They import Node builtins, so this may
require `@types/node` as a dev dependency and a `types` entry — add it if and only if the
typecheck demands it, and pin it the way the other dev dependencies are pinned. Run the
web typecheck and the web tests; both must pass. If adding them surfaces a real error in
either config file, fix the error rather than narrowing the include.

**6c. `rust-toolchain.toml` pins 1.95.0; `deploy/Containerfile` builds on `rust:1.95-*`.**
They agree today only because the image digest is pinned. A future digest bump to a
1.95.1 image would make `rust-toolchain.toml` trigger a network `rustup` download inside
the build — turning a hermetic container build into a network-dependent one, which is
exactly wrong for air-gapped deployment. Do not change either pin. Add an assertion to the
build stage that fails loudly if the image's `rustc` is not the pinned version, with a
comment saying what to do when it fires (bump `rust-toolchain.toml` and the digest
together). Verify the assertion by running the same check against the current image.

---

## Task 7 — Capture the prototype knowledge that was lost

**Follow-up 8.** The spec made recording seven areas a precondition of deleting the Node
prototype. `docs/prototype-notes.md` covers four — domain shape, search payload, LOD
approach, and what was deliberately dropped. Three were missed. All are recoverable from
`origin/main`, which is why this is not urgent — but slicer profile parsing is hard-won
and heads for `lapidary-targets`, so capture it before that work starts rather than after.

**Read from `origin/main`** (the Node prototype; do not check it out over the working
tree — use `git show origin/main:<path>` or `git grep origin/main`). Extract and write up:

1. **`assetPipeline` / `meshSidecar` — ingest ordering and stages.** Only the LOD
   algorithm survived into the notes. What ran in what order, what was idempotent, what
   could be resumed, where the hash was taken, and what happened on partial failure.
2. **`libraryScan` — directory-walk and debounce behaviour.** Walk order, what was
   skipped, how filesystem events were coalesced and over what window, and what happened
   when a file changed mid-scan.
3. **`profileImport` / `printerSettings` / `printerType` — slicer profile parsing.** The
   `.ini`/`.json` shapes handled, inheritance between profiles, which keys mattered, and
   the edge cases the code visibly worked around. This is the item with the most
   hard-won detail; be concrete and quote real key names.

**Write to `docs/prototype-notes.md`**, matching the existing four sections' structure and
depth. For each area, record what it did, why it was done that way, and what the Rust
rewrite should keep or discard — the notes exist to inform Phase 1, not to eulogise the
prototype.

**Real content only.** Real key names, real defaults, real file extensions. Never
"setting1 / setting2".

**Verify:** every path, key name and behaviour in the new sections is traceable to
something you actually read in `origin/main`. Cite the prototype file each section came
from. If an area turns out to be thinner than the to-do list implies, say so explicitly
rather than padding — a note that says "there was no debounce, events were handled
synchronously" is a useful finding, not a failure.

---

## After all seven

Update `docs/superpowers/plans/2026-09-01-phase-0a-followups.md`: mark the items this plan
closed, and leave the deferred ones with their reasons intact. That document stays the
open list; this one becomes history.
