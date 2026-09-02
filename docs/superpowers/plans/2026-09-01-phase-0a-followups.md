# Phase 0a — follow-ups

**Status:** open — partially closed. Seven of the items below were executed on 2026-09-02;
see the execution note below and the item markers throughout.
**Date:** 2026-09-01
**Source:** the Phase 0a whole-branch review, plus findings deferred during execution.
**Phase 0a itself is complete** — 8 of 8 exit criteria verified from a clean clone; see
`2026-09-01-phase-0a-verification.md`.

Everything below was found, judged, and deliberately **not** done in 0a. Each item says what
it is, why it was left, and what it costs to leave. Nothing here blocks the phase.

**2026-09-02 execution pass.** A seven-task plan ran against the self-contained,
verifiable subset of this list —
`docs/superpowers/plans/2026-09-02-phase-0a-followups-execution.md` has the task-by-task
detail, and `git log` on `rust-rewrite` (commits `c656dc4`..`ae49f39`) has the actual
diffs. Items it closed are marked **Closed** below with the task and commit that did it.
Everything else — including both owner decisions and the whole Phase 0b section — is
untouched and still open.

---

## Decisions waiting on the owner

### Push the branch, and let CI run for the first time
**Still open.** Still unanswered by the owner, and still the reason `.github/workflows/ci.yml`
and `containers.yml` have never executed. Not in scope for the 2026-09-02 execution pass —
pushing is outward-facing and the controller does not decide it.

The branch has never been pushed, and its lead over `origin/rust-rewrite` keeps growing with
every commit, so a fixed count here goes stale immediately. Recompute with
`git rev-list --count origin/rust-rewrite..HEAD`; it was 56 as of 2026-09-02. Pushing still sends
the commit deleting the Node prototype (64 files) and starts consuming Actions minutes on every
subsequent push.

### Whether `lapidary-enterprise` should be structurally prevented from being a dependency of `lapidary-api`
**Closed — Task 1 (`f3a252b`, `733edad`), 2026-09-02.** `lapidary-enterprise` was promoted out
of L3 into its own `Enterprise` tier: `xtask/src/layers.rs` now rejects `lapidary-api ->
lapidary-enterprise` structurally (with a violation message naming the product rule it
protects), keeps `lapidary-enterprise -> lapidary-api` legal, and `docs/ARCHITECTURE.md`'s
crate table and layering-rule paragraph were updated to match. Covered by new tests in
`xtask/src/layers.rs`.

The layering rule used to permit L3 → L3, so `lapidary-api → lapidary-enterprise` passed CI.
That edge would have made the free application depend on the enterprise crate, contradicting
"the application is free and complete." It had been enforced only by review, documented at
`xtask/src/layers.rs:9-16`.

---

## Before or during Phase 1

### 1. Id newtypes cannot be built from stored values
**Closed — Task 2 (`e02736d`), 2026-09-02.** `LibraryId`, `PartId` and `RevisionId` all gained
`from_uuid` and `FromStr` (parsing via `Uuid::parse_str`, erroring through a new `CoreError`
variant), inside the `uuid_newtype!` macro in `crates/lapidary-core/src/ids.rs`. Round-trip and
rejection tests added in `crates/lapidary-core/src/lib.rs`. `From<Uuid>` was deliberately not
added, so the three id types still cannot silently interconvert.

`LibraryId`, `PartId`, `RevisionId` used to expose `new()`, `as_uuid()` and `Display`, but no
`from_uuid` and no `FromStr`, and the tuple field was private. `PartRepository::page` already
commits to returning them, so the first real query could not compile.

Note `lapidary-core` may not depend on `sqlx` (enforced by `deny.toml`), so the
`Uuid ↔ newtype` conversion for query binding still belongs in `lapidary-db` and is not part of
what closed here.

