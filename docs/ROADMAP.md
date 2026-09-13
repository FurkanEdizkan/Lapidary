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

**Measured 2026-09-13, both clauses pass.** Phase 0 was cut in two — 0a without OCCT, 0b with
it — and the halves passed eleven days apart.

| Clause | Measured | Verdict |
|---|---|---|
| `podman compose up` on a clean machine serves a page | Phase 0a, 8 of 8 exit criteria from a clean clone — `superpowers/plans/2026-09-01-phase-0a-verification.md` | pass |
| `occt-bridge` converts a 200-part STEP assembly to glTF + tree + entities in under 30 s | **111 ms**: `fixtures/step/fixture-plate-assembly-lp-9000-00.step` (190 KB, 200 placed parts, 8 prototypes) to an L0 GLB rung of 53.7 KB, the assembly tree, 75 entities and a thumbnail — 28,576 triangles | pass |

How it was measured: `cargo xtask verify occt`, which builds the `occt-test` stage of
`deploy/Containerfile` — OCCT 8.0.1 built from source, `occt-bridge`, and a release build of
`OcctKernel` — and runs `crates/lapidary-cad/tests/occt_bridge.rs` there. The timing runs from
the input bytes to the finished `KernelOutput`: the bridge process, its B-rep read, meshing and
JSON, then the mesh pipeline's clustering and GLB writer and the thumbnail. Kernel version
`occt-8.0.1-bridge-1+deflection-0.1+glb-1+cpu-1`, on the 12-core development machine.

**What the number does not say.** The fixture is generated, so it is licence-clean and
reproducible, and its parts are boxes, cylinders and booleans of them. A real assembly's
freeform and trimmed surfaces take OCCT far longer to read, heal and mesh, so 111 ms is where
the kernel starts on the simplest honest input, not what a user's file will take. The criterion
passes as written; the first corpus of real STEP files is the measurement worth taking next.
Same run, same stage: the 22 mm cylinder reads with its volume exact to 1e-9 and one
cylindrical face of radius 11; the same cylinder written in inches reads back in millimetres;
IGES reads with no volume claimed; half a STEP file is refused rather than crashing the kernel.

**Verify here, not later:** `pgvector` installs against `postgres:18`; the Turkish
snowball `tsvector` config is present.

---

## Phase 1 — Ingest and grid

