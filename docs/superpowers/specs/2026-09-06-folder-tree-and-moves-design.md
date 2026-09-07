# Storage layout and folders — a store the user can open in a file manager

**Status:** design, awaiting owner review. **Branch:** `worktree-folder-tree-design`,
cut from `feat/corpus-and-upload` at `f210d96`.

**This is the slice `handler.rs:50` named**, plus a storage-layout reversal that arrived
with it. Slice 6a deferred rename detection to *"the slice that owns incremental directory
sync"* and made `part.source_path` its prerequisite. This slice owns location: where a model
lives becomes a real directory the user can open, and a thing they can change.

**Exit:** scan a nested corpus, then open the storage folder in a file manager and find one
directory per model holding its file, its metadata and its images — and move a model to
another category in the UI and find it moved on disk, with a re-scan of the original
directory leaving it where the user put it.

---

## 0. Where this sits, and what it reverses

Five sub-projects came out of the owner's storage-lifecycle request. This is the first:

| # | Sub-project | Status |
|---|---|---|
| **1** | **Storage layout, folder tree, moves** | **this spec** |
| 2 | Archive extraction ingest (zip / tar.xz / 7z) | not specified |
| 3 | Storage root relocation and re-adoption | not specified — but §2 is its foundation |
| 4 | Cold tiering + compression opt-out | `DATA.md` §1.3, unbuilt |
| 5 | Purge, quarantine, permanent delete | `DATA.md` §1.6, unbuilt |

**Two written decisions are reversed here, both on the owner's explicit instruction, both
stated so nobody re-derives the old ones from the docs.**

1. **The blob store is a user-owned host directory, not a named volume.**
   `deploy/compose.yaml` says of the volume: *"Named, not a bind mount: these are our data,
   not the user's files, and deleting the compose project must not take a host directory
   with it."* The owner wants the inverse — a directory they own that outlives the app.
2. **Source files are path-addressed, not content-addressed.** `DATA.md` §1.1 puts every
   blob at `blobs/ab/cd/<hash>`. The owner wants *"a single folder for each model ingested,
   with its own name … every metadata file inside the folder, every image inside the folder
   of a model, so when a user wants to look at it they can go into its specific folder."*

`compose.yaml`'s comment and `DATA.md` §1.1 both need rewriting when this ships. They are
not wrong about anything except which goal won.

**What the reversal costs, stated plainly:** identical bytes in two models are two copies on
disk. Source dedup is gone. That is the price of a browsable store and it is not
recoverable by cleverness — a store cannot both be one file per model and one file per
distinct content. Measured against the corpus this is a real but bounded cost: 1,614 loose
STLs holding some cross-pack duplication, not the 90% of bytes that sit in archives
(sub-project 2).

**What survives untouched, because it is easy to think it does not:** `CLAUDE.md`'s *"Hash
first, always. BLAKE3 before anything else in ingest. A known hash short-circuits the whole
pipeline."* That rule is about ingest ordering and the re-scan short-circuit, and both still
hold — `PgBlobs::library_holds` keys on `(library_id, source_path, blake3)` and none of
those three change. Hashing still happens first, a re-scan still settles as `Skipped`
without re-reading, and the download route still verifies bytes against the stored hash.
Only the *filename the bytes are written under* changes.

**Ordering, from measuring the real corpus** (`/mnt/Storage2/All/STL Files`, 320 GB):
**288.6 GB — 90.2% — sits inside 737 archives** the app cannot read, so sub-project 2 is the
largest single win and needs this layout to extract into. Cold tiering measures a genuine
**30.1% gain** of zstd `-19` over `-3` on representative binary STLs, but applied to the
23 GB of loose STL it reclaims ~4.8 GB, **1.5% of the corpus**. Worth building; not first.

## 1. The layout

