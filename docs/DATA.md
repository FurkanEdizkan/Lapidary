# Data architecture

Container-first. Supersedes any earlier draft describing embedded Postgres, Tauri
sidecar bundling, or Windows desktop concerns.

---

## 1. Storage

### 1.1 Three classes, three lifecycles

Treating these the same is the most common way this kind of app becomes slow and fat.

| Class | Examples | Size | Re-derivable? | Access |
|---|---|---|---|---|
| **Source** | STL, 3MF, OBJ — and the STEP and IGES files an install held before they were refused | 1 MB – 2 GB | **Never** | Cold |
| **Derivative** | the two slicer exports, `export_stl` and `export_3mf` | 100 KB – 200 MB | Yes, deterministically | Warm |
| **Preview** | a part's own `part_image` pictures | 5 – 60 KB | **No** — somebody chose them | **Hot** |

**The middle row used to read "glTF LODs, structure, entities, PMI" and the bottom one
"thumbnails".** Nothing produces any of those (R3), and `0055` deleted their rows (R4). The
bottom row changing class is the part worth noticing: a preview is no longer something we
derive cheaply, it is a picture a person added, so it is **not re-derivable** and the
`ref_count` rules that protect a source protect it too. That is also the one reference class
the quarantine machinery does not yet know about — see R4's finding, repeated under §1.6.

```
<ingest mount>/                        LAPIDARY_INGEST_DIR: the owner's folders, mounted :ro
  Terrain/Rocks/cliff.stl              read where it is; nothing is ever written here
<inbox>/                               LAPIDARY_INBOX_DIR: Lapidary writes here, once
  uploads/<path the browser reported>  an upload; `name (2).stl` beside it, never over it
  imports/<site>/<creator>/<model>/…   a link import; a re-import may replace its own file
  imports/bundles/<source path>        an imported bundle's parts
  examples/…                           the bundled examples, copied in by the first start
<storage-root>/                        LAPIDARY_STORAGE_ROOT: Lapidary's own store
  blobs/ab/cd/<blake3>                 exports, pictures over 64 KB, imported bundles
  libraries/…                          files of parts indexed before M4, left where they are
```

