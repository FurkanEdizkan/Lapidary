# Feature list

Phase tags map to `docs/ROADMAP.md`. `[—]` means deliberately not planned.

---

## 1. Libraries and ingest

| Feature | Phase |
|---|---|
| Create libraries; `hobby` or `controlled` mode per library | 1 |
| Drag-drop upload: files, folders, mixed formats | 1 |
| Client-side BLAKE3 + probe → transfer only unknown blobs | 1 |
| Resumable chunked upload, server-verified hash | 1 |
| Non-blocking ingest with live SSE progress; UI stays interactive | 1 |
| Crash-resumable job queue (rows in Postgres, not memory) | 1 |
| Per-file stage names: hashing, parsing, tessellating, rendering | 1 |
<!--
  **Half of this row is delivered and half of it is not**, recorded here on 2026-09-08
  after a Phase 1 audit rather than left for a reader to discover by looking for a stage
  name that is not there.

  *Delivered:* the upload half, per file, client-side. `web/src/lib/upload.ts` reports
  `hashing`, `probing`, `transferring` and `committing` for each file as it goes, which is
  the first noun in this row and the slowest part of a drag-drop of 1,700 meshes.

  *Not delivered:* the server half. `parsing`, `tessellating` and `rendering` all happen
  inside the worker, and what the worker publishes is a **batch** count by outcome —
  `pending`, `running`, `ingested`, `skipped`, `rendered`, `scanned` — plus a per-file list
  of the ones that *failed*, with the path and the reason. So a person watching a scan
  reads "Scanning — 320 of 1,703 files" and can see exactly which files broke, but cannot
  see that `nema-17-motor-mount.stl` is tessellating right now.

  The gap is not an oversight in a slice; it is a shape the job queue does not have. Per-
  file stages need a `stage` column on `job`, the worker writing it at each transition, the
  SSE stream carrying per-file rows rather than one aggregate, and a UI that can show a
  list that long without becoming the page. That is a slice, not a fix.

  **Open question, deliberately not decided here:** build it, or amend this row to the
  aggregate-plus-failures shape that shipped — the way the "per library" row above was
  amended, with the reason recorded. Worth noting for whoever decides: the failure list is
  the part an operator acts on, and it is per file already.
-->
| Mesh formats: STL, 3MF, OBJ | 1 |
| Folder tree per library, mirrored on disk — one directory per part | 1 |
| Move a part between folders; renames its directory, `source_path` unchanged | 1 |
| Move history per part, newest first — route only, deliberately unread ([—] UI) | 1 |
<!--
  The move-history row keeps its Phase 1 tag because `GET /api/parts/{id}/moves` shipped
  with the folder tree and answers correctly. What was decided on 2026-09-07 is that
  **nothing displays it**, and that is a product call rather than an oversight: a "moved
  from" line in the inspector is history about a thing whose current location the card
  already shows, and nobody asked where a part used to be.

  Recorded here so the next reader does not file it as unfinished. The route stays, and so
  does its `[]` answer for a part id that names nothing — which is the same answer as "this
  part has never moved", deliberately, because nothing acts differently on the two. If a UI
  ever reads it, that 404 starts to matter and belongs in the same change.
-->
| B-rep formats: STEP, IGES | 2 |
| Failed-file drawer with actionable errors and per-file retry | 2 |
| Watched-folder ingest (agent) | 4 |
| Auto-detect and offer duplicate merge on ingest | 6 |

## 2. Browse, search, triage

| Feature | Phase |
|---|---|
| Virtualized part grid, keyset pagination | 1 |
<!--
  "Virtualized" shipped as `content-visibility: auto` plus a 500-row ceiling on a page,
  not as a windowing library, and that substitution is argued in `index.tsx` but was never
  recorded here — amended for the same reason "per library" below was.

  What the row asks for is that a page of cards stays cheap when most of them are off
  screen. One CSS property gets that: the browser skips layout, paint and image decode for
  a card outside the viewport, which is what a virtualizer buys, without a component that
  has to be told the viewport's size and re-measured whenever the density control changes
  it. The keyset pagination half is delivered as written.

  The ceiling is what makes it hold: `MAX_LIMIT` is 500 (`parts.rs`), so a page is bounded
  even though the DOM is not windowed. A library of 100,000 parts is 200 pages, not one
  enormous one. If a page size ever exceeds what the DOM can hold, this row needs the real
  thing rather than another amendment.
