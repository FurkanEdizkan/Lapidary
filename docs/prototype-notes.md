# What the Node prototype knew

The prototype (Fastify + SQLite, ~1,000 LOC of services) is deleted. It remains on
`main` and in git history. This records what it established, so the knowledge outlives
the code.

## Domain shape that survived contact with real files

`model.service.ts` settled on a row shape worth carrying into `lapidary-core`:
identity (`id`, `name`, `creator`), classification (`type`, `mesh_kind`, `format`),
geometry (`bbox_x/y/z`, `triangle_count`, `file_size_bytes`), provenance
(`created_date`, `added_date`, `original_path`), and derivative presence flags
(`thumbnail_path`, `lod_path` → `hasThumbnail`, `hasLod`, `hasOriginal`).

The three many-to-many attachments — tags, groups, printer types — were each a join
table resolved to a sorted name list. Lapidary keeps that shape; the join resolution
moves behind repository traits in `lapidary-db`.

**Note the bug we are not carrying forward:** `listModels` did `SELECT * FROM models`
and filtered in TypeScript. Phase 1 filters and paginates in SQL with keyset
pagination.

## Search behaviour worth reproducing

`search.service.ts` returned three result classes from one query — matching models,
creators with counts, and tags drawn from matched models — plus a header that changes
between `POPULAR TAGS` (empty query) and `TAGS & SUGGESTIONS` (non-empty). Caps were
5 models, 4 creators, 8 tags, 9 popular tags, 12 rail tags.

Tag counts were computed by a full `GROUP BY` on every keystroke. `lapidary-index`
replaces this with `tsvector` + `pg_trgm` and the 10k exact-count threshold from
`docs/DATA.md`, but the *shape* of the suggestion payload is right.

## Ingest pipeline: ordering, idempotency, and failure modes

`assetPipeline.service.ts`'s `ingestMesh(modelId, originalName, buffer)` was the only ingest
entry point, called from `libraryScan.service.ts` per file. It ran two stages, strictly in
order, with a third tier deferred entirely:

1. **Tier 3 (original), always first, unguarded.** `compress()` picks `zstd` when the
   running Node's `zlib` exposes `zstdCompressSync`, else falls back to `gzip`, and writes
   the result to `${modelId}${ext}.zst` (or `.gz`) in `config.modelsDir` — e.g. an ingested
   `bracket.stl` becomes `u<nanoid>.stl.zst`. This write (`fs.writeFileSync(originalPath,
   data)`) has no try/catch around it: if it throws, `ingestMesh` throws immediately and no
   DB row is ever created.
2. **Tier 2 (LOD + metrics), via the external sidecar.** The raw buffer is written to a
   temp file in `lodDir` (`${modelId}.raw${ext}`), handed to `analyzeMesh()`
   (`meshSidecar.service.ts`), which shells out to the optional `rust-mesh` binary
   (`execFile`, 60s timeout, `--lod <path> --json`) and parses its JSON stdout for `bbox`
   and `triangles`. The temp raw file is deleted in a `finally` block regardless of outcome.
3. **Tier 1 (thumbnail) was explicitly out of scope for ingest** — the code comment says it
   is "left to be generated client-side and cached back." It arrived later via a separate
   route (`POST /api/models/:id/thumbnail` in `routes/api.ts`, calling `updateModel(id, {
   thumbnailPath })`), never through `ingestMesh`.

