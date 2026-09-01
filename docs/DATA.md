# Data architecture

Container-first. Supersedes any earlier draft describing embedded Postgres, Tauri
sidecar bundling, or Windows desktop concerns.

---

## 1. Storage

### 1.1 Three classes, three lifecycles

Treating these the same is the most common way this kind of app becomes slow and fat.

| Class | Examples | Size | Re-derivable? | Access |
|---|---|---|---|---|
| **Source** | STEP, STL, 3MF, OBJ, drawing PDF | 1 MB – 2 GB | **Never** | Cold |
| **Derivative** | glTF LODs, structure, entities | 100 KB – 200 MB | Yes, deterministically | Warm |
| **Preview** | thumbnails | 5 – 60 KB | Yes, cheaply | **Hot** |

```
/var/lib/lapidary/          (named volume)
  blobs/ab/cd/abcdef01…     content-addressed, 2-level hex sharding
  workspace/                agent checkout dir (agent host only)
  quarantine/               ref_count=0, 30-day hold before removal
```

Two-level sharding gives 65,536 buckets, keeping any directory under ~2k entries at a
million blobs. **BLAKE3**, not SHA-256 — ingest is hash-bound before it is anything else.
Blobs never live in Postgres, with one deliberate exception (§1.5).

### 1.2 Compression — per-role first, per-age second

A pure age rule slows the app down: it compresses the derivatives on the hot open path
for ~2% space and an added decode stage.

| Role | Policy |
|---|---|
| STEP, IGES, ASCII STL | zstd -3 at ingest → -19 when cold. 6–10× on STEP |
| Binary STL, OBJ | zstd -3 → -19 when cold. ~2–2.5× |
| 3MF | **as-is** — already a deflate ZIP |
| PDF, PNG, JPEG | **as-is** |
| glTF derivatives | **never** — already meshopt-packed, hot path |
| Thumbnails | **never** |

**Zstd dictionaries are the real win.** A library is hundreds of structurally similar
STEP files sharing headers and unit definitions. Train per library:

```
zstd --train sample/*.step -o lib-{id}.dict --maxdict=112640
```

Expect 20–40% better ratio than dictionary-less zstd on small-to-medium STEP. Store
`dict_id` on the blob row. **Dictionaries are immutable content-addressed artifacts** —
losing or mutating one makes blobs unreadable. Always write with `--content-size` so the
decompressor allocates once.

### 1.3 Tiering job

```
Nightly, only when idle > 10 min:
  SELECT blake3 FROM blob
   WHERE last_accessed_at < now() - interval '30 days'
     AND zstd_level = 3 AND role = 'source'
   ORDER BY size_bytes DESC LIMIT 500
  → recompress at -19 (--ultra -22 above 100 MB)
  → temp file, fsync, atomic rename, then update row
```

Idempotent, resumable, abortable, rate-limited to one core. **The rename is the commit
point** — a read arriving mid-recompression gets the old blob. Never blocks a read.

### 1.4 Access tracking

`UPDATE blob SET last_accessed_at = now()` per read turns a read-mostly workload into a
write-heavy one and generates a dead tuple per read. A grid scroll touching 300
thumbnails would be 300 writes for zero value.

Keep an in-memory `HashMap<Blake3, Instant>` in the API process, flush every 5 minutes in
one batched `UPDATE … FROM (VALUES …)`, and only write rows more than a day stale. Day
precision is plenty for a 30-day rule.

### 1.5 Eviction beats compression — for derivatives only

- **Source blobs:** compress hard, **never delete**. `ref_count` guards removal.
- **Derivative blobs:** never compress, **evict freely**. `kernel_version` +
  `params_json` make regeneration deterministic.

A "clear render cache" action dropping derivatives for parts untouched in 90 days
recovers far more than compression would. **UI wording is a product rule:** it must say
render cache, show what it reclaims, and state that source files are untouched. A user
who reads "reclaim 40 GB" and fears for their models has lost trust in the thing we sell.

**Thumbnails are the exception to "no blobs in Postgres."** Store WebP under 64 KB as
`bytea` on the derivative row so they arrive in the same query as the grid page instead
of costing 100 filesystem round trips per scroll. This is worth more to perceived speed
than any compression decision above.

### 1.6 Deletion is three steps

