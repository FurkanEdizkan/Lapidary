# Migration from the prototype

**Delete this file once Phase 1 passes.** It describes a one-time cutover, not
steady-state architecture.

## Situation

The repository at `FurkanEdizkan/Lapidary` (10 commits) is a working **Node.js
prototype**: Fastify + SQLite (`better-sqlite3`), an optional Redis cache, a React/Vite
frontend, and a small optional Rust sidecar (`rust-mesh`) for bbox, triangle count and
LOD. Language split is roughly HTML 41% / TypeScript 30% / JavaScript 27% / Rust 2%.

The architecture in `docs/ARCHITECTURE.md` is a **Rust workspace**: axum as a library,
PostgreSQL 18, a Postgres-backed job queue with no broker, a native OCCT sidecar.

These are not reconcilable by refactoring. The prototype validated the product idea —
image-first grid, three-tier asset storage, the viewer — and several of its decisions
were correct enough that they survived into the plan. It is now a **reference
implementation to read and then delete**, not a base to build on.

Do not attempt an incremental port. A half-Node, half-Rust codebase with two databases
will cost more than a clean rewrite of ~10 commits of prototype.

---

## Disposition of every path

### Keep as-is

| Path | Why |
|---|---|
| `design/` | Design handoff bundle. Still authoritative for the visual language. |
| `fixtures/` | Sample models. Needed for tests and Phase 1 first-run seeding. **Verify every fixture is licence-clean before the repo gets attention** — a CC-BY-NC model in a shipped seed is a real problem. |
| `.claude/skills/` | Review contents; keep anything still accurate, delete anything describing the Node stack. |
| `.gitignore` | Extend for Rust (`target/`) and Node. |

### Keep, adapt

| Path | Action |
|---|---|
| `web/` | The React/Vite app survives, but every API call changes. Port component by component during Phases 1–3. Add TanStack Router + Query, Tailwind v4, `ts-rs`-generated types. Strip Fastify-shaped fetch logic. |
| `README.md` | Rewrite. The current one documents `npm run dev`, SQLite and Redis — all wrong now. Its feature list is a good record of what the prototype proved. |
| `.env.example` | Rewrite for `LAPIDARY_*` prefix, `DATABASE_URL`, no `REDIS_URL`. |
| `Dockerfile` → `Containerfile` | Rename per the Podman-first convention, rewrite for the Rust workspace + OCCT build. |
| `compose.yaml` | Rewrite: `web`, `api`, `worker`, `db` (`postgres:18`). **Remove the `redis` service** — the job queue is `FOR UPDATE SKIP LOCKED` + `LISTEN/NOTIFY`, and the plan takes no broker dependency. |

### Read, then delete

| Path | Why it goes |
|---|---|
| `server/` | Fastify + SQLite + `better-sqlite3`. Replaced entirely by the Rust crate graph. **Read `server/src/services` first** — the service decomposition encodes real domain knowledge about ingest ordering, LOD generation and slicer-profile parsing that is worth carrying into the Rust crates. |
| `rust-mesh/` | Superseded by `lapidary-cad` + the OCCT sidecar. Read the decimation code before deleting; the LOD approach may transfer directly. |
| `package.json`, `package-lock.json` (root) | Root-level Node monorepo goes. `web/` keeps its own. |
| `data/` if committed | Runtime state, must not be in git. |

### Explicitly rejected, do not carry forward

- **SQLite.** PostgreSQL everywhere. See `docs/ARCHITECTURE.md`.
- **Redis.** The job queue and cache are both Postgres. One less service to run in an
  air-gapped shop.
- **Per-model procedural sample shapes.** Phase 1 seeds one real licence-clean example.
- **`npm run dev` as the primary run path.** Container-first;
  `podman compose up` is the documented entry point.

---

## Cutover order

Work on a branch. Do not delete anything until its replacement passes tests.

1. **Branch `rust-rewrite`.** Leave `main` on the working prototype so you always have
   something that runs.
2. **Add the docs** (`CLAUDE.md`, `docs/`). Commit alone, so the diff is readable.
3. **Scaffold the Cargo workspace** alongside the existing tree. Nothing deleted yet.
4. **Phase 0** — Containerfile with OCCT, compose stack up, CI layering check,
   `cargo-deny`, pinned Actions. `server/` still present and still runs.
5. **Phase 1** — Rust ingest + grid reaches parity with the prototype's gallery.
6. **Delete `server/`, `rust-mesh/`, root `package.json`, and the `redis` service.**
   One commit, clearly messaged. This is the point of no return; `main` still has the
   prototype in history.
7. **Rewrite `README.md`** for the container-first reality.
8. **Merge to `main`, tag `v0.1.0-alpha`.** Delete this file.

## Before any of it

Two things worth doing while the repo has no stars and no forks:

- **Settle the licensing conflict** in `docs/ARCHITECTURE.md`. AGPL app plus a
  proprietary enterprise crate cannot coexist, and DCO-signed contributions cannot be
  relicensed later. There is currently no `LICENSE` file — add one deliberately rather
  than by default.
- **Audit `fixtures/` licences.** Non-commercial models shipped as seeds become a
  problem the moment someone sells prints.
