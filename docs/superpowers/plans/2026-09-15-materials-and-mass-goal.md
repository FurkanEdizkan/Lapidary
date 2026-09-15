# Goal 5: materials, mass and field filters

Written 2026-09-15. **This file is the goal's source of truth.** After any context summary, re-read it
together with ROADMAP's "Open points, 2026-09-15" section and this goal's record there, which is the
progress ledger.

Run it after `2026-09-15-occt-goal.md`: STEP materials and a STEP centre of mass come from the bridge.

## Why

The revision diff has had no mass or centre of mass since Phase 4 slice 1, because nothing stores a
density, and no mesh part can even have a material. The owner decided (2026-09-15):
- a density per material, typed in once per library;
- a part's material is editable like its tags, and what a file states fills it only while nobody has typed
  one;
- custom fields get number ranges and counts per choice.

**Decided while planning, for the owner to see:** mass is always shown ≈, even over an exact STEP volume,
because a typed density is not a measurement (CLAUDE.md, "Measurement must not lie").

## Facts already checked

Line numbers are as of `fc0f683`.

- **No mesh part has a material.** Meshes come through with `metadata: None`
  (`crates/lapidary-cad/src/mesh_kernel.rs:112`), and the OBJ reader drops its `.mtl`
  (`crates/lapidary-cad/src/obj.rs:6`). Only STEP names a material, through the bridge, which reads the
  file's density and throws it away (`sidecar/occt-bridge/src/main.cpp:684-694`).
- **`part.materials`** is `text[]` with a GIN index (`crates/lapidary-db/migrations/0021_part_materials.sql`),
  on `part`, set only by ingest stage 4 when the kernel returns metadata
  (`crates/lapidary-ingest/src/handler.rs:956-960`). `set_metadata` (`crates/lapidary-db/src/repo.rs:1787`)
  overwrites it on every CAD re-ingest. The part page does not show materials. The facet is per library
  (`crates/lapidary-api/src/parts.rs:390`).
- **Tags are the pattern for editing:** `SetTags`, `setPartTags` (`web/src/components/PartDetail.tsx:357`).
- **Volume** is a signed-tetrahedron sum over closed meshes (`crates/lapidary-cad/src/measure.rs:29-43`),
  in `MeshMeasurements` (`crates/lapidary-core/src/measurement.rs:72`), stored as `revision.volume` and
  `volume_source` (`0002_parts.sql:65-66`), written at `repo.rs:1241`.
- **No centre of mass is computed.** `revision.mass_props_json jsonb` exists and nothing reads or writes it
  (`0002_parts.sql:78`, DATA §3). The bridge already builds the volume properties whose `CentreOfMass()`
  would give it (`main.cpp:728-729`).
- **The diff** is `Delta` and `RevisionDiff` (`crates/lapidary-core/src/diff.rs:11,31`): volume, area,
  box per axis and triangle count. It is arithmetic over stored rows (`crates/lapidary-vcs/src/diff.rs`).
  The api builds figures in `figures()` and `delta_from_parent` (`crates/lapidary-api/src/revisions.rs:141,146`).
  The web shows the volume delta on the history strip (`PartDetail.tsx:1215,1249-1256`) and the Compare
  table rows at `:1408-1430`.
- **≈** is `Approximate<T>` (`crates/lapidary-core/src/approximate.rs:14`), rendered by
  `web/src/components/Figure.tsx:20-28`.
- **A per-library settings list** to copy: `custom_field` (`0027_custom_fields.sql`), `PgCustomFields`
  (`crates/lapidary-db/src/custom_fields.rs`), routes in `crates/lapidary-api/src/fields.rs` mounted in
  `crates/lapidary-api/src/lib.rs`, and `FieldsMenuItem` / `FieldsDialog` (`web/src/components/Fields.tsx`)
  opened from the library menu (`web/src/routes/index.tsx:1502`).
- **Custom field filters today:** `filter_of` builds one exact `{key: value}` object for `@>`, and a number
  is matched for equality only (`fields.rs:302-326`). The number filter is one text box
  (`Fields.tsx:365-381`). `Facets` holds formats, materials and tags only (`parts.rs:388-394`).

## Stages

Each stage gets its own branch, merged before the next starts. Defaults are stated; list each one you
take under "Decided without the owner" in this goal's ROADMAP record.

### 0. Preflight (no branch)

- `lapidary-test-db` is up, `cargo deny check` passes, and goal 4 is merged.
- Start the OCCT worker the way goal 4's record says.

### 1. A part's material, editable: `feat/part-materials`

- `PUT /api/parts/{id}/materials` takes the whole list, checked as tags are (trimmed, unique, bounded).
- A migration adds `part.materials_typed boolean not null default false`. The route sets it; an empty list
  clears it.
