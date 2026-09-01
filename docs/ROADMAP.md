# Roadmap

Each phase has a hard exit criterion. Do not start the next phase until it passes.
Phases 1–4 are the product; everything after is expansion.

---

## Phase 0 — Foundation (days, not weekends)

Container-first removed the OCCT bundling gate. This is no longer a go/no-go risk.

- Cargo workspace with the crate graph from `docs/ARCHITECTURE.md`
- CI layering check: L2 crates may not depend on each other or on L3
- `Containerfile` builds OCCT from source, produces `occt-bridge`
- Compose stack up: `web`, `api`, `worker`, `db` (`postgres:18`)
- `cargo-deny` with `[sources]` allow-list, lockfiles committed, Actions pinned to SHAs
- `ts-rs` export pipeline wired into the frontend build

**Exit:** `podman compose up` on a clean machine serves a page, and `occt-bridge` converts
a 200-part STEP assembly to glTF + tree + entities in under 30 s.

**Verify here, not later:** `pgvector` installs against `postgres:18`; the Turkish
snowball `tsvector` config is present.

---

## Phase 1 — Ingest and grid

- Blob CAS: BLAKE3, 2-level sharding, `ref_count`, zstd -3 on source
- Postgres job queue: `FOR UPDATE SKIP LOCKED` + `LISTEN/NOTIFY`, crash-resumable
- Upload: client-side WASM BLAKE3 → probe → chunked resumable transfer
- Mesh ingest (STL/3MF/OBJ) → thumbnail + L0/L1/L2
- Virtualized grid, keyset pagination, inline `bytea` thumbnails
- SSE progress; UI never blocks
- Download `variant=original` with hash displayed
- First run seeds a bundled licence-clean example part — never an empty grid

**Exit:** drop a folder of 1,000 STLs, grid is interactive immediately, all thumbnails
land, re-dropping the same folder completes in seconds via hash short-circuit, and grid
page load is under 80 ms warm.

---

## Phase 2 — CAD ingest and search

- `lapidary-cad` drives the sidecar with timeout and crash handling
- STEP + IGES; assembly tree persisted and navigable
- Metadata extraction stages 1–4, each committing independently
- `tsvector` + `pg_trgm` dual search with identifier-aware ranking
- Faceted filters with the 10k exact-count threshold
- Failed-file drawer with actionable errors

**Exit:** ingest a mixed folder of STEP and STL with no manual steps; searching a part
number like `A1234-56-B` by the fragment `1234` returns it at position one.

---

## Phase 3 — Viewer and measurement

- three.js + glTF/meshopt, LOD streaming, immutable blob caching
- Prefetch L0 on hover, L1 on inspector open, pool bounded at 2
- Measurement: point-to-point, edge, diameter, angle, wall thickness
- Snap to analytic entities; mesh values labelled approximate
- Derivative downloads with `.lapidary.` infix

**Exit:** measure a known cylinder from STEP and get the exact nominal diameter, not a
tessellated approximation. Part open to first paint under 120 ms warm.

---

## Phase 4 — Versioning, agent, round-trip

**This phase is the differentiator. Everything before it is a file browser.**

- Immutable revisions, lineage DAG, `origin` tracking, pessimistic locks
- Geometric diff + visual overlay; version history strip in the inspector
- `Target` trait with automatic format negotiation
- **`lapidary` binary ships** — `agent`, `worker`, `up` subcommands
- `lapidary://` scheme, checkout to workspace, launch external tool
- Native watcher with debounce, write-settle, hash-before-believing, Windows buffer
  overflow rescan, macOS file-level FSEvents
- Storage tiering job, quarantine, three-step deletion

**Exit:** open a STEP from Lapidary in FreeCAD, change it, save, and a new revision
appears automatically with a correct volume delta — on Linux, macOS and Windows.

---

## Phase 5 — Source links, bundles, collections

- `part_source` + `part_image` with the full SSRF control set
- OpenGraph preview fetch behind an explicit button
- Streaming ZIP bundles with `manifest.json`
- Saved filters, custom fields, section plane, PMI display
- Turkish search config

**Exit:** paste a Printables URL, get title, licence and cached image; export a 40-part
assembly as a bundle another user can import with full lineage intact.

---

## Phase 6 — Dashboard and similarity

- Widget registry, drag-resize layout, named groups
- Single batched `/api/dashboard/resolve` with per-key timeouts and partial results
- Live patches over the existing SSE stream
- Geometry embeddings + pgvector; near-duplicate clustering with merge/link-as-variant

**Exit:** a 12-widget dashboard settles in one round trip; uploading a known part surfaces
its near-duplicates.

---

## Phase 7 — Build graph

Comparable in size to everything before it. **Do not start early.** A half-working
planner attached to a good browser makes the whole app feel unfinished.

