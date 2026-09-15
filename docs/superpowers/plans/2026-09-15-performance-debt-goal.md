# Goal 6: performance and debt

Written 2026-09-15. **This file is the goal's source of truth.** After any context summary, re-read it
together with ROADMAP's "Open points, 2026-09-15" section and this goal's record there, which is the
progress ledger.

Run it after `2026-09-15-materials-and-mass-goal.md`. None of it needs OCCT, so it may also run earlier if
goal 4 stalls.

## Why

Each item was recorded as known and left alone by goals 2 and 3, by the code review of goal 3, or by a
`ponytail:` ceiling, and a read-only sweep on 2026-09-15 found every one still true. A change that claims
speed is measured before and after, on the same data, and recorded either way.

## Facts already checked

Line numbers are as of `fc0f683`.

- **Grid sort** (`crates/lapidary-db/src/repo.rs:3266`, `ponytail:`). The `keyed` CTE runs a `JOIN
  LATERAL` onto each matching part's latest revision, coalesces the sort key, then orders and limits. The
  doc comment above it measured 63 ms a page over 20k parts; a `revision` index did not help.
- **The if-absent enqueue** (`PgJobs::enqueue_if_absent`, `crates/lapidary-db/src/jobs.rs:327`,
  `ponytail:`). The only job indexes are `job_dequeue_idx` and `job_expired_lease_idx` (partial, on state)
  and `job_batch_idx` (`0003_jobs.sql:40-46`). Nothing covers `(library_id, kind)`.
- **Grid idle warm-up.** `warmViewerWhenIdle` (`web/src/routes/index.tsx:538`) and hover (`:534`) call
  `warmViewer`, which imports the viewer and calls `prepare()` (`web/src/components/PartDetail.tsx:606-607`).
  `prepare` makes a `WebGLRenderer` and runs `compileAsync` (`web/src/components/Viewer.tsx:104,202`), and
  `hasWebGL()` opens another context (`web/src/lib/viewer-math.ts:49`). **The owner decided** idle loads the
  code only. Goal 2 measured the shader warm-up at 270–300 ms of a cold link open.
- **Folder tree** (`web/src/components/FolderTree.tsx`, used at `index.tsx:925`). Every node filters the
  whole list for its children (`:493`) and recurses (`:501`), rendered expanded (`:164`). Goal 2 measured
  first render at 1,389 ms flat and 1,676 ms ten-way for 10,000 categories.
- **Assembly tree** (`PartDetail.tsx:1092`, `ponytail:`): every node is in the DOM, open or not.
- **Bundle planning** (`crates/lapidary-api/src/download.rs:721`, `ponytail:`): per part it calls
  `library_of`, `detail`, `part_sources` and `history` (`:779-786`), and `source_for_download` per revision
  (`:792`).