-->
| Page size 50/100/250/500 and card density, both persisted per viewer, per library | 1 |
<!--
  "per library" was the original wording and would have meant a column. There is no user
  table, no session table and no auth in Phase 1, so a column would make one operator's
  choice everybody's — and be wrong the moment auth arrives. These live in the browser's own
  storage keyed by library id: each library remembers its own setting, which is the intent,
  and the memory is that browser's, which is the limit. Amended rather than left to read as
  met by something narrower than it says.
-->
| Thumbnails inline from Postgres `bytea`, rendered at ingest or on demand per library | 1 |
| Full-text search over names, tags, materials | 1 |
<!--
  **Names only.** Recorded 2026-09-08, from the same audit as the stage-names row above.

  `part.search` is `setweight(part_number, 'A') || setweight(name, 'B')` and has been since
  `0002`. Tags and materials are not missing from the tsvector — they do not exist: there
  is no `tag` table, no `material` column, and nothing anywhere in the tree writes either.
  `part.classification` and `part.metadata_json` exist and are likewise unwritten.

  So this is a dependency rather than a gap. Phase 2's metadata extractor is what produces
  tags and materials, and the search that indexes them belongs in the same slice as the
  thing that fills them — indexing an empty column now would be `0016`'s `part_number_trgm`
  again, which is a defensible thing to build early but not a feature anyone can use.

  The names half is delivered and does the work the row is really about: multi-word AND
  over names, either order, neither adjacent, plus the trigram half for fragments.
-->
| Trigram search for part numbers and filenames | 1 |
| Faceted filters: format, material, library, tags, lifecycle | 2 |
| Sort by any promoted geometric column | 2 |
| Saved filters / smart collections | 5 |
| User-defined custom fields, max 8 indexed | 5 |
| Turkish text search config per library | 5 |
| Near-duplicate clustering with merge-or-link-as-variant | 6 |
| Similarity search by geometry embedding (pgvector) | 6 |

## 3. Viewer and measurement

| Feature | Phase |
|---|---|
| three.js viewer, glTF + meshopt, LOD streaming | 3 |
| Prefetch L0 on hover, L1 on inspector open | 3 |
| Assembly tree navigation, isolate and hide | 3 |
| Point-to-point, edge length, diameter, angle | 3 |
| Snap to analytic B-rep entities; exact nominal values | 3 |
| Mesh-derived values visibly labelled "approximate" | 3 |
| Wall thickness | 3 |
| Section plane | 5 |
| PMI / GD&T display from AP242 | 5 |
| Explode view | 5 |

## 4. Versioning

| Feature | Phase |
|---|---|
| Immutable revisions, content-addressed | 4 |
| Lineage DAG with `origin` on each revision | 4 |
| Version history strip: thumbnails + volume delta between revisions | 4 |
| Geometric diff: Δ volume, bbox, mass, counts | 4 |
| Visual overlay diff (grey ghost vs solid) | 4 |
| Pessimistic check-out / check-in locks | 4 |
| Lifecycle states + approvals (`controlled` libraries only) | 8 |
| Per-face Hausdorff heatmap, async | 8 |
| Merge, branches, textual diff | **[—]** |

## 5. External tools

| Feature | Phase |
|---|---|
| `Target` trait with automatic format negotiation | 4 |
| Download `variant=original` (byte-identical, hash shown) | 1 |
| Download derivatives, `.lapidary.` infix | 3 |
| Streaming ZIP bundle + `manifest.json` | 5 |
| `lapidary://` URI scheme + agent launch | 4 |
| Native watcher → automatic new revision on external save | 4 |
| Time-limited supplier share links | 9 |
| Rhino `.rhp` / Blender addon | **[—]** until a customer asks by name |
| Direct-to-printer sending | **[—]** we hand off, never print |
| Geometry editing | **[—]** ever |

## 6. Source links and images

Pulled forward from Phase 5 by owner decision, 2026-09-07 — a part with a photograph and a
link to where it came from is most of what makes a library worth sitting in front of, and
none of it depends on anything Phase 2 onwards adds.

| Feature | Phase |
|---|---|
| Attach product/source URL, vendor, SKU, price | 1 |
| Licence field surfaced on the **detail page and quick-look**, not the card | 1 |
| Image: upload file | 1 |
| Image: paste URL, cached locally, never hotlinked | 1 |
| Image: adjust the framing, non-destructively, with size bounds both ends | 1 |
| Image: fetch OpenGraph preview on explicit button press | **[—]** |
| SSRF controls + bounded image decode + EXIF strip | 1 |
| Catalogue scraping | **[—]** |