### 2. `KernelOutput` will have to change, despite a README saying it will not
`crates/lapidary-cad/src/kernel.rs` returns `{triangle_count, bbox_mm, entities: Vec<String>}`.
`ARCHITECTURE.md` specifies `{tessellation_l0/l1/l2.glb, structure.json, entities.json}`, and
`sidecar/occt-bridge/README.md` asserts "the trait does not change."

It will. Phase 0b needs blob references for the LOD ladder, and measurement cannot snap to
opaque strings like `"CYLINDRICAL_SURFACE:22.000"` — it needs axes, radii and normals. The
change is cheap now (`mock.rs` plus four tests are the only construction sites) and the
mesh-empty invariant survives any richer entity type.

**README half closed — Task 6 (`cbb4a9f`), 2026-09-02.** `sidecar/occt-bridge/README.md` no
longer claims the trait does not change. It now states what is actually stable — the `Kernel`
trait's shape and the mesh-yields-no-analytic-entities invariant — and says plainly that
`KernelOutput`'s fields will change in Phase 0b. **The substance of this item is still open**:
the `KernelOutput` redesign itself is Phase 0b work and has not happened. Only the false
documentation claim was corrected.

### 3. No `mock-kernel` feature path reaches the binary
**Closed — Task 3 (`e63034e`, `12cf99b`), 2026-09-02.** The `worker` compose service now
reaches the mock kernel: `bin/lapidary-server/Cargo.toml` gained an optional `lapidary-cad`
dependency behind a `mock-kernel` feature (`["dep:lapidary-cad", "lapidary-cad/mock-kernel"]`,
which also suppresses Cargo's implicit `lapidary-cad` feature — `12cf99b` closed a second leak
of the same kind that the first pass missed), `bin/lapidary-server/src/main.rs` logs the kernel
description at startup, and `deploy/Containerfile` now builds with `--features mock-kernel`.
Default builds are unaffected. The worker reaches `lapidary-cad` directly rather than through a
feature chain via `lapidary-api`, which also let Task 3 drop the unused `api -> cad` edge (item
5 below).

The spec said the compose `worker` runs with `mock-kernel` enabled in 0a. It did not:
`deploy/Containerfile` passed no `--features`, and no crate exposed a passthrough, so the mock
kernel was compiled out of the container entirely.

### 4. `worker` and `api` are the same process on different ports
Correct for 0a's "prove the topology" goal. Phase 1 needs a role switch — a `LAPIDARY_ROLE`
env var or a subcommand — before the worker leases jobs, or the queue is served by two full
HTTP routers and `lapidary-server`'s "api + optionally in-process worker" description stays
fiction.

### 5. `lapidary-api` depends on `lapidary-cad` without using it
**Closed — Task 3 (`e63034e`), 2026-09-02.** The `lapidary-cad.workspace = true` line was
removed from `crates/lapidary-api/Cargo.toml`, and `docs/ARCHITECTURE.md` was amended to say
`lapidary-api` depends on the L2 crates it uses, not "all L2" — with the reason `cad` is
excluded stated explicitly: the open path lives here and must never invoke the kernel.

`ARCHITECTURE.md` used to say L3 depends on all L2, so this was plan-mandated. But the open
path lives in this crate and "the open path never invokes the CAD kernel" was one `use` away
with nothing mechanical stopping it.

### 6. `Approximate<T>` is exported and used nowhere
It exists so the approximate label is "unavoidable at the type level rather than a UI
convention," yet `PartSummary` carries a bare `bool` and `triangle_count: Option<u32>` — an
inherently mesh-derived number — is unwrapped. Defensible for a grid row that wants one badge,
but its first real use in Phase 3 will decide whether it gets used at all.

### 7. The empty-state copy promises an interaction that does not exist
`strings.emptyLibrary.body` reads "Drop a folder of STL or STEP files to begin." Nothing
implements dropping a folder. This becomes true exactly when Phase 1 ships ingest — so make it
a Phase 1 acceptance item: implement the drop affordance, or change the copy.

