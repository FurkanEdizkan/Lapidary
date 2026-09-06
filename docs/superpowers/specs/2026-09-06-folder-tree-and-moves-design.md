# Folder tree and moves — location becomes a thing you can change

**Status:** design, awaiting owner review. **Branch:** `worktree-folder-tree-design`,
cut from `feat/corpus-and-upload` at `f210d96`.

**This is the slice `handler.rs:50` named.** Slice 6a deferred rename detection with the
words *"closing that needs a source-path column and the slice that owns incremental
directory sync"*. `part.source_path` is that column and it landed in `0007`. This slice
owns the other half: a part's **location** becomes a first-class, mutable, auditable thing,
separate from the path it was ingested under.

**Exit:** scan a nested corpus and the grid shows the directory tree it came from; drag a
part into another folder and it stays there across a re-scan of the original directory.

---

## 0. Where this sits

The owner asked for a storage lifecycle covering seven things. Measured against the real
corpus (`/mnt/Storage2/All/STL Files`, 320 GB) they decompose into five sub-projects, and
this is the first:

| # | Sub-project | Status |
|---|---|---|
| **1** | **Folder tree + moves** | **this spec** |
| 2 | Archive extraction ingest (zip / tar.xz / 7z) | not specified |
| 3 | Storage root, relocation, re-adoption | not specified |
| 4 | Cold tiering + compression opt-out | `DATA.md` §1.3, unbuilt |
| 5 | Purge, quarantine, permanent delete | `DATA.md` §1.6, unbuilt |

It is first because the other four need somewhere to put things. The measurement that
settled the order, and which belongs in the record:

- **288.6 GB of the 320 GB corpus (90.2%) sits inside 737 archives** — 122 GB zip, 96 GB
  tar.xz, 50 GB rar, 20 GB 7z. Sub-project 2 is the largest single win, and extracting a
  2.7 GB zip holding 58 STLs under `supported/`/`unsupported/` needs a folder model to
  land in or it produces 58 rootless parts.
- **Cold tiering reclaims ~1.5%.** Measured on four representative binary STLs near the
  p50 of 8.1 MB: zstd `-3` gives 1.46×, `-19` gives 2.10× — a real **30.1% gain over
  ingest level**, but applied to the 23 GB of loose STL it is ~4.8 GB of 320 GB. Worth
  building; not worth building first. (`DATA.md` §1.2's "~2–2.5× for binary STL" describes
  `-19`. Ingest writes `-3`, which measures 1.46×. The doc reads as though ingest gets the
  higher number.)

## 1. The split: `source_path` is identity, `folder_id` is location

**`part.source_path` does not change. Ever.** It is the ingest identity key that `0007`
just made load-bearing, and `0003_jobs.sql:52` records what depends on it agreeing with
`PgBlobs::library_holds`. Mutating it on a move would mean a re-scan of the original
directory no longer recognises the file, re-ingests it as a second part, and reports
success — silent duplication, which is the same shape of lie 6a exists to refuse.

So location moves to its own column:

| | Column | Mutable? | Means |
|---|---|---|---|
| Identity | `part.source_path` | **no** | where this file was when we first saw it |
| Location | `part.folder_id` | yes | where the user has since decided it lives |

At ingest they agree: `folder_id` is seeded from `source_path`'s directory. After a move
they diverge, and that divergence is the feature.

**The discriminating case, which §9 tests:** ingest `Terrain/rock.stl`; move it to
`Bases/`; re-scan. `library_holds(library, 'Terrain/rock.stl', hash)` still matches on the
unchanged `source_path`, the file settles as `Outcome::Skipped`, and the part stays in
`Bases/`. Not resurrected, not duplicated, not moved back. This falls out of the split for
free, which is the argument that the split is the right one.

**Two different things are both called "moving", and they share no code.** Said here once
so the relocation job does not drift into this slice:

- **A folder move is logical.** One `UPDATE`, zero bytes. Blobs are content-addressed and
  live at `blobs/ab/cd/<hash>` regardless of which folder the user has put the part in.
- **A storage-root relocation is physical.** Every blob, one at a time, resumable. That is
  sub-project 3 and is not in this spec.