Three rows changed meaning rather than only phase, and each is a decision:

- **The licence is not on the grid card.** `DATA.md` §4 asks for it there and the reason is
  good — somebody selling prints needs to see `CC-BY-NC` before they print. The grid query
  is at its 16-column `FromRow` ceiling, and reworking that tuple to put a sentence on a tile
  somebody is scanning at a glance is the wrong trade. It is one click away, on the detail
  page and in the quick-look, which is where the decision to print is actually made.
- **Framing is a new row, and it is what "adjust the image view" turned into.** The framing
  is three columns on `part_image` applied by the browser as `object-fit` and
  `object-position` — so nothing is re-encoded, the adjustment is free and reversible, and
  the picture never loses a generation of quality to being re-cropped. Bounds are 64 px
  minimum edge (below that it is an icon somebody dragged off a web page), 2048 px maximum
  (over which it is resized, not refused), 10 MB input.
- **OpenGraph fetching is out**, by owner decision, and `DATA.md` §4 is emphatic about where
  that road ends: do not build a scraper. `part_image.origin` still carries `og_fetched`, so
  adding it later stays additive.

A user image does not yet win over the render on a grid card. A blob-backed image cannot
reach the grid without either a 17th column or a card-sized second copy on the row, and
neither is worth it for a picture the gallery already shows one click away.

## 7. Build graph (blueprint board)

The largest subsystem. Detailed spec below.

| Feature | Phase |
|---|---|
| Node board: pan, zoom, connect, auto-layout | 7 |
| Node kinds: part, operation, assembly, purchase, consumable | 7 |
| Process types with JSON Schema params (print, mold, cut, mill…) | 7 |
| User-defined process types per library | 7 |
| Edges carry quantity; cycle rejection at write time | 7 |
| Import BOM from CAD assembly tree to seed a graph | 7 |
| Graph versioned like a part | 7 |
| Build runs separate from graphs | 7 |
| Ready-set: "what do I make next", critical path + batch affinity | 7 |
| Guide view — queue mode and sequential mode, mobile-shaped | 7 |
| Auto-generated first-draft steps from process templates | 7 |
| Photo capture on step completion | 7 |
| PDF export of the guide with thumbnails | 7 |
| Subgraph references (reusable sub-assemblies) | 8 |
| Full MRP: lead times, POs, inventory netting | **[—]** |

## 8. Dashboard

| Feature | Phase |
|---|---|
| Widget registry with sizing constraints and config schema | 6 |
| Drag-resize grid layout, persisted per user per workspace | 6 |
| Named groups / sections | 6 |
| Single batched `/api/dashboard/resolve` endpoint | 6 |
| Live patches over the existing SSE stream | 6 |
| Per-widget polling | **[—]** self-inflicted DoS |

## 9. Server, fleet, enterprise

| Feature | Phase |
|---|---|
| Multi-user auth, RBAC | 8 |
| Audit log | 8 |
| Settings → Compute: enrol, list, drain, revoke worker nodes | 8 |
| Remote worker lease protocol with heartbeats | 8 |
| Kernel-version pinning across the fleet | 8 |
| Ed25519 offline licence, `max_workers`, grace period on expiry | 8 |
| Air-gapped install: image bundle + Quadlet units | 8 |
| Export-everything bundle (non-lock-in) | 8 |
| Cloud sync, per-GB, zero-egress storage | 9 |
| Phone-home licence checks | **[—]** ever |

---

## Build graph — detailed spec

### Two graphs, not one

Conflating these is what makes assembly tools unusable.

| | **BOM tree** | **Process graph** |
|---|---|---|
| Answers | What is it made of? | How do I make it? |
| Shape | Hierarchy | DAG |
| Source | Imported from CAD structure | Authored by the user |
| Changes when | The design changes | The method changes |

The board edits the **process graph**. A BOM can seed one, but the same BOM has different
process graphs for FDM versus injection moulding — which is exactly the
hobbyist/industrial overlap, and it only works if they are separate objects.

### Schema

