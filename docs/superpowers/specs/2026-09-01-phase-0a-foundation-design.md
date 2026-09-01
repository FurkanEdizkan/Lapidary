# Phase 0a — Greenfield foundation

**Status:** design, approved. Revised 2026-09-01 after a review pass over the implementation plan — see *Revisions*.
**Date:** 2026-09-01
**Branch:** `rust-rewrite`
**Supersedes:** `docs/MIGRATION.md` (deleted by this phase)

---

## Context

`docs/MIGRATION.md` described a staged cutover from the Node prototype: keep `web/`,
adapt the deploy files, delete `server/` only after Phase 1 reached parity. That plan is
withdrawn. This is a **fresh start**, not a migration.

The prototype — Fastify + SQLite, the React/Vite app, the `rust-mesh` sidecar — remains
on `main` and in git history. It is a reference implementation to read, not a base to
build on, and nothing in this phase depends on it continuing to run.

`docs/ARCHITECTURE.md`, `docs/DATA.md`, `docs/FEATURES.md` and `docs/ROADMAP.md` stay
authoritative and are unchanged by this phase, except for removing the MIGRATION row from
`docs/README.md`.

## Scope

Phase 0 in `docs/ROADMAP.md` bundles two very different risks: predictable Rust and
container scaffolding, and an OCCT-from-source C++ build that is the highest-uncertainty
item left in the plan. This spec covers **0a only** — everything except OCCT.

**In scope:** Cargo workspace and crate graph, CI layering check, `cargo-deny`, pinned
Actions, AGPL LICENSE, container and compose stack, fresh `web/` on TanStack, `ts-rs`
export pipeline, deletion of the prototype.

**Out of scope, deferred to 0b:** the `occt-bridge` C++ sidecar, OCCT built from source
in the Containerfile, and the 200-part STEP assembly exit test. `sidecar/occt-bridge/`
exists in 0a as a directory with a README and nothing else.

**Out of scope entirely:** any Phase 1 behaviour. No blob CAS implementation, no job
queue implementation, no ingest, no grid. Crates get their trait surface and error types;
bodies arrive in later phases.

---

## Decisions taken

| Decision | Choice | Why |
|---|---|---|
| Frontend framework | Vite + React + TanStack Router + TanStack Query | Next.js would put a Node runtime in the production container. `ARCHITECTURE.md` specifies a static SPA in a `web`/`api`/`worker`/`db` topology; `CLAUDE.md` demands fewer moving parts for air-gapped deployment; the later Tauri app bundles only our own binaries and cannot ship a Next server. SSR buys nothing for an authenticated tool that opens on a virtualized grid. |
| Layering enforcement | `xtask` crate over `cargo metadata` | Unit-testable against a synthetic graph, no host dependencies, identical locally and in CI. `cargo-deny`'s `[bans]` targets external crates and duplicate versions and cannot express intra-workspace layering. |
| Kernel during 0a | In-process `MockKernel` behind a cargo feature | Boring option. `CLAUDE.md` already requires keeping the `Kernel` trait for test doubles. Subprocess spawn, timeout and crash handling land in 0b with the real sidecar rather than being written twice. |
| Prototype disposition | Deleted in this phase | User decision, overriding `MIGRATION.md` step 6. Accepted cost is recorded under Risks. |
| Licence | AGPL-3.0-only, single licence for the whole workspace | `ARCHITECTURE.md`'s own recommendation. `lapidary-enterprise` stays AGPL; the Ed25519 licence file becomes a contractual and support boundary, not technical DRM. Keeps DCO, avoids a CLA. Must land before the first external contribution. Landing it means the `LICENSE` file **and** the four documents that currently record the licence as undecided — `README.md`, `CONTRIBUTING.md`, `ARCHITECTURE.md`, `ROADMAP.md`. |

---

## Repo end state

