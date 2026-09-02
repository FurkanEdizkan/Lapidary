# Phase 0a — follow-ups

**Status:** open — partially closed. Two execution passes on 2026-09-02 have closed most of
what was self-contained and verifiable; see the two execution notes below and the item markers
throughout for what remains.
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

**2026-09-02 second execution pass.** A four-task plan then ran against the push decision
plus three more self-contained items from this list —
`docs/superpowers/plans/2026-09-02-phase-0a-followups-round-2.md` has the task-by-task
detail and its "Scope rulings" section explains what it deliberately left out and why;
`git log` on `rust-rewrite` (commits `f4c89d9`..`0095d1b`) has the actual diffs. Items it
closed are marked **Closed** below with the task and commit that did it.

Everything not marked **Closed** — including the remaining owner decision, the whole
Phase 0b section, and every item both passes left untouched — is still open.

---

## Decisions waiting on the owner

### Push the branch, and let CI run for the first time
**Closed, 2026-09-02.** The owner answered and authorised the push. `origin/rust-rewrite` moved
`16b60b3..15494c3` — a clean fast-forward of 63 commits, confirmed with
`git merge-base --is-ancestor 16b60b3 15494c3`. CI ran for the first time in the project's
history: run `33608501887`
(`https://github.com/FurkanEdizkan/Lapidary/actions/runs/33608501887`), triggered by that push,
all four jobs green — `rust`, `deny`, `web`, `bindings`.

**The half that is still true: `containers.yml` has still never run.** It triggers only on
`workflow_dispatch` and on `push: tags: ["v*"]` (see the file itself), and a branch push is
neither, so this push did not exercise it. That is by design, not a defect — but it means "CI is
proven" would overstate what happened. What ran: `ci.yml`, on the push, green. What has not:
`containers.yml`, which needs either a manual dispatch or a `v*` tag push to run for the first
time.

Was previously the reason `.github/workflows/ci.yml` and `containers.yml` had never executed;
the branch had never been pushed, and its lead over `origin/rust-rewrite` kept growing with
every commit.

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
**Copy half closed — round-2 Task 3 (`db02e85`, `0095d1b`), 2026-09-02.**
`strings.emptyLibrary.body` no longer says "Drop a folder of STL or STEP files to begin." It now
reads "Parts will appear here as your library grows." — true today, and it promises no
interaction the app does not implement. `web/src/routes/index.test.tsx` gained a regression test
asserting the component renders `strings.emptyLibrary.body`, so the component's *use* of the
string cannot drift silently.

**The ingest half is still open.** The copy no longer lies, but the app still cannot ingest
anything — there is no drop target, file picker, or ingest path. Phase 1 shipping ingest and
restoring a truthful call to action remains an acceptance item: implement the drop affordance
(or whatever Phase 1 lands on), and update the copy to match.

Previously: `strings.emptyLibrary.body` read "Drop a folder of STL or STEP files to begin."
Nothing implemented dropping a folder.

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
**Closed — round-2 Task 1 (`132a349`, `adaa7f3`), 2026-09-02.** `xtask/src/layers.rs` gained a
`FORBIDDEN_PAIRS` constant — a list of `(from, to, why)` triples, checked in `check()` alongside
the existing tier rule — with one entry today: `("lapidary-api", "lapidary-cad", …)`, naming the
open-path rule in its `why`. `lapidary-api -> lapidary-cad` now fails `cargo xtask check-layers`
structurally, with a `Violation` variant whose `Display` names both crates and states the rule;
a different L3→L2 edge (`lapidary-api -> lapidary-index`) is still allowed, so this is a named
pair, not a blanket L3→L2 ban. Proven to bite: adding `lapidary-cad.workspace = true` back to
`crates/lapidary-api/Cargo.toml` was confirmed to fail the check before being reverted.
`adaa7f3` trimmed `xtask/src/main.rs`'s footer to stay generic rather than naming today's one
`FORBIDDEN_PAIRS` entry, so it does not go stale when a second pair is added. **This is the
mechanism for adding another named-pair prohibition in future — extend `FORBIDDEN_PAIRS` in
`xtask/src/layers.rs`, not `edge_allowed`, which still operates on `Layer` tiers and cannot
express a single-pair exception.**

Previously: item 5 (closed, above) removed the actual dependency edge and left only a comment in
`bin/lapidary-server/Cargo.toml` asserting the invariant; nothing structural stopped the edge
coming back, because `edge_allowed(Layer::L3, Layer::L2)` returned `true` unconditionally for
any L3→L2 pair, `lapidary-cad` included.

### 13. The `api` container links `lapidary-cad`
**Images half closed — round-2 Task 2 (`664279c`), 2026-09-02.** `deploy/Containerfile` now
takes the feature list as a build arg, `SERVER_FEATURES`, defaulting to empty and declared
inside the build stage (a bare `ARG` before `FROM` would have been the wrong scope). Only
`deploy/compose.yaml`'s **worker** service sets it, to `mock-kernel`; **api** passes nothing and
builds without `lapidary-cad` linked at all. Verified by running both images and reading their
startup kernel line: `api` logs `kernel=none`, `worker` logs `kernel=mock 0a`.