```sql
process_type(
  id, key, label, icon,
  params_schema_json,       -- JSON Schema; drives the node's form UI
  default_instruction_md,   -- template for auto-generated guide steps
  is_builtin bool, library_id uuid   -- null library_id = shipped builtin
);

build_graph(id, library_id, name, description, created_at, updated_at);

build_graph_revision(
  id, graph_id, parent_revision_id,
  blake3,                   -- content-addressed serialized graph
  author, message, created_at
);

build_node(
  id, graph_id,
  kind text,                -- part|operation|assembly|purchase|consumable|subgraph
  part_id uuid, revision_id uuid,   -- revision_id null = "latest" (floating)
  subgraph_id uuid,         -- reserved; phase 8
  process_type_id uuid,
  label text, position_json jsonb, params_json jsonb,
  est_duration_s integer, qty_produced integer DEFAULT 1
);

build_edge(id, graph_id, from_node, to_node, kind, qty integer DEFAULT 1);
  -- kind: consumes | precedes

build_step(id, node_id, seq, instruction_md, image_blake3, tool, torque_spec);

build_run(id, graph_revision_id, name, qty, status, started_at, finished_at);
  -- status: planned|active|paused|done|abandoned

build_run_node(
  run_id, node_id,
  state text,               -- blocked|ready|in_progress|done|failed|skipped
  qty_required, qty_done,
  started_at, finished_at, notes, operator, photo_blake3,
  PRIMARY KEY (run_id, node_id)
);
```

**Ship ~15 builtin process types:** `print_fdm`, `print_resin`, `mold_injection`, `cast`,
`cnc_mill`, `cnc_turn`, `laser_cut`, `sheet_bend`, `weld`, `assemble`, `finish`,
`heat_treat`, `purchase`, `inspect`, `manual`. Each carries its own params schema —
printing needs printer/material/layer height, moulding needs tool number/shot
weight/cycle time/cavities. A free-text tag loses all of it; a fixed enum locks out
workflows we did not predict. Filter the palette by library type so a hobbyist does not
scroll past injection moulding to find FDM.

**Runs are instances, graphs are templates.** Without this split you cannot build the
same assembly twice, cannot track two units in parallel, and editing a graph destroys the
history of what was already built.

**Pinned vs floating revisions.** A node with `revision_id` null silently changes what
gets built when someone uploads a new revision. Support both, show the difference
clearly, default to pinned in `controlled` libraries and floating in hobby libraries.

### Ready-set

```
ready(run) = { n : state(n) ∈ {blocked, ready}
                 ∧ ∀ p ∈ predecessors(n): qty_done(p) ≥ qty_required(p) }
```

Ordering — this is what makes it useful rather than merely correct:

1. **Critical path first** — longest weighted path from the node to the root using
   `est_duration_s`. Anything on it delays the whole build.
2. **Batch affinity** — group ready nodes sharing material, colour and machine profile.
   For FDM this is enormous: five ready parts in the same filament is one plate, not five.
3. **Shortest first** among ties, so progress feels visible.

Quantities multiply through the graph:
`qty_required(n) = Σ over outgoing edges (qty(edge) × qty_required(target))`, seeded by
`build_run.qty` at the root. This is MRP-lite and must stay that way.

**Cycles are rejected at write time**, via `petgraph::is_cyclic_directed` before commit,
returning which edge closed the loop so the UI can highlight it. A cycle reaching the
database makes the ready-set query loop forever. When subgraphs land in phase 8, cycle
detection must span graphs.

### Two UIs on the same data

| | Board | Guide |
|---|---|---|
| Purpose | Design the process | Follow the process |
| Shape | Spatial DAG (React Flow) | Linear, one step at a time |
| Device | Desk, mouse | **Phone or tablet, at the bench** |
| Writes | `build_graph` | `build_run_node` |

The guide is a **linearization of the DAG** using the same ranking as the ready-set. But
allow marking any *ready* step done out of order — no real shop works strictly
sequentially.

Guide screen needs: large touch targets, one step at a time, the part thumbnail inline
("which bracket is this" is the actual question), the local slice of the graph (what
feeds in, what this feeds), a timer for print/cure/cool durations, and photo capture on
completion. That photo is a build log for hobbyists and inspection evidence for industry.

Two modes: **queue** (ready-set, grouped by material and machine — for a printer farm)
and **sequential** (step 1..n, IKEA-style — for assembly).

**Auto-generate the first draft** from each process type's `default_instruction_md` with
quantities and parameters interpolated. Nobody writes forty steps from a blank page — if
the guide starts empty it stays empty and the feature dies.

### Board implementation

`@xyflow/react`. Do not hand-roll a node editor.

- `onlyRenderVisibleElements` past ~300 nodes
- Debounce position persistence to ~800 ms after drag end, never per frame
- Custom node component per `kind`; part nodes show their thumbnail, which makes the
  board legible at a glance
- `dagre` or `elkjs` auto-layout for imported BOMs — 200 nodes at random coordinates is
  unusable