```
Cargo.toml                    workspace root, edition 2024, resolver 3
rust-toolchain.toml           pinned 1.95.0
deny.toml                     cargo-deny with [sources] allow-list
LICENSE                       AGPL-3.0-only
crates/
  lapidary-core/         L0   domain types, error enum, ts-rs exports. No dependencies on our crates.
  lapidary-db/           L1   sqlx, migrations, repository traits. ALL SQL lives here.
  lapidary-storage/      L1   blob CAS, object_store, zstd, tiering, quarantine
  lapidary-cad/          L2   Kernel trait + MockKernel (OcctKernel in 0b)
  lapidary-jobs/         L2   Postgres queue, leases, heartbeats, SSE progress
  lapidary-index/        L2   metadata extraction, tsvector + trigram search, facets
  lapidary-vcs/          L2   revisions, lineage DAG, locks, geometric diff
  lapidary-build/        L2   build graph, runs, ready-set, guide linearization
  lapidary-targets/      L2   Target trait, format negotiation, export bundles
  lapidary-api/          L3   axum Router. A LIBRARY, never a binary.
  lapidary-enterprise/   L3   licence verify, auth, RBAC, audit, worker fleet
bin/
  lapidary-server/            container entrypoint: api + optionally in-process worker
  lapidary/                   desktop binary: agent | worker | up  (subcommand stubs)
sidecar/occt-bridge/          README only until 0b
xtask/                        cargo xtask check-layers | export-bindings
web/                          Vite + React + TanStack, fresh
deploy/
  Containerfile               multi-stage Rust build
  db/Containerfile            FROM postgres:18, adds pgvector
  compose.yaml                web, api, worker, db
  .env.example                LAPIDARY_* + DATABASE_URL
.github/workflows/ci.yml      Actions pinned to commit SHAs
docs/                         unchanged, minus MIGRATION.md
design/                       kept — authoritative visual language
fixtures/                     kept, licence-audited
```

### Deleted by this phase

`server/`, `rust-mesh/`, `package.json`, `package-lock.json`, `Dockerfile`,
`compose.yaml`, `.dockerignore`, `.env.example`, `MIGRATION.md`, `docs/MIGRATION.md`,
`lapidary-docs.zip`, `web/src/**`, `web/package.json`, `web/vite.config.ts`,
`web/tsconfig.json`, `web/index.html`.

### Kept

`docs/` (minus MIGRATION), `design/`, `fixtures/`, `.claude/skills/` (all three are
stack-agnostic), `CLAUDE.md`, `CONTRIBUTING.md`, `.gitignore` (extended for `target/`).

### Read before deleting

`server/src/services/` is ~1,000 LOC encoding real domain knowledge. Before deletion,
record in the implementation plan what carries into which crate:

| Prototype service | Carries into | What is worth keeping |
|---|---|---|
| `assetPipeline.service.ts`, `meshSidecar.service.ts` | `lapidary-cad`, `lapidary-jobs` | ingest ordering, LOD generation stages |
| `libraryScan.service.ts` | `lapidary-storage`, later the `agent` watcher | directory walk and debounce behaviour |
| `model.service.ts` (249 LOC) | `lapidary-core`, `lapidary-db` | the domain shape that survived contact with real files |
| `search.service.ts` | `lapidary-index` | identifier-aware ranking behaviour |
| `profileImport.service.ts`, `printerSettings.service.ts`, `printerType.service.ts` | `lapidary-targets` | slicer profile parsing |
| `group.service.ts`, `pin.service.ts`, `tag.service.ts` | `lapidary-db` | organisation primitives |
| `cache.service.ts` | nothing | Redis is rejected; cache is Postgres |

`rust-mesh/src/main.rs` decimation approach is read and summarised before deletion; it
may transfer to `lapidary-cad` LOD generation in Phase 1.

---

## Pinned versions

Resolved against crates.io and npm on 2026-09-01. Exact versions, no carets — `CLAUDE.md`
requires pinning everything.

**Toolchain:** Rust 1.95.0, edition 2024, resolver 3. Node 24 for the web build only.

**Rust workspace dependencies**

