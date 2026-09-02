# Phase 1, slice 1 — local ingest to a visible grid

**Date:** 2026-09-02
**Status:** design approved, not yet planned
**Phase:** 1 (Ingest and grid), first of five slices
**Predecessor:** Phase 0a, complete — see `docs/superpowers/plans/2026-09-01-phase-0a-verification.md`

---

## 1. Why this slice exists

Phase 0a proved the topology and built nothing else. As of `3c6a0ea` the running app has
**one API route** (`/api/healthz`), **zero tables** (the only migration creates the
`pg_trgm` extension), no implementation of `PartRepository`, and six L2 crates that are
17–18 lines of doc comment each. A user who opens it sees "No parts yet" and has no way
to change that.

This slice makes one sentence true: **a mesh file on disk becomes a card with a thumbnail
in the grid.**

### Sequencing

Phase 1 as roadmapped is five independent subsystems. Building them in dependency order
leaves nothing visible until the third slice, so they are sequenced as a vertical slice
instead — thinnest end-to-end path first, then thickened:

| Slice | Adds | Visible result |
|---|---|---|
| **1 (this)** | schema, blob CAS, mesh parse, thumbnail, grid | **a part on screen** |
| 2 | job queue, worker leasing (ingest becomes async) | progress survives a crash |
| 3 | LOD ladder, 3MF/OBJ | viewer-ready derivatives |
| 4 | browser upload: probe, resumable chunks; download `variant=original` | drag-and-drop |
| 5 | SSE progress, virtualized grid, seed part | the roadmap's 1,000-STL exit |

The cost of this order is that ingest is written synchronously in slice 1 and moved behind
the queue in slice 2. That is accepted deliberately: the move is a trigger change, not a
rewrite, because slice 1 puts the scan route on the worker from the start (§4.1).

---

## 2. Scope

**In:**

- Schema for `library`, `part`, `revision`, `file`, `blob`, `derivative`
- Blob CAS: BLAKE3, two-level hex sharding, `ref_count`, zstd -3 on source
- A read-only mounted ingest directory and a scan endpoint that walks it
- Binary and ASCII STL parsing → bounding box, triangle count, volume, watertightness
- CPU-rasterized 512 px WebP thumbnail, stored inline as `bytea`
- Keyset-paginated grid endpoint and a grid that renders real cards
- Minimal role split (`LAPIDARY_ROLE`) so ingest routes never mount in the `api` process
- The `lapidary-storage` source/derivative handle split
- A default library, seeded by migration, so the scan endpoint has an id to address (§6.1)

**Out, with the slice that covers each:**

| Deferred | Slice |
|---|---|
| Job queue, worker leasing, crash resumption | 2 |
| LOD ladder (`L0`/`L1`/`L2`), meshopt encoding | 3 |
| 3MF, OBJ | 3 |
| Browser upload, probe endpoint, resumable chunks | 4 |
| `GET …?variant=original` download | 4 |
| SSE progress | 5 |
| Virtualized grid, bundled seed part | 5 |
| STEP/IGES, the OCCT sidecar | Phase 2 |
| Per-library zstd dictionaries (`DATA.md` §1.2) | later; `dict_id` column exists, stays NULL |
| Search, facets (`tsvector` column exists, unqueried) | Phase 2 |
| Soft delete / purge / quarantine mechanics | later; columns exist, unused |

---

## 3. Decisions

Four were put to the owner. The rest follow from `docs/DATA.md`, which already settles
most of Phase 1's technical choices and is treated here as binding.

### 3.1 Thumbnails are rendered on the CPU

Parse, project, shade with a fixed headlight, rasterize — pure Rust, no GPU.

The deciding argument is `DATA.md`'s own classification: derivatives are re-derivable
**"deterministically"**, guaranteed by `kernel_version` + `params_json`. A GPU renderer
makes output driver- and version-dependent, so byte-identical regeneration stops holding
and golden-image tests become perceptual-diff tests with a tolerance. Secondary: a Vulkan
stack (lavapipe on GPU-less hosts) is a large dependency and a driver-bug class that is
miserable to support in air-gapped installs, against a project rule that prefers fewer
moving parts.

Accepted cost: flat/matcap shading rather than lit-and-glossy. Adequate for triage, which
is what the grid is for. Revisit only if visual triage measurably suffers.

No prior art exists — `docs/prototype-notes.md` records that the Node prototype left
thumbnails **out of ingest entirely**, generating them through a separate manual route.

### 3.2 Files enter through a read-only mounted directory

`deploy/compose.yaml` mounts a host directory at `/ingest:ro` in the worker.
`POST /api/libraries/{id}/scan` takes **no path argument**; it walks the configured
directory.

