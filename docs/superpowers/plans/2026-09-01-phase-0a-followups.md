# Phase 0a — follow-ups

**Status:** open
**Date:** 2026-09-01
**Source:** the Phase 0a whole-branch review, plus findings deferred during execution.
**Phase 0a itself is complete** — 8 of 8 exit criteria verified from a clean clone; see
`2026-09-01-phase-0a-verification.md`.

Everything below was found, judged, and deliberately **not** done in 0a. Each item says what
it is, why it was left, and what it costs to leave. Nothing here blocks the phase.

---

## Decisions waiting on the owner

### Push the branch, and let CI run for the first time
The branch is ~42 commits ahead of `origin/rust-rewrite` and has never been pushed. Pushing is
outward-facing, so it was not done unprompted. Until it happens, `.github/workflows/ci.yml` and
`containers.yml` are the only deliverables in this phase that have never executed — every gate
they run has been verified locally by hand, but GitHub Actions itself is unproven.

A push sends ~42 commits including the one deleting the Node prototype (64 files), and starts
consuming Actions minutes on every subsequent push.

### Whether `lapidary-enterprise` should be structurally prevented from being a dependency of `lapidary-api`
The layering rule permits L3 → L3, so `lapidary-api → lapidary-enterprise` passes CI. That edge
would make the free application depend on the enterprise crate, contradicting "the application
is free and complete." It is enforced by review, documented at `xtask/src/layers.rs:9-16`.

The structural fix is to promote `lapidary-enterprise` to its own layer above L3, so
`enterprise → api` is permitted and `api → enterprise` is not. Roughly four lines in
`layer_of` and `edge_allowed`, plus a note in `ARCHITECTURE.md`. Deferred because it changes the
spec's documented four-layer scheme and both crates are empty in 0a.

---

## Before or during Phase 1

### 1. Id newtypes cannot be built from stored values
`LibraryId`, `PartId`, `RevisionId` expose `new()`, `as_uuid()` and `Display`, but no
`from_uuid` and no `FromStr`, and the tuple field is private. `PartRepository::page` already
commits to returning them, so the first real query cannot compile. Additive to fix.

Note `lapidary-core` may not depend on `sqlx` (enforced by `deny.toml`), so the
`Uuid ↔ newtype` conversion belongs in `lapidary-db`. Decide that deliberately rather than
discovering it.

### 2. `KernelOutput` will have to change, despite a README saying it will not
`crates/lapidary-cad/src/kernel.rs` returns `{triangle_count, bbox_mm, entities: Vec<String>}`.
`ARCHITECTURE.md` specifies `{tessellation_l0/l1/l2.glb, structure.json, entities.json}`, and
`sidecar/occt-bridge/README.md` asserts "the trait does not change."

It will. Phase 0b needs blob references for the LOD ladder, and measurement cannot snap to
opaque strings like `"CYLINDRICAL_SURFACE:22.000"` — it needs axes, radii and normals. The
change is cheap now (`mock.rs` plus four tests are the only construction sites) and the
mesh-empty invariant survives any richer entity type. **Amend the README's stability claim
rather than letting it stand.**

### 3. No `mock-kernel` feature path reaches the binary
The spec says the compose `worker` runs with `mock-kernel` enabled in 0a. It does not:
`deploy/Containerfile` passes no `--features`, and no crate exposes a passthrough, so the mock
kernel is compiled out of the container entirely. Harmless while the worker idles.

Wiring it means either a feature chain `lapidary-server → lapidary-api → lapidary-cad`, or
having the worker depend on `lapidary-cad` directly. The second is cleaner and would also let
you drop the unused `api → cad` edge below. Decide once.

### 4. `worker` and `api` are the same process on different ports
Correct for 0a's "prove the topology" goal. Phase 1 needs a role switch — a `LAPIDARY_ROLE`
env var or a subcommand — before the worker leases jobs, or the queue is served by two full
HTTP routers and `lapidary-server`'s "api + optionally in-process worker" description stays
fiction.

### 5. `lapidary-api` depends on `lapidary-cad` without using it
`ARCHITECTURE.md` says L3 depends on all L2, so this is plan-mandated. But the open path lives
in this crate and "the open path never invokes the CAD kernel" is now one `use` away with
nothing mechanical stopping it. The dependency is unused today, so dropping it is zero-cost
hardening; if it must stay, put the reason in a comment beside it.

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
The spec made recording seven areas a precondition of deleting the Node prototype.
`docs/prototype-notes.md` covers four: domain shape, search payload, LOD approach, and what was
deliberately dropped. Three were missed:

- `assetPipeline` / `meshSidecar` — ingest **ordering and stages** (only the LOD algorithm survived)
- `libraryScan` — directory-walk and debounce behaviour
- `profileImport` / `printerSettings` / `printerType` — slicer profile parsing

All recoverable from `main`, which is why this is not urgent. Slicer `.ini`/`.json` parsing is
hard-won and headed for `lapidary-targets`; capture it before that work starts.

---

## Hardening, any time

### 9. `publish = false` on the application crates
Would prevent an accidental `cargo publish` of an AGPL app crate, and would let `deny.toml` use
`allow-wildcard-paths` instead of the current `version = "0.1.0"` on every internal path
dependency — removing the coupling that requires updating eleven lines on a workspace version
bump. Rejected during execution only because it touched five task briefs to fix a one-file
problem. Cheap now: 13 manifests plus one `deny.toml` line.

### 10. Secrets are visible to `docker inspect`
Compose interpolates `POSTGRES_PASSWORD` into `DATABASE_URL` for `api` and `worker`, so the
plaintext password is readable by anyone with engine access. Standard for compose without a
secrets backend, not baked into an image layer. Consider podman/docker `secrets:` or an
external manager when the fleet story lands.

### 11. Smaller items, none load-bearing
- `crates/lapidary-db` maps every `connect()` failure to `Unreachable`, so a wrong password
  reads as "could not reach the database."
- `web/tsconfig.json`'s `"include": ["src"]` leaves `vite.config.ts` and `vitest.config.ts`
  untypechecked.
- Error variants stringly-type ids that `lapidary-core` already models (`StorageError::NotFound
  { hash: String }`, `VcsError::RevisionNotFound { revision: String }`). Consistent across all
  ten enums, so it is house style rather than drift — but Phase 1 either propagates the strings
  or edits every variant.
- `deploy/Containerfile`'s `EXPOSE 8080` is shared by `api` and `worker`, which binds 8081.
- `rust-toolchain.toml` pins 1.95.0 while the build image is `rust:1.95-*`; they match today
  because the digest is pinned, but a future digest bump to a 1.95.1 image would turn the
  container build into a network-dependent `rustup` download.
- `web/src/lib/api.ts` hand-writes the `Health` wire type. Correct today — that shape belongs
  to `lapidary-api` and has no binding — but it is the template Phase 1 will copy, in a branch
  whose `types.ts` says never to import domain types except through it.

---

## Phase 0b, unchanged from the spec

The `occt-bridge` C++ sidecar, OCCT built from source, `OcctKernel`, and the 200-part STEP
assembly exit test. **That fixture does not exist and must be sourced or generated first.**
Also audit `fixtures/` for licences — it currently holds only `cube.stl`, and Phase 1 needs a
licence-clean example part for first-run seeding.
