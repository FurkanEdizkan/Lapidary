# After the folder tree — what Phase 1 still owes, and in what order

Written the day the folder tree merged into `main` (`295c02e`). It is a planning
document, not a spec: it says what is left and why one thing goes before another, and it
stops short of designing any of it. Each item that turns into work gets its own spec in
`docs/superpowers/specs/`.

Two of the questions below are product decisions, not engineering ones. They are marked
**Yours** and are not answered here.

---

## 1. Where Phase 1 actually stands

`ROADMAP.md`'s Phase 1 exit criterion is one sentence and it has four clauses:

> drop a folder of 1,000 STLs, grid is interactive immediately, every part appears —
> with a thumbnail where the library renders them automatically, and with "No preview
> yet" plus a working `POST /api/libraries/{id}/thumbnails` where it does not —
> re-dropping the same folder completes in seconds via hash short-circuit, and grid page
> load is under 80 ms warm.

Every clause has code behind it. **None of them has a number behind it.** The corpus on
this machine is 156 parts, not 1,000, and nothing in the repo has ever timed a warm grid
page. The suite proves the mechanisms; it does not prove the criterion, because the
criterion is about a size and a latency and the tests use neither.

So the honest status is: Phase 1 is feature-complete except for the rows listed in §3,
and it is **unmeasured**. Those are two different kinds of not-done and they want
different work.

Beyond the exit criterion, `FEATURES.md` marks four more rows Phase 1 that have no code:
search (two rows), library creation, and the per-library page-size and density settings.
Search is the large one — `lapidary-index` is a twenty-line stub, and the `tsvector`
column and `pg_trgm` extension that `0001`/`0002` create are read by nothing.

---

## 2. The debt this merge created — do this first

**A purge does not remove a migrated model's directory.**

Pinned, not described: `crates/lapidary-ingest/tests/reap.rs`,
`a_purged_model_directory_outlives_the_sweep_that_reports_removing_it`. The test asserts
today's behaviour, so the fix cannot land without someone editing it.

The thirty-day sweep unlinks `blobs/<ab>/<cd>/<hash>`. That was every source file's
address before the folder tree; it is now only the address of files `migrate_storage` has
not reached. A migrated part keeps its bytes at `file.storage_path`, `purge` deletes the
`file` row, and by the time the clock runs out nothing knows where the bytes went.
`SourceStore::remove` unlinks a path holding nothing and reports success, because a
missing file is success to a reaper and deliberately so.

Nothing is lost, which is the right way round for this area to fail. What breaks is the
promise: `strings.parts.purgeConfirm` tells a person their bytes are kept for 30 days and
then deleted, and for a migrated part the second half never happens. Every part ingested
from now on is migrated, so this is the normal case and not an edge one.

**Shape of the fix.** Somewhere to record the path a purge is about to forget. `blob` is
keyed per hash and one hash can be several model files now, so it cannot go there — a new
`quarantined_file(blake3, storage_path, quarantined_at)` row written inside `purge`'s
transaction, and a sweep that unlinks the path and prunes the empty model directory above
it. A migration, a repository method, a change to `reap::sweep`, and the test above
inverted. Call it a small slice.

**Why first.** It is the only thing on this list that makes the app tell a person
something untrue, and it gets worse with time rather than better: every day of use adds
model directories that a purge will strand.

---

## 3. What Phase 1 is still missing

Ordered by what would hurt most to ship without.

### 3.1 Search — full-text and trigram

`FEATURES.md` rows "Full-text search over names, tags, materials" and "Trigram search for
part numbers and filenames", both Phase 1. `crates/lapidary-index/src/lib.rs` is twenty
lines and implements nothing.

The schema half is already there and has been since `0002`: `part.search` is a `STORED`
generated `tsvector` weighting `part_number` at A and `name` at B, and `pg_trgm` is
installed by `0001`. What is missing is a query, a route, a binding and a box to type in.

This is the largest remaining Phase 1 item and the one a person notices first. A grid
that pages through 1,000 parts but cannot find one is a filing cabinet with no labels —
and the folder tree makes that sharper, not softer, because now there are categories to
search *within*.

Note for whoever specs it: Phase 2's exit criterion is a search assertion
(`A1234-56-B` found by the fragment `1234`, at position one). Phase 1's search and Phase
2's identifier-aware ranking are the same code path reached twice. Build the ranking
seam now even if only the simple half is wired.

### 3.2 More than one library

The web hard-codes `DEFAULT_LIBRARY_ID` (`web/src/lib/api.ts:41`) and the API has no
route that creates a library. `LibraryMode` exists in `lapidary-core` and is exported and
read by nothing — `hobby` versus `controlled` is a type with no behaviour behind it yet,
which is correct until governance arrives but does mean the row in `FEATURES.md` is not
met.

Smaller than it sounds on the server (`POST /api/libraries`, a list route, and every
existing route is already keyed by library id) and mostly a front-end job: a library
switcher, and a decision about what the URL looks like when a library is selectable.

### 3.3 Page size and card density, persisted per library