Because no caller-supplied path ever reaches the filesystem, traversal is structurally
impossible rather than defended against — the same reasoning `DATA.md` §4.1 applies to
SSRF. The read-only mount means an ingest bug cannot damage the user's library.

### 3.3 Measurement provenance is per-measurement, not per-row

`revision` carries `volume_source` and `bbox_source` (`'tessellated' | 'analytic'`)
beside the values they describe.

Slice 1 writes `'tessellated'` everywhere, because STL has nothing else — so the columns
look redundant now. They are not: in Phase 2 a STEP revision has an analytic volume and a
tessellated triangle count **on the same row**, and any single row-level flag must lie
about one of them. Fixing that later is a migration plus every consumer, after the UI
already renders a badge.

This is also the first real consumer of `Approximate<T>` (`lapidary-core`), which open
follow-up item 6 records as exported and unused.

The prototype made precisely this mistake in the other direction: `routes/api.ts` wrote
user-**typed** dimensions into the same `bbox_x/y/z` columns as sidecar-**measured** ones
with no marker, so nothing downstream could tell them apart.

### 3.4 A minimal role split lands here, not in slice 2

`LAPIDARY_ROLE=api|worker` selects which routes `lapidary_api::router()` mounts.

This is forced by the slice, not chosen: ingest must not run in the `api` process, whose
image deliberately does not link `lapidary-cad` (follow-up items 13 and 15). Since both
containers run one binary with one router today, any route the router mounts is served by
both. A role is the smallest thing that keeps the ingest path out of the open-path binary.

It is the core of open follow-up item 4, without the job-leasing machinery that item also
describes — that arrives in slice 2.

---

## 4. Architecture

### 4.1 Where each piece lives

| Crate | Layer | Gains |
|---|---|---|
| `lapidary-core` | L0 | `MeshMeasurements`, provenance enum; `Approximate<T>` gets its first use |
| `lapidary-db` | L1 | schema migration, `PartRepository` impl, blob and revision repositories |
| `lapidary-storage` | L1 | blob CAS; the source/derivative handle split (§4.3) |
| `lapidary-cad` | L2 | `MeshKernel` — STL parse + thumbnail raster, beside `MockKernel` |
| `lapidary-api` | L3 | scan route (worker role), grid route (api role), role selection |
| `bin/lapidary-server` | bin | reads `LAPIDARY_ROLE`; already links `lapidary-cad` under `mock-kernel` |
| `web` | — | grid page consuming the parts endpoint |

**Mesh work belongs in `lapidary-cad`.** STL parsing and rasterization are geometry, and
placing them behind the kernel boundary preserves the product rule exactly as written:
ingest invokes the kernel, opening a part never does. `MeshKernel` sits beside
`MockKernel` under the existing `Kernel` trait; the worker links the crate, the `api`
image structurally cannot (enforced by `FORBIDDEN_PAIRS` and `check-deploy`).