```
<storage-root>/                        chosen on first run; app-owned, user-browsable
  lapidary.toml                        app config, user-editable
  libraries/
    default/                           one directory per library
      Terrain/                         category folders — the tree of §3
        Rocks/
          cliff/                       one directory per model
            cliff.stl                  the source, under its original filename
            metadata.json              part + revision facts, human-readable
            images/
              thumbnail.webp
              render-01.webp
  cache/
    blobs/ab/cd/<blake3>               derivatives only. Content-addressed. Evictable.
```

**Correction, after the slice shipped: the code writes a different tree, and the spec is
the defect.** `blob_path` is `root.join("blobs")`, so derivatives land at `<root>/blobs/`,
a sibling of `libraries/` with no `cache/` level; nothing writes an `images/` directory
(thumbnails are Postgres `bytea`) and nothing reads or writes a `lapidary.toml`. `docs/DATA.md`
§1.1 describes what is actually on disk, including what naming the cache `blobs/` costs.
The block above stands as the record of what was designed, not of what exists.

**`libraries/<slug>/` exists even though there is one library today.** Two libraries each
holding a `Terrain` category would collide at the root, and retrofitting the level later
means moving every file in the store. Cheap now, expensive later.

**`cache/` is a stated assumption, and the review gate is where to reject it.** The owner
named metadata and images as belonging inside the model folder and said nothing about glTF
LODs or `structure.json`. Those are treated here as cache, not user-facing content, for
three reasons: `DATA.md` §1.5 already calls them regenerable and freely evictable;
`/api/blob/{blake3}` can only promise `Cache-Control: immutable` because its URL contains
the hash of what it returns, which path-addressing would break for the viewer's hot path;
and `CLAUDE.md`'s rule that *"derivative cache eviction … must never read as data loss"*
becomes self-evident when the directory is literally named `cache/`. **If derivatives should
also live in the model folder, this section is what to reject** — it is the difference
between rewriting half of `lapidary-storage` and all of it.

**`metadata.json` is what makes the store self-describing**, and it is the whole of
sub-project 3's re-adoption story. It carries enough to rebuild the rows: the part's id,
library, display name, part number, classification, `source_path` and `metadata_json`; each
revision's label, origin, measured values and their provenance columns; and each file's
role, format, `blake3`, size and on-disk name. Delete the database and the store still
describes itself. There is no separate manifest directory, because a manifest that is not
beside the thing it describes is a manifest that goes stale.

**Which one wins on divergence:** the database is authoritative while the app runs;
`metadata.json` is authoritative for re-adoption into an empty database. Reconciling a store
a user has edited by hand is Phase 4's watcher and a non-goal here (§12).

**`metadata.json` is machine-owned, and the user may delete it.** They have been promised a
folder they can edit freely, so this will happen. It is not defended, it is degraded around:
a model directory with no readable `metadata.json` is an orphan, and re-adoption **skips it
and reports it** rather than failing the walk. One hand-edited file must not cost the other
1,613 models their re-import. The file carries a `schema` version field for the same reason —
a store written by an older build has to be readable by a newer one, and that is cheaper to
add now than to infer later.

## 2. Naming a model's directory

The display name cannot be the directory name. Slice 6a decided two parts called `bracket`
are the truth and `source_path` tells them apart — but two directories called `bracket/`
inside one category are not a naming preference, they are impossible.

**The rule:** slugify the part name; if that directory already exists in the target
category, append `_` and the first six hex characters of the source `blake3`.

- `cliff` → `cliff/`
- a second `cliff` → `cliff_a1b2c3/`

**Directory names are cosmetic, not identity.** Nothing parses them — the database stores
the real relative path in `file.storage_path`, and re-adoption reads `metadata.json` inside
each directory rather than the directory's name. That is what lets the naming optimise for a
human reading a file manager instead of for machine round-tripping, and it means a
re-ingest that assigns a different suffix breaks nothing.

**Slugging rules, which exist because this store must survive Windows** (the agent binary
and the Tauri shell both target it):

