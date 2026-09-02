# Architecture

## Distribution model

**One codebase, three artifacts.** All of them are thin shells over `lapidary-api`,
which builds an axum `Router`. Never fork.

| Artifact | What it is | Ships when |
|---|---|---|
| **Container stack** | `api`, `worker`, `web`, `db` via Compose Spec | Phase 1 (primary) |
| **`lapidary` binary** | Single Rust executable: `agent`, `worker`, `up` | Phase 4 |
| **Tauri desktop app** | Webview + agent, connects to a server | Phase 7 |

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
              │  named volume      │  │ OCCT native sidecar │
              └────────────────────┘  └────────────────────┘
                         ▲
                         │ leases over HTTP, outbound only
              ┌──────────┴──────────────────────────────────┐
              │  remote workers (lapidary worker)           │
              │  registered in Settings → Compute           │
              └─────────────────────────────────────────────┘
```

**Single origin is mandatory.** The `web` container reverse-proxies `/api` and `/events`
to `api`. No CORS, no cookie-domain problems, no preflight per request.

**SSE through the proxy needs `proxy_buffering off`** (nginx) or the Caddy equivalent,
plus `X-Accel-Buffering: no` on the response. Default buffering holds progress events
until the buffer fills and ingest appears frozen. This is the single most common
"works in dev, breaks in prod" bug in this stack.

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
├── lapidary-cad/         L2  Kernel trait + OCCT sidecar driver
├── lapidary-jobs/        L2  Postgres queue, leases, heartbeats, SSE progress
├── lapidary-index/       L2  metadata extraction, tsvector + trigram search, facets
├── lapidary-vcs/         L2  revisions, lineage DAG, locks, geometric diff
├── lapidary-build/       L2  build graph, runs, ready-set, guide linearization
├── lapidary-targets/     L2  Target trait, format negotiation, export bundles
├── lapidary-api/         L3          axum Router. Depends on the L2 crates it uses,
│                                     lapidary-cad included since Task 9's worker-only
│                                     scan handler; the open path (the routes mounted
│                                     under Role::Api) still never invokes the kernel — a
│                                     runtime role split, not a dependency-graph ban,
│                                     enforces that now. A LIBRARY.
└── lapidary-enterprise/  Enterprise  licence verify, auth, RBAC, audit, worker fleet
bin/
├── lapidary-server/          container entrypoint: api + optionally in-process worker
└── lapidary/                 desktop binary: agent | worker | up
sidecar/occt-bridge/          C++ OCCT → {tessellation.glb, structure.json, entities.json}
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
features. This is what keeps the monolith from congealing. That edge, and every
workspace member's `publish = false`, are checks in `edge_allowed` and `check_publish`
(`xtask/src/layers.rs`) rather than restated here.

`lapidary-api -> lapidary-cad` used to be a second named-pair prohibition in
`FORBIDDEN_PAIRS`, on the reasoning that the open path (opening a part for viewing) lives
in `lapidary-api` and must never invoke the CAD kernel. It no longer is: the role split
(`Role::Api` / `Role::Worker`, `crates/lapidary-api/src/lib.rs`) put the open-path grid
route and the worker-only ingest scan route in one crate, and the scan handler
(`crates/lapidary-api/src/scan.rs`) genuinely needs `lapidary-cad::MeshKernel` — a
crate-level ban can no longer express a rule about *which route*, only about the whole
crate. The product rule is unchanged (`docs/DATA.md` §2); what changed is the mechanism:
`Role::Api` simply never mounts `scan::scan`, and `xtask/src/deploy.rs`'s
`check_open_path_boundary` keeps `SourceStore` — the type that actually reaches a source
file — out of every file in the crate except `scan.rs`.

## The kernel simplification

Container-first removes the WASM kernel variant entirely. OCCT always runs native in the
worker container. Keep the `Kernel` trait for test doubles, but ship **one**
implementation. This deletes the highest-risk item in the original plan.

```rust
trait Kernel {
    fn version(&self) -> KernelVersion;   // pinned across the fleet — see below
    async fn process(&self, src: &Path, params: &KernelParams) -> Result<KernelOutput>;
}
// KernelOutput = { tessellation_l0/l1/l2.glb, structure.json, entities.json }
```

**Pin the kernel version across the whole worker fleet.** Different OCCT builds produce
different tessellations from identical input. `derivative.kernel_version` records it and
geometric diff depends on determinism. The coordinator must reject leases from workers
whose kernel hash does not match the pool's pinned build, or two revisions of an
unchanged part will show a phantom volume delta depending on which machine processed it.

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
| Watching | `notify` | agent binary only |
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
| 3D | three.js + glTF with `EXT_meshopt_compression` |
| Grid | `@tanstack/react-virtual` |
| Node editor | `@xyflow/react` (React Flow) |
| Layout | `react-grid-layout` for the dashboard |
| Motion | CSS transitions + Web Animations API. **No anime.js** |

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
- OCCT built from source in our own CI. LGPL-2.1 output stays a separately replaceable
  file.

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