## 2. Schema — migration `0008_folders.sql`

```sql
create table folder (
  id          uuid primary key,                        -- uuid v7
  library_id  uuid not null references library(id),
  parent_id   uuid references folder(id),              -- null = library root
  name        text not null,
  created_at  timestamptz not null default now(),
  deleted_at  timestamptz,                             -- soft, like part
  constraint folder_name_unique_per_parent
    unique nulls not distinct (library_id, parent_id, name)
);

create index folder_library_parent on folder (library_id, parent_id);

alter table part add column folder_id uuid references folder(id);   -- null = library root
create index part_folder_id on part (folder_id);
```

**`nulls not distinct` is not decoration.** PostgreSQL treats NULLs as distinct in a unique
constraint by default, so a plain `unique (library_id, parent_id, name)` would silently
permit two root folders both named `Terrain` — the exact case a corpus scan hits first.
Verified against the project's own PostgreSQL 18.6 container rather than asserted:

```
=== two ROOT folders named Terrain (parent_id NULL) — second must be REFUSED ===
ERROR:  duplicate key value violates unique constraint "folder_name_unique_per_parent"
DETAIL:  Key (library_id, parent_id, name)=(0193…0001, null, Terrain) already exists.

=== same name under DIFFERENT parents — must be ALLOWED ===
INSERT 0 1   (Terrain/Rocks)
INSERT 0 1   (Bases/Rocks)

=== get-or-create under concurrency: ON CONFLICT DO NOTHING must not raise ===
INSERT 0 0
```

**`null` means library root for both columns**, and there is deliberately no seeded root
row per library. A synthetic root would need a nullable `parent_id` for itself anyway, so
it buys nothing and costs a seed that every future library-creation path has to remember.
Listing the root is `where folder_id is null`, matching `where parent_id is null`.

**`folder_id` stays nullable rather than `NOT NULL` after a backfill.** Every pre-0008 part
is at the library root by construction, so a backfill would write the same `null` the
column already defaults to. Nothing to reconstruct, no `NOT NULL` to earn.

## 3. The scan creates folders

`WorkerHandler::scan_directory` already yields `/`-separated relative paths capped at
`MAX_DEPTH = 16`. Ingest splits the path's directory portion and get-or-creates one folder
row per segment, then sets `part.folder_id` to the last.

**Get-or-create is `insert … on conflict do nothing` followed by a select**, not a
select-then-insert. Two workers scanning concurrently genuinely race the same directory —
`0003_jobs.sql` records the same race for parts — and the constraint is what makes it safe.
The validation above confirms `on conflict do nothing` returns `INSERT 0 0` rather than
raising.

**Folders are created only for files that are actually ingested**, after the
`library_holds` short-circuit, not before it. Creating them during the walk would mean a
re-scan of a directory whose parts have all been moved away silently re-creates the
now-empty original folders on every scan.

**A failed part insert may leak an empty folder row.** Accepted, not fixed: folder creation
sits outside the part's transaction, and the cost of the leak is an empty folder the user
can delete. Wrapping the walk's folder writes into each file's transaction would serialise
concurrent workers on the shared parent rows for no benefit a user can perceive.

## 4. Moving a part

`PATCH /api/parts/{id}` with `{ folderId }`. One `UPDATE`, plus a bump to the existing
`part.updated_at`, plus one `part_move` row (§6).

**Collision warns, it does not refuse.** Slice 6a decided two parts named `bracket` are the
truth and the path is what tells them apart; refusing a move on a name collision would
contradict that, and would refuse an arrangement that is already legal on disk. So the API
answers a `409` naming the existing part, and the client re-sends with `{ folderId,
acknowledgeDuplicate: true }`. One extra round trip, only on the collision path.

**Parts and folders differ here, deliberately.** Two parts named `bracket` in one folder are
disambiguated by `source_path`. Two folders named `Terrain` under one parent are not
disambiguated by anything — the folder path *is* a folder's whole identity — so a folder
rename into a collision is a plain `409` refusal with no override. The asymmetry is the
point, not an inconsistency.

## 5. Folder management, and the boundary it stops at

Create, rename, move and delete, per the owner's answer.