| Crate | Version | Used by |
|---|---|---|
| `axum` | 0.8.9 | `lapidary-api` |
| `tokio` | 1.53.1 | api, jobs, bins |
| `sqlx` | 0.9.0 | `lapidary-db` only |
| `serde` | 1.0.229 | core, api |
| `serde_json` | 1.0.151 | core, cad, api |
| `ts-rs` | 12.0.1 | `lapidary-core` |
| `thiserror` | 2.0.20 | every library crate |
| `anyhow` | 1.0.104 | `bin/` only |
| `blake3` | 1.8.7 | `lapidary-storage` |
| `zstd` | 0.13.3 | `lapidary-storage` |
| `object_store` | 0.14.1 | `lapidary-storage` |
| `tower` | 0.5.3 | `lapidary-api` |
| `tower-http` | 0.7.1 | `lapidary-api` |
| `tracing` | 0.1.44 | all |
| `tracing-subscriber` | 0.3.23 | `bin/` only |
| `uuid` | 1.26.0 | core |
| `jiff` | 0.2.35 | core — timestamps |
| `clap` | 4.6.6 | `bin/lapidary` |
| `figment` | 0.10.19 | `bin/` config |
| `async-trait` | 0.1.92 | trait surfaces |

**Web dependencies**

| Package | Version | Note |
|---|---|---|
| `react`, `react-dom` | 19.2.8 | |
| `vite` | 8.2.2 | |
| `@vitejs/plugin-react` | 6.1.1 | |
| `typescript` | 7.0.2 | Current stable. The 6.x line never shipped stable — `dist-tags.beta` is `6.0.0-beta`, `latest` is 7.0.2. |
| `@tanstack/react-router` | 1.170.32 | |
| `@tanstack/router-plugin` | 1.168.35 | file-based routing via the Vite plugin |
| `@tanstack/react-query` | 5.102.8 | |
| `@tanstack/react-query-devtools` | 5.102.8 | dev only |
| `tailwindcss`, `@tailwindcss/vite` | 4.3.3 | v4, Vite plugin, no PostCSS config |
| `vitest` | 4.1.11 | |

`three` 0.185.1 is **not** added in 0a. The viewer is Phase 3.

`ts-rs` carries the `uuid-impl` and `jiff-impl` features. They are not optional: without
them ts-rs has no `TS` impl for `Uuid` or `jiff::Timestamp` and `lapidary-core` does not
compile. `jiff::Timestamp` maps onto the TypeScript `string` primitive, so it produces no
file of its own.

Container base images and GitHub Action SHAs are pinned to the literal digests recorded in
the implementation plan, resolved 2026-09-01. `deploy/db/Containerfile` builds
`FROM postgres:18` and installs pgvector; the official image does not include it.

---

## Components

### `lapidary-core` (L0)

Not an empty shell. It carries enough genuine domain surface that the `ts-rs` pipeline is
real and the layering check has actual edges to police:

- Newtype ids: `LibraryId`, `PartId`, `RevisionId`, `BlobHash` (BLAKE3, 32 bytes).
- `PartSummary` — the grid row shape: id, name, part number, thumbnail reference,
  approximate flag, timestamps.
- `LibraryMode` — `Hobby` | `Controlled`, since governance is opt-in per library.
- `Approximate<T>` — the wrapper that makes "mesh-derived, label it approximate"
  unavoidable at the type level rather than a UI convention.
- `CoreError` via `thiserror`, with messages that say what broke and what to do.

Every public type derives `serde` and `ts_rs::TS`.

### Other crates

Each gets its `thiserror` error enum, its public trait surface, and no implementation
bodies. `lapidary-db` gets the repository traits and an empty `migrations/` directory —
and is the only crate permitted to depend on `sqlx`, enforced by `deny.toml`.
`lapidary-api` builds and returns a `Router` with `/api/healthz`; it is a library and
never grows a `main`.