**Not the same as splitting the binary by role — item 4 stays open and is still the real fix.**
Separating the images removes the kernel from the artifact that serves the open path today, but
`api` and `worker` are still the same binary run with different flags, not a role-aware process;
item 4's `LAPIDARY_ROLE` switch (or a subcommand) is the change that actually gives the worker
its own identity, and it stays blocked on job leasing existing first.

Previously: because both compose services built from the same `deploy/Containerfile` with
`--features mock-kernel` hardcoded (item 3 above, closed), the single binary that serves the
open path linked the kernel crate even though `lapidary-api` itself did not depend on it (item 5
above, closed, kept that edge out of the crate graph).

### 15. The `worker`-only-links-kernel invariant is enforced by comment, not by CI
**Configuration half closed — deploy-check Task 1 (`34cabe4`, `69385e4`, `f5af5ad`), 2026-09-02.** `cargo
xtask check-deploy` (new, `xtask/src/deploy.rs`) now checks three rules against the deploy
config: exactly the services in `KERNEL_LINKED_SERVICES` (today `["worker"]`) set
`SERVER_FEATURES` in `deploy/compose.yaml`; `deploy/Containerfile`'s `cargo build` line routes
through the `${SERVER_FEATURES:+--features "$SERVER_FEATURES"}` expansion rather than a
hardcoded flag; and `ARG SERVER_FEATURES` is visible to that build line — declared before it,
with no `FROM` in between, checked across every stage rather than just the first. A fourth
violation class catches the parser itself going stale (no `services:` key, no service block, no
`cargo build` line, or no `ARG` declaration found), naming the function to fix rather than
blaming the config. `.github/workflows/ci.yml` runs `cargo xtask check-deploy` on every push,
beside `check-layers`. **The check is static**: it reads the text of `deploy/compose.yaml` and
`deploy/Containerfile`; it does not build an image, and its own failure message says so.

**The image half is still open.** `.github/workflows/containers.yml` is unchanged by this work
(last touched in `158bfa6`, before any of the three passes) and still runs
`docker build -f deploy/Containerfile -t lapidary-server:${{ github.sha }} .` with no build arg.
So no CI build exercises the `SERVER_FEATURES=mock-kernel` path, and — this is the part a green
`check-deploy` cannot speak to — **nothing verifies the built artifacts**, only the configuration
that would produce them. Harm today is still zero: that workflow has never run — it triggers
only on `workflow_dispatch` or a `v*` tag push, neither of which has happened — and pushes to no
registry. The fuller fix — a second `docker build --build-arg SERVER_FEATURES=mock-kernel` step
in `containers.yml` — starts to pre-empt item 4 (the `worker`/`api` role split) and may be
better left until that is decided.

Previously: item 12 converted the kernel-dependency invariant from comment-enforced to
check-enforced (`lapidary-api -> lapidary-cad` fails `cargo xtask check-layers`, in `ci.yml` on
every push). Item 13 created a sibling invariant — that only the `worker` image links the
kernel — and left it enforced by nothing but a comment in `deploy/Containerfile`: setting
`SERVER_FEATURES: mock-kernel` on the `api` service in `deploy/compose.yaml`, or re-hardcoding
`--features` in `deploy/Containerfile`, would fail no check. That configuration gap is what
closed here; the image-verification gap did not.

### 16. The open-path rule's other half — never touches a source file — is unenforced
Item 12 closed the kernel half of "the open path never touches a source file and never invokes
the CAD kernel" — twice over, given item 13. The source-file half is untouched: `lapidary-api`
still depends on `lapidary-storage`, so nothing structural stops an open-path handler from
reading source bytes instead of derivatives.

A `FORBIDDEN_PAIRS` entry cannot express this one. `lapidary-api` legitimately needs
`lapidary-storage` for derivatives — thumbnails, tessellations — so the crates must stay
connected; the distinction is *which bytes* a handler reads, not whether the crates may be
connected at all, and a dependency-graph check only sees the latter. This likely needs an
API-level boundary rather than a dependency-level one — something like a derivatives-only handle
into `lapidary-storage` that open-path handlers can hold and a source-reading one they cannot —
and is Phase 1 design work.

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
**Closed — round-2 Task 1 (`132a349`), 2026-09-02.** `xtask/src/layers.rs` gained
`check_publish`, a pure function over member-name and whether `cargo metadata`'s `publish` field
is `null` (unpublishable crates report `[]`; the 14 current members were verified to all report
`[]`), called from `main.rs` for every workspace member `check-layers` walks. A member missing
`publish = false` now fails the check, with a `Violation::Publishable` whose message
says what to add and why — that `deny.toml`'s `allow-wildcard-paths` depends on every member
being unpublishable. Proven to bite: temporarily removing `publish = false` from one manifest
was confirmed to fail the check, naming that crate, before being reverted.

Previously: item 9 (closed, above) set `publish = false` on all 13 internal manifests and added
`allow-wildcard-paths = true` to `deny.toml`, with only a comment tying the two together — a new
crate added to the workspace without `publish = false` would have silently inherited the
wildcard-path exemption, with nothing failing to tell you.

---

## Phase 0b, unchanged from the spec

The `occt-bridge` C++ sidecar, OCCT built from source, `OcctKernel`, and the 200-part STEP
assembly exit test. **That fixture does not exist and must be sourced or generated first.**
Also audit `fixtures/` for licences — it currently holds only `cube.stl`, and Phase 1 needs a
licence-clean example part for first-run seeding.