`FEATURES.md` names 50/100/250/500 and a density control, both persisted per library. The
route already clamps `limit` to `MAX_LIMIT` and the settings `PATCH` already exists for
`autoThumbnail`, so this is two more fields on a row that is already there plus the
controls. Small, and it is the one item here a user asks for by name once a library gets
big.

### 3.4 Measure the exit criterion

Not a feature. A run:

- A thousand-STL folder through the browser drop, wall-clock timed.
- Re-drop the same folder and time the hash short-circuit.
- Warm grid page load, measured against the 80 ms number, with `migrate_storage` drained
  so the measurement describes the layout we actually ship.
- Confirm every part appears, with a preview or with "No preview yet".

The corpus at `/mnt/Storage2/All/STL Files` has 1,703 loose meshes at depth 2–9, which is
more than enough and is the right shape (nested, with 102 basename collisions — the case
`0007`'s per-path uniqueness exists for).

Do this *after* §3.1, not before: search is the thing most likely to change what the grid
query looks like, and measuring a query you are about to rewrite buys a number with a
short shelf life.

---

## 4. Folder-tree follow-ups, re-ranked after the merge

`2026-09-07-folder-tree-and-moves-followups.md` listed six. What the merge changed about
each:

| # | Item | Status now |
|---|---|---|
| 1 | A renamed category does not rename its directory on disk | Unchanged. Blocked on a decision — see §5. |
| 2 | No UI for creating or renaming a category | Unchanged, and now the more visible half: the merge shipped the sidebar, so the gap is on screen. |
| 3 | `GET /api/parts/{id}/moves` has no consumer | Unchanged. Blocked on a decision — see §5. |
| 4 | The descendant CTE lives in the page query, not in `PgFolders` | **Settle it as accepted.** The merge put the `Shows` predicate and the folder CTE in the same query and the suite proves they agree; splitting it now would create the second descent implementation the original note was worried about. |
| 5 | Two scale ceilings (tree fan-out, whole-tree fetch) | Unchanged, and §3.4's run is the natural place to get a number for both. |
| 6 | `file.storage_path` is still nullable | **More urgent, and now cheap.** `migrate_storage` drained a real library to zero pending in this deployment. When the `NOT NULL` migration lands, four places simplify together: the move route's `409 migrationPending`, the card's "not migrated yet" note, `PartCard.directory`'s nullability, and the `coalesce(f.stored_bytes, f.size_bytes)` fallback in `storage_totals`. Leave them and they read as defence against a state that can no longer happen. |

Item 2 is the smallest real feature on this whole page and the routes it needs already
answer. It is a good first task for anyone picking the codebase back up.

---

## 5. Two decisions that are yours

Neither is defaulted here, because a wrong default in either is expensive to reverse.

### Does renaming a category move bytes on disk?

A user who fixes `Terain` to `Terrain` expects the folder to follow. A user who renames
`WIP` to `Archive 2024` may not expect ten thousand files to be rewritten. A resumable
`repath_category` job that reports progress answers both honestly, but whether a rename
*offers* to move bytes at all — always, never, or with a checkbox — is a product call.

Until it is answered, `file.storage_path` stays authoritative and correct and nothing
breaks; what stops being true is that directory names in the store mirror category names
on screen for anything ingested before the rename. That matters exactly as much as
browsing the store in a file manager matters, which is the feature the whole layout is
for.

### Is a part's move history ever shown?

`GET /api/parts/{id}/moves` answers and nothing reads it. It also answers `[]` for a part
id that names nothing, which is the same answer as "this part has never moved" —
deliberate, because nothing acts differently on the two.

- Show it (a "moved from" line in the inspector) → the 404 starts to matter and should be
  added in the same change.
- Do not → keep the route, keep the `[]`, and this paragraph is the record that it was a
  decision rather than an oversight.

---

## 6. Recommended order

1. **The purge gap** (§2). Small slice, and it is the only correctness item.
2. **Category create/rename UI** (§4 item 2). Small, routes exist, visible.
3. **`storage_path` NOT NULL** (§4 item 6). Small, and four simplifications ride on it.
4. **Search** (§3.1). The large one. Spec it properly; build the Phase 2 ranking seam.
5. **Page size and density** (§3.3). Small, and best done while the grid query is open
   from step 4.
6. **Measure the exit criterion** (§3.4) against the 1,703-file corpus.
7. **More than one library** (§3.2) — last of the Phase 1 items, because everything above
   it is work a single-library user feels and this is work they do not.

Then Phase 2, whose first question is `lapidary-cad` driving the OCCT sidecar — and whose
exit criterion is a search assertion, which is why step 4 is where it is.

## Housekeeping this merge left

- `storage/blobs` still holds 9.4 MB of derivative rungs (correct — derivatives stay
  content-addressed) after `migrate_storage` moved 145 MB of sources into
  `storage/libraries`.
- The `lapidary_lapidary-blobs` Docker volume is still on this machine, holding the
  pre-merge copy of the store. Nothing reads it. Removing it is a deliberate act and is
  left to the owner, per "we never delete user data implicitly".
- Every `lane-*`, `worktree-*` and `feat/*` branch is now an ancestor of `main`. The
  worktrees are gone; the refs are left alone.