`lapidary-db` also owns the migrator. `sqlx::migrate!` embeds the SQL at compile time and
`lapidary-server` applies it at startup, so an image carries its own schema and an
air-gapped operator needs no migration tooling on the host. `api` and `worker` are the
same binary and start together; sqlx takes a Postgres advisory lock, so the second waits.

### `xtask`

`cargo xtask check-layers` parses `cargo metadata --format-version 1`, assigns each
workspace member its layer from a table in the xtask source, and asserts:

- L0 depends on no workspace crate.
- L1 depends only on L0.
- L2 depends only on L0 and L1 — never on another L2, never on L3.
- L3 may depend on anything below it.

Failure prints the offending edge by name: `lapidary-vcs (L2) -> lapidary-index (L2)`.
Unit tests run the rule against a synthetic in-memory graph and assert it **catches** a
violation, not merely that the real graph passes today.

`cargo xtask export-bindings` runs the `ts-rs` export and writes `web/src/bindings/`.

### `lapidary-cad` MockKernel

The `Kernel` trait from `ARCHITECTURE.md` unchanged. `MockKernel`, behind the
`mock-kernel` feature, returns fixture `KernelOutput` from files under
`crates/lapidary-cad/fixtures/`. The compose `worker` service runs with the feature
enabled in 0a. 0b adds `OcctKernel` and flips the default; the trait does not change.

### `web/`

File-based routing via `@tanstack/router-plugin`. Query client configured but no
endpoints beyond a health check. Established from the first commit because retrofitting
is expensive:

- Dark only. No light mode, no theme toggle.
- `src/lib/strings.ts` — every user-facing string routed through it. English only;
  Turkish is the planned second locale.
- Motion tokens at 120/180/280ms, `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity
  only, `prefers-reduced-motion` respected.
- `src/bindings/` is generated by `cargo xtask export-bindings` and **is committed**, not
  gitignored — CI compares regenerated output against it to catch staleness. `src/lib/types.ts`
  re-exports all of it, so a type renamed on the Rust side breaks the typecheck rather than
  regenerating cleanly into nothing.
- `src/routeTree.gen.ts` is likewise committed. It has to be: `npm run build` and
  `npm run typecheck` both start with `tsc`, and `main.tsx` imports it, so on a clean clone
  a gitignored route tree fails the typecheck before Vite can write it.

Visual language comes from `design/`, which is unchanged and authoritative.

### `deploy/`

`compose.yaml` defines four services — `web`, `api`, `worker`, `db` — with no Redis and
no broker. `:Z` labels on bind mounts for SELinux. Compose Spec syntax, working under
both `podman compose` and `docker compose`, with `--env-file` passed explicitly rather
than relying on auto-loading, which the two runtimes do not agree on.

Ignore files exist under both names — `.containerignore` for Podman and `.dockerignore`
for Docker, which reads only the latter — at the repo root and in `deploy/`, since the
`db` service builds from a different context.

`deploy/Containerfile` is a multi-stage Rust build producing `lapidary-server`. It does
not build OCCT in 0a.

---

## Data flow in 0a

Deliberately thin. A browser loads the static SPA from `web`; TanStack Query calls
`GET /api/healthz` on `api`; `lapidary-api` asks `lapidary-db` for a trivial round trip
against Postgres 18 and returns its status. `worker` starts, connects, registers nothing,
and idles. That is the whole path — its purpose is to prove the topology, the types and
the build, not to do work.

## Error handling

`thiserror` in every library crate, `anyhow` only at the `bin/` edges. No `unwrap()`
outside tests — enforced by a clippy lint at deny level, not by review.

Error text follows `CLAUDE.md`: what broke and what to do about it. The health endpoint's
failure mode is the first test of this — "Could not reach the database at $DATABASE_URL.
Check that the `db` service is running and the credentials in `.env` match." Not
"connection refused".

## Testing

- `xtask` layering rule: unit tests over a synthetic graph, including a violation case.
- `lapidary-core`: `ts-rs` export test, plus round-trip serde tests on the id newtypes.
- `lapidary-api`: `/api/healthz` returns 200 against a real Postgres via `sqlx::test`.
- `web`: Vitest smoke test that the router mounts and the health query renders both
  states.
- Container: `podman compose up` verified by hand against the exit criteria. Not in CI
  on every push — the build is too heavy for 8 cores.

CI on every push runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo test --workspace`, `cargo deny check`, `cargo xtask check-layers`, a
stale-bindings check (`cargo xtask export-bindings && git diff --exit-code
web/src/bindings/`), and the web typecheck and build. The container build is a separate
workflow, triggered manually and on release tags.