- `set_metadata` writes `materials` from the file only while `materials_typed` is false.
- The part page lists materials beside tags, editable in controlled and hobby libraries alike. The facet
  is unchanged.
- **Tests:** a typed material survives a CAD re-ingest that states another; clearing the list lets the
  file's material back in on the next ingest; the facet counts a typed material. Mutation-check the guard.

### 2. Density per material: `feat/material-density`

- `material_density (library_id, material, density_kg_m3 numeric)`, keyed by the material name exactly as
  parts hold it (case kept, as the facet keeps it), `ON DELETE CASCADE` from the library.
- Routes: list, set (upsert) and remove, under `/api/libraries/{id}/densities`. A density is a finite
  number above 0 and below 25,000 kg/m³; anything else is refused in words.
- A Densities dialog from the library menu, listing the library's materials from the facet with a density
  box each. **Default:** typed and shown in g/cm³, stored in kg/m³.
- **Tests:** the bounds, an upsert, a removal, a material no part holds yet. Mutation-check the bounds.

### 3. Mass: `feat/mass`

- Worked out when read, never stored: in `figures()`, `mass_g = volume_mm3 × density_kg_m3 × 1e-6`, only
  when the part has exactly one material and that material has a density.
- Always ≈. The Compare table gains a Mass row, and the history strip shows its delta beside volume's.
- Both sides of a diff use today's material and density; the row's note says so.
- **Tests:** a 21,478.5 mm³ part of a 7,850 kg/m³ steel reads ≈ 168.6 g; two materials, or none, or no
  density, show no mass. Mutation-check the one-material rule.

### 4. Centre of mass: `feat/centre-of-mass`

- **Mesh:** in the same loop as the volume (`measure.rs`), sum `v·(a+b+c)/4` per triangle and divide by
  the signed total volume, not its absolute value, so an inward-wound mesh gets the same centre. Closed
  meshes only, like the volume, and ≈.
- **STEP:** the bridge writes `CentreOfMass()` into `measurements.json`, exact. Rebuild the `occt` stage
  and run `cargo xtask verify occt`.
- Stored as `revision.mass_props_json = {"centre_mm": [x, y, z]}` with its provenance. It needs no
  density.
- The diff carries a per-axis delta. Revisions ingested before this have none, and the table says so.
- **Tests:** a unit cube moved by (10, 20, 30) has its centre at (10.5, 20.5, 30.5); the same cube wound
  inward gives the same centre; `occt_bridge.rs` asserts the plain cylinder's centre on its axis at half
  its length.

### 5. Number ranges for custom fields: `feat/field-number-ranges`

- `fieldMin` and `fieldMax` beside `field` for a number field, either or both, in the grid, the facets and
  saved filters, and carried in the URL.
- **Default:** `(metadata_json->'custom'->>key)::numeric` between the bounds, written so a non-number value
  never errors the query. The GIN index cannot serve a range; record the ceiling with `ponytail:`.
- The number filter becomes two boxes, "from" and "to".
- **Tests:** db range through page, search and a facet; api refusal of a range on a text or choice field;
  web URL round trip, digits included (the `validateSearch` numeric case). Mutation-check the bounds.

### 6. Counts per choice: `feat/field-choice-counts`

- Each option of a choice field offered as a filter shows how many parts hold it, within the grid's other
  filters, as `tag_facet` counts tags.
- **Tests:** db counts under another filter; web shows the count beside each option. Mutation-check the
  count's filter.

## Before teardown

- A fresh reader reviews this goal's diff: a subagent, or the advisor.
- The ROADMAP record is updated after each merge, not at the end.

## How to work, stop conditions, when done

As `2026-09-15-phase-4-slice-2-goal.md` states in "How to work", "Stop and ask before" and "When done",
and as goal 4 amended them for OCCT stage builds. The essentials:
- **Worktree:** your own, from local `main`, with `web/node_modules` linked and unlinked before removal.
- **TDD:** see each test fail first, or mutation-check it and say so.
- **Gates:** `cargo xtask export-bindings`, then
  `CARGO_BUILD_JOBS=4 DATABASE_URL=postgres://lapidary:localdev@localhost:55432/lapidary cargo xtask verify slice`
  in the foreground, logging to `target/`; `cargo xtask verify occt` after any bridge change.
- **Browser checks:** headless Chrome with a throwaway profile, through the UI.
- **Commits:** conventional, with no AI attribution trailer. **Merges:** `git merge --no-ff` into local
  `main`. Never push.
- **When done:** every stage merged or recorded with its reason, FEATURES §4 amended for mass and centre of
  mass, and DATA amended for `material_density`, `materials_typed` and `mass_props_json`.