- Unicode is preserved. Turkish part names contain ğ, ş and ı, `DATA.md` §5.1 already
  handles them in `Content-Disposition`, and every filesystem this ships on stores UTF-8.
  Stripping them would make the folder unreadable to the person who named the part.
- Path separators, control characters, and the characters Windows reserves (`< > : " | ? *`)
  are replaced with `-`.
- Trailing dots and spaces are trimmed — Windows silently drops them, so `bracket.` and
  `bracket` would become the same directory after a round trip through a Windows client.
- The reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`) get a
  `_` suffix. A model legitimately called `AUX` is not hypothetical in a parts library.

## 3. Identity, location, and the two things called "moving"

**`part.source_path` does not change. Ever.** It is the ingest identity key `0007` made
load-bearing, and `0003_jobs.sql:52` records that it must agree with
`PgBlobs::library_holds`. Mutating it on a move means a re-scan of the original directory
stops recognising the file, ingests it a second time, and reports success — the silent
duplication 6a exists to refuse.

| | Column | Mutable? | Means |
|---|---|---|---|
| Identity | `part.source_path` | **no** | where the file was when we first saw it, in the *ingest* directory |
| Location | `part.folder_id` | yes | which category the user has since put it in |
| Bytes | `file.storage_path` | yes | where it actually sits in the *store* |

Three columns, three jobs. `source_path` and `storage_path` are different things and the
names must stay distinct: the first names a directory the app only ever reads, the second
names one it owns.

**The discriminating case, which §8 tests:** ingest `Terrain/rock.stl`; move it to `Bases/`;
re-scan. `library_holds` matches on the unchanged `source_path`, the file settles as
`Outcome::Skipped`, and the model stays in `Bases/` on disk and in the grid.

**Both kinds of move now touch bytes, and they differ in scale, not in kind.** The previous
draft of this spec claimed a folder move was "one UPDATE, zero bytes". That was true when
folders were database rows and is false now.

- **Moving a model** renames one directory: `libraries/default/Terrain/Rocks/cliff/` →
  `libraries/default/Bases/cliff/`.
- **Moving a category** renames one directory and every model under it comes with it, for
  free, because they are inside it.
- **Relocating the storage root** is still sub-project 3: every file, resumable.

A same-filesystem rename is atomic and O(1) regardless of subtree size, so even moving a
category holding 412 models is one syscall.

## 4. Keeping the database and the disk in agreement

A move is a filesystem rename plus a row update, and either can fail. **Rename first, inside
the transaction, and commit only if it succeeded**; a failed rename rolls the transaction
back and nothing moved. The window that remains is a rename that succeeds and a commit that
then fails, leaving the disk ahead of the database.

**That window is survivable, and `metadata.json` is why.** Every model directory identifies
itself, so a reconciliation pass can always work out what a directory is regardless of where
it sits or what the database last recorded. This is the same reasoning that makes ingest's
blob reap safe: prefer a recoverable inconsistency over a distributed transaction.

The reverse ordering — commit then rename — was rejected because its failure leaves the
database pointing at a path that does not exist, which every read then hits. A disk that is
ahead of the database is a repair job; a database that is ahead of the disk is a broken
grid.

## 5. Schema — migration `0009_folders.sql`

```sql
create table folder (
  id          uuid primary key,                        -- uuid v7
  library_id  uuid not null references library(id),
  parent_id   uuid references folder(id),              -- null = library root
  name        text not null,                           -- display name
  slug        text not null,                           -- the on-disk directory name
  created_at  timestamptz not null default now(),
  deleted_at  timestamptz,
  constraint folder_name_unique_per_parent
    unique nulls not distinct (library_id, parent_id, name),
  constraint folder_slug_unique_per_parent
    unique nulls not distinct (library_id, parent_id, slug)
);

create index folder_library_parent on folder (library_id, parent_id);

alter table part add column folder_id uuid references folder(id);   -- null = library root
create index part_folder_id on part (folder_id);