All Actions pinned to commit SHAs, resolved at implementation time with `gh api`.

---

## Exit criteria

0a is done when every one of these passes:

1. `cargo xtask check-layers` passes, and provably fails on an injected violation.
2. `cargo clippy --workspace --all-targets -- -D warnings` is clean and
   `cargo test --workspace` is green.
3. `cargo deny check` passes with the `[sources]` allow-list; `Cargo.lock` and
   `web/package-lock.json` are committed; every GitHub Action is pinned to a commit SHA.
4. `podman compose -f deploy/compose.yaml up` on a clean machine brings up `web`, `api`,
   `worker` and `db`, and `GET /api/healthz` returns 200 having actually queried
   Postgres 18.
5. `pgvector` installs against `postgres:18`, and the Turkish snowball `tsvector`
   configuration is present and queryable. `ROADMAP.md` says verify here, not later.
6. Changing a type in `lapidary-core` regenerates `web/src/bindings/*.ts`, and CI fails
   when they are stale.
7. `LICENSE` is AGPL-3.0-only, and `docs/README.md` no longer references `MIGRATION.md`.
8. `docker compose` is verified as well as `podman compose` — both are supported per
   `CLAUDE.md`, and this machine has both.

---

## Risks and accepted costs

**No runnable application on this branch until Phase 1.** Deleting `server/` before its
replacement exists is a deliberate departure from `MIGRATION.md`'s "do not delete
anything until its replacement passes tests". The prototype survives on `main` and in
history, so nothing is lost, but there is no live reference to diff behaviour against
during Phase 1.

**Discarded viewer work.** `web/src/lib/threeViewer.ts`, `mesh3d.ts` and `thumbs.ts` are
working three.js code representing Phase 3 groundwork. They remain readable on `main` and
should be consulted deliberately in Phase 3 rather than carried forward now.

**TypeScript 7.** The native compiler is current stable but young. If ecosystem tooling
proves incompatible during implementation, falling back to the last 5.x release is a
contained change to `web/package.json` and must be recorded here if taken.

**sqlx 0.9 against Postgres 18.** Verify at implementation time that the generated column
`STORED` requirement and `LISTEN/NOTIFY` behave as expected. Confined to `lapidary-db`.

**Fixture licences.** `fixtures/` currently holds only `cube.stl`. Audit it before the
repo gets attention; a non-commercial model shipped as a seed becomes a real problem the
moment someone sells prints. Phase 1 needs a licence-clean example part for first-run
seeding.

## Open items

- The 200-part STEP fixture for the 0b exit test does not exist and must be sourced or
  generated.

## Revisions

**2026-09-01, after reviewing the implementation plan.** Twenty-one findings, four of which
would have stopped a task cold: `ts-rs` was missing `uuid-impl`, so `lapidary-core` would
not have compiled; `routeTree.gen.ts` was gitignored while `tsc` ran before Vite, breaking
every clean-clone typecheck; `deny.toml` allowed the deprecated `AGPL-3.0` rather than the
`AGPL-3.0-only` our crates declare, failing `cargo deny` on all fourteen workspace members;
and no `.dockerignore` existed, so Docker builds would have shipped `target/` as context.

Two things were documented as doing something they did not do: `#[sqlx::test]` silently
runs no migrations unless told where they live, and nothing applied migrations at runtime
at all. Both are fixed above. `PartSummary` was also brought back to the shape this spec
specifies — it had lost the thumbnail reference and timestamps.
