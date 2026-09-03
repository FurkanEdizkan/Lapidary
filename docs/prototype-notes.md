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

`assetPipeline.service.ts`'s `ingestMesh(modelId, originalName, buffer)` is the ingest
primitive, called from **two** places: `libraryScan.service.ts:62` (bulk folder scan, one call
per file) and `routes/api.ts:93` (the multipart branch of `POST /api/models`, the direct upload
route). Confirmed with `git grep -n ingestMesh origin/main -- server/src`, which returns exactly
these two call sites plus the definition and the import in each caller — no others. The two
callers share the primitive but diverge afterward in ways covered below. `ingestMesh` itself ran
two stages, strictly in order, with a third tier deferred entirely:

1. **Tier 3 (original), always first, unguarded.** `compress()` picks `zstd` when the
   running Node's `zlib` exposes `zstdCompressSync`, else falls back to `gzip`, and writes
   the result to `${modelId}${baseExt}${ext}` (`baseExt` is the uploaded file's own extension,
   e.g. `.stl`; `ext` is the compression suffix, `.zst` or `.gz`) in `config.modelsDir` — e.g.
   an ingested `bracket.stl` becomes `u<nanoid>.stl.zst`. This write (`fs.writeFileSync(
   originalPath, data)`) has no try/catch around it: if it throws, `ingestMesh` throws
   immediately and no DB row is ever created.
2. **Tier 2 (LOD + metrics), via the external sidecar.** The raw buffer is written to a
   temp file in `lodDir` (`${modelId}.raw${baseExt}`), handed to `analyzeMesh()`
   (`meshSidecar.service.ts`), which shells out to the optional `rust-mesh` binary
   (`execFile`, 60s timeout, `--lod <path> --json`) and parses its JSON stdout for `bbox`
   and `triangles`. The temp raw file is deleted in a `finally` block regardless of outcome.
3. **Tier 1 (thumbnail) was explicitly out of scope for ingest** — the code comment says it
   is "left to be generated client-side and cached back." It arrived later via a separate
   route (`POST /api/models/:id/thumbnail` in `routes/api.ts`, calling `updateModel(id, {
   thumbnailPath })`), never through `ingestMesh`.

**The two callers diverge after `ingestMesh` returns, and the divergence is not cosmetic:**

