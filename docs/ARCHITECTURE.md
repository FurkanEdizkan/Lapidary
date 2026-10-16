# Architecture

## Distribution model

**One codebase, two artifacts.** All of them are thin shells over `lapidary-api`,
which builds an axum `Router`. Never fork.

| Artifact | What it is | Ships when |
|---|---|---|
| **Container stack** | `app` and `db` via Compose Spec | Phase 1 (primary) |
| **Installer** | The app for one machine, without containers | later; its database is decided then |

Podman is the **recommended** runtime and leads all documentation; Docker is supported.
This is deliberate: Docker Desktop requires a paid subscription above 250 employees or
$10M revenue, and government entities are excluded from the free tier at any size. Our
industrial and defence buyers are exactly those cases.

## Deployment topology

```
                 ┌──────────────────────────────────────────┐
                 │  web (caddy)                             │
                 │  serves dist/, reverse-proxies /api /sse │
                 └────────────────┬─────────────────────────┘
                                  │  one origin — no CORS
                 ┌────────────────▼─────────────────────────┐
                 │  api    (lapidary-server, axum)          │
                 └───────┬──────────────────────┬───────────┘
                         │                      │
              ┌──────────▼─────────┐  ┌─────────▼──────────┐
              │  db (postgres:18)  │  │ worker  ×N          │
              │  named volume      │  │ mesh parsers only   │
              └────────────────────┘  └────────────────────┘
                         ▲
                         │ leases over HTTP, outbound only
              ┌──────────┴──────────────────────────────────┐
              │  remote workers (lapidary worker)           │
              │  registered in Settings → Compute           │
              └─────────────────────────────────────────────┘
```

**Single origin is mandatory.** The `web` container reverse-proxies `/api` to `api`. No
CORS, no cookie-domain problems, no preflight per request. There is no separate `/events`
path: both streams live under `/api` — the app-wide one at `/api/events` and the per-batch
one at `/api/libraries/{library}/jobs/{batch}/events` — so the one proxy rule covers them.

**SSE through the proxy needs `proxy_buffering off`** (nginx) or the Caddy equivalent,
plus `X-Accel-Buffering: no` on the response. Default buffering holds progress events
until the buffer fills and ingest appears frozen. This is the single most common
"works in dev, breaks in prod" bug in this stack. Both streams send that header and
`Cache-Control: no-cache`, from `no_buffering()` in `crates/lapidary-api/src/events.rs`.
Whether `deploy/web/Caddyfile`'s `encode gzip zstd` holds an event stream anyway is **not
yet measured** — G1 stage 5 measures it with `curl -N -H 'Accept-Encoding: gzip' -D -`, and
adds an `@notsse` matcher to that directive if it does.

**Version skew:** build all images from the same commit, tag identically, and expose
`/api/version` returning the build SHA. The frontend compares against its own baked-in
value and shows a banner on mismatch.

### Deployment modes

- **A. Server-managed** — IT runs the stack on one Linux host, users connect over the
  network. No container runtime on workstations. This is the enterprise shape and it is
  what sidesteps Docker Desktop licensing entirely.
- **B. Local single-user** — `lapidary up` writes the compose file, pulls pinned
  digests, waits on healthchecks, opens the browser.
- **C. Air-gapped** — `podman load` from an exported image bundle, Quadlet units for
  systemd, offline Ed25519 licence file.

## Crate graph

Feature isolation enforced by the Cargo dependency graph and a CI check, not by network
boundaries. Modular monolith.

```
crates/
├── lapidary-core/        L0  domain types, errors, ts-rs exports. Depends on nothing.
├── lapidary-db/          L1  sqlx, migrations, repository traits. ALL SQL lives here.
├── lapidary-storage/     L1  blob CAS, object_store, zstd, tiering, quarantine
├── lapidary-cad/         L2  mesh parsers (stl, obj, 3mf), measurement, the two export writers
├── lapidary-jobs/        L2  Postgres queue, leases, heartbeats, SSE progress
├── lapidary-index/       L2  metadata extraction, tsvector + trigram search, facets
├── lapidary-build/       L2  build graph, runs, ready-set, guide linearization
├── lapidary-targets/     L2  Target trait, format negotiation, export bundles
├── lapidary-api/         L3          axum Router: the open path (grid, health) and the
│                                     download. Depends on the L2 crates it uses — never
│                                     lapidary-cad, and never lapidary-storage's
│                                     SourceStore. It may name SourceReader, the
│                                     read-only source handle, in download.rs and nowhere
│                                     else; `cargo xtask check-deploy` enforces both
│                                     halves. A LIBRARY.
└── lapidary-ingest/      L3          axum Router: the worker-only scan route. The one
│                                     crate allowed to depend on lapidary-cad and to hold
│                                     a SourceStore — see its module doc for why this is a
│                                     separate crate from lapidary-api rather than a role
│                                     check inside it. It is also where a STEP or IGES file
│                                     is refused by name, since `is_model_file` still admits
│                                     one so that a scan reports it rather than skipping it
│                                     in silence. A LIBRARY.
bin/
└── lapidary-server/          container entrypoint: the API, the job worker and the browser app
web/                          React SPA
deploy/                       Containerfile, compose, quadlet, install.sh, install.ps1
```