- Process types with JSON Schema params; ~15 builtins + user-defined
- Board on `@xyflow/react`, auto-layout, cycle rejection at write
- Graph versioning; runs separate from graphs; quantity multiplication
- Ready-set with critical path + batch affinity
- Guide view: queue and sequential modes, mobile-shaped, photo capture
- Auto-generated draft steps; PDF export

**Exit:** model a three-level assembly where sub-parts are printed and moulded, start a
run of 4 units, and the queue correctly answers what to make next with correct quantities
— then follow it to completion on a phone.

---

## Phase 8 — Enterprise and fleet

- Auth, RBAC, audit log, lifecycle states and approvals
- Settings → Compute: enrolment tokens, worker registry, drain, revoke
- Lease protocol with heartbeats; kernel-version pinning enforced
- Ed25519 offline licence with `max_workers` and grace-period expiry
- Air-gapped image bundle + Quadlet units
- Export-everything bundle
- Subgraph references; Hausdorff diff

**Exit:** register three remote workers on separate machines, process a 500-part library
across the fleet, revoke one mid-run and watch its leases expire and requeue cleanly.

---

## Phase 9 — Cloud

Only after the local product has users.

- Zero-egress object storage (R2 or B2 behind Cloudflare) — **day one, not an
  optimization**
- Per-tenant dedup only; never cross-tenant
- Egress instrumented per tenant from the first beta day
- Share links, remote viewer access, lock authority
- Lapsed subscription → read-only with full download, never deletion

---

## Phase 10 — Tauri shell

- Three connection modes: remote server, managed local stack, embedded (never built)
- Bundles **only our own binaries**. If you are tempted to bundle Postgres or OCCT, stop
- Authenticode + Apple notarization

---

# Commercial model

## Shape

| Tier | What | Price mechanism |
|---|---|---|
| **Local** | Everything, unlimited, single machine | Free, AGPL |
| **Team** | Self-hosted server, worker fleet up to N | Annual, MoR checkout |
| **Enterprise** | Large/unlimited fleet, air-gapped, support SLA | Invoice + wire, perpetual + maintenance option |
| **Cloud** | Hosted, per-GB | Subscription |

## Why the worker fleet is the right gate

Distributed compute is **inherently multi-machine**. A hobbyist on one laptop loses
nothing. A shop with eight workstations gets real value. Near-perfect price/value
alignment with zero crippling of the free app.

**Meter per worker node, not per seat.** Seats are annoying to administer, easy to fudge
and resented. Worker count is a hard number the coordinator already knows, scales with
delivered value, and drops into `max_workers` in an offline signed licence.

**Grace period, never a hard stop.** An industrial customer whose line halts over a
lapsed licence will not renew, and will tell people.

## Two payment rails, both required

- **MoR** (Paddle, Polar, Stripe Managed Payments) for cloud and small self-hosted card
  purchases. Note Stripe acquired Lemon Squeezy and is folding it into Stripe Managed
  Payments. Confirm current Turkey payout support directly with each provider — this
  changes often.
- **Invoice + purchase order + e-fatura + wire.** A €5,000 self-hosted licence does not
  go through a checkout page. This is where the highest-value customers live and it is
  the rail solo developers forget to build.

## Structural lever

Erciyes Teknopark is in Kayseri. Under law 4691, earnings derived exclusively from
software, design and R&D activity within a technology development zone are exempt from
income and corporate tax until 31/12/2028, with a matching VAT exemption on qualifying
software deliveries plus income tax withholding relief and SGK employer-share support on
R&D personnel.

Conditions are strict: `münhasıran` (only in-zone activity qualifies), management-company
approval and registration are prerequisites, IP-derived income is subject to a qualified-
expenditure ratio, and above 1M TL of exempt earnings 2% must go to venture capital funds
or forfeit 20% of the exemption.

Combined with the software export deduction on foreign-currency invoices, this is
plausibly worth more in year one than any pricing decision. **Take it to a mali müşavir
before incorporating** — some of it depends on day-one entity structure.

---

# Open items

- **Licensing conflict.** AGPL app + proprietary enterprise crate cannot coexist. Decide
  before the first external contribution. See `docs/ARCHITECTURE.md`.
- **Trademark.** "Lapidary" is a common English word — check TÜRKPATENT and EUIPO in the
  relevant software classes before registering a domain.
- **`pgvector` and Turkish `tsvector`** against `postgres:18` — verify in Phase 0.
- **zstd dictionary gain** — measure on a real STEP corpus before committing to
  per-library dictionaries.
- **MoR Turkey payout support** — confirm directly, do not rely on this document.
- **WebKitGTK variance** — test the Tauri shell on at least two Linux distros.