- Blob CAS: BLAKE3, 2-level sharding, `ref_count`, zstd -3 on source
- Postgres job queue: `FOR UPDATE SKIP LOCKED` + `LISTEN/NOTIFY`, crash-resumable
- Upload: client-side WASM BLAKE3 → probe → chunked resumable transfer
- Mesh ingest (STL/3MF/OBJ) → thumbnail (per-library, on by default) + L0
<!--
  This line read "+ L0; L1/L2 on demand" until 2026-09-13, and the "on demand" half was
  never Phase 1's. Slice 4 moved it: ingest stores L0 only, and the route that would
  enqueue L1/L2 is assigned to **Phase 3's viewer** — see
  `superpowers/plans/2026-09-05-phase-1-slice-4-HANDOFF.md:255` ("the three trigger routes
  only render thumbnails, which is all any consumer needs today") and
  `superpowers/specs/2026-09-04-phase-1-slice-4-derivatives-design.md:46`. The amendment
  was recorded in `2026-09-05-phase-1-remaining-slices.md:103` and never reached this file,
  so a Phase 1 audit read it as an unbuilt row and nearly built it.

  The kernel produces all three rungs today (`lapidary-cad`); nothing asks it for the top
  two, which is why they are not written. Phase 3's prefetch line below is the consumer.
-->
- Virtualized grid, keyset pagination, inline `bytea` thumbnails
- SSE progress; UI never blocks
- Download `variant=original` with hash displayed
- First run seeds a bundled licence-clean example part — never an empty grid

**Exit:** drop a folder of 1,000 STLs, grid is interactive immediately, every part
appears — with a thumbnail where the library renders them automatically, and with "No
preview yet" plus a working `POST /api/libraries/{id}/thumbnails` where it does not —
re-dropping the same folder completes in seconds via hash short-circuit, and grid page
load is under 80 ms warm.

**Measured 2026-09-07. Three clauses passed as measured; the fourth failed, was fixed, and
passes on re-run.**

Corpus: `Bases/` from the owner's own library — **1,095 STL files, 15.67 GB**, nested two
to six directories deep, no basename collisions. A real folder rather than a generated one.
Ingested into a library of its own on the development machine (12 cores, 15 GB RAM,
PostgreSQL 18.6 in a container, worker concurrency 4).

| Clause | Measured | Verdict |
|---|---|---|
| A folder of 1,000 STLs in | **130.3 s** for 1,095 files / 15.67 GB — 8.4 files/s, ~120 MB/s | pass |
| Re-drop completes in seconds via hash short-circuit | **40.1 s**, 1,094 skipped, **0 re-ingested** | pass |
| Grid page load under 80 ms warm | **5.3 ms** median at page size 50 (p95 5.9 ms, 2.4 MB body). At 500: **54.7 ms** median, 28.8 MB body | pass |
| Every part appears | **1,094 of 1,095** at measurement; **1,095 of 1,095** after the fix below | pass, after a fix |

**The one that failed, and what it turned out to be.** `Trench battlefield-80mm(B).stl` is
635,470 triangles that parse cleanly and cluster to nothing. `MeshKernel::process` produced
each requested derivative with `?`, so a derivative that could not be made came back
indistinguishable from "this file is not readable" — the ingest job failed, **no part row
was written**, and the model was absent from the library. The criterion asks for the
opposite in as many words, and the machinery for it already existed: `auto_thumbnail =
false` leaves a part with no thumbnail and the grid renders "No preview yet". Only a
*failed* derivative was fatal where a *skipped* one was not.

Fixed in `490ba34`. Parsing stays fatal — a mesh nobody can read has no measurements and no
part to hang them on — and a derivative that cannot be made is recorded in
`KernelOutput::unproduced` with its kind and reason, which the handler logs at `warn`.
Re-run against the same file afterwards: **ingested, one part, 635,470 triangles.**

And the measurement corrected its own diagnosis. `CadError::Unrenderable` is raised by the
thumbnail rasterizer *and* the glTF writer, and its message said "Could not render a
thumbnail" for both — so this was written up as a thumbnail failure. It was the **L0
tessellation rung**. The re-run's log names `tessellation_l0`, and the part carries a
perfectly good thumbnail. The wording is about a view of the mesh now, and which view is
recorded where the caller asked for it.

Timing note: the two figures above measure the **scan** path — the worker walking a mounted
directory — rather than a browser drop, which hashes client-side and uploads. Both reach the
same ingest pipeline after the bytes are in reach; the drop adds transfer and client-side
BLAKE3 that this run did not measure. And "interactive immediately" was observed rather than
timed: the grid answered throughout the ingest, which is what the job queue is for, but no
number was taken for it.

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

**Measured 2026-09-13, both clauses pass. The phase is not finished:** the only facet is
format — material, tags and lifecycle wait for their columns — and stage 4 does not read PMI or
GD&T yet. The failed files are listed inline under the progress line rather than in a drawer:
every failure, a page past the first hundred at a press, with a Retry per file and for the batch. Sorting by a measured figure shipped without an
index, for the reason `DATA.md` §3.2 records.

| Clause | Measured | Verdict |
|---|---|---|
| Ingest a mixed folder of STEP and STL with no manual steps | One scan of a folder holding `cad/` (three STEP files and one IGES, from `fixtures/step`) and `mesh/` (three STLs): **7 of 7 ingested, 0 failed, in 0.62 s** from the scan request to a finished batch. Each CAD part has its L0 rung, thumbnail, assembly tree, entities and header. The 200-part assembly's tree reads back with 200 parts and 8 prototypes, served as `application/json`. The 22 mm cylinder's volume (11,403.98 mm³), area and box are stored as exact | pass |
| Searching `A1234-56-B` by the fragment `1234` returns it at position one | Through the API: `PUT /api/parts/{id}/part-number` gave the cylinder the number `A1234-56-B`, and `GET /api/libraries/{id}/parts?q=1234` returned it first in 2.6 ms, ahead of `bracket-1234-mount.stl`, a part only named for the digits | pass |

How it was measured: `docker compose -p lapidaryphase2` built the `api` and `worker` targets of
`deploy/Containerfile` from commit `11d5f07`, with its own env file, storage and ingest
directories and database volume. `curl` drove the scan, the batch status, the detail and blob
routes and the search, and `psql` read the stored headers and derivative kinds. Worker
concurrency 2, kernel `occt-8.0.1-bridge-2+deflection-0.1+glb-1+cpu-1`, on the 12-core
development machine.

**Where the part number came from.** Nothing reads a part number out of a file. Ingest writes
none, because one made up from a filename is one nobody gave the part, and the fixtures' STEP
headers carry only OCCT's defaults. The number in the second clause was set by a request, which
is the only way one enters a library. The ranking is held in general by
`a_part_number_fragment_returns_the_part_at_position_one` in `crates/lapidary-db/tests/repo.rs`,
and through the router by `a_part_number_set_through_the_api_is_found_first_by_its_fragment`.

**What the numbers do not say.** The CAD files are small generated fixtures, so 0.62 s is the
pipeline's own overhead on honest input, not what a real assembly takes. The Phase 0 entry above
says the same of the kernel.

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
- Storage tiering job, and the instance-wide storage view that can report quarantined
  bytes (a purged blob belongs to no library, so no library's panel can count it)
  <!-- Three-step deletion and quarantine landed early, in Phase 1 slice 7, on the
       owner's priority call: a library you can add to and never remove from was the
       more pressing gap. What stayed here is tiering, because tiering is what makes
       quarantine's physical layout matter, and building a layout before the job that
       uses it is building for a caller that does not exist. -->

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

- **Trademark.** "Lapidary" is a common English word — check TÜRKPATENT and EUIPO in the
  relevant software classes before registering a domain.
- **`pgvector` and Turkish `tsvector`** against `postgres:18` — verify in Phase 0.
- **zstd dictionary gain** — measure on a real STEP corpus before committing to
  per-library dictionaries.
- **MoR Turkey payout support** — confirm directly, do not rely on this document.
- **WebKitGTK variance** — test the Tauri shell on at least two Linux distros.