**Your folders, read where they are (M4, owner, 2026-10-17: "use the directed file path of each
file or folder").** A scan records where each file is and reads it there. Lapidary never copies,
renames, moves, writes or deletes anything under the ingest mount — no `metadata.json` beside a
file, no `revisions/` directory, no model directory of its own. `LibraryReader` is the only handle
onto those folders and has no method that writes; the `:ro` mount is the backstop, and `cargo xtask
check-deploy` refuses a compose file that mounts them writable.

**Every `file` row names its root** (`0062`): `library` (the owner's folders, `storage_path` is the
library's own folder joined with its `source_path`), `inbox`, or `store` — every row from before M4,
whose bytes stay exactly where they were and still download, at a path under the store or, when
`storage_path` is NULL, at `blobs/ab/cd/<hash>` decoded at the level the row recorded. Nothing writes
a `store` source row any more, and nothing moves or deletes one.

**A library has a folder** (`library.root`, under the mount; `''` is the whole of it, which is what
every library scanned before M4). A part's `source_path` is relative to that folder and stays its
identity. **A category is the library's own grouping**: moving a part between categories, or renaming
one, is a database change and moves nothing on disk.

**A changed file is a new revision with its file where it is.** The owner overwrote the previous
bytes, and Lapidary keeps no copy: the earlier revision keeps its figures and its hash, is marked
`file.on_disk = false`, and its download answers 410 with a sentence. An earlier file at another place
— a pre-M4 store copy, an earlier upload under another inbox name — is still there and still
downloads.

**Byte-identical as long as the file has not changed since it was indexed.** `file.mtime_ns` and
`size_bytes` are what the file was when it was read; a download compares both before it sends a byte
and refuses a difference with "This file changed in your folder since Lapidary last read it — scan
again." (409). The end-of-stream BLAKE3 stays, for an edit that kept both. A re-scan that finds the
same bytes with a new time records the time, so a `touch` never leaves a file undownloadable.

**`CLAUDE.md`'s "Hash first, always" still holds:** BLAKE3 is computed before anything else in
ingest, and a known `(library_id, source_path, blake3)` still short-circuits a re-scan. Two identical
files at two paths are two parts sharing one `blob` row, and — since nothing is copied — one file
each, which is what the owner already had.

**A category's `slug` is allocated once, when it is created**, and a rename changes only its `name`.
Since M4 the slug names no directory; it keeps the per-parent uniqueness that `slugTaken` refuses on,
and a scan still matches a folder to a category by slug (`get_or_create`), so re-scanning a library
whose categories were renamed finds them rather than forking new ones. Re-parenting a category is
still refused (`400 cannotMove`); with no directory behind it that refusal is now only a decision
nobody has revisited.

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
grid renders normally, because a card's picture comes from `part_image` and never from the store's
model directories. The first symptom is a download answering 500: the row names a source file that is
in the volume, not in the new root. Since M4 nothing moves those files anywhere, so the copy is the
whole remedy: stop the stack, run it, start it again. Nothing is deleted by the upgrade, so this is
recoverable after the fact as well as before: the volume stays until an operator removes it.

### 1.2 Compression — per-role first, per-age second

A pure age rule slows the app down: it compresses the derivatives on the hot open path
for ~2% space and an added decode stage.

| Role | Policy |
|---|---|
| STEP, IGES, ASCII STL | zstd -3 at ingest → -19 when cold. 6–10× on STEP |
| Binary STL, OBJ | zstd -3 → -19 when cold. ~2–2.5× |
| 3MF | **as-is** — already a deflate ZIP |
| PDF, PNG, JPEG | **as-is** |
| The two slicer exports | **never** — a slicer is handed them on a download, so they are the hot path |
| Pictures (`part_image`) | **never** — already PNG/WebP, and the grid reads them on every page |

**Zstd dictionaries are the real win.** A library is hundreds of structurally similar
STEP files sharing headers and unit definitions. Train per library:

```
zstd --train sample/*.step -o lib-{id}.dict --maxdict=112640
```

Expect 20–40% better ratio than dictionary-less zstd on small-to-medium STEP. Store
`dict_id` on the blob row. **Dictionaries are immutable content-addressed artifacts** —
losing or mutating one makes blobs unreadable. Always write with `--content-size` so the
decompressor allocates once.

**Not for a file in a library folder.** Since slice 7 (`0009`, `0013`), a source file is written raw
into its model directory, so the owner can open it. The zstd rows above now describe only the legacy
`blobs/` copies. Dictionaries are retired together with the tiering job below (2026-09-15).

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

**Retired for source files, by owner decision, 2026-09-15.**
- **Why.** The job recompresses a source in place. A source file now lives raw in its model directory
  so that a file manager, a slicer and a re-scan all see the real file, and compressing it would break
  that.
- **What that leaves.** Nothing is tiered today. Space comes from freeing the slicer exports (§1.5)
  instead — which, now that the renders are gone, is the only cache there is.
- **When to revisit.** Only with a store that also keeps a raw copy the owner opens.

### 1.4 Access tracking

`UPDATE blob SET last_accessed_at = now()` per read turns a read-mostly workload into a
write-heavy one and generates a dead tuple per read. A grid scroll touching 300
thumbnails would be 300 writes for zero value.

Keep an in-memory `HashMap<Blake3, Instant>` in the API process, flush every 5 minutes in
one batched `UPDATE … FROM (VALUES …)`, and only write rows more than a day stale. Day
precision is plenty for a 30-day rule.

**Built** in the correctness-and-debt goal, 2026-09-15 (`lapidary_db::Touches`).
- **Recording.** A read records its hash and time in the api process's map.
- **The flush.** It runs every five minutes and when the server stops. One `UPDATE … FROM unnest($1::text[], $2::bigint[])` writes rows more than a day stale.
- **Why `unnest`.** Two typed arrays keep the statement static, where `VALUES` would be SQL built
  from data.
- **Tests** flush explicitly before they read the column.
- **What a crash costs.** Up to five minutes of access times. That moves a blob's place in an age
  ordering, and nothing else.

### 1.5 Eviction beats compression — for derivatives only

- **Source blobs:** compress hard, **never delete**. `ref_count` guards removal.
- **Derivative blobs:** never compress, **evict freely**. `kernel_version` makes
  regeneration deterministic.

**Since the pivot the derivatives are the two slicer exports and nothing else.**
`DerivativeKind` is `ExportStl` and `Export3mf` (R3), migration `0055` deleted every
`thumbnail`, `tessellation_l0/l1/l2`, `structure`, `entities` and `pmi` row (R4), and
nothing can rebuild one of those because nothing produces them. So this section is about a
much smaller table than it used to describe, and the rule it states is simpler: **the
`derivative` table holds only what something can still build.** `0055`'s predicate is
written `kind NOT IN ('export_stl', 'export_3mf')` so that it says that invariant rather
than listing seven kinds somebody has to keep in step.

**UI wording is still a product rule:** the eviction action must show what it reclaims and
state that source files are untouched. A user who reads "reclaim 40 GB" and fears for
their models has lost trust in the thing we sell. The strings say *slicer files* now,
because the three that said "detailed 3D previews", "opens at once on its coarse preview
while the detailed one is rebuilt" and "render cache" all described a rebuild nothing can
perform (R4). The wire names (`renderCacheBytes`, `POST /api/storage/render-cache`) are
unchanged.

**Built in Phase 4 slice 2** (`docs/superpowers/specs/2026-09-15-phase-4-slice-2-design.md` §4).
- **What is evictable.** "Free cache space" removes the 3MF and STL exports written for slicers whose
  blob was last read more than 90 days ago. A blob never read since tracking began counts from when it
  was written. `cache_kinds` is `DerivativeKind::ALL`, so the list *is* what something can still build.
- **What it never touches.** Source files — and, while an upgraded installation still held them, any row
  of an unrebuildable kind: evicting one would have been data loss dressed as a cache sweep.
  **Two clauses name a kind in that SQL and the second is the guard**; dropping either alone changes
  nothing, which is precisely how both come to be dropped by somebody tidying up (R4 found this by a
  mutation that survived). The finding is written into the statement where it was found.
- **Where the bytes go.** The rows go, `ref_count` is recomputed the way delete recomputes it, and a
  blob nothing else references enters the 30-day quarantine. The bytes leave with the hourly sweep.
- **What it reports.** Bytes entering quarantine, never "freed", because nothing is gone on the day.
  The quarantine clause does not say "purged" or "deleted" either: after `0055` most of that figure is
  what *Lapidary* deleted, and telling somebody their store holds deleted data is the data-loss reading
  `CLAUDE.md` forbids. It reads `N on hold for 30 days before the space comes back`.
- **What happens next.** An export is written again when it is next asked for. There is no rung to fall
  back to and no part that stops opening: a part's page shows its picture, and **view** reads the
  original file.

**`derivative.thumb_bytes` survives with no writer** (R4, deliberately). It was the
exception to "no blobs in Postgres" — WebP under 64 KB on the derivative row, so a
thumbnail arrived in the same query as the grid page instead of costing a filesystem round
trip per card. `0055` deleted every row of it and `upsert_derivative` writes the column
`NULL`, so it can only ever read zero; `inline_preview_bytes` was therefore removed from
the wire and the panel rather than left printing "0 B of thumbnails in the database" for
ever. The column itself stays because dropping it would need
`derivative_storage_is_exclusive` restated and would return no space without the same
`VACUUM FULL derivative` the deleted rows already need — risk without a byte to show for
it. **The grid's one-query property is unchanged**, and now comes from `part_image`: the
page query carries the first picture's inline bytes on the row, and an image over
`INLINE_LIMIT` arrives as a hash the card emits as `/api/blob/{hash}` — one immutable
`<img>` load, never a second API request (R1).

### 1.6 Deletion is one step

**Delete is delete** (owner, 2026-10-17: "we don't want a restore option; if it is deleted it is
deleted"). `PgParts::delete` takes a part out of the library at once, in one transaction: its
revisions, files, derivatives, pictures, sources and move trail, child-first because nothing
declares `ON DELETE CASCADE`. Deleting a category does the same to every part under it, through
the same helper, and then deletes the folders. There is no restore, no purge and no removed list;
the confirmation in front of the button is where a person changes their mind.

**The model file is never touched.** Delete takes rows and nothing else. No `quarantined_file`
row is written and a source file's hash is not recounted — those two were how purge used to hand
a part's own file to the reaper — so nothing the hourly sweep reads ever names it. The cost is a
`blob.ref_count` that stays above the true count for those bytes, which only errs towards keeping
them. Only the deleted part's **derivative** hashes are recounted, so a cached export nothing
points at any more enters the ordinary 30-day quarantine cache eviction (§1.5) uses.

A re-scan of a deleted part's file finds no row and ingests it as a new part, naming the very file
the deleted one did, where it is (M4): no copy is made.

Migration `0061` deleted every part and folder that was soft-deleted before this, the same way,
and dropped `deleted_at` from `part` and `folder`.

**What stays from the old three steps** is the quarantine machinery, because cache eviction uses it
and because a purge from before 2026-10-17 may have queued files that are still waiting.

*`recount` recomputes rather than decrementing.* A counter maintained only by increments and
decrements drifts, and the drift is invisible until something acts on it — the thing that
acts on it deletes bytes. Recomputing makes any accumulated drift self-heal the moment the
recount touches that blob.

*Quarantine is a timestamp, not a `quarantine/` tree.* The bytes do not move until the sweep
removes them, and no reader needs a second lookup location.

*What actually keeps a referenced blob safe is neither the counter nor the reaper's own
`WHERE` clause.* It is `file.blake3` and `derivative.blake3`, both foreign keys to `blob`:
deleting a referenced row raises a constraint violation whatever any query says. The
reaper's reachability check is what lets it *decline* such a row instead of failing the
whole sweep on it.

*And the third foreign key nothing counts — `part_image.blake3`.* **Latent, with no
reachable path found; checked rather than assumed** (R4). `recount` and `PgBlobs::reap`'s
`DELETE` both count `file` and `derivative` alone, and delete recounts derivative hashes only —
so a picture's hash never reaches the recount, and a gallery blob is therefore never
quarantined and never reaped. The trap is the next person's: if such a blob were ever
quarantined, the reaper's delete would be refused by `part_image_blake3_fkey` rather than
declined by its own `NOT EXISTS`, and because the sweep is one transaction **the whole hourly
sweep would roll back, every hour, for ever**. The first route that removes a picture lands in
it: it wants the third term in `recount` and in `reap`'s predicate, added together with that
route.

*What the panel does not say.* A quarantined blob has nothing pointing at it and therefore no
library, so quarantined bytes appear only in the instance-wide view. A deleted part's model
file, left on the disk, is in no tracked figure at all; the instance view's directory walk is
the number that still sees it.

**What `blob.stored_bytes` counts** (2026-09-15, migration `0026`).
- It is the size of the content-addressed copy under `blobs/`, and 0 when the bytes are filed only in
  model directories.
- A filed file's size is on its `file` row, and the sweep counts it there, once.

Keep the two apart in every string that reaches a user. **Delete** takes a part out of the
library and leaves its file on disk. **Cache eviction** (§1.5) drops derivatives the app can
rebuild and never touches a source file. Wording that lets either read as the other is a
defect, not a nicety.

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
that owned that file was purged (before 2026-10-17; delete never writes one). Source dedup is gone (§1.1), so one hash can be several
model files with several independent lifetimes — which is exactly why the path cannot live
on the `blob` row.

One sweep, one transaction, one cutoff. What differs is the guard: a referenced blob is
protected by `file.blake3`'s foreign key whatever any query says, and the reaper's
reachability check merely lets it *decline*; nothing references `file.storage_path`, so
there the check is the only protection and is written as such.

*"Reachable by hash" is true of a quarantined blob and false of a quarantined file.* Its
bytes sit at a path no route serves once the `file` row naming them is gone. Both are
invisible in the UI, and neither has a restore.

*The sweep removes no model file at all since M4.* Its path-addressed half — which unlinked a
purged part's file, the `metadata.json` beside it and an emptied directory, as listed in
`quarantined_file` — is gone, and `0062` dropped that table. What the sweep can reach is a
content-addressed blob in Lapidary's own store and nothing else; the files those last queue rows
named stay where they are, which is the safe direction to be wrong in.

---

## 2. Fast open

> **Opening a part reads metadata and pictures, never a model file.** The one exception is
> deliberate and asked for: pressing **view** hands the browser the original mesh to draw,
> which is the whole point of the button. Nothing loads a model because a page opened or a
> pointer moved.

### 2.1 No ladder: the picture at ingest, the original on a **view** press

**The LOD ladder is gone** (owner, 2026-10-03). This section used to be a four-row table —
`thumb` at ingest, `L0` at ingest, `L1` and `L2` on demand — and the argument for which of
them to build when. Nothing produces any of them: R3 deleted the kinds, R4 deleted the
rows, and `docs/ROADMAP.md` § "The pivot" carries the shas.

What a part costs to open now:

| | Where it comes from | When |
|---|---|---|
| The card's picture | `part_image`, the part's own first image — inline bytes on the grid row under `INLINE_LIMIT` (64 KB), else `/api/blob/{hash}` | Whenever somebody adds one. A part with none names its format instead |
| The part page | the same metadata the grid reads, plus the gallery's own request | On open |
| The 3D view | `GET /api/revisions/{id}/download?variant=original`, read by three's own loader for the format (`web/src/components/mesh.ts`) | **Only on a `view` press** |

**Ingest writes no derivative at all.** `produce` is empty: the handler parses the mesh,
measures it, writes the figures, and stops. That is what makes the ingest rate what it is,
and it is also why there is nothing for a stale-derivative sweep to re-queue — R3's
`two_worker_starts_over_a_library_of_dead_derivatives_queue_nothing` holds that shut, with
a positive control, because `enqueue_stale_derivatives` logs and swallows every database
error and a sweep whose SQL no longer parses is exactly as silent as one with nothing to do.

**The trade, said plainly rather than left as an oversight:** a large mesh is a large
download with nothing to paint first. There is no coarse rung, so a 200 MB STL is 200 MB
before anything appears. The picture holds the frame until it does, and a failure says to
download the file and open it in a real tool. That is the honest shape of a visual library
that is not a renderer.

### 2.2 meshopt, not Draco — and then neither

Kept as a ledger entry, because the choice was made carefully and then made moot. Draco
compressed ~15–20% smaller; **meshopt decoded roughly an order of magnitude faster** and
was SIMD-friendly, and decode latency dominates for visual triage — so
`EXT_meshopt_compression` was the default everywhere, with Draco held for a cloud tier
where egress costs money.

There is no glTF to compress now. `glb.rs` went with the render pipeline and `meshopt`
left `Cargo.lock` with its `[workspace.dependencies]` pin (R5) — it was the one dependency
whose build script compiled bundled C++. The browser reads the ingested mesh as it is. If a
coarse rung ever comes back, this is the decision to reopen; nothing else here depends on it.

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

### 2.4 Nothing prefetches, and nothing warms

**This section said "Hover a grid card → prefetch `L0`" and it is now a rule in the other
direction** (R1): a card has no hover handler at all. Not an omission — a spin or a
prefetch on hover would fetch an *original* mesh per hovered card, which is fifty files for
one sweep down a grid, and `Grid.test.tsx` asserts that no `<canvas>` appears on hover at
any setting.

Deleted with it: `lib/prefetch.ts`, `turntable.ts`, `warmViewer`, `prepare()` (which
created a `WebGLRenderer`, and therefore a canvas, on hover) and `loadViewerWhenIdle`. The
three.js chunk is reached only through the **view** press —
`grep -l WebGLRenderer web/dist/assets/*.js` matches that chunk and nothing else.

What is left in its place is one lazy boundary and one honest cost: pressing **view** loads
the renderer chunk and the file together, so the first press in a session is slower than
the ones after it, and the picture is what a person looks at until it is not.

The remaining prefetch-shaped idea worth having is the picture, not the mesh: `QuickLook`
could hand `Preview` the card's own `image` as its placeholder, so the part page's stage
does not open on an empty frame while `['part-images', id]` answers. Named in R1's Record
as the fix when it matters.

### 2.5 Targets — treat as regression tests

| Operation | Warm | Cold |
|---|---|---|
| Grid page of 100 | < 80 ms | < 250 ms |
| Part open → the part on screen | < 120 ms | < 400 ms |
| **view** press → first frame | — | the file's own size; see below |
| Search, 100 k parts | < 150 ms | — |
| Facet counts, 100 k parts | < 300 ms | — |

**The third row has no number on purpose**, and that is the pivot in one line. It used to be
"Part open → L2, < 600 ms warm", which was a promise about *our* tessellation of a part. There
is no tessellation: a **view** press downloads the ingested file and reads it in the browser, so
the time is the file's size over the link plus one parse, and a 200 MB STL takes as long as 200
MB takes. A target here would be a target on somebody else's mesh. What is held instead is that
**nothing at all is loaded until the press** — `DATA.md` §2.4, and `Grid.test.tsx` asserts it.

**Timing both presses.** `web/scripts/open-timing.mjs` measures them against a running stack,
with nothing but Node and `google-chrome`: `node web/scripts/open-timing.mjs --gl swiftshader`
opens every card in the grid three rounds over and presses **view** on each, reporting the two
numbers separately. `--part <name>` does one part from a fresh session each run. `--gl gpu` draws
on the machine's GPU instead, `--throttle` emulates 100 Mbit, and `--dwell` is how long the
pointer rests on a card before it presses. `--direct` opens each part's own page from a link
instead, a fresh session each time, and times the view press on a cold page. It needs a stack and
a browser, so it is not a verify gate; `ROADMAP.md` records what it measured and how. A stack need
not be compose: `lapidary-server` run natively as `api` and `worker`, with the built web served by
`npx vite preview`, whose `/api` proxy is the dev server's, times the same pages. `scripts/e2e/`
drives it as the `TIMING` half of a `stack.sh drive` run.

---

## 3. Metadata and search

### 3.1 Extraction stages — each commits independently

1. **Identity** — BLAKE3, size, format sniff, filename. Always succeeds. **A known hash
   short-circuits the entire pipeline** and the part appears instantly. This is what
   makes re-import feel free.
2. **Geometric** — bbox, volume, area, centre of mass, triangle count, watertightness,
   units. Read from the mesh, so **every figure is approximate and labelled so** — there is
   no analytic half any more, which makes `CLAUDE.md`'s "figures must not lie" simpler
   rather than weaker.
3. **Semantic** — the name, the part number and the path the file came in under; material
   and the user-defined fields a person types (Phase 5).

A stage-3 failure still leaves a usable, searchable part.

**Two stages left with the pivot, and the numbering shifted with them.** Stage 2 was
*Structural* — the assembly tree, instances and transforms out of `structure.json` — and
stage 5 was *Derived*: thumbnails, LODs and an embedding vector. Nothing produces any of it
(R3), and what stage 4 read out of a STEP header is unreachable because a STEP file is now
refused at ingest by name. `revision.face_count` and `edge_count` are still columns and
still read, because an install upgraded across the pivot holds revisions a STEP file
ingested before it, and dropping them would lose real data from a page.

### 3.2 Columns vs JSONB — one rule

> If it appears in a filter, a sort, or a facet, it is a typed column with an index.
> Everything else is JSONB.

`ORDER BY (metadata->>'volume')::float` is unindexable in practice and becomes a seq scan
at ~50k rows.

**Sort reads the part row, one index per key.** The grid's order (volume, surface area, longest
side, triangle count) is the *latest* revision's. While a LATERAL found that revision part by
part, no index could hand parts back in order: on 20,000 parts in one library a sorted page took
63 ms against 0.5 ms newest first, and `revision (volume DESC NULLS LAST)` changed neither the
plan nor the time (2026-09-13). Since `0035` every revision write copies its figures onto `part`,
and `PgParts::sorted` reads them off `(library_id, coalesce(key, '-infinity') DESC, id DESC)`.
Measured on 20,000 parts through the api (goal 6, a debug build, the same data): a sorted page
went from 45–47 ms to 3.0–3.2 ms, against 2.8 ms newest first, and the query itself from 65 ms to
1.6 ms. Backfilling those parts left the api ready 1.2 s after it started.

```sql
part(
  id uuid PRIMARY KEY,                  -- uuid v7
  library_id uuid NOT NULL,
  part_number text, name text NOT NULL,
  classification text,
  created_at timestamptz, created_by uuid,
  metadata_json jsonb DEFAULT '{}',
  materials text[] NOT NULL DEFAULT '{}',   -- what a person typed, else what the file declares; GIN, `materials @> array[$n]`
  materials_typed boolean NOT NULL DEFAULT false,  -- a person typed `materials`, so a file's statement no longer replaces them (0033)
  tags text[] NOT NULL DEFAULT '{}',        -- what a person gave it; GIN, `tags @> array[$n]`
  latest_volume double precision,           -- the latest revision's figures, copied by every revision write for the
  latest_surface_area double precision,     --   grid's order (0035); NULL where it has none. One index per key:
  latest_longest_side double precision,     --   (library_id, coalesce(key, '-infinity') DESC, id DESC)
  latest_triangle_count integer,
  search tsvector GENERATED ALWAYS AS (...) STORED   -- STORED IS MANDATORY (PG18)
);

revision(
  id uuid PRIMARY KEY, part_id uuid NOT NULL,
  rev_label text NOT NULL, parent_revision_id uuid,
  origin text NOT NULL,                 -- ingest|external_edit|import|assembly_promote
  author uuid, message text, created_at timestamptz,
  volume double precision, surface_area double precision,
  bbox_x double precision, bbox_y double precision, bbox_z double precision,
  triangle_count integer, is_watertight boolean, units text,
  mass_props_json jsonb                 -- {"centre_mm": [x, y, z], "source": "analytic"|"tessellated"}: the
                                        -- centre of the volume, needing no density; NULL before goal 5 or
                                        -- with no volume. Mass is not stored: volume × density, when read
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
  kind text,                            -- export_stl|export_3mf, and nothing else since 0054/0055
  blake3 text, thumb_bytes bytea,       -- thumb_bytes has no writer: see §1.5
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
  blake3 text, image_webp bytea,        -- always cached locally, never hotlinked
  origin text NOT NULL,                 -- uploaded|url_supplied|og_fetched|rendered; `rendered`
                                        -- is in the CHECK constraint and has never had a producer
  source_url text, position integer, created_at timestamptz,
  fit text, focus_x double precision, focus_y double precision
);
```

Two deviations from that sketch, both built and both deliberate. **`position`, not
`is_primary`** — owner decision, 2026-09-05: a part carries an ordered gallery, user images
first and generated views appended after them, which a boolean could not express. And
**bytes inline or in a blob**, whichever the 64 KB line in §1.5 says, so there is one rule in
the store about small images rather than two.

`fit`, `focus_x` and `focus_y` are `0019`, and they are CSS's `object-fit` and
`object-position`: the framing is applied by the browser at display time and the stored WebP
is never re-encoded, so adjusting it is free and reversible.

`part_source.license` is not bureaucracy — half of hobbyist STL libraries are
non-commercial, and a user selling prints needs to see that before they print. Nobody else
surfaces it.

**It is on the detail page and the quick-look, not the grid card.** That is a change from
what this section originally said, and the reason is the grid query's 16-column `FromRow`
ceiling: reworking the tuple every other column on a card already fits inside, to put a
sentence on a tile somebody is scanning at a glance, is the wrong trade. The quick-look is
one click from the card and is where the decision to print is actually made.

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

Every library searches with the `simple` configuration: words as written, with no stemming.
- **Turkish search was built, then removed.** A `language` column on `library` chose `simple` or
  `turkish` at creation, and each part copied it into `part.search_config` (`0028`, `aad407e`). The
  owner does not need it (2026-09-15): regular word search is fine. `0031` put the literal `simple`
  expression back and dropped both columns (`8bb5ea4`).
- **Two costs went with it.**
  - `pg_upgrade` refuses a cluster with a `regconfig` column in a user table.
  - A `tsquery` built from a library's language could not fold to a constant, so it was parsed again
    for every candidate row.

  `no_table_holds_a_type_pg_upgrade_refuses` keeps a `reg*` column from coming back.

### 3.4 Facets

Under 10k matching rows: exact counts in one query with `FILTER (WHERE …)` aggregates.
Above that: a rollup table refreshed on ingest, or drop counts and show only which values
are non-empty. Users tolerate missing counts; they do not tolerate a 900 ms filter panel.

### 3.5 Custom fields

`custom_field(id, library_id, key, label, type, options_json, indexed bool)`, with values in
`part.metadata_json->'custom'`, beside `cad`.
- **Types:** `text`, `number`, `choice`.
- **Keys** are slugs, never renamed.

**Amended 2026-09-15, on the owner's behalf** (`2026-09-15-local-product-design.md` §1):
- **One index for every field:** a GIN index over `(metadata_json->'custom') jsonb_path_ops`, with
  filters written as `@>`. This replaces an expression index per indexed field, which would be DDL
  built from a key a user typed.
- **A number field also filters by a range** (`fieldMin`, `fieldMax`, either or both, each inclusive), written
  as `@?` over the jsonpath `$."<key>" ? (@ >= min && @ <= max)`. The GIN index cannot serve a comparison, so a
  range compares every part the other filters leave; an expression index on one field's value is the upgrade
  when a large library measures slow. A value that is not a number is passed over, never an error.
- **A choice field's options are counted** in one `jsonb_each_text` query over every choice field offered as a
  filter, under the grid's other filters. A field filter never narrows its own field's counts, by §3.4's rule for
  the other lists, and an option no part holds is left out.
- **`indexed` means "offered as a grid filter".** Still **capped at 8**, now as the grid's limit
  rather than a write cost.
- **Removing a field removes its definition only.** Its values stay.
- **A field defined again under that key** takes back the values it can show. Values it could not show
  refuse the key.
- **A value set** is a row change and nothing else: since M4 no `metadata.json` is written beside a
  file, and the `describe_part` job that kept one current is gone.

### 3.6 Saved filters

A saved filter is a name for the grid's filters (search, category, format, material, tag), kept
per library in `saved_filter` (`0023`). It is shared by everyone who opens that library, because
there are no users yet.

- **JSONB, by §3.2's rule.** Nothing filters, sorts or facets on a saved filter's contents.
- **The API keeps only what the grid's URL carries,** trimmed. It refuses any other key, and refuses
  a category from another library.
- **Removing one removes the name and nothing else.**
- **A saved category does not follow a deletion, and the grid says so.**
  - The list marks a filter whose category is gone.
  - The grid, opened on a category the live tree does not hold, says it was deleted and offers the
    same filters without it (`2026-09-15-local-product-design.md` §3).
- **Renamed and ordered by hand.** A name stays unique in its library, and a filter moves up or down
  one place at a time.
- **An indexed custom field** (§3.5) is one more thing a filter may carry: `field` with `fieldValue`, or, for a
  number field, with `fieldMin` and `fieldMax`, either or both.

### 3.7 Material densities

`material_density(library_id, material, density_kg_m3 numeric)` (`0034`): a library's density for a
material, keyed by the material exactly as parts hold it, and removed with its library.
- **Typed, never measured.** Set in the library's Densities dialog, typed in g/cm³ and stored in kg/m³.
  The API refuses anything but a finite number above 0 and below 25,000 kg/m³, in words.
- **A material no part holds yet** may have one, so a density can be set before the parts that need it.
- **What it is for:** a part's mass, worked out when read from its volume and the density of its one
  material, and always approximate (goal 5).

### 3.8 Shape similarity, removed — and the two tables that went with it (Phase 6)

**`part_shape` and `part_link` no longer exist.** Migration `0054` dropped both (R3), and the
owner's decision behind it is one sentence: **duplicates are not a feature. Identity is the
content hash in the background, and no real file is ever refused for sharing a name.**

This section described `part_shape(part_id PK, library_id, revision_id, l0_blake3, version
smallint, size_mm float8, descriptor real[35], computed_at)` (`0047`) and
`part_link(part_id, other_id, library_id, kind, created_at)` (`0048`), the 35-float descriptor —
32 D2-distribution bins stored as square roots so that Euclidean distance is Hellinger distance,
then λ2/λ1, λ3/λ1 and `ln(area/m²)` — and the `variant` / `distinct` / `folded_into` decisions a
person could record about a pair. `docs/ROADMAP.md` § "The pivot" carries the shas; the routes
(`/likeness`, `/duplicates`, `/fold`, `/folds`, the link routes) and the queue page went with
them.

**It was not removed because it did not work; it was removed because the thing it was derived
from is gone.** The profile was computed from the part's stored **L0 tessellation**, in line
inside the ingest job, and there is no L0. Two measurements are worth keeping, because they are
what any future attempt starts from rather than re-discovers:

- On a simple part the descriptor is **exactly** rotation-invariant: a 20-triangle bracket turned
  37° off-axis scored a distance of **0.000000** against a 0.04 threshold, with `size_mm`
  identical to four decimals (T1).
- On a real corpus mesh it is not. `ln(area/m²)` carries the re-tessellation, so the same solid
  turned off-axis scored **0.105 — 2.6× the threshold** at 84,418 triangles, while sitting
  comfortably inside the size band; and **`size_mm` itself moved 1.55 % under a rigid turn**,
  which a mean surface point-pair distance cannot legitimately do. The band was being spent on
  re-tessellation noise before any real size difference was considered (T1).

So the honest reading is that the approach needed an orientation-independent tessellation it
never had. What survives of the idea, and is enough for what the product promises, is the
**content hash**: `PgBlobs::library_holds` joins on `source_path` **and** hash, so the same bytes
at a second path are a second part rather than a skipped file — and the store gives that second
part a row and transfers nothing. "Hash first, always", visible from outside.

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
GET /api/revisions/{id}/download?variant=original  → bracket-lp-1042-03.stl
GET /api/revisions/{id}/download?variant=3mf       → bracket-lp-1042-03.lapidary.3mf
```

**`?variant=original` is also what the browser's 3D view reads** (R1): pressing **view**
fetches this exact URL and hands the bytes to three's own loader. So this route is no longer
only a download — it is the one path by which a model file reaches anybody, which is why the
re-hash below matters to more than a save dialog.

`original` returns byte-identical ingested bytes, verifiable against the stored BLAKE3 —
**show the hash next to the button** so a user can check it themselves. Anything we
produced carries the `.lapidary.` infix.

`Content-Disposition` must use RFC 5987 (`filename*=UTF-8''…`). Turkish part names
contain ğ, ş, ı and a naive `filename=` mangles or breaks the download.

Download is just a `Target` whose `accepts()` the user picks manually — so send-to-app
degrades to download naturally when no agent is present.

**Built in slice 5, with three things this section did not specify.** `variant=original`
was the only legal value then — `variant=3mf` was a 400 naming what to send (goal 4 made
it a download, below), never a silent fallback, because a download that quietly returns
something other than what was asked for is the failure this section exists to forbid. The route **re-hashes the bytes
before serving them** and refuses with a 500 on mismatch, which is what makes "verifiable
against the stored BLAKE3" true rather than asserted; it costs microseconds against the
transfer that follows. And `Cache-Control: no-cache` — revalidate before reuse. The blob
route can promise `immutable` because its URL contains the hash of what it returns; this
URL names a *revision*, whose source could be re-pointed, and sending no directive at all
would leave heuristic freshness free to hand back a stale file.

**Negotiated since goal 4.** A format is a download of that one format, a `Target` whose
`accepts()` is just it, so the route runs `negotiate`:
- the part's own file is already in that format: the original, byte-identical and under
  its own name (`variant=stl` on an STL);
- else a mesh export the worker wrote from the part's mesh, `variant=3mf` or `variant=stl`,
  named `*.lapidary.3mf` or `*.lapidary.stl`;
- a B-rep format for a mesh part (`variant=step` on an STL): a 400 naming what there is.

Nothing converts on the download itself. An export not written yet is a 404 naming
`POST /api/parts/{id}/exports/{format}`, which queues it as a derive job and answers with
the batch. **`?variant=l0|l1|l2` falls through to `unknown_variant`** (R3), whose sentence no
longer offers a rung — those links are in people's bookmarks and in older harnesses, and a
stale one deserves a sentence rather than a 500. An export of a pre-pivot STEP revision is
refused in `cad_refusal`'s own words rather than the parser's, because that path is reachable
on an upgraded install: `build()` reads `latest_revision(part)` without looking at its format
(R5).

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

**As designed in Phase 4 slice 2** (`docs/superpowers/specs/2026-09-15-phase-4-slice-2-design.md`
§6–7).
- **Not `async-zip`.** The writer is hand-rolled: STORE entries with data descriptors, and the central
  directory at the end.
  - `zip` needs a seekable writer and `async-zip` is not in the tree.
  - A bundle stops short of 4 GiB, since there is no ZIP64 yet.
- **Paths follow each part's `source_path`,** with earlier revisions under `revisions/<label>/`. Since
  M4 a bundle carries the revisions whose file is still on disk; one a later revision replaced in the
  owner's folder has no bytes left to carry, and is left out with its figures.
- **Import:**
  - It validates the whole archive (§5.4) before queuing anything.
  - It then replays each part's revisions through ingest, oldest first, each written into the inbox at
    `imports/bundles/<source path>` and indexed from there — so the older ones end as rows, marked no
    longer on disk, and the newest as the file.
  - Labels, parents, origins and hashes survive. `created_at` becomes the import time.
  - A part the import creates also arrives with its number, its tags and its sources, the licence
    among them (`634df2c`). A part the library already holds keeps what was typed here instead: the
    bundle's values are older, and nothing records which of the two a person wrote.

### 5.4 Archive security

- **3MF is a ZIP → zip-bomb vector.** Cap decompressed size, entry count and compression
  ratio during extraction. Abort on breach, not after.
- **Reject path traversal** in entries — absolute paths and `..` segments write outside
  the extraction dir. Old, still exploited.

---

## 6. Versioning

What transfers from Git: immutable history, content addressing, lineage, authorship, dedup. What does
not: merge, textual diff, branches, rebase. **A part keeps its file history; there are no locks,
states or approvals** (S4, 2026-10-16). Until then a library was `hobby` (no revisions) or
`controlled` (revisions and check-out locks), and a desktop agent checked parts out and saved them back;
migration `0057` dropped `part_lock`, `library.mode` and the never-written `revision.lifecycle_state`.

The rules, as built (design: `docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md`):

- **Every library.** A changed file at a known path becomes a new revision (`revised`). A revert is a
  revision too. The same bytes again are `skipped`. Finished jobs from before S4 may still hold
  `unkept`, a hobby library's old answer; nothing writes or counts it.
- **Newest on top, where the file is** (M4). The owner saved over their file, so the new revision's row
  names the same path and the previous one is marked `on_disk = false` in the same transaction, with
  the part row locked first. Nothing is moved, copied or set aside: the previous bytes are gone, and its
  figures and hash are what history keeps. Its download answers 410.
- **Origin:** `ingest` (scan), `upload` (browser). `agent` is read, never written: rows and bundles from
  the desktop agent still hold it.
- **History:** `GET /api/parts/{id}/revisions`, newest first, each with its figures and a download of its
  own original file. There is no comparison: the figure-by-figure diff went with S4.
- **Bundles** replay every revision of every part, oldest first. A bundle written before S4 also carries
  `library.mode`; it is read and ignored.

## 7. Sharing

Removed on 2026-10-16 (S3, the owner's decision). Migration `0056` dropped the ten tables `0037`–`0046`
and `0052` made for it; no core table carried a column of its own for it. `docs/ROADMAP.md` has what it was.
