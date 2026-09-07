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
<storage-root>/                        LAPIDARY_STORAGE_ROOT; a host directory, bind-mounted
  libraries/default/
    Terrain/Rocks/cliff/               one directory per model, named after the part
      cliff.stl                        the source, under its original filename
      metadata.json                    part + revision facts, human-readable
  blobs/ab/cd/<blake3>                 derivatives only, content-addressed, evictable
```

That is the whole of it, and it is what the code writes: `SourceStore::put_at` for the
model directories, `blob_path` — `root.join("blobs")` — for the cache. Two things a
reader might expect are deliberately absent. **Thumbnails are not files**: they live in
Postgres as `bytea` on the derivative row (§1.5), so no `images/` directory is written.
**There is no config file in the store**: nothing in the workspace reads or writes a
`lapidary.toml`, and configuration is environment variables the container gets
(`deploy/.env.example` names them). The root itself is not chosen in a first-run dialog
either — it is `LAPIDARY_STORAGE_ROOT`, a bind mount, and defaults to `deploy/../storage`.

**Sources are path-addressed now, not content-addressed.** Each ingested model gets its
own directory named after the part, under a folder tree mirroring the ingest directory's
own nesting — `Terrain/Rocks/cliff/` for a file ingested at `Terrain/Rocks/cliff.stl`.
This reverses the rule that stood here: every source used to land at
`blobs/ab/cd/<hash>` so identical bytes anywhere in a library shared one file — the same
directory the derivative cache still writes to, which is why `migrate_storage` moves
sources out of it and leaves the derivatives where they are. The owner wants the
opposite — a folder a user can open in a file manager and find their model by name, not
by hash.

**The cost, stated plainly: source deduplication is gone.** Two identical files ingested
at two source paths are now two files on disk. That is the price of a browsable store,
and it is not recoverable by cleverness — a store cannot be both one file per model and
one file per distinct content. **`CLAUDE.md`'s "Hash first, always" still holds:** BLAKE3
is still computed before anything else in ingest, and a known `(library_id, source_path,
blake3)` still short-circuits a re-scan of the same path. Only the filename the bytes are
written under changed; which paths dedupe against which did not.

**A category's directory name is allocated once, when the category is created.** Renaming
a category changes the row's `name` and nothing on disk — the decision, taken 2026-09-07:
a rename is a correction to a label, and rewriting every file under `WIP` because somebody
renamed it `Archive 2024` is not what that person asked for. So `folder.slug` is the
category's *address* and `folder.name` is its *label*, they agree at creation, and they
stop agreeing at the first rename. `PgFolders::rename` cannot write a slug — the parameter
is not in the signature — and the one job that legitimately repoints a category at another
directory says so by calling `reslug` instead.

The alternative is worse than a stale directory name, which is why this is a rule rather
than a deferral. Nothing rewrites `file.storage_path`, so parts already ingested stay where
they are whatever the row says; if the slug followed the name, `slug_path` would start
answering the new directory and the next ingest or move into that *same* category would
land there — one category, two directories, and another with every further rename. A store
you can browse must have one directory per category more than it must have a current name
on it.

What follows from it: a scan matches a directory to a category **by slug, not by name**
(`get_or_create`), so re-scanning a renamed library finds the category rather than forking
a second one under the old name; and `slugTaken` can now refuse a name because a sibling
*renamed away from it* still holds the directory, which is why that message names the
directory instead of explaining which of the two cases happened.

Re-parenting a category splits it the same way one level up, because `slug_path` joins
*ancestor* slugs and this folder's own slug staying put says nothing about theirs. Only
`PATCH /api/folders/{id}` with `parent_id` reaches it, no client sends it, and closing it
needs the stored per-folder directory path this rule made unnecessary for renames.

**Content addressing survives for `blobs/` only** — derivatives, which are evictable and
rebuildable, never the source of truth. Two-level hex sharding gives 65,536 buckets,
keeping any cache directory under ~2k entries at a million derivatives, still keyed on
**BLAKE3**, not SHA-256. Blobs never live in Postgres, with one deliberate exception
(§1.5).

**What the flat layout costs, recorded rather than argued away.** The design for this
store called the cache `cache/blobs/`, and hung a product rule on the name: CLAUDE.md's
"cache eviction must never read as data loss" is self-evident to anyone who opens a
directory literally called `cache/`. What shipped is `blobs/` as a sibling of
`libraries/` — never-deleted user data and a freely evictable cache, side by side at the
top of the very directory this layout exists to make users open, with nothing on disk
telling them which is which. This document describes `blobs/` because that is what
`blob_path` writes; the gap is real and is not closed by documenting it. Closing it is a
follow-up — a change to `blob_path` plus a relocation of any existing store, with the
same "copy it across first" problem as the upgrade below — and until then the distinction
between the two lives only in UI wording (§1.5), never on disk.

#### Upgrading a store created before this layout

Deployments from before this change kept everything in a named volume, `lapidary-blobs`,
mounted at `/var/lib/lapidary`. The storage root is now a bind-mounted host directory and
`deploy/compose.yaml` no longer declares that volume, so **`compose up` after the upgrade
starts api and worker on an empty directory while the old volume still holds the store.**
Nothing copies it for you. Do it before the first start:

```sh
# Confirm the volume's name first: compose prefixes it with the project name, so the
# default is `lapidary_lapidary-blobs`, and podman-compose has not always agreed.
podman volume ls
mkdir -p storage
podman run --rm --user 0 \
  -v lapidary_lapidary-blobs:/from:ro \
  -v "$PWD/storage":/to:z \
  docker.io/library/debian:trixie-slim \
  cp -a /from/. /to/