The caller (`libraryScan.service.ts`) then does two more writes in order: `createModel()`
inserts the row (without `triangle_count` — that column isn't in the INSERT list), and only
afterward, if `ingest.triangleCount` is truthy, a follow-up `UPDATE models SET
triangle_count = ? WHERE id = ?`. So a fully-succeeded ingest is four sequential writes:
blob, (optional) LOD file, INSERT, conditional UPDATE.

**Hash: there wasn't one.** Grepping `assetPipeline.service.ts`, `meshSidecar.service.ts`,
`libraryScan.service.ts` and `model.service.ts` for `hash|blake|sha256|checksum` returns
nothing. The prototype never computed a content hash anywhere in ingest — this is a direct
divergence from Lapidary's "hash first, always" rule, not a variant of it. Duplicate
detection instead happened one layer up, by filename (see the library-scan section below).

**Idempotency / resumability: effectively none, by design accident rather than intent.**
`modelId` is a freshly minted `nanoid` per call (`u${nanoid(10)}` in
`libraryScan.service.ts`), never derived from the file's content, so `ingestMesh` has no way
to recognize "I've already ingested this exact file." Re-running ingest for the same bytes
after a failure produces a new `modelId`, new blob paths, and (if it gets that far) a new DB
row. Nothing inside `ingestMesh` is resumable; the only reason a rescan doesn't re-import
everything is the filename-stem dedup living in the caller.

**Partial failure:** Tier 2 degrades gracefully — `analyzeMesh` internally catches every
error (bad binary, timeout, malformed JSON) and returns `null` rather than throwing, so a
sidecar failure just yields `size: null`, `triangleCount: 0`, `lodPath: null` while Tier 3
stays intact. There is no equivalent guard around the handoff to the database: if
`ingestMesh` succeeds (Tier 3 blob written, possibly Tier 2 too) but the subsequent
`createModel()` call throws — a DB constraint, a lock — `libraryScan.service.ts`'s catch
block only increments `skipped`; it never unlinks the blob it just wrote. That is an
orphaned-blob leak on every DB-side ingest failure, and there was no compaction or GC job in
the prototype to reap it.

**Keep:** graceful LOD-stage degradation (a sidecar failure shouldn't fail the whole
ingest) is worth preserving as a behavior, if the same shape holds in Rust. **Discard/fix:**
identity must come from the content hash, not a random id, so a retried or resumed ingest is
naturally recognized rather than accidentally re-run; and blob writes need to be reconciled
with the DB write (staged blob + transactional commit, or an explicit reaper) so a DB
failure after a successful write can't leak storage silently — silent leaks are exactly what
"we never delete user data implicitly" was written against, but the inverse failure (never
*reclaiming* orphaned data) is just as much a problem to design out.

## LOD approach

`rust-mesh` was dependency-free so it compiled offline in the container build — a
constraint worth preserving. It computed an exact bounding box in mm and a triangle
count, and generated LOD by **vertex clustering on a 48³ grid**, writing binary STL.

Vertex clustering is the right first LOD algorithm for `lapidary-cad`: single pass,
no topology required, degrades gracefully on the malformed meshes real libraries
contain. The 48 constant was tuned by eye and should become an L0/L1/L2 ladder.

## Library scan: directory walk and the debounce that never existed

`libraryScan.service.ts`'s `scanFolder()` was the only entry point for bulk import, invoked
exclusively from `POST /api/scan` (`routes/api.ts`) with a folder path from the request body
or `config.libraryPath` (the `LIBRARY_PATH` env var). It ran one synchronous walk-then-ingest
pass and returned `{ scanned, imported, skipped }`.

**Walk order:** `collect()` is a recursive `readdirSync(dir, { withFileTypes: true })` walk,
depth-first in whatever order the OS returns directory entries (not sorted), capped at depth
8 as a hard recursion guard. Any `readdirSync` failure — permission denied, a bad symlink —
is caught and the directory is silently skipped; nothing is logged, nothing surfaces to the
caller. Only three extensions are collected: `SUPPORTED = new Set(['.stl', '.3mf', '.obj'])`,
matched case-insensitively; anything else (`.step`, `.gcode`, textures, READMEs) is silently
excluded.

**Dedup — and the divergence from hashing:** before importing, `scanFolder` loads every
existing model name (`SELECT name FROM models`) into a `Set`. For each file, `name =
path.basename(file, ext)`; if that name is already in the set, the file is counted as
`skipped` and never read. The set is updated (`existingNames.add(name)`) synchronously
*before* the async ingest starts, so two files with the same stem within one scan can't both
import — but this means dedup is by filename stem alone, not path, not content, not hash.
Two identically-named files in different subfolders will race under the bounded concurrency
below: whichever task claims the name first wins the import; the other is silently treated as
a duplicate and dropped.

**Concurrency:** bounded to 3 concurrent ingest tasks via `p-limit`, with the comment "bounded
concurrency so a large library cannot exhaust memory." This reorders execution relative to
walk order — files are *discovered* depth-first but *ingested* out of order as `pLimit` slots
free up.

**Debounce / filesystem events: there was none to speak of.** Neither `libraryScan.service.ts`
nor its only caller contains `fs.watch`, `chokidar`, or any change-event handling —
confirmed by grepping the whole `server/src` tree for `watch|debounce|chokidar`, which
matched nothing outside this file's own name. There was no background watcher, no polling
loop, no scheduled rescan, and therefore no coalescing window to record: **scanning was
purely on-demand, one HTTP POST triggering exactly one pass.** This is thinner than the task
brief implied — there is no debounce behavior to carry forward or discard, because none was
ever built.

**Mid-scan changes:** with no watcher, "mid-scan" only means the gap between `collect()`
enumerating the tree up front and each file's `fs.readFileSync(file)` happening later, inside
whichever `pLimit` slot reaches it. A file edited in that window is read at whatever state it
is in when its turn comes — silently, no re-check, no staleness detection. A file deleted in
that window throws inside the task's try/catch and is counted as `skipped`, same as any other
per-file ingest error.

**Keep:** bounded ingest concurrency (a small fixed worker pool) is a reasonable default to
carry forward as-is. The on-demand-scan-as-an-explicit-action shape is also worth keeping —
it matches "nothing happens implicitly." **Discard/redesign:** dedup must move from
filename-stem matching to content hash, so same-named files in different folders don't
silently lose one to a race; directory-walk errors (permission denied, symlink loops) should
surface as diagnosable warnings instead of vanishing; and if Phase 1 wants filesystem-watch
behavior at all (debounce window, event coalescing), that is new design work — there is
nothing proven here to port, only the absence of it.

## Slicer profile parsing

Source: `profileImport.service.ts` (parsing), `printerSettings.service.ts` (persistence),
`printerType.service.ts` (unrelated — see below). This is the area with the most concrete,
reusable detail, and the one flagged as heading into `lapidary-targets`.

**Formats handled, selected purely by filename extension**
(`filename.toLowerCase().endsWith('.json')`, else treated as `.ini`):

- **`.ini`** — the doc comment names it explicitly: "PrusaSlicer / OrcaSlicer / SuperSlicer
  `.ini` (key = value lines)." `parseIni()` skips blank lines, lines starting with `#` or
  `;` (comments), and lines starting with `[` (section headers, e.g. `[print:0.20mm
  QUALITY]`). **Section headers are recognized only well enough to be skipped — they are not
  tracked.** The first `=` in a non-comment, non-section line splits key from value
  (`indexOf('=')`, both sides trimmed). Consequence: identically-named keys under different
  sections in the same file (e.g. a `[print:...]` and a `[filament:...]` block both setting a
  key) silently collide, and whichever appears later in the file wins. No inheritance
  handling exists at all — see below.
- **`.json`** — Cura. `parseJson()` + `walk()` handle two shapes: a flat `{ key: value }`
  object, or the nested Cura profile shape `{ settings: { key: { default_value: ... } } }`
  (falls back to `overrides` before falling back to treating the whole object as the
  settings map: `obj.settings ?? obj.overrides ?? obj`). `walk()` recurses into any nested
  object; for a leaf object it prefers `default_value`, then `value`, else recurses deeper.
  Malformed JSON is caught and yields an empty map rather than throwing.

**Inheritance between profiles: none was implemented.** There is no `inherits` key handling,
no parent-profile lookup, no merge of a parent's settings with a child's overrides — despite
`inherits = <parent name>` being a real, common feature of PrusaSlicer/OrcaSlicer `.ini`
profile bundles. Each file `parseProfile()` sees is parsed in complete isolation. This is a
genuine gap, not a subtlety worth glossing over: a Rust parser that ignores both `[section]`
boundaries and `inherits` chains will misattribute or drop settings on any real multi-profile
slicer export.

**The curated key map** (`KEY_MAP` in `profileImport.service.ts`) — every canonical label and
every source key it actually recognizes, verbatim, tried in the listed order with the first
present non-empty value winning:

| Label | Source keys (first match wins) |
|---|---|
| Layer height | `layer_height` |
| First layer height | `first_layer_height`, `initial_layer_height` |
| Infill | `fill_density`, `infill_sparse_density` |
| Infill pattern | `fill_pattern`, `infill_pattern` |
| Perimeters | `perimeters`, `wall_loops`, `wall_line_count` |
| Nozzle temp | `temperature`, `nozzle_temperature`, `material_print_temperature` |
| Bed temp | `bed_temperature`, `material_bed_temperature` |
| Supports | `support_material`, `support_enable`, `enable_support` |
| Nozzle | `nozzle_diameter`, `machine_nozzle_size` |
| Material | `filament_type`, `material` |
| Speed | `perimeter_speed`, `speed_print`, `speed_wall_0` |

This table is how PrusaSlicer/OrcaSlicer/SuperSlicer's `.ini` vocabulary and Cura's `.json`
vocabulary get reconciled into one set of labels — there's no per-slicer branch, just this
shared candidate list evaluated against whatever `raw` map the format-specific parser
produced.

**Fallback edge case:** if none of the curated keys matched anything in the file, the code
doesn't return an empty result — it surfaces the first 6 raw key/value pairs found
(`Object.entries(raw).slice(0, 6)`), with the comment "Nothing matched our curated keys —
surface the first handful so the import is visible." An exotic or newer slicer's profile
still produces *something* in the UI rather than a blank import.

**Value-formatting edge cases** (`formatValue`): a bare-numeric "Infill" value gets a `%`
suffix appended — `fill_density`/`infill_sparse_density` are stored as raw numbers in the
source files, not pre-formatted percentages. A "Supports" value matching `0`/`1`/`true`/
`false` (case-insensitive) becomes the human label "Enabled" or, deliberately, **"None
needed" rather than "Disabled"** — a softer phrasing choice worth preserving if the UI voice
carries forward. Every other label passes its raw value through unchanged.

**Persistence and the manual/imported boundary:** `printerSettings.service.ts`'s
`replaceSettings(modelId, rows, source, profilePath)` — called from the route as
`replaceSettings(id, parsed.rows, 'profile', profilePath)` — deletes only the existing rows
for that model **scoped to the same `source` value**, then re-inserts the new rows with a
fresh `ord` sequence and `profilePath` recorded per row. So `source` (`'manual'` vs.
`'profile'`) is the actual isolation boundary: re-importing a profile clears only
previously-imported rows, never rows a person typed in by hand. `raw` (every key found,
curated or not) is returned from `parseProfile` but the route only persists `rows` (the
curated/fallback subset) — the rest of `raw` is discarded after the request, not retained
anywhere.

**`printerType.service.ts` is unrelated to parsing** — twelve lines, a flat `printer_types`
name list (`INSERT OR IGNORE`) used for compatibility toggles. Its only relevance here is
confirming the data model kept "which printers this part fits" (`printer_types` /
`model_printer_types`) and "what settings to print it with" (`printer_settings`) as two
separate, uncoupled tables — a profile import never touched printer-type compatibility.

**Keep:** the key-map table above as the seed for `lapidary-targets`'s equivalent — it's
already done the cross-slicer vocabulary reconciliation for the handful of settings a human
actually wants to see at a glance. The `source`-scoped replace (manual settings survive a
profile re-import) is also a sound persistence pattern to keep. **Discard/rebuild:**
section-blind `.ini` parsing and the total absence of `inherits` resolution — both need real
design work before Phase 1 can claim to parse a real multi-profile slicer bundle correctly,
not a port of what's here.

## Explicitly not carried forward

- **`cache.service.ts`** — Redis. The cache and the job queue are both Postgres.
- **Per-model procedural sample shapes** in `seed.ts`. Phase 1 seeds one real
  licence-clean example part.
- **`npm run dev` as the primary run path.** `podman compose up` is the entry point.
