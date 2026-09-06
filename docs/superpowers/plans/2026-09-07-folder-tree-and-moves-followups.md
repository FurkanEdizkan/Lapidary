# Folder tree and moves — what is left after tasks 9, 10 and 11

Slice `2026-09-06-folder-tree-and-moves` is implemented and green on branch
`worktree-folder-tree-design`: the move route, folder CRUD, the subtree-inclusive grid
filter, `FolderNode.partCount`, the model directory on `PartCard`, and the sidebar that
calls all of it. `cargo xtask verify` is 14/14, the workspace suite is 587 tests, the web
suite is 79.

This file is the leftovers, ordered by what would hurt most to forget. None of it blocks a
merge — every item is a known, written-down edge, not a bug in what shipped.

## 1. A renamed category does not rename its directory on disk

**The state.** `PATCH /api/folders/{id}` changes a row. `file.storage_path` still names the
old directory, and stays authoritative and correct, so nothing breaks — what stops being
true is that directory names in the storage folder mirror category names on screen for
models ingested before the rename. Someone browsing the store in a file manager (which is
the whole point of the path-addressed layout) sees the old name.

**Why it was left.** `xtask/src/deploy.rs`'s `RELOCATE_MODULE` allows the store's rename
handle only in `crates/lapidary-api/src/moves.rs`, so a folder handler structurally cannot
re-path a subtree — and that gate is the design, not an obstacle. Re-pathing hundreds of
model directories is also not work an HTTP handler should hold open.

**The shape of the fix.** A `repath_category` job in `lapidary-ingest`, beside
`migrate_storage` and modelled on it: resumable, one model directory at a time, `UPDATE
file SET storage_path` per move so a crash costs one directory rather than the walk. The
route enqueues; the worker walks.

**Decide first, before writing it:** is a rename allowed to move bytes at all? A user who
renames `Terain` to `Terrain` expects the folder to follow. A user who renames
`WIP` to `Archive 2024` may not expect ten thousand files to be rewritten. A job that
reports progress and can be left running is the honest answer to both, but the question is
a product one.

## 2. No UI for creating or renaming a category

`POST /api/libraries/{id}/folders` and the rename half of `PATCH /api/folders/{id}` are
built and server-tested (`crates/lapidary-api/tests/folders.rs`) with no web consumer — the
sidebar can delete a category and drop models into one, but not make one or rename one.

Small and well-bounded: a "New category" control on the tree, a rename affordance on the
row, both against routes that already answer. The refusals to render are `nameTaken`,
`slugTaken`, `wouldCycle`, `crossLibrary`, `emptyName` — all already carrying prose and a
machine-readable `reason`, the same shape `d495753` taught the move dialog to read.

Note while building it: **there is no rename UI to test the `renamedAfterMove` path with
yet.** A `PATCH` carrying both a new name and a new parent applies the move first and the
rename second, and answers `409 renamedAfterMove` if the name is refused at the
destination. That branch has no test because no client sends both fields; if the UI ever
does, test it.

## 3. `GET /api/parts/{id}/moves` has no consumer

The route answers the audit trail `part_move` was created for, and nothing reads it. It
also answers `[]` for a part id that names nothing, which is the same answer as "this part
has never moved" — deliberate, because nothing acts differently on the two and telling them
apart costs a query on every read of a legitimately empty history.

Two ways to close it, and the choice depends on whether a history is ever shown:
- Show it (a "moved from" line in the inspector) → then the 404 starts to matter, and it
  should be added at the same time.
- Leave it → keep the route, keep the `[]`, and this note is the record that it was a
  decision.

## 4. The descendant CTE lives in the page query, not in `PgFolders`

The brief said "implement the descendant set with a recursive CTE in `PgFolders`"; it went
inline into `PgParts::page` instead — one query rather than two round trips, and one
descent implementation rather than two to keep in step. Flagged in the lane report, not
silent, and the invariant it protects (the sidebar's count equals the cards the filter
returns) is asserted end to end in
`the_tree_counts_every_model_under_a_category_and_no_deleted_one`.

**Owner's call, one line either way:** accept it, or move it and accept the second round
trip. Nothing else depends on the answer.

## 5. Two scale ceilings, both fine today, both worth a number

- `PgFolders::tree` pairs every folder with each of its descendants in one recursive CTE:
  O(nodes × depth) rows, depth bounded at 16. Hundreds of categories is nothing; tens of
  thousands would want a materialised count or a closure table. Measure before rewriting.
- The tree is fetched whole on every sidebar load, by design (design §10 — a lazy tree
  costs a round trip per expand on the interaction that has to feel instant). Same
  threshold, same advice.

## 6. Adjacent, not this slice: `file.storage_path` is still nullable

Migration `0008` says a later migration makes it `NOT NULL`, once `migrate_storage` has
drained every library. Until then `null` is a live state and three places encode it: the
move route's `409 migrationPending`, the card's "not migrated yet" note, and
`PartCard.directory` being nullable at all. When that migration lands, all three simplify —
and they should be simplified together, or the ones left behind will read as defensive code
against a state that can no longer happen.