1. **Delete** — sets `deleted_at`, hides the part. Nothing touches disk. Reversible
   indefinitely.
2. **Purge** — separate, explicitly worded action. Decrements `ref_count`.
3. **Quarantine** — `ref_count = 0` moves the blob to `quarantine/` for 30 days.
   Reachable by hash, invisible in UI, restorable. Only then removed.

---

## 2. Fast open

> **The open path never touches a source file and never invokes the CAD kernel.**

### 2.1 LOD ladder — all generated at ingest

| LOD | Triangles | Used for |
|---|---|---|
| `thumb` | — | 512px WebP, grid card |
| `L0` | ~5 k | instant viewer paint, hover preview |
| `L1` | ~50 k | default inspector |
| `L2` | full | measurement, zoom |

Deriving lazily on first open is the tempting optimization that makes first open slow,
which is the impression that sticks.

### 2.2 meshopt, not Draco

Draco compresses ~15–20% smaller; **meshopt decodes roughly an order of magnitude
faster** and is SIMD-friendly. Decode latency dominates for visual triage. Default
`EXT_meshopt_compression` everywhere; encode Draco additionally only for the cloud tier
where egress bytes cost money.

### 2.3 Immutable caching — free, from content addressing

`/api/blob/{blake3}` can never return different bytes:

```
Cache-Control: public, max-age=31536000, immutable
ETag: "{blake3}"
Accept-Ranges: bytes
```

Repeat opens hit the browser cache with no round trip and no invalidation logic.

**But content addressing is not authorization.** Every blob request must verify the
principal has access to a part referencing that blob in their tenant. Hashes leak into
manifests, logs, bundles and support tickets — knowing one must never be a capability.

### 2.4 Prefetch on intent

Hover a grid card → prefetch `L0`. Open the inspector → prefetch `L1` for the next and
previous parts in sort order. Bound the pool at 2 concurrent and cancel on navigate, or
fast scrolling saturates the queue with parts already passed.

### 2.5 Targets — treat as regression tests

| Operation | Warm | Cold |
|---|---|---|
| Grid page of 100 | < 80 ms | < 250 ms |
| Part open → first paint | < 120 ms | < 400 ms |
| Part open → L2 | < 600 ms | < 1.5 s |
| Search, 100 k parts | < 150 ms | — |
| Facet counts, 100 k parts | < 300 ms | — |

---

## 3. Metadata and search

### 3.1 Extraction stages — each commits independently

1. **Identity** — BLAKE3, size, format sniff, filename. Always succeeds. **A known hash
   short-circuits the entire pipeline** and the part appears instantly. This is what
   makes re-import feel free.
2. **Structural** — assembly tree, instances, transforms (`structure.json`)
3. **Geometric** — bbox, volume, area, centre of mass, inertia, triangle count,
   watertightness, units
4. **Semantic** — material, author, originating CAD system, PMI/GD&T, STEP header
5. **Derived** — thumbnails, LODs, embedding vector (later)

A stage-4 failure still leaves a usable, searchable part.

### 3.2 Columns vs JSONB — one rule

> If it appears in a filter, a sort, or a facet, it is a typed column with an index.
> Everything else is JSONB.

`ORDER BY (metadata->>'volume')::float` is unindexable in practice and becomes a seq scan
at ~50k rows.