-- Where the bytes actually are, relative to the storage root. Stays nullable: null means
-- "still at the old content-addressed path", which is a live state for as long as the
-- migrate_storage job of 5.2 takes to drain. A later migration makes it NOT NULL.
alter table file add column storage_path text;
```

**`nulls not distinct` is load-bearing, not decoration.** PostgreSQL treats NULLs as distinct
in a unique constraint by default, so a plain `unique (library_id, parent_id, name)` silently
permits two root categories both named `Terrain` — the first thing a corpus scan produces.
Verified against the project's own PostgreSQL 18.6 rather than asserted:

```
=== two ROOT folders named Terrain (parent_id NULL) — second must be REFUSED ===
ERROR:  duplicate key value violates unique constraint "folder_name_unique_per_parent"
DETAIL:  Key (library_id, parent_id, name)=(0193…0001, null, Terrain) already exists.

=== same name under DIFFERENT parents — must be ALLOWED ===
INSERT 0 1   (Terrain/Rocks)     INSERT 0 1   (Bases/Rocks)

=== get-or-create under concurrency: ON CONFLICT DO NOTHING must not raise ===
INSERT 0 0
```

**`name` and `slug` are both unique per parent and both are needed.** `name` is what the user
typed and what the grid shows; `slug` is what the filesystem got after §2's rules ran. Two
categories named `Rocks?` and `Rocks*` are distinct names that slug to the same directory,
so without the second constraint the tree is legal and the disk is not.

**`blob.ref_count` keeps its meaning and loses an implication.** It still counts how many
`file` rows reference a hash. What it no longer implies is one copy on disk — two models
holding identical bytes are two `file` rows, `ref_count` 2, and two files. The consequence
lands on sub-project 5: purge deletes a `storage_path`, and only decrements `ref_count`.
Written down here because a purge that deletes "the blob" would take another model's file.

### 5.1 The backfill, and why skipping it strands every existing library

The tempting claim is that every pre-`0009` part sits at the library root. **That was true
before slice 6a and is false after it.** 6a made the scan recursive, so every part ingested
since carries a nested `source_path` like `Terrain/Rocks/rock.stl`. Only rows predating 6a
are flat, because `0007` rebuilt them as `name || '.' || format`.

That would be cosmetic if a later scan repaired it. It does not, and §6 is why: folders are
created only for files that actually ingest, so re-scanning a library whose parts are
already present settles every file as `Skipped`, creates nothing, and **leaves the library
permanently flat with no user-reachable repair.**

The backfill splits each `source_path`'s directory portion and get-or-creates the tree level
by level, bounded at the scan's own cap of 16. Validated against PostgreSQL 18.6 on a fixture
holding one flat row and five nested ones, including the same folder name under two different
parents — the case a naive path-keyed backfill collapses into one row:

```
=== folders created (path, depth) ===          === each part and the folder it landed in ===
 Bases                |     1                   Bases/Rocks/base-rock.stl      | Bases/Rocks
 Bases/Rocks          |     2                   Bases/round-32mm.stl           | Bases
 Terrain              |     1                   bracket.stl                    | (library root)
 Terrain/Rocks        |     2                   Terrain/Rocks/Cliffs/spire.stl | Terrain/Rocks/Cliffs
 Terrain/Rocks/Cliffs |     3                   Terrain/Rocks/cliff.stl        | Terrain/Rocks
                                                Terrain/rock.stl               | Terrain