**The boundary, stated for the review gate because it is an interpretation and not a
quotation.** The owner wrote *"full folder management only on the allowed storage folder"*.
Read here as: the app may fully manage folders **within its own library and store**, and
must never create, rename, move or delete a directory in the user's source library. That
reading is already structurally true — `deploy/compose.yaml` mounts the ingest directory
`:ro` precisely so ingest cannot modify what it was pointed at — and this slice does not
weaken it. If the intended reading was instead that folders should be **real directories on
disk inside the storage root**, this spec is wrong at the root and should be rejected here,
not patched: it would replace content-addressed storage with a mirrored tree, which
forfeits dedup (`ref_count`) and turns every folder move into a byte move.

- **Create** — `POST /api/libraries/{id}/folders { parentId, name }`. Empty folders are
  legal; the user asked to be able to organise before ingesting.
- **Rename** — `PATCH /api/folders/{id} { name }`. A label change. No `source_path` is
  touched, no blob moves, no part row changes.
- **Move** — `PATCH /api/folders/{id} { parentId }`. **Refused if it would cycle.** A
  folder moved into its own descendant orphans the subtree and makes the tree query loop.
  Checked with an ancestor walk before the update, bounded at the same 16 the scan uses,
  and validated against the live database:

  ```
  === is Cliffs a descendant of Terrain? (must be TRUE -> refuse move) ===
   would_cycle | walked
   t           |      3
  === is Terrain a descendant of Cliffs? (must be FALSE -> allow) ===
   would_cycle | walked
   f           |      1
  ```

  A recursive CTE rather than `petgraph`, which `FEATURES.md` names for the build graph:
  this needs an ancestor walk of one node, not cycle detection over a whole DAG, and the
  check belongs beside the write it guards.
- **Delete** — soft, and it **cascades through subfolders**. `folder.deleted_at` on the
  folder and every descendant, `part.deleted_at` on every part in any of them. Deleting
  `Terrain` when it holds `Terrain/Rocks/Cliffs` must not leave `Rocks` alive and
  unreachable, which is what a one-level delete produces. The same bounded ancestor walk
  as the move check, run downward.

A folder's `parentId` on create, and on move, must name a folder **in the same library**.
Cross-library parenting is refused with a `409`, not silently accepted — `folder.library_id`
would then disagree with its parent's and the tree query would return a subtree from
another library. The constraint cannot express this (it is a cross-row invariant), so it is
a check at the write.

Every tree and grid read filters `deleted_at is null`, on folders as well as parts. A
soft-deleted folder is invisible in the sidebar and its parts are invisible in the grid,
which is what soft delete means everywhere else in this schema.

**Delete obeys `CLAUDE.md`'s rule that we never delete user data implicitly**, which here
means three things and the wording is a product requirement, not a nicety:

1. The confirmation names the count: *"Delete Terrain? The 412 parts inside will be moved
   to deleted."*
2. It states what is untouched: *"Your original files are not affected, and nothing is
   removed from disk."* True, and it is true because this action never decrements
   `ref_count` and never reaches a blob.
3. It states that it is reversible. Soft delete is reversible indefinitely per `DATA.md`
   §1.6 step 1. **Purge and quarantine are steps 2 and 3 and are sub-project 5.** This
   slice must not grow a permanent-delete button; the confirmation that names unrecoverable
   history belongs with the action that actually destroys something.

## 6. Move history

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
move changes no bytes, so it has no content to address and would produce a revision
identical to its parent in every measured column. Recording moves as revisions would make
the version history strip — which shows volume deltas between revisions — display a run of
zero-delta entries that are not design changes. So moves are an audit log, queried by
`GET /api/parts/{id}/moves`, and the version history strip does not show them.

**Folder-move history is not recorded.** Moving a folder changes the folder's `parent_id`,
not any part's `folder_id`, so it cannot honestly be written as `part_move` rows — a row
with `from_folder = to_folder` would be a lie. A `folder_move` table is the obvious
addition and is deliberately deferred until someone asks for it: the request was to track
where a *part* has been.

## 7. API surface