```sql
part(
  id uuid PRIMARY KEY,                  -- uuid v7
  library_id uuid NOT NULL,
  part_number text, name text NOT NULL,
  classification text,
  created_at timestamptz, created_by uuid,
  deleted_at timestamptz,               -- soft delete
  metadata_json jsonb DEFAULT '{}',
  search tsvector GENERATED ALWAYS AS (...) STORED   -- STORED IS MANDATORY (PG18)
);

revision(
  id uuid PRIMARY KEY, part_id uuid NOT NULL,
  rev_label text NOT NULL, parent_revision_id uuid,
  origin text NOT NULL,                 -- ingest|external_edit|import|assembly_promote
  author uuid, message text, created_at timestamptz,
  lifecycle_state text,                 -- in_work|in_review|released|obsolete
  locked_by uuid, locked_at timestamptz,
  volume double precision, surface_area double precision,
  bbox_x double precision, bbox_y double precision, bbox_z double precision,
  triangle_count integer, is_watertight boolean, units text,
  mass_props_json jsonb
);

file(id, revision_id, role, format, blake3, size_bytes, created_at);

blob(
  blake3 text PRIMARY KEY,
  size_bytes bigint, stored_bytes bigint,   -- show real disk usage
  zstd_level smallint, dict_id uuid,
  ref_count integer NOT NULL DEFAULT 0,
  quarantined_at timestamptz,
  last_accessed_at timestamptz, created_at timestamptz
);

derivative(
  id uuid PRIMARY KEY, revision_id uuid,
  kind text,                            -- tessellation_l0|l1|l2|structure|entities|thumbnail
  blake3 text, thumb_bytes bytea,       -- inline if < 64 KB
  kernel_version text, params_json jsonb, created_at timestamptz
);

part_source(
  id uuid PRIMARY KEY, part_id uuid NOT NULL,
  url text, vendor text, external_id text, title text,
  license text,                         -- CC-BY, CC-BY-NC-SA, proprietary…
  price_minor bigint, currency text, retrieved_at timestamptz,
  UNIQUE (part_id, url)
);

part_image(
  id uuid PRIMARY KEY, part_id uuid NOT NULL,
  blake3 text,                          -- always cached locally, never hotlinked
  origin text NOT NULL,                 -- uploaded|url_supplied|og_fetched|rendered
  source_url text, is_primary boolean, created_at timestamptz
);
```

`part_source.license` is not bureaucracy — half of hobbyist STL libraries are
non-commercial, and a user selling prints needs to see that on the card. Nobody else
surfaces it.

### 3.3 Search — identifiers and prose need different indexes

`to_tsvector('english', 'A1234-56-B')` mangles identifiers; searching `1234` will not
find it. This bites every parts application eventually.

```sql
CREATE INDEX part_search_gin  ON part USING gin(search);
CREATE EXTENSION pg_trgm;
CREATE INDEX part_number_trgm ON part USING gin(part_number gin_trgm_ops);
CREATE INDEX part_name_trgm   ON part USING gin(name gin_trgm_ops);
```

Run both, union, and rank trigram similarity above text rank when the query looks like an
identifier (contains a digit and a separator). A user typing a part number wants an
exact-ish hit at position one, always.

Turkish: `tsvector` config is fixed at index time, so put a `language` column on
`library` rather than using a global setting.

### 3.4 Facets

Under 10k matching rows: exact counts in one query with `FILTER (WHERE …)` aggregates.
Above that: a rollup table refreshed on ingest, or drop counts and show only which values
are non-empty. Users tolerate missing counts; they do not tolerate a 900 ms filter panel.

### 3.5 Custom fields

`custom_field(id, library_id, key, label, type, options_json, indexed bool)` with values
in `part.metadata_json`. When `indexed`, create a matching expression index. **Cap
indexed custom fields at 8** — each is a write cost on every ingest.

---

## 4. Source links and images

Three paths, build in this order:

1. **User uploads a file.** Always works.
2. **User pastes an image URL.** Fetch once, store as a blob. Never hotlink — hosts
   rotate URLs and hotlinking leaks a referrer on every grid scroll.
3. **User pastes a product page URL** → offer a "Fetch preview" button that reads
   OpenGraph tags. One request, explicit user action.

**Do not build a scraper.** Systematic harvesting breaks on every redesign and violates
GrabCAD and TraceParts ToS specifically.

### 4.1 Fetching a user-supplied URL is SSRF

Mandatory controls:

- Resolve DNS **first**, check the resolved IP against RFC1918, loopback, link-local
  (**`169.254.169.254` especially**), IPv6 ULA and mapped-v4. **Re-check after every
  redirect** — DNS rebinding.
- `http`/`https` only. Max 3 redirects, 10 s timeout, 10 MB cap enforced while streaming.
- Validate content-type **and** magic bytes.
- Decode with explicit `image::Limits` — a 200 KB PNG can declare 50000×50000 and take
  the process out.
- Re-encode to WebP at bounded size. This normalizes, mitigates bombs, and strips EXIF
  (downloaded images carry GPS coordinates surprisingly often) in one step.

---

## 5. Upload and download

We are storage. This changes what we are liable for.

### 5.1 Download — never silently convert