**Layering rule, CI-enforced:** L2 crates may depend on L0 and L1 and never on each other
or on L3. If two L2 crates need to share something, it moves to `lapidary-core`.
`lapidary-enterprise` sits in a wrapper tier, `Enterprise`, above L3: it may depend on
`lapidary-api`, since enterprise wraps auth, RBAC and audit around the API, but L3 crates
— `lapidary-api` included — may never depend on `Enterprise`. That edge is forbidden
structurally, not just by review, because it would make the free application depend on
the enterprise crate, breaking the rule that the app is free and complete with no gated
features. This is what keeps the monolith from congealing. `lapidary-api -> lapidary-cad`
is forbidden the same way, and every workspace member must carry `publish = false`; both
are named-pair and per-crate checks in `FORBIDDEN_PAIRS` and `check_publish`
(`xtask/src/layers.rs`) rather than restated here.

**Why ingest is its own crate, not a role inside `lapidary-api`.** An earlier pass on
this branch put the scan handler in `lapidary-api` behind a runtime `Role::Worker` check,
reasoning that the open path never *mounting* the route was enough. It is not: the `api`
and `worker` container images are also split at build time by `deploy/Containerfile`'s
`SERVER_FEATURES` arg, specifically so the `api` image never *links* the parsers — and
`lapidary-api` depending on `lapidary-cad` at all, even for a route `Role::Api` never
mounts, makes the `api` image link it regardless. `cargo tree -p lapidary-server -e
normal | grep lapidary-cad` is what caught it. `lapidary-ingest` exists so
`FORBIDDEN_PAIRS` can forbid `lapidary-api -> lapidary-cad` unconditionally again, with
the actual file-reading code sitting in a crate that may depend on it freely.
`bin/lapidary-server` links `lapidary-ingest` behind its `worker` Cargo feature — which
gates `lapidary-cad`, `lapidary-ingest` and `lapidary-jobs` together — and merges its
router into the process's router only when running as the worker.

## No kernel at all (owner, 2026-10-03)