### 8. Prototype knowledge that was not captured before deletion
**Closed — Task 7 (`ae49f39`), 2026-09-02.** All three missing areas are now in
`docs/prototype-notes.md`, read from `origin/main` and cited to the prototype files they came
from: "Ingest pipeline: ordering, idempotency, and failure modes", "Library scan: directory walk
and the debounce that never existed" (the notes record that no debounce existed — a real
finding, not padding), and "Slicer profile parsing".

The spec made recording seven areas a precondition of deleting the Node prototype.
`docs/prototype-notes.md` used to cover only four: domain shape, search payload, LOD approach,
and what was deliberately dropped. Three were missed:

- `assetPipeline` / `meshSidecar` — ingest **ordering and stages** (only the LOD algorithm survived)
- `libraryScan` — directory-walk and debounce behaviour
- `profileImport` / `printerSettings` / `printerType` — slicer profile parsing

All recoverable from `main`, which is why this was not urgent. Slicer `.ini`/`.json` parsing is
hard-won and headed for `lapidary-targets`; it was captured before that work starts.

### 12. `lapidary-api → lapidary-cad` is still permitted by `edge_allowed` (L3→L2)
**Still open.** Item 5 above (closed) removed the actual dependency edge and left a comment in
`bin/lapidary-server/Cargo.toml` asserting the invariant ("lapidary-api never depends on
lapidary-cad — the open path lives there and must never invoke the kernel"), but nothing
structural stops the edge coming back: `edge_allowed(Layer::L3, Layer::L2)` in
`xtask/src/layers.rs` returns `true` unconditionally for any L3→L2 pair, `lapidary-cad` included.
A future contributor who adds `lapidary-cad.workspace = true` back to `crates/lapidary-api/
Cargo.toml` passes `cargo xtask check-layers` cleanly.

The same argument that justified item 1 above ("enforced today only by review — and I am not
always the reviewer") applies here, and this rule has a non-negotiable product statement behind
it: "the open path never touches a source file and never invokes the CAD kernel." A tier rule
can't express it, because it is a named-pair exception (this one L3 crate, this one L2 crate),
not a layer relation — `edge_allowed` operates on `Layer`, not on crate names. It needs a
different mechanism: an explicit forbidden-pairs list checked alongside the tier rule, or an
allow-list of the specific L2 crates `lapidary-api` may depend on.

### 13. The `api` container links `lapidary-cad`
**Still open.** Because both compose services (`api`, `worker`) build from the same
`deploy/Containerfile` with `--features mock-kernel` (item 3 above, closed), the single binary
that serves the open path links the kernel crate even though `lapidary-api` itself does not
depend on it (item 5 above, closed, keeps that edge out of the crate graph). Harmless in 0a
while nothing in the `api` role calls into `lapidary-cad` — but it means the open-path binary
and the worker binary are, today, literally the same artifact. Worth separating the images, or
splitting the binary by role (see item 4 above), before Phase 1 puts real code behind the
open-path/kernel boundary.

---

## Hardening, any time

### 9. `publish = false` on the application crates
**Closed — Task 4 (`d350ab6`), 2026-09-02.** All 13 manifests (11 under `crates/`, 2 under
`bin/`) now carry `publish = false`, the `version = "0.1.0"` pins were dropped from all 11
internal path entries in `[workspace.dependencies]`, and `deny.toml` gained
`allow-wildcard-paths = true` with a comment tying the two changes together. The guard was
proven to bite (`cargo deny check bans` fails naming the internal path dependencies with the
line commented out) before being restored.

This would have prevented an accidental `cargo publish` of an AGPL app crate, and let
`deny.toml` use `allow-wildcard-paths` instead of `version = "0.1.0"` on every internal path
dependency — removing the coupling that required updating eleven lines on a workspace version
bump. It had been rejected during Phase 0a's own execution only because it touched five task
briefs to fix a one-file problem.

### 10. Secrets are visible to `docker inspect`
Compose interpolates `POSTGRES_PASSWORD` into `DATABASE_URL` for `api` and `worker`, so the
plaintext password is readable by anyone with engine access. Standard for compose without a
secrets backend, not baked into an image layer. Consider podman/docker `secrets:` or an
external manager when the fleet story lands.

### 11. Smaller items, none load-bearing
- **Closed — Task 5 (`8d0c617`, `f941b8c`), 2026-09-02.** `crates/lapidary-db` used to map
  every `connect()` failure to `Unreachable`, so a wrong password read as "could not reach the
  database." It now classifies by SQLSTATE into `AuthenticationFailed`, `DatabaseMissing`, and
  `Unreachable` (the fallback), each carrying only a redacted target — never the raw URL — and
  is covered by a regression test asserting no rendered message leaks credentials.
  `bin/lapidary-server/src/main.rs`'s startup context no longer overrides the classified message
  with a hardcoded "the database is unreachable" assertion.
- **Closed — Task 6 (`cbb4a9f`), 2026-09-02.** `web/tsconfig.json`'s `"include"` now covers
  `vite.config.ts` and `vitest.config.ts`; both typechecked clean as-is, so no new dev
  dependency was needed.
- **Still open.** Error variants stringly-type ids that `lapidary-core` already models
  (`StorageError::NotFound { hash: String }`, `VcsError::RevisionNotFound { revision: String }`).
  Consistent across all ten enums, so it is house style rather than drift — but Phase 1 either
  propagates the strings or edits every variant. Deliberately out of scope for the 2026-09-02
  pass: changing house style once is a Phase 1 decision, not a cleanup.
- **Closed — Task 3 (`e63034e`), 2026-09-02.** `deploy/Containerfile`'s `EXPOSE` line now lists
  both ports (`EXPOSE 8080 8081`) instead of naming only `api`'s and silently omitting `worker`'s.
- **Closed — Task 6 (`cbb4a9f`, `bbcd8ee`), 2026-09-02.** `rust-toolchain.toml` still pins
  1.95.0 and the build image is still `rust:1.95-*` — neither pin changed — but
  `deploy/Containerfile`'s build stage now asserts the image's actual installed `rustc` matches
  the pin, failing loudly (with a non-empty-string guard against a silent parse-degradation
  bypass, added in `bbcd8ee` after review) rather than letting a future digest bump silently
  turn the build network-dependent.
- **Still open.** `web/src/lib/api.ts` hand-writes the `Health` wire type. Correct today — that
  shape belongs to `lapidary-api` and has no binding — but it is the template Phase 1 will copy,
  in a branch whose `types.ts` says never to import domain types except through it. Deliberately
  out of scope for the 2026-09-02 pass: there was nothing wrong to fix yet.

### 14. The `publish = false` / `allow-wildcard-paths` pair is enforced by comment only
**Still open.** Item 9 above (closed) set `publish = false` on all 13 internal manifests and
added `allow-wildcard-paths = true` to `deny.toml`, with a comment tying the two together —
`allow-wildcard-paths` is workspace-wide, and it is only safe because every member that could be
reached by a wildcard path dependency also carries `publish = false`. But that pairing is
enforced by the comment alone: a new crate added to the workspace without `publish = false`
silently inherits the wildcard-path exemption, and nothing fails to tell you.
`cargo xtask check-layers` already fails on a workspace member missing from `layer_of`, so it is
a natural place to also assert `publish = false` on every member it walks. Low risk today — one
person adding crates, reviewing their own PRs — but this repo's stated style is "enforced here
rather than by review," and this is exactly the kind of invariant that erodes silently.

---

## Phase 0b, unchanged from the spec

The `occt-bridge` C++ sidecar, OCCT built from source, `OcctKernel`, and the 200-part STEP
assembly exit test. **That fixture does not exist and must be sourced or generated first.**
Also audit `fixtures/` for licences — it currently holds only `cube.stl`, and Phase 1 needs a
licence-clean example part for first-run seeding.