```

A level-by-level loop rather than one recursive CTE, because a CTE cannot insert rows and
then use the ids it just generated as the next level's parents.

### 5.2 Moving the files is a job, not part of the migration

Existing blobs sit at `blobs/ab/cd/<hash>` and have to end up in per-model directories with a
`metadata.json` beside them. **That cannot go in `0009`.** `sqlx` runs a migration in one
transaction at startup and it either finishes or rolls back; copying 23 GB is not that, and
it needs a worker the migration cannot assume is running.

So it splits, and the split is the design:

- **`0009` is schema plus the SQL tree backfill above.** Fast, transactional, and correct as
  validated.
- **A new `migrate_storage` job kind** does the files. Enqueued once, reported through the
  existing batch and SSE machinery the scan already uses, and resumable because the job queue
  is. It **copies before it deletes**, so an interrupted run has the file at both paths and
  never at neither.

**`file.storage_path` therefore stays nullable, and that nullability is an invariant every
reader of `file` inherits until the job drains: a null `storage_path` means the bytes are
still at the old content-addressed path.** Reads must handle both for as long as the
migration takes — which on the owner's corpus is hours, not seconds. It goes `NOT NULL` in a
later migration, once the job has drained everywhere, and not before.

An operator who stops it halfway has a store that is half-migrated and entirely readable.

## 6. The scan creates categories

`WorkerHandler::scan_directory` already yields `/`-separated relative paths capped at
`MAX_DEPTH = 16`. Ingest splits the directory portion, get-or-creates one `folder` row and
one real directory per segment, creates the model's own directory per §2, writes the source
file, `metadata.json` and any images into it, and sets `part.folder_id`.

**Get-or-create is `insert … on conflict do nothing` then select**, not select-then-insert.
Two workers scanning concurrently genuinely race the same directory, and the constraint is
what makes it safe. `mkdir -p` is idempotent and races harmlessly, so the filesystem half
needs no coordination the database half does not already provide.

**Categories are created only for files that actually ingest**, after the `library_holds`
short-circuit. Creating them during the walk would mean a re-scan of a directory whose models
have all been moved away silently re-creates the now-empty originals on every scan — on disk
as well as in the database. This is also what makes §5.1's backfill mandatory: a library
ingested between 6a and `0009` never reaches this code path again.

## 7. Moving, renaming, deleting

`PATCH /api/parts/{id} { folderId }` moves a model. `PATCH /api/folders/{id} { name?,
parentId? }` renames or moves a category. Both follow §4's ordering and write a `part_move`
row (§9).

**A name collision warns; a directory collision cannot.** The two are now different
questions and both answers are kept:

- **Display name** — allowed, with a warning. 6a decided two parts named `bracket` are the
  truth. The API answers `409` naming the existing part; the client re-sends with
  `acknowledgeDuplicate: true`.
- **Directory name** — never a conflict, because §2's suffix rule resolves it without asking.
  The second `cliff` becomes `cliff_a1b2c3/` and the user is not consulted about a detail
  they did not choose.
- **Category rename into a collision** — plain `409`, no override. A category's path is its
  whole identity; there is no `source_path` to tell two `Terrain`s apart.

**A category cannot move into its own descendant.** Checked with a bounded ancestor walk
before the write, at the same 16 the scan uses, and validated against the live database:

```
=== is Cliffs a descendant of Terrain? (must be TRUE -> refuse) ===   would_cycle: t, walked 3
=== is Terrain a descendant of Cliffs? (must be FALSE -> allow) ===   would_cycle: f, walked 1
```

A recursive CTE rather than `petgraph`, which `FEATURES.md` names for the build graph: this
is an ancestor walk of one node, not cycle detection over a DAG, and it belongs beside the
write it guards. On disk the same move would be `mv Terrain Terrain/Rocks/Cliffs/Terrain`,
which the kernel refuses too — but by then the row is written, so the check goes first.

**Parenting across libraries is refused** with a `409`. A cross-row invariant no constraint
can express, so it is a check at the write.

**Delete is soft, and cascades through subcategories.** `folder.deleted_at` on the category
and every descendant, `part.deleted_at` on every model in any of them. Deleting `Terrain`
while it holds `Terrain/Rocks/Cliffs` must not leave `Rocks` alive and unreachable.

**Nothing is removed from disk.** A soft-deleted model keeps its directory exactly where it
is; only its visibility changes. This is `DATA.md` §1.6 step 1, and steps 2 and 3 — purge and
quarantine — are sub-project 5. Per `CLAUDE.md`'s rule that we never delete user data
implicitly, the confirmation says three things and the wording is a product requirement:

1. The count: *"Delete Terrain? The 412 models inside will be moved to deleted."*
2. What is untouched: *"Nothing is removed from your storage folder."* True, and true because
   this action never touches a file.
3. That it is reversible.

All three go through `src/lib/strings.ts`, which `web/src/no-bare-strings.test.ts` gates.

## 8. Testing

1. **A move survives a re-scan.** Ingest `Terrain/rock.stl`, move to `Bases/`, re-scan.
   Assert: one part, `folder_id` is `Bases`, `source_path` unchanged, outcome `Skipped`, and
   the directory is on disk under `Bases/`. *If only one test is written, it is this one.*
2. **Two models with the same name get distinct directories** — `cliff/` and `cliff_a1b2c3/`,
   both with correct `metadata.json`, and the grid shows both named `cliff`.
3. **A slug collision from distinct names is refused** — `Rocks?` and `Rocks*` slug alike.
4. **Reserved and hostile names survive a round trip** — `AUX`, `bracket.`, a name with `:`
   and one with `ğ`. Assert the directory exists and re-reads.
5. **Two root categories cannot share a name** — the `nulls not distinct` case, which a plain
   unique constraint passes silently and wrongly.
6. **Same name under different parents is legal** — `Terrain/Rocks` and `Bases/Rocks`.
7. **Concurrent get-or-create does not raise** — two workers, one directory, one row.
8. **A category cannot move into its own descendant.**
9. **A failed rename leaves the database untouched** — inject an `EACCES` on the rename and
   assert the row still points at the original path and the model still opens.
10. **Deleting a category cascades to subcategories and touches no file** — assert every
    descendant is hidden and every file is still on disk and still readable.
11. **The `0009` backfill rebuilds the tree and moves the files** — seed a library the way
    6a's scan leaves it, run it, assert the tree of §5.1 *and* that every source is now at
    its `storage_path` with a `metadata.json` beside it. Then **re-scan and assert nothing
    changed** — that half catches a backfill that works once and then misbehaves.
12. **An interrupted `migrate_storage` loses no file** — kill it midway, assert every source
    is readable at either its old or its new path, and that resuming completes.
13. **A part with a null `storage_path` still downloads** — the half-migrated state of §5.2,
    which is live for hours on a real corpus and is therefore a supported state, not an edge
    case. Assert `variant=original` returns byte-identical bytes from the old CAS path.
14. **A model directory whose `metadata.json` is missing or corrupt is skipped, not fatal** —
    delete one and mangle another in a fixture of six, and assert the other four re-adopt and
    both failures are reported by path.

## 9. Move history

```sql
create table part_move (
  id           uuid primary key,
  part_id      uuid not null references part(id),
  from_folder  uuid references folder(id),
  to_folder    uuid references folder(id),
  moved_at     timestamptz not null default now(),
  moved_by     uuid
);
create index part_move_part_id on part_move (part_id, moved_at desc);
```

The owner asked to *"track their location like git … so we can version them"*. **A move is
not a revision.** A revision is an immutable content-addressed snapshot (`DATA.md` §6); a
move changes no bytes, so it would produce a revision identical to its parent in every
measured column, and the version history strip — which shows volume deltas — would fill with
zero-delta entries that are not design changes. So moves are an audit log, read at
`GET /api/parts/{id}/moves`, and the history strip does not show them.

**Category-move history is not recorded.** Moving a category changes its `parent_id`, not any
part's `folder_id`, so writing `part_move` rows for it would mean rows whose `from` and `to`
are equal. A `folder_move` table is the obvious addition, deferred until asked for.

## 10. API surface

All on `lapidary-api`. Nothing here invokes the kernel.

**The move route needs a capability that does not exist yet, and this is the decision.**
`lapidary-storage` reaches source paths through exactly two handles: `SourceStore`, which
demands a `WorkerRole`, and `SourceReader`, which is read-only and which
`check_open_path_boundary` permits in `download.rs` and nowhere else. Neither can rename. So
there are two ways to build a move and hedging between them would be discovered
mid-implementation, when it costs a route moving across crates:

1. **A third handle, `SourceRelocator`, that can `rename` and nothing else** — no read, no
   write, no delete — allowed in the move route and nowhere else, enforced by the same
   `check-deploy` grep that already names the other two.
2. **A `move_part` job**, the way slice 5 turned the scan into one, because the browser can
   only reach `lapidary-api` and a job is the only thing an api-side route can hand a worker.

**Taking (1).** The boundary's stated purpose is that *the open path never parses a source
file to draw something* — the grid, the viewer, the detail card. A directory rename parses
nothing, reads no bytes and invokes no kernel, so routing an O(1) syscall through the job
queue to satisfy the rule would be honouring its letter against its reason, and it would put
a poll cycle between a user dragging a card and the card arriving. The capability stays
narrow and the narrowness stays machine-checked, which is what the rule actually protects.

If `SourceRelocator` ever grows a method that reads or writes contents, that is the signal it
has become `SourceStore` and the route belongs in `lapidary-ingest` after all.

```
GET    /api/libraries/{id}/folders        → the tree, one query
POST   /api/libraries/{id}/folders        { parentId, name }
PATCH  /api/folders/{id}                  { name?, parentId? }
DELETE /api/folders/{id}                  → soft, cascades
PATCH  /api/parts/{id}                    { folderId, acknowledgeDuplicate? }
GET    /api/parts?folderId=…              → existing grid, filtered
GET    /api/parts/{id}/moves              → history, newest first
```

The tree comes back whole rather than lazily per level: at corpus scale it is hundreds of
rows, and a lazy tree costs a round trip per expand on the one interaction that must feel
instant. Revisit past ~10k categories.

## 11. Frontend

A category tree beside the existing grid; selecting one filters the grid through the existing
keyset pagination, with `folderId` as a typed TanStack Router search param so the filter
lives in the URL and survives a reload.

Per `CLAUDE.md`: dark only; motion 120/180/280 ms on `cubic-bezier(0.2, 0, 0, 1)`, transform
and opacity only, `prefers-reduced-motion` respected; no bare user-facing strings.

Drag a card onto a category to move it, and a context-menu *"Move to…"* beside it — drag into
a scrolled tree is a poor trackpad target and unusable from a keyboard.

**One addition the layout earns: a "Show in folder" action** on the model detail card,
revealing its directory. The whole point of this layout is that the user can go and look, and
an app that hides the path is an app that did not need the layout.

## 12. Not in this slice

- **Reconciling a store the user edited by hand.** They may edit freely, so paths *will* go
  stale. Detecting that is Phase 4's watcher, which `DATA.md` §6.2 already specifies down to
  the debounce and the Windows buffer-overflow rescan. Named here, designed there.
- **Rename/move detection on re-scan** — same bytes at a new ingest path still produces a
  second part. 6a deferred it; it needs incremental directory sync and a decision about what
  a disappeared file means.
- **ELK.** Search is `tsvector` + `pg_trgm` at Phase 2 (`FEATURES.md`); an industrial search
  layer is Phase 8+ and putting it in an MVP slice would be scope creep. Recorded because the
  owner raised it, not because it is planned here.
- **Multi-user.** No principal exists until Phase 8, so `moved_by` is written null.
- **Archive extraction** (2), **root relocation and re-adoption** (3), **tiering and
  compression opt-out** (4), **purge and permanent delete** (5).
- **Category-move history**, per §9.
- **Smart/virtual folders** — `FEATURES.md` has saved filters at Phase 5; a query is not a
  location.