```

Run it from the repository root, or point `/to` at whatever `LAPIDARY_STORAGE_ROOT` names.
`docker volume ls` and `docker run` take the same arguments. The plain image tag is
deliberate and is not a lapse in "pin everything": this is a throwaway `cp` container that
never runs in the deployment, and copying `deploy/Containerfile`'s digest here would only
give it a second copy to rot.

`cp -a` preserves ownership, which matters because the container writes as uid 10001
(`lapidary`). The destination directory must be writable by that user too:
`podman unshare chown -R 10001:10001 storage` under rootless Podman, which maps that uid
into your subuid range, or `sudo chown -R 10001:10001 storage` under rootful Docker.

**Skip the copy and nothing fails at boot** — which is what makes this worth reading. The
grid renders normally, because thumbnails are `bytea` in Postgres (§1.5) and never came
from the store at all. The first symptom is a download answering 500: the row names a
source file that is in the volume, not in the new root. Two classes of bytes are stranded
and only one of them is cheap — the derivative cache would rebuild itself, but a source
blob still sitting at `blobs/ab/cd/<hash>` because `migrate_storage` has not moved it yet
is the only copy those bytes have. That job is enqueued at every worker boot for any
library still holding sources in the old layout, so it also loops: it reads from the empty
root, gets ENOENT, records the failure as transient, and re-enqueues.

Nothing is deleted by the upgrade, so this is recoverable after the fact as well as
before: the volume stays until an operator removes it. Stop the stack, run the copy, start
it again, and the pending migration finishes on that boot.

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
2. **Purge** — separate, explicitly worded action, and refused on a part that has not been
   deleted first. Removes the part chain and **recomputes** each affected blob's
   `ref_count` from what actually points at it.
3. **Quarantine** — a blob whose recomputed count reaches zero gets `quarantined_at` set,
   and is left where it is for 30 days. Reachable by hash, invisible in the UI, restorable.
   Only then removed. A purged part's **model directory** is held the same way and for the
   same 30 days, by path rather than by hash — see *Two quarantines, one timer* below.

**Amended by slice 7, and two of these lines changed.**

*Purge recomputes rather than decrementing.* A counter maintained only by increments and
decrements drifts, and the drift is invisible until something acts on it — the thing that
acts on it deletes bytes. Recomputing makes any accumulated drift self-heal the moment a
purge touches that blob, and it is the reason a reference count the project has never
fully audited is still safe to ship.

*Quarantine is a timestamp, not a `quarantine/` tree.* The bytes do not move. Restoring
then needs no path rewrite, no reader needs a second lookup location on the hot path to
serve a case that is meant to be rare, and the store's layout has a pending change (the
folder-tree work) that a second tree built now would be built against.

*What actually keeps a referenced blob safe is neither the counter nor the reaper's own
`WHERE` clause.* It is `file.blake3` and `derivative.blake3`, both foreign keys to `blob`:
deleting a referenced row raises a constraint violation whatever any query says. The
reaper's reachability check is what lets it *decline* such a row instead of failing the
whole sweep on it.

*What the panel does not say.* A purged blob has no `file` row, no revision, no part and
therefore no library, so quarantined bytes cannot be attributed to a library's storage
figure — the number is library-less by construction. `LibraryStorage.removed_bytes` covers
the deleted-but-not-purged case, which is attributable; the quarantined figure belongs to
an instance-wide view arriving with Phase 4's tiering job.

Keep the three apart in every string that reaches a user. **Delete** hides a part and
leaves the bytes. **Purge** is a second, explicit action against data the user has already
said they are done with. **Cache eviction** (§1.5) is neither — it drops derivatives the
app can rebuild and never touches a source file. Wording that lets any one of them read as
another is a defect, not a nicety.

#### Two quarantines, one timer

The 30 days above were written for a content-addressed store, where one blob could be the
last reference several parts shared and a hold protected all of them at once. One file per
model is a different question — nobody is sharing that file — and this document used to
leave it open. Migration `0014` answers it: **the same 30 days, one constant**
(`lapidary_ingest::reap::QUARANTINE`) for both. A second retention would be a second
promise to explain inside the same confirmation dialog, and nothing has argued the two
should differ.

They are two quarantines because they are keyed on different things, and neither subsumes
the other. `blob.quarantined_at` is per hash and enters when a recomputed `ref_count`
reaches zero; `quarantined_file` is per store-relative path and enters when the one part
that owned that file is purged. Source dedup is gone (§1.1), so one hash can be several
model files with several independent lifetimes — which is exactly why the path cannot live
on the `blob` row.

One sweep, one transaction, one cutoff. What differs is the guard: a referenced blob is
protected by `file.blake3`'s foreign key whatever any query says, and the reaper's
reachability check merely lets it *decline*; nothing references `file.storage_path`, so
there the check is the only protection and is written as such.

*"Reachable by hash" is true of a quarantined blob and false of a quarantined file.* Its
bytes sit at a path no route serves once the `file` row naming them is gone. Both are
invisible in the UI; neither has a restore yet.

*Removing a model file removes what is around it.* The file, the `metadata.json` beside it,
and the directory that held them — but only if nothing else is in it. A directory the
owner has put something of their own into keeps that something, and keeps standing; the
sweep logs it and carries on rather than failing, because failing would stop every other
removal for as long as their file sat there.

---

## 2. Fast open

> **The open path never touches a source file and never invokes the CAD kernel.**

### 2.1 LOD ladder — L0 at ingest, L1 and L2 on demand

| LOD | Triangles | Used for | Built |
|---|---|---|---|
| `thumb` | — | 512px WebP, grid card | At ingest, unless the library sets `auto_thumbnail = false`; `POST /api/libraries/{id}/thumbnails` fills the rest |
| `L0` | ~5 k | instant viewer paint, hover preview | At ingest, always |
| `L1` | ~50 k | default inspector | On demand, the first time something asks for it |
| `L2` | full | measurement, zoom | On demand, the first time something asks for it |

**This reverses the rule that stood here — "all generated at ingest", on the grounds that
deriving lazily on first open is the tempting optimization that makes first open slow,
which is the impression that sticks.** That reasoning was not wrong when it was written.
Two things changed it, and neither was available at the time.

The first is a measurement. Slice 3's exit run built the whole ladder over a 150-file
corpus and recorded what each rung cost: **L0 7.3 MB, L1 41 MB, L2 52 MB**. Ninety-three
per cent of the bytes sit in the two rungs the grid never opens. Slice 4's exit run over
the same corpus writes **7.5 MB** — the same L0 and nothing else, a **92.5%** drop.

The second is the constraint that number runs into. Not every computer has this kind of
free memory: the stack is built for a modest workstation, and `deploy/compose.yaml` says
so in ceilings — a 2 GiB worker, a 1 GiB database, a 512 MB api. Spending 93 MB per 150
parts on rungs nothing has asked for is not a latency decision at that ratio; it is a
decision to spend somebody else's disk on a refinement that may never be opened.

**What the old rule protected is still protected, by the same mechanism.** L0 is still
built at ingest, so the open path still has bytes to paint without touching a source file
or the kernel. Only the refine step behind it is deferred. If that step ever reads as
slow, the answer is a background sweep after ingest — the machinery slice 4 built for
thumbnails — not a return to building three rungs per part on the chance one is opened.

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

**Built in slice 5, with three things this section did not specify.** `variant=original`
is the only legal value today — `variant=3mf` is a 400 naming what to send, never a
silent fallback, because a download that quietly returns something other than what was
asked for is the failure this section exists to forbid. The route **re-hashes the bytes
before serving them** and refuses with a 500 on mismatch, which is what makes "verifiable
against the stored BLAKE3" true rather than asserted; it costs microseconds against the
transfer that follows. And `Cache-Control: no-cache` — revalidate before reuse. The blob
route can promise `immutable` because its URL contains the hash of what it returns; this
URL names a *revision*, whose source could be re-pointed, and sending no directive at all
would leave heuristic freshness free to hand back a stale file.

Measured on the 150-file corpus: the served bytes `cmp` clean against the file on disk,
and a name carrying parentheses arrives as `filename*=UTF-8''…tex%28B%29.stl` with an
ASCII fallback beside it.

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