- **A file that fails every attempt** (goal 2's review record). Disambiguation runs once
  (`crates/lapidary-ingest/src/handler.rs:1137`, `{slug}_{6 hex}` from `crates/lapidary-core/src/slug.rs:90`).
  `put_new_at` meeting other bytes returns `Transient` (`handler.rs:816-826`), and a purge leaves its bytes
  in place, recorded in `quarantined_file` (`repo.rs:2247`), so every retry meets them for 30 days.
- **Watch and symlinks.** The agent's listing uses `entry.file_type()` and `entry.metadata()`, which do not
  follow a symlink (`bin/lapidary/src/folder.rs:199-210`); the scan follows a symlinked file
  (`crates/lapidary-ingest/src/scan.rs:234-236`).
- **The library menu** is `popover="auto"` (`index.tsx:1556`). Its items open dialogs by local state
  (`index.tsx:1789`, `web/src/components/Fields.tsx:53`), the dialog is a portal overlay
  (`web/src/components/Dialog.tsx:122-124`), and nothing calls `hidePopover`.
- **The agent's lock check** (`bin/lapidary/src/main.rs:433-439`) has no test; `main.rs` has no test module.
- **Step 10's first manifest** for a new part is built from the ingest's own ids and written without the
  part-row hold `PgRevisions::write_manifest` gives the other two writers (goal 3's code review record).
- **Unmeasured:** how long an access-tracking flush takes on a real corpus (goal 2's record), and whether
  leaving a core free during a commit helps the rest of the api (the code review's record).
- **Housekeeping**, rechecked read-only on 2026-09-15: `storage/` in the repo root is 155 MB, owned by uid
  10001, mode 755; Docker volumes `lapidary_lapidary-blobs`, `lapidary_lapidary-db` and
  `lapidary_lapidary-uploads` exist. Which of them the current `deploy/compose.yaml` still uses was not
  checked.

## Stages

Each stage with code gets its own branch, merged before the next starts. Defaults are stated; list each
one you take under "Decided without the owner" in this goal's ROADMAP record.

### 0. Preflight (no branch)

- `lapidary-test-db` is up, and `cargo deny check` passes on `main`.

### 1. Grid sort off the part row: `perf/grid-sort-columns`

- **Measure first:** a page sorted by volume over 20k parts, as the doc comment measured it.
- Copy volume, surface area, longest side and triangle count onto `part` in a migration with a backfill,
  kept current by every write of a part's current revision (ingest, revision, import, migration).
- One index per key: `(library_id, key DESC, id DESC)`. The page query reads the part row.
- **Tests:** sort order and keyset paging unchanged across a revision that changes the key; a revision
  write updates the copy. Mutation-check the revision write.
- **Measure after**, same data. Keep it only if it is faster; record either way.

### 2. The if-absent enqueue's index: `perf/job-active-index`

- A migration: `CREATE INDEX … ON job (library_id, kind) WHERE state IN ('pending', 'running')`.
- **Check:** `EXPLAIN` of the enqueue's `existing` CTE before and after, with a few thousand done jobs.
  Remove the `ponytail:` comment.

### 3. Grid idle warm-up, code only: `perf/idle-warm-code-only`

- Idle imports the viewer module and stops there; hover and a part page's mount still call `prepare()`.
  `hasWebGL()` releases its probe context.
- **Tests:** idle warming imports and does not prepare; hover prepares. Mutation-check the idle path.
- **Measure:** a cold link open and a grid-then-open, before and after, headless Chrome, goal 2's method.

### 4. Folder tree: `perf/folder-tree`

- Group the folders by parent once per render, not a filter per node.
- A branch is collapsed by default below the first level, and its children render only once it opens;
  the selected category's ancestors open. Default: the open set is not persisted.
- **Tests:** children grouped once; a collapsed branch renders no rows; the selected category's path is
  open. Mutation-check the lazy render.
- **Measure:** first render at 1,000 and 10,000 categories, flat and ten-way, as goal 2 did.

### 5. Assembly tree: `perf/assembly-tree-lazy`

- A branch's children render only once it is opened. Remove the `ponytail:` comment.
- **Test:** a closed branch has no child rows in the DOM; opening it renders them.

### 6. Bundle planning in one query: `perf/bundle-plan-query`

- One query for the selection's parts, sources and revisions with their download sources, replacing the
  per-part and per-revision calls.
- **Tests:** the existing bundle tests unchanged; a query-count check on a 40-part selection. Measure the
  plan route before and after on 40 and 500 parts.

### 7. A file that fails every attempt: `fix/disambiguate-past-quarantine`

- When `put_new_at` meets other bytes and no live part names that path, take a longer disambiguated name
  (`{slug}_{12 hex}`), then the full hash, before failing.
- **Test:** goal 2's recorded state (a controlled part in a disambiguated directory, revised, purged,
  then its first revision dropped again) ingests instead of failing. See it fail first.

### 8. Watch follows a symlinked file: `fix/watch-symlinked-files`

- A non-directory entry is read with `std::fs::metadata(&path)`, which follows, so a symlinked model file
  is watched as the scan ingests it. A symlinked directory is still not descended.
- **Test:** a symlinked STL in the folder is listed; a symlinked directory is not. Mutation-check.

### 9. The library menu closes: `fix/menu-closes-for-dialog`

- Opening a dialog from the library menu hides the menu.
- **Test:** after choosing New library or Fields, the menu is closed. See it fail first.

### 10. The agent's lock check, tested: `test/agent-lock-check`

- Move the comparison into a function in a module with tests; `main.rs` calls it.
- **Test:** a released lock and another holder's lock both refuse, the held lock passes.

### 11. Step 10's first manifest, held: `fix/first-manifest-held`

- A new part's first `metadata.json` is written through `PgRevisions::write_manifest`, from the rows,
  like the other two writers.
- **Test:** the written manifest equals the rows' manifest for a new part. Mutation-check.

### 12. Two measurements (no branch unless a finding needs code)

- An access-tracking flush over the STL corpus stand-in's 150 parts read in one interval, and over 20k
  touched blobs.
- Grid search round trips during a 150-file commit, at `available_parallelism` less one and at all cores,
  on the same build. Change the default only if the numbers say so.

### 13. Housekeeping (no branch)

- Read `deploy/compose.yaml`'s volume names, and list what `storage/` and each volume holds, read-only.
- **Ask the owner** before removing anything, naming each path or volume and its size.

## Recorded, not built

Until a real library needs each:
- streaming bundle import, and ZIP64 (`crates/lapidary-ingest/src/import.rs:211`,
  `crates/lapidary-targets/src/bundle.rs:21`);
- `notify` in place of the watch's poll;
- per-file job stages (decided against, FEATURES §1);
- the `renameat2(RENAME_NOREPLACE)` fallback for filesystems without hard links;
- a lock across the adopting and reaping jobs;
- a 3MF upload's staged copy counted as phantom by migration `0026`.

## Before teardown

- A fresh reader reviews this goal's diff: a subagent, or the advisor.
- The ROADMAP record is updated after each merge, not at the end.

## How to work, stop conditions, when done

As `2026-09-15-phase-4-slice-2-goal.md` states in "How to work", "Stop and ask before" and "When done".
The essentials:
- **Worktree:** your own, from local `main`, with `web/node_modules` linked and unlinked before removal.
- **TDD:** see each test fail first, or mutation-check it and say so.
- **Gates:** `cargo xtask export-bindings`, then
  `CARGO_BUILD_JOBS=4 DATABASE_URL=postgres://lapidary:localdev@localhost:55432/lapidary cargo xtask verify slice`
  in the foreground, logging to `target/`.
- **Measurements:** native stack, scratch database dropped afterwards, headless Chrome with a throwaway
  profile, scripts in `target/`, and servers started outside any `tee` pipe.
- **Commits:** conventional, with no AI attribution trailer. **Merges:** `git merge --no-ff` into local
  `main`. Never push.
- **Stop and ask before:** any Docker image build, pull or prune; anything needing sudo; pushing; removing
  user data or touching `deploy/.env`; a decision that contradicts a doc.
