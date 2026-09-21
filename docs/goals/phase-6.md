# Phase 6 — dashboard and similarity: the shared design

The design every Phase 6 goal (W0, G1–G6) builds against. The roadmap bullets are in `docs/ROADMAP.md` § "Phase 6";
the goals are on [`BOARD.md`](BOARD.md). Decided with the owner on 2026-09-21; anything here a goal wants to change
goes to the lead first, because other goals build against it.

**Exit (the roadmap's):** a 12-widget dashboard settles in one round trip; uploading a known part surfaces its
near-duplicates.

## Owner's decisions

1. **No pgvector in Phase 6.** A part's shape profile is a plain `real[]`, compared exactly in Rust. At 10,000 parts ×
   35 floats a full scan reads about 1.6 MB and ranks in about 10 ms, and exact beats HNSW's approximate at that size.
   The test databases (dev `postgres:18` on 55432, CI's service) stay as they are. The ceiling is around 100k parts;
   the upgrade is one statement, `ALTER TABLE part_shape ALTER descriptor TYPE vector(35) USING
   descriptor::vector`, plus moving the test databases to `deploy/db`'s image. Leave a `ponytail:` naming it.
2. **A dashboard's layout is stored per browser until there are users** (Phase 8), in localStorage, as
   `web/src/lib/preferences.ts` already argues for the grid.

## Shape profile — `lapidary-core::shape`

- **Input:** the stored L0 tessellation (`derivative.kind = 'tessellation_l0'`), decoded from its GLB — never the
  source file, never the kernel. Ingest and backfill read the same bytes, so they give bit-identical results, and an
  algorithm change costs a 5k-triangle decode, not a re-tessellation. L0's clustering smooths the difference between
  an STL and a STEP of the same part.
- **Computation** (`crates/lapidary-cad/src/shape.rs`, pure Rust, no new crates):
  - principal axes: area-weighted surface covariance, exact per triangle; eigenvalues λ1 ≥ λ2 ≥ λ3 from a closed-form
    3×3 solver;
  - D2 distribution: 65,536 point pairs, triangles chosen by area (binary search on cumulative area), points uniform
    inside them, splitmix64 with a fixed seed; distances divided by their mean `m`, 32 bins over `[0, 3m)`, overflow
    into the last; stored as **square roots of the probabilities**, so Euclidean distance on that block is Hellinger
    distance;
  - three ratios: λ2/λ1, λ3/λ1, ln(area / m²).
- **Result:** `ShapeProfile { size_mm: m, descriptor: [f32; 35] }` — `size_mm` is the mean distance between two
  surface points, so open meshes work. `SHAPE_VERSION: i16 = 1`; a change to the sampler, seed, bins or ratios bumps
  it, and rows of an older version are ignored and re-profiled.
- **Invariant** (each tested): translation, rotation, triangle and vertex order, re-tessellation (within noise),
  uniform scale (the descriptor; `size_mm` scales). **Not invariant, documented:** mirror images look identical — the
  "Not the same" decision handles left and right hands.

## Three kinds of alike

| | Rule | Needs a profile |
|---|---|---|
| **Identical** | same current source `file.blake3`, same library | no |
| **Near-duplicate** | `distance ≤ NEAR_DUPLICATE_DISTANCE` (0.04 to start; G2 calibrates) **and** `|ln(size_a/size_b)| ≤ ln(1.02)` | yes |
| **Similar** ("more like this") | top k by distance, size ignored | yes |

Clusters are **never stored**: read-time, sorted by `size_mm`, swept within the size band, union-find over undecided
near-duplicate pairs. Candidates come from an index on `(library_id, size_mm)` and a ±2% band. No score is shown to
anyone, so there is no approximate figure to label.

## Links, and folding — `part_link`

- `kind`: `variant` (they belong together), `distinct` (not the same — never propose again), `folded_into`.
- **"Fold into"** is Phase 6's name for the roadmap's "merge" — CLAUDE.md reserves "no merge" for versioning. In one
  transaction it soft-deletes the duplicate by the same rule as `soft_delete` and writes a `folded_into` row naming the
  part kept. **Nothing is moved and nothing is deleted:** the folded part keeps its tags, sources, images and revisions,
  and Restore brings it back. The dialog says so.
- `variant` and `distinct` take the pair out of the review queue for good. Cross-library links are refused 404.
- **Purge takes links from either side.** Purging the part something was folded *into* removes that `folded_into` row
  too, so the folded part stays removed as an ordinary removed part — no "folded into" line, Restore as usual. That is
  expected, not an orphan: G3's folds list simply does not name it, and G6 shows nothing special for it.

## App-wide events — `GET /api/events`

- **Source:** migration `0049` adds the repository's first trigger function, `lapidary_changed()`, which runs
  `pg_notify('lapidary_events', library_id::text)` after each insert/update/delete on `part` (per row) and after
  `UPDATE OF state` on `job` when the new state is `done` or `failed`. Every writer notifies without remembering to;
  Postgres collapses identical notifications within one transaction.
- **Fan-out:** one `PgListener` **per api process** (`crates/lapidary-db/src/events.rs`), never one per tab — that is
  what the batch stream's comment rejected. A hub task (`crates/lapidary-api/src/events.rs`) groups notifications per
  library over 250 ms and sends `AppEvent::Changed { library }` on a `tokio::sync::broadcast`. A reconnect or a
  lagging subscriber becomes `AppEvent::Resync`. The hub ends on shutdown, which ends every stream.
- **Route:** `GET /api/events`, built by `events::router(hub)` and merged in `bin/lapidary-server/src/main.rs` for the
  api role only (its own router, because `AppState` is built in 88 places). Headers `X-Accel-Buffering: no` and
  `Cache-Control: no-cache`, on this route and on the existing batch stream.
- **Payload:** only the library id; the client asks again. Browser: one shared, reference-counted `EventSource` per
  tab (`web/src/lib/events.ts`, `useAppEvents`), a reopen treated as a resync.

## Dashboard — `POST /api/dashboard/resolve`

- **Widgets:** a serde enum tagged by `kind` in `crates/lapidary-api/src/dashboard.rs`, exported by ts-rs; the enum is
  the config schema. Phase 6's kinds: `storage { library }`, `instanceStorage`, `recent { library, limit ≤ 12 }`,
  `savedFilter { library, filter, limit ≤ 12 }`, `facet { library, facet: format|material|tag, limit }`,
  `queue { library }`, `duplicates { library }`.
- **Web registry:** `Record<Widget['kind'], WidgetSpec>` (label, min/max/default size, renderer, settings form), keyed
  by the Rust union, so a new Rust kind fails `tsc` until the web registers it.
- **Resolve:** body `{ widgets: [{ key, widget }] }`, 1–32 unique keys (else 422). Always answers 200 with
  `{ results: [{ key, result }] }` in request order, `result` = `ok { value } | timedOut | failed { message }`. A
  `JoinSet` with a semaphore of 4 and a 2 s timeout per key **starting once the key has its permit** — the pool is
  `max_connections(8)`, and without the semaphore keys would time out waiting for the pool, not their query. An unknown
  library fails only its key.
- **No per-widget polling** (FEATURES §8 calls it a self-inflicted DoS): no per-widget endpoint exists, and a web test
  fails if `refetchInterval` appears under the dashboard's directory.
- **Layout:** per browser, localStorage `lapidary.dashboard`, version 1. Opening the dashboard costs one request.
- **Drag and resize, hand-rolled:** a 12-column CSS grid; pointer events with capture for drag and resize; arrow keys
  move, Shift+arrow resizes; `aria-live` says where; vertical compaction; the motion is `web/src/lib/flip.ts`
  (transform only, 180 ms, reduced motion respected); the layout math is a pure `layout.ts`. No react-grid-layout: its
  CSS transitions clash with the motion rules, and WCAG needs the keyboard support we would write anyway.
- **Named groups:** each a titled `<section>` with its own grid; widgets move between groups and groups reorder
  through menus.

## Contracts (W0 lands these before anything else)

| Contract | Where | Used by |
|---|---|---|
| `ShapeProfile`, `SHAPE_VERSION`, `DESCRIPTOR_LEN = 35`, `distance()`, `is_near_duplicate()`, thresholds | `crates/lapidary-core/src/shape.rs` | G2, G3 |
| `AppEvent { Changed { library }, Resync }`, tagged by `type` | `crates/lapidary-core/src/event.rs` | G1, G5 |
| `PartLinkKind` | `crates/lapidary-core/src/link.rs` | G3, G6 |
| `0047_part_shape.sql`, `0048_part_link.sql` | `crates/lapidary-db/migrations/` | G2, G3 |
| `PgShapes::record` (upserts only when the revision is ≥ the stored one) | `crates/lapidary-db/src/shapes.rs` | G2, G3's tests |
| `purge` deletes both new tables; `PgParts::rows_by_id` | `crates/lapidary-db/src/repo.rs` | G3, G4 |
| Wire types: `Likeness`, `DuplicateClusters`, `SetLink`, `FoldPart`; `Widget`, `ResolveRequest`, `ResolveResponse`; empty routers | `crates/lapidary-api/src/{likeness,dashboard}.rs` | G3, G4, G5, G6 |
| `to_card` made `pub(crate)` | `crates/lapidary-api/src/parts.rs` | G3, G4 |
| `strings.ts` blocks `dashboard`, `likeness`; titles | `web/src/lib/strings.ts` | G5, G6 |