All of it on `lapidary-api`. Nothing here reads a source file or invokes the kernel, so the
open-path boundary and `FORBIDDEN_PAIRS` are untouched — these are row reads and row
writes, the same class as the existing `auto_thumbnail` setter.

```
GET    /api/libraries/{id}/folders        → the tree, one query
POST   /api/libraries/{id}/folders        { parentId, name }
PATCH  /api/folders/{id}                  { name? , parentId? }
DELETE /api/folders/{id}                  → soft, cascades to parts
PATCH  /api/parts/{id}                    { folderId, acknowledgeDuplicate? }
GET    /api/parts?folderId=…              → existing grid, filtered
GET    /api/parts/{id}/moves              → history, newest first
```

`GET /folders` returns the whole tree in one response rather than a lazy per-level fetch. At
corpus scale the tree is hundreds of rows, not hundreds of thousands, and a lazy tree costs
a round trip per expand on the one interaction that has to feel instant. Revisit if a
library ever exceeds ~10k folders.

Types via `ts-rs` as everywhere else; `web/src/bindings` is generated and CI-gated.

## 8. Frontend

A folder tree beside the existing grid. Selecting a folder filters the grid through the
existing keyset pagination — `folderId` becomes another typed search param on the TanStack
Router route, so the filter is in the URL and survives a reload like every other filter.

Per `CLAUDE.md`: dark only; motion 120/180/280 ms on `cubic-bezier(0.2, 0, 0, 1)`, transform
and opacity only, `prefers-reduced-motion` respected; **no bare user-facing strings** — the
collision warning, the delete confirmation and the tree's empty state all go through
`src/lib/strings.ts`, which `web/src/no-bare-strings.test.ts` already enforces.

Drag a card onto a folder to move it. Drag is an affordance, not the only path: a
context-menu "Move to…" exists because drag-and-drop into a scrolled tree is a poor target
on a trackpad and unusable with a keyboard.

## 9. Testing

The named cases, each of which fails if the design is wrong rather than if a helper is:

1. **A move survives a re-scan.** Ingest `Terrain/rock.stl`, move to `Bases/`, re-scan the
   same directory. Asserts: one part, `folder_id` still `Bases`, `source_path` still
   `Terrain/rock.stl`, outcome `Skipped`. *This is the test that proves the split; if only
   one test is written, it is this one.*
2. **Two root folders cannot share a name** — the `nulls not distinct` case, which a plain
   unique constraint passes silently and wrongly.
3. **Same name under different parents is legal** — `Terrain/Rocks` and `Bases/Rocks`.
4. **Concurrent get-or-create does not raise** — two workers, one directory, one folder row.
5. **A folder cannot move into its own descendant** — the ancestor walk refuses it.
6. **A part move into a name collision answers 409, then succeeds on acknowledge.**
7. **A folder rename into a collision answers 409 with no override** — the asymmetry in §4.
8. **Deleting a folder soft-deletes its parts and touches no blob** — assert `ref_count` and
   `stored_bytes` unchanged, and the blob still readable by hash.
9. **Deleting a folder cascades through subfolders** — delete `Terrain` holding
   `Terrain/Rocks/Cliffs`; assert no descendant folder and no part in any of them is left
   visible. The one-level bug passes every other test in this list.
10. **A folder cannot be parented into another library** — `409`, and the tree of the second
    library is unchanged.
11. **A scan of a nested fixture builds the tree it came from** — depths 1 through 4, using
    real names from the corpus (`Terrain/Rocks/Cliffs`, `Bases/`), never `Folder 1`.

## 10. Not in this slice

Named so they do not drift in:

- **Rename/move detection on re-scan** — same bytes at a new path still produces a second
  part sharing one blob. 6a deferred it and it stays deferred; it needs incremental
  directory sync, which is its own decision about what a disappeared file means.
- **Archive extraction** (sub-project 2), **storage root and relocation** (3), **cold
  tiering and compression opt-out** (4), **purge, quarantine and permanent delete** (5).
- **Folder-move history**, per §6.
- **Folder-scoped permissions.** There is no principal until Phase 8.
- **Smart/virtual folders** — `FEATURES.md` has saved filters at Phase 5 and they are a
  different object: a query, not a location.