```
GET /api/revisions/{id}/download?variant=original  → bracket_v3.step
GET /api/revisions/{id}/download?variant=3mf       → bracket_v3.lapidary.3mf
```

`original` returns byte-identical ingested bytes, verifiable against the stored BLAKE3 —
**show the hash next to the button** so a user can check it themselves. Anything we
produced carries the `.lapidary.` infix.

`Content-Disposition` must use RFC 5987 (`filename*=UTF-8''…`). Turkish part names
contain ğ, ş, ı and a naive `filename=` mangles or breaks the download.

Download is just a `Target` whose `accepts()` the user picks manually — so send-to-app
degrades to download naturally when no agent is present.

### 5.2 Upload — hash first, client-side

Compute BLAKE3 in WASM **before** uploading, then probe:

```
POST /api/uploads/probe { hashes: [...] } → { have: [...], need: [...] }
```

Drag in 500 files where 480 are known and only 20 transfer. Re-importing a library
completes in seconds.

**Resumable chunked upload is mandatory.** A 2 GB STEP over a corporate VPN as one POST
will fail, and it will fail at 90%. Chunk-with-offset or tus. The server assembles and
**verifies the assembled BLAKE3 against the client's claim** before committing — never
trust the client hash, it is the dedup key and a wrong one silently corrupts another
user's part.

Folder upload: `webkitdirectory`, and `DataTransferItem.webkitGetAsEntry()` for drag-drop.

### 5.3 Bundles

Streaming ZIP via `async-zip`, never buffered server-side. **STORE, not DEFLATE** —
contents are already compressed, so deflating again burns CPU for nothing. Include
`manifest.json` with part numbers, revisions, hashes and source licences, which makes the
bundle verifiable rather than a folder of mystery files.

### 5.4 Archive security

- **3MF is a ZIP → zip-bomb vector.** Cap decompressed size, entry count and compression
  ratio during extraction. Abort on breach, not after.
- **Reject path traversal** in entries — absolute paths and `..` segments write outside
  the extraction dir. Old, still exploited.

---

## 6. Versioning

What transfers from Git: immutable history, content addressing, lineage, "what changed",
authorship, tags, dedup. What does not: three-way merge (undefined for a B-rep solid),
textual diff, branches, rebase.

So: **immutable snapshots + lineage DAG + pessimistic locks.**

### 6.1 Geometric diff replaces textual diff

Between two revisions: Δ volume (absolute and %), Δ bbox per axis, Δ mass and centre of
mass, Δ triangle/face/edge count, plus a visual overlay (old as grey ghost, new solid).

Per-face Hausdorff heatmap is the premium version — expensive, async job, cached result.
Never in the synchronous diff path.

### 6.2 External round-trip (agent binary)

Checkout to `workspace/{part_number}_{rev}/PartName.step` — flat, human-readable, because
users open this folder in a file manager.

Watcher rules, none optional:

- Debounce 500 ms, then **wait for write-settle**: size and mtime stable for 2 s.
- Ignore `.bak`, `.tmp`, `~$*`, `.lck`, `*.autosave`, `.DS_Store`, `Thumbs.db`, `.3dm.bak`.
- **Hash before believing anything.** Editors touch mtime constantly without changing
  content. Identical bytes are not a revision.
- Windows `ReadDirectoryChangesW` has a fixed kernel buffer that overflows during bulk
  operations and silently drops events — handle the overflow signal with a full rescan.
- macOS FSEvents coalesces at directory granularity by default; enable file-level events
  and expect duplicates.

**inotify does not propagate through Docker Desktop bind mounts on macOS or Windows.**
This is why the watcher lives in the native agent binary and not in a container.

### 6.3 Which tools round-trip — be honest in the UI

| Tool | Round-trip | Why |
|---|---|---|
| Rhino, FreeCAD, Blender, SolidWorks (local) | yes | writes the file we handed it |
| Orca / Bambu / Cura / PrusaSlicer | yes (project files) | 3MF saves back |
| Fusion 360 | **no** | cloud-backed; import is a copy |
| Onshape | **no** | browser, no local file |

For cloud tools the honest flow is export-and-reimport, and the UI must say so.
Overpromising here destroys trust in version history, which is the feature everything
else hangs on.