`KernelOutput` will need fields this slice does not have (`DATA.md` §2.1's LOD paths).
Follow-up item 2 already records that it must change and that `sidecar/occt-bridge/README.md`
no longer claims otherwise. Slice 1 adds what it needs and leaves the shape open.

### 4.2 Routes by role

```
role=api      GET  /api/healthz
              GET  /api/libraries/{id}/parts?after=&limit=

role=worker   GET  /api/healthz
              POST /api/libraries/{id}/scan
```

Both roles share the database. In slice 2 the scan handler enqueues instead of executing;
no route moves and no client changes.

### 4.3 The storage boundary

`lapidary-storage` exposes two handles from the outset:

- **`DerivativeStore`** — read/write derivatives and thumbnails. Held by both roles.
- **`SourceStore`** — read/write source bytes. Obtainable only in the worker role.

This is open follow-up item 16, which records that `lapidary-api` still depends on
`lapidary-storage` and that nothing structural stops an open-path handler reading source
bytes. A dependency-graph rule cannot express it, because the dependency is legitimate for
derivatives; the distinction is *which bytes*, so it must be an API-level boundary.

Building it now costs a type; retrofitting it after handlers exist costs a refactor of
working code. That is the whole argument.

---

## 5. Data flow

```
POST /api/libraries/{id}/scan                                    (worker role only)
│
├─ walk /ingest (read-only), collect *.stl
│
└─ per file:
   ├─ BLAKE3 over the bytes                                      ← hash first, always
   ├─ blob row exists?
   │    yes → link a new file row to the existing blob,
   │          bump ref_count, skip parse and raster               ← short-circuit
   │    no  ↓
   ├─ parse mesh    → bbox, triangle_count, volume, is_watertight
   ├─ rasterize     → 512 px WebP, assert ≤ 64 KB
   ├─ write blob    → zstd -3, blobs/ab/cd/<hash>, ref_count = 1
   └─ one transaction:
        INSERT part → revision (measurements + *_source) → file → derivative(thumbnail)
│
└─ 200 { ingested, skipped, failed: [{ file, reason }] }

GET /api/libraries/{id}/parts?after=&limit=                      (api role only)
└─ one keyset query, thumbnails inline from bytea                ← no per-card round trip
```

Ordering note: the blob is written **before** the transaction, and a failed transaction
reaps it. The prototype's ordering leaked an orphaned blob on exactly this path — a
successful Tier-3 write followed by a failed DB insert, with no cleanup — recorded in
`docs/prototype-notes.md`. A test pins the reaping.

---

## 6. Schema

`DATA.md` §3.2 as written, minus `part_source` and `part_image`, plus §3.3's provenance
columns. `library` is added because §3.2 references `part.library_id` without defining it.

```sql
library(
  id uuid PRIMARY KEY,
  name text NOT NULL,
  mode text NOT NULL DEFAULT 'hobby',      -- hobby | controlled, per LibraryMode
  created_at timestamptz NOT NULL DEFAULT now()
);

part(
  id uuid PRIMARY KEY,                     -- uuid v7, PartId
  library_id uuid NOT NULL REFERENCES library(id),
  part_number text, name text NOT NULL,
  classification text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  created_by uuid,
  deleted_at timestamptz,                  -- soft delete; never hard-deleted here
  metadata_json jsonb NOT NULL DEFAULT '{}',
  search tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(part_number, '')), 'A') ||
    setweight(to_tsvector('simple', name), 'B')
  ) STORED                                 -- STORED is mandatory on PG18
);

revision(
  id uuid PRIMARY KEY, part_id uuid NOT NULL REFERENCES part(id),
  rev_label text NOT NULL, parent_revision_id uuid,
  origin text NOT NULL,                    -- 'ingest' in this slice
  author uuid, message text,
  created_at timestamptz NOT NULL DEFAULT now(),
  lifecycle_state text,
  locked_by uuid, locked_at timestamptz,
  volume double precision, volume_source text,        -- tessellated | analytic
  surface_area double precision, surface_area_source text,
  bbox_x double precision, bbox_y double precision, bbox_z double precision,
  bbox_source text,
  triangle_count integer,                  -- always tessellated; no source column needed
  is_watertight boolean, units text,
  mass_props_json jsonb
);

file(
  id uuid PRIMARY KEY, revision_id uuid NOT NULL REFERENCES revision(id),
  role text NOT NULL, format text NOT NULL,
  blake3 text NOT NULL REFERENCES blob(blake3),
  size_bytes bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

blob(
  blake3 text PRIMARY KEY,
  size_bytes bigint NOT NULL, stored_bytes bigint NOT NULL,
  zstd_level smallint, dict_id uuid,        -- dict_id NULL until dictionaries land
  ref_count integer NOT NULL DEFAULT 0,
  quarantined_at timestamptz,
  last_accessed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

derivative(
  id uuid PRIMARY KEY, revision_id uuid NOT NULL REFERENCES revision(id),
  kind text NOT NULL,                       -- 'thumbnail' in this slice
  blake3 text, thumb_bytes bytea,           -- inline when < 64 KB
  kernel_version text NOT NULL, params_json jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
```

`triangle_count` deliberately has no `_source` column: it is a count of tessellated
primitives and cannot be analytic. Recording that here prevents a later reader adding one
for symmetry.

The `search` column uses the `simple` configuration, not `english` or `turkish`. Phase 2
owns the real search design — `DATA.md` §3.3 requires identifiers and prose to be indexed
differently, and part numbers must not be stemmed. `simple` is the choice that does not
pretend to have made that decision. The column exists now only because adding a generated
column later is a table rewrite, and because the `STORED` requirement is easy to forget.

DDL order: `blob` is created before `file`, which references it. The block above is grouped
for reading, not for execution order.

Indexes: keyset pagination needs `(library_id, id DESC)` on `part`. Everything else waits
until there is a query to justify it.

### 6.1 A library has to exist before anything can be scanned into one

`POST /api/libraries/{id}/scan` needs an id, and nothing in this slice creates a library:
there is no library-management UI, no create endpoint, and no reason to build either yet.

The migration seeds one row — name `Default`, mode `hobby`, with a fixed uuid recorded in
the migration and in `README.md` so it can be curled without a lookup. Library CRUD arrives
when there is a second library to manage, which is not this slice.

Seeding is an accepted pattern here rather than an expedient: the roadmap already specifies
that Phase 1 ships a bundled example part so the grid is never empty on first run.

**This is the seam to watch.** A fixed id in a migration is convenient until it is a
multi-tenant assumption nobody noticed — `LibraryId` is already a distinct newtype and
`PartSummary` already carries `library`, so the type-level plumbing is honest; only the
*provisioning* is hardcoded. Whichever slice adds a second library must replace the seed,
not build beside it.

---

## 7. Domain types

`PartSummary` already exists with a single `approximate: bool`, documented as *"True when
any geometric figure on this part is mesh-derived."* That is correct for a grid card,
which wants one badge, and it derives from the per-measurement columns as an `any`
reduction — the grid row and the detail view legitimately differ in granularity.

The detail view (Phase 3) reads per-measurement `Approximate<f64>` values. Slice 1
implements only the reduction, but the columns it reads are already the right shape.

`PartRepository::page` exists as a trait with the comment *"Implementation lands in
Phase 1."* This slice is where that happens.

Ids are constructible from stored values — `from_uuid` and `FromStr` landed in Phase 0a's
follow-ups precisely because `page` could not otherwise compile. `lapidary-core` may not
depend on `sqlx` (enforced in `deny.toml`), so the `Uuid ↔ newtype` conversion lives in
`lapidary-db`.

---

## 8. Error handling

**A scan is not transactional across files.** One malformed STL must not abort the walk.
Each failure is collected as `{ file, reason }` and returned with the summary, and the
scan exits 200 with a non-empty `failed` list rather than an error status — a partial
success is the accurate description.

Reasons follow the project rule that errors say what broke and what to do. `lapidary-cad`'s
existing `CadError` is the model:

> `Could not read bracket.stl — the file is 84 bytes, too short to contain an STL header.
> It may be a placeholder or a truncated download.`

The prototype's scan counted `skipped` with no reasons at all, which is why a failed
import there was undiagnosable.

`DbError` already classifies connection failures (`AuthenticationFailed`, `DatabaseMissing`,
`Unreachable`) and never carries a raw URL. New variants follow that pattern and the
existing regression test asserting no message leaks credentials extends to cover them.

---

## 9. Testing

| Layer | Method |
|---|---|
| Mesh parse | fixture STLs — binary, ASCII, truncated, empty, non-manifold — asserting measurements and errors |
| Rasterizer | **golden image**, byte-compared. Viable only because §3.1 chose determinism |
| Blob CAS | round-trip by hash; `ref_count` on re-link; sharded path shape |
| Orphan reaping | force a transaction failure after a blob write; assert no orphan remains |
| Repositories | `#[sqlx::test]` against live PG 18, as `lapidary-api`'s health tests already do |
| Storage boundary | assert `lapidary-api` never names `SourceStore` (grep test in `xtask`, beside `check-layers`) — not a `trybuild` compile-fail test, which would add a dependency for one assertion |
| Scan endpoint | end-to-end against a fixture directory: one part, decodable WebP, correct counts |
| Idempotence | scan twice; second run reports `ingested: 0, skipped: N` |
| Grid | the existing web test pattern, asserting cards render from the API shape |

Fixtures must be licence-clean and real — `fixtures/` currently holds only `cube.stl`, and
Phase 0b's notes already flag that a licence audit is owed there.

---

## 10. Exit criterion

Mount a directory of STL files, `POST /api/libraries/{id}/scan`, and the grid shows a card
per file with a real rendered thumbnail. Re-running the scan reports `ingested: 0` and
completes without parsing or rasterizing anything, demonstrating the hash short-circuit.

Deliberately weaker than the roadmap's Phase 1 exit (1,000 STLs, interactive immediately,
warm page under 80 ms). That criterion needs the queue, SSE and virtualization from slices
2–5, and claiming it here would be false.

---

## 11. Risks

**The rasterizer is the only piece with no prior art.** It is bounded — project, shade,
scanline-fill, encode — and the golden-image test makes regressions loud. If it proves
harder than estimated, slice 1 can ship placeholder tiles and take the renderer as its own
piece of work; the schema and pipeline do not change.

**Volume and watertightness on arbitrary STL are unreliable.** Real-world meshes are
frequently non-manifold, and signed-volume integration over an open mesh returns a number
that means nothing. `is_watertight` must be computed and honoured: when false, `volume` is
NULL rather than a plausible-looking lie. This is "measurement must not lie" applied to
the case where the honest answer is no answer.

**Thumbnails must fit 64 KB** to stay inline as `bytea` per `DATA.md` §1.5. WebP at 512 px
should land far under, but the encoder needs a quality-reduction retry and a hard failure
if it cannot, rather than silently writing an oversized row.

---

## 12. What this unblocks

Slice 2 replaces the synchronous scan with a queued job and gives the worker real leasing,
completing follow-up item 4. Slice 3 extends `MeshKernel` to the LOD ladder and settles
`KernelOutput`'s shape, closing the substance of follow-up item 2. Slices 4 and 5 add the
browser path and the roadmap's real exit test.