- **`libraryScan.service.ts` (bulk scan):** `createModel()` inserts the row (without
  `triangle_count` — that column isn't in the INSERT list), and only afterward, if
  `ingest.triangleCount` is truthy, a follow-up raw `UPDATE models SET triangle_count = ?
  WHERE id = ?`. A fully-succeeded scan-path ingest is four sequential writes: blob, (optional)
  LOD file, INSERT, conditional raw `UPDATE`. This path wraps the whole thing (`ingestMesh` +
  `createModel` + the follow-up update) in a `try`/`catch` that increments `skipped` on any
  failure — see partial-failure note below.
- **`routes/api.ts`'s upload route (multipart `POST /api/models`):** calls `createModel(...)`
  (passing `ingest.triangleCount` nowhere in that call), then, only if `ingest.triangleCount` is
  truthy, `updateModel(id, { triangleCount: ingest.triangleCount })` — the same service-layer
  helper the PATCH route uses, not a raw `UPDATE`. Same two-write shape as the scan path (INSERT,
  conditional follow-up) but through a different mechanism. **There is no `try`/`catch` around
  any of this.** A `createModel` failure here both orphans the Tier-3 blob `ingestMesh` already
  wrote (the leak described below) and propagates as an unhandled rejection — a 500 to the
  client, not a caught-and-counted failure the way the scan path handles it.

**Dedup is a scan-path behavior, not a property of `ingestMesh` or of ingest as a whole.**
`scanFolder` dedups by filename stem before ever calling `ingestMesh` (see the library-scan
section below); the upload route has **no dedup of any kind** — every multipart POST mints a new
`modelId` and creates a new row unconditionally, so re-uploading the same file, or two
differently-folder'd files sharing a name, just produces two models. "The only reason a rescan
doesn't re-import everything is the filename-stem dedup living in the caller" is true of the
scan caller only; the upload caller has no such guard and the notes previously implied all
callers did.

**Measured-vs-typed dimensions are written to the same columns with no provenance marker — the
divergence that matters most.** `routes/api.ts`'s upload route:

```ts
size: ingest.size ?? [
  Number(fields.sx) || 0, Number(fields.sy) || 0, Number(fields.sz) || 0,
],
```

When the sidecar returns no bounding box (`ingest.size` is `null` — an ordinary outcome; see
partial failure below), the route falls back to `sx`/`sy`/`sz` multipart form fields —
**user-typed numbers** — and writes them into the exact same `bbox_x/y/z` columns a
sidecar-measured box would occupy. Nothing downstream (no column, no flag, no separate field)
can distinguish a measured value from a typed one after that write. `libraryScan.service.ts`'s
path has no equivalent fallback: its `size: ingest.size ?? [0, 0, 0]` degrades to a literal zero,
never a user-supplied value, so it cannot be mistaken for a measurement (a silent `[0, 0, 0]` is
its own, smaller, honesty problem).

This is a direct instance of Lapidary's rule violation, not a stylistic nit: **measurement must
not lie** — analytic values from B-rep entities where available, mesh-derived values labelled
"approximate", always. A hand-typed dimension is neither of those, and the schema currently has
no way to say so. **Discard, do not port:** this fallback must not reach Phase 1 in this shape.
Either persist a provenance tag alongside every dimension triple (`measured` / `approximate` /
`user-entered`) so the UI can render the distinction honestly, or refuse to let a user-typed
dimension land in the same columns as a measured one at all — a separate, clearly-labelled
optional field, never silently merged into `bbox_x/y/z`.

**Hash: there wasn't one.** Grepping `assetPipeline.service.ts`, `meshSidecar.service.ts`,
`libraryScan.service.ts` and `model.service.ts` for `hash|blake|sha256|checksum` returns
nothing. The prototype never computed a content hash anywhere in ingest — this is a direct
divergence from Lapidary's "hash first, always" rule, not a variant of it. Duplicate
detection instead happened one layer up, by filename, and only on the scan path (see the
library-scan section below) — the upload path had no duplicate detection at all.

**Idempotency / resumability: effectively none, by design accident rather than intent, for
either caller.** `modelId` is a freshly minted `nanoid` per call (`u${nanoid(10)}`, in both
`libraryScan.service.ts` and `routes/api.ts`), never derived from the file's content, so
`ingestMesh` has no way to recognize "I've already ingested this exact file." Re-running ingest
for the same bytes after a failure produces a new `modelId`, new blob paths, and (if it gets
that far) a new DB row. Nothing inside `ingestMesh` is resumable; the scan path's rescan-safety
comes entirely from the filename-stem dedup in `scanFolder`, and the upload path has no
rescan-safety at all.

**Partial failure:** Tier 2 degrades gracefully — `analyzeMesh` internally catches every
error (bad binary, timeout, malformed JSON) and returns `null` rather than throwing, so a
sidecar failure just yields `size: null`, `triangleCount: 0`, `lodPath: null` while Tier 3
stays intact. There is no equivalent guard around the handoff to the database on either path.
On the scan path: if `ingestMesh` succeeds (Tier 3 blob written, possibly Tier 2 too) but the
subsequent `createModel()` call throws — a DB constraint, a lock — `libraryScan.service.ts`'s
catch block only increments `skipped`; it never unlinks the blob it just wrote. On the upload
path, there is no catch block at all, so the same orphaned-blob leak happens *and* the request
500s instead of failing gracefully. Neither path had a compaction or GC job to reap orphaned
blobs.

**Keep:** graceful LOD-stage degradation (a sidecar failure shouldn't fail the whole
ingest) is worth preserving as a behavior, if the same shape holds in Rust. **Discard/fix:**
identity must come from the content hash, not a random id, so a retried or resumed ingest is
naturally recognized rather than accidentally re-run; blob writes need to be reconciled
with the DB write (staged blob + transactional commit, or an explicit reaper) so a DB
failure after a successful write can't leak storage silently — silent leaks are exactly what
"we never delete user data implicitly" was written against, but the inverse failure (never
*reclaiming* orphaned data) is just as much a problem to design out; and — emphatically — the
upload route's user-typed-dimensions-into-measured-columns fallback must not survive into
Phase 1 in any form, per the provenance discussion above.

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
`skipped` and never read. The `has`/`add` pair on `existingNames` is synchronous and runs to
completion — before the task's first `await` (`await ingestMesh(...)`) — so two files with the
same stem within one scan can't both import. But this means dedup is by filename stem alone,
not path, not content, not hash. **This is not a race in the concurrent-nondeterminism sense**:
because the check-and-claim on `existingNames` happens synchronously before any `await` yields
control, and JS is single-threaded and run-to-completion, the winner between two identically-
stemmed files in different subfolders is decided deterministically by `files`' iteration order —
which is `collect()`'s depth-first, *unsorted* `readdirSync` walk order — not by which
`pLimit`-scheduled task happens to finish first. The user-visible outcome (one file is silently
dropped) is real; describing it as racing under bounded concurrency overstates the mechanism —
serializing ingest (dropping the concurrency) would change nothing, since the walk order, not
the scheduling, decides the winner.

**Concurrency:** bounded to 3 concurrent ingest tasks via `p-limit`, with the comment "bounded
concurrency so a large library cannot exhaust memory." This reorders execution relative to
walk order — files are *discovered* depth-first but *ingested* out of order as `pLimit` slots
free up.

**Debounce / filesystem events: there was none to speak of.** Neither `libraryScan.service.ts`
nor its only caller contains `fs.watch`, `chokidar`, or any change-event handling — confirmed by
`git grep -nE "watch|debounce|chokidar" origin/main -- server/src`, which matches **nothing at
all**, not even inside this file despite its own name. There was no background watcher, no
polling loop, no scheduled rescan, and therefore no coalescing window to record: **scanning was
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
silently lose one depending on unsorted walk order; directory-walk errors (permission denied, symlink loops) should
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
anywhere, **despite there being a column reserved for exactly that.** `database.ts` defines
`printer_settings.raw_json TEXT`, and `profileImport.service.ts`'s own doc comment says "the
rest is kept in raw_json" — but `replaceSettings()`'s INSERT never populates `raw_json`, so the
column is always `NULL` at runtime. The doc comment describes intent that was never wired up;
the discard is real, the column and the comment are vestigial.

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