**There is no CAD kernel, no `Kernel` trait and no sidecar.** This section used to print
the trait's signature, a seven-field `KernelOutput`, `OcctKernel` and `occt-bridge
convert`; `docs/ROADMAP.md` § "The pivot" carries the shas where each of them left. What
replaced all of it is two free functions in `lapidary-cad`:

```rust
pub fn parse(bytes: &[u8], format: &str) -> Result<Mesh, CadError>;
pub fn measure(mesh: &Mesh) -> (MeshMeasurements, Option<[f64; 3]>);
```

Bytes, not a path, for the reason that always held: ingest has already hashed them, and
anything that re-opened the path could read something different from what was recorded.
`lapidary-ingest`'s handler calls the two in order and writes the figures; it produces no
derivative at all. A parser's verdict on immutable bytes is always `Permanent` — there is
nothing left that could be a timeout, a crash or an unavailable process, so `CadError` is
four variants and `classify_cad` has no `Transient` arm.

**A STEP or IGES file is refused by name**, in `lapidary_core::cad_refusal`, from four
paths with one sentence: a scan's per-file job, an upload's probe and commit, and the
folder agent's manifest. It is never skipped in silence, which is why `CAD_FORMATS` is
still chained into `is_model_file`.

**`derivative.kernel_version` is now a cache key for the two export writers**, and it is
frozen at `mesh {format}-1+glb-3+cpu-1` with the dead `glb-3` and `cpu-1` tokens inlined
(R5). The column's contract is "the bytes this build would write could differ", nothing in
the pivot changed `write_stl` or `write_3mf` by a byte, and
`PgJobs::enqueue_stale_derivatives` compares the string against every stored export row —
so bumping it would queue a byte-identical rebuild of every export in the corpus at the
next worker start. The reason is in the function's own doc; read it before touching it.

## Tech stack

### Backend — Rust

| Concern | Choice | Note |
|---|---|---|
| HTTP | `axum` (as a library), `tower-http`, `hyper` | Router built by `lapidary-api` |
| Async | `tokio` | |
| DB | `sqlx` + **PostgreSQL 18.6** | official `postgres:18` image; no SQLite |
| Object storage | `object_store` | local FS, S3, R2. Must expose range reads |
| Hashing | **BLAKE3** | not SHA-256; ingest is hash-bound |
| Compression | `zstd` with trained dictionaries | see `docs/DATA.md` §1.2 |
| TS types | **`ts-rs`** | one source of truth. **No gRPC** — see below |
| Streaming | SSE (`axum::response::sse`) | identical in every distribution |
| Jobs | Postgres `FOR UPDATE SKIP LOCKED` + `LISTEN/NOTIFY` | no Redis, no broker |
| Archives | `async-zip` | streaming bundles, STORE not DEFLATE |
| Graphs | `petgraph` | cycle detection before write |
| Config | `figment` | `LAPIDARY_*` env prefix |
| Errors | `thiserror` / `anyhow` | |
| Tracing | `tracing` | |
| IDs | uuid v7 | time-ordered |

### Frontend

| Concern | Choice |
|---|---|
| Framework | React + Vite + TypeScript. **No meta-framework** |
| Routing | TanStack Router — typed search params carry filter state |
| Data | TanStack Query |
| Styling | Tailwind v4 (`@tailwindcss/vite`, CSS-first) |
| Components | shadcn/ui (new-york) — copy-in source, not a dependency |
| 3D | three.js, loading the part's **original** mesh on a **view** press — `STLLoader`, `OBJLoader`, `ThreeMFLoader` (`web/src/components/mesh.ts`). No glTF, no meshopt: nothing produces either |
| Grid | `@tanstack/react-virtual` |
| Node editor | `@xyflow/react` (React Flow) |
| Layout | Hand-rolled, on a 12-column CSS grid (`web/src/components/dashboard/layout.ts`, pure and unit-tested). Not `react-grid-layout`: its CSS transitions fight the motion rules, and the keyboard move/resize WCAG needs is code we would write anyway (G5, 2026-09-27) |
| Motion | CSS transitions for everything a single element does, and **anime.js (pinned)** for sequenced or staggered motion — imported by `web/src/lib/motion.ts` and nowhere else. `CLAUDE.md` § Style holds the durations, the curve and the reduced-motion rule |

### Why not gRPC

Considered and rejected for the browser path.

- `ts-rs` already gives typed contracts from one source of truth. `.proto` would be a
  third schema alongside SQL and Rust.
- Blobs must stay on plain HTTP for range requests and `Cache-Control: immutable`, so
  we would run both protocols anyway.
- `protoc` becomes a build dependency for every contributor.
- proto3 field presence blurs "volume is 0.0" and "volume was never computed", which are
  genuinely different states here.
- Tonic is in the CNCF gRPC project and not accepting significant new features.

**Revisit only** when building the second native plugin (Rhino `.rhp`, Blender addon),
and then mount it on a separate port alongside axum. Add `utoipa` → OpenAPI first if a
public API contract is needed — it generates C#/Python clients without protoc.

## Supply chain

There is no official embedded PostgreSQL, which is why we abandoned that path. Container
mode uses the official `postgres:18` Docker Official Image and the problem disappears.

Policy, all CI-enforced:

- `cargo-deny` with a **`[sources]` allow-list**, not just advisories and licences.
  Without it a transitive dependency can silently come from an arbitrary git URL.
- `Cargo.lock` and the npm lockfile committed.
- GitHub Actions pinned to commit SHAs, not tags. Tags are mutable and this is actively
  exploited.
- `cargo vendor` for release builds — a regulated customer will ask whether you can build
  without internet access. The answer must be demonstrably yes.
- SBOM generated per release: CycloneDX for the crate graph, `syft` for images.
- Sign everything: cosign for images, and Authenticode + Apple notarization once the
  Tauri app ships.
- **No bundled C or C++ anywhere in the tree.** OCCT was built from source in our own CI
  and `meshopt`'s build script compiled bundled C++; both left with the pivot, and nothing
  in the workspace compiles a native library today. A new dependency that does is a
  supply-chain decision, not a convenience.

## Licensing — decided: AGPL-3.0-only

The plan was AGPL-3.0-only app plus a proprietary `lapidary-enterprise`. **These
conflict.** A proprietary crate linking AGPL crates must itself be AGPL, and DCO-signed
community contributions cannot be relicensed by us.

Two coherent options were considered:

1. **Everything AGPL, sell the licence contractually.** The Ed25519 file gates fleet size
   and support entitlement as a contractual and support boundary, not technical DRM.
   AGPL already blocks a competitor from hosting without publishing changes. No CLA
   fight, no relicensing problem. **Recommended, and decided.**
2. CLA instead of DCO, keeping copyright assignment so `lapidary-enterprise` could stay
   proprietary — considered and rejected for the goodwill and contributor friction it
   would cost.

Option 1 stands: the entire workspace, including `lapidary-enterprise`, is AGPL-3.0-only.
Contributions are taken under the DCO.
