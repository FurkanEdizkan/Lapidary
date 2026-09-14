# Goal: correctness and debt, before more features land

Written 2026-09-15. **This file is the goal's source of truth.** After any context summary, re-read it
together with ROADMAP's "Open points, 2026-09-15" section and this goal's record there, which is the
progress ledger.

Run it after `2026-09-15-phase-4-slice-2-goal.md`, and before `2026-09-15-local-product-goal.md`.

## Why

Each item below was recorded as known and left alone. None needs OCCT, another OS or an installed
application. Several of them are measurements that currently lie, or races that can lose bytes, and
those come before features.

## Facts already checked

- **`touch_blob`** (`crates/lapidary-db/src/repo.rs`) runs one `UPDATE blob SET last_accessed_at = now()`
  per deliberate read. DATA §1.4 forbids exactly that pattern. The api tests that read the column are
  `crates/lapidary-api/tests/blob.rs` (`a_head_request_warms_last_accessed_at_the_way_a_get_does`) and
  `crates/lapidary-api/tests/download.rs`.
- **Phantom bytes.** `docs/superpowers/specs/2026-09-07-purge-removes-the-model-directory-design.md`
  §6 measured them:
  - purging one 9,684-byte part with one 3,292-byte rung reported 12,976 bytes quarantined;
  - the sweep then reported 22,660 bytes freed;
  - 12,976 bytes actually left the disk.

  The spec also names what to change: `PurgeReport.quarantined_bytes` and `ReapReport.bytes` (both in
  `repo.rs`), taken together. `migrate_storage` sets `blob.stored_bytes = size_bytes` after it empties
  the content-addressed copy (`crates/lapidary-db/src/migrate.rs`). Check whether that line is the
  cause.
- **`put_at`** (`crates/lapidary-storage/src/lib.rs`) writes a temporary file and renames it over the
  target, so it silently replaces an existing file. The race this allows is recorded in slice 1's spec
  §3.3.
- **Re-parenting.**
  - `PATCH /api/folders/{id}` accepts `parent_id` (`crates/lapidary-api/src/folders.rs`). Re-parenting
    splits a category's directory (`docs/DATA.md`, the paragraph beginning "Re-parenting a category").
  - No client sends `parent_id`.
  - `409 renamedAfterMove` and `wouldCycle` can only happen with a `parent_id`. Neither has a test
    (`web/src/lib/api.ts`, the comment above the folder patch).
- **Mutation-checked only.** Slice 1's revision tests (`crates/lapidary-db/tests/revisions.rs`) and
  lock tests (`crates/lapidary-db/tests/locks.rs`) were mutation-checked and never seen failing first
  (ROADMAP, Phase 4 slice 1, "Recorded rather than fixed").
- **Unmeasured** (ROADMAP):
  - Phase 1's browser drop path and "interactive immediately" were never timed (the Phase 1 timing
    note).
  - A link open stays over 400 ms on SwiftShader, and where that time goes was not measured (Phase 3,
    the before/after table).
  - L0 and L1 are not quantized (Phase 3, "L0 and L1 could shrink further").
  - The folder tree's fan-out ceiling was never measured
    (`docs/superpowers/plans/2026-09-07-folder-tree-and-moves-followups.md` §5).

## Stages

Each stage gets its own branch, merged before the next starts. This goal has no spec stage: every
decision here is already settled by a doc, or its default is stated below. List each default you take
under "Decided without the owner" in this goal's ROADMAP record.

### 0. Preflight (no branch)

The same as the slice 2 goal's stage 0:
- `lapidary-test-db` is up;
- `cargo deny check` passes on `main`, or its advisory is fixed first on its own branch.

### 1. Batched access tracking: `fix/access-tracking`

Follow DATA §1.4.
- **The map.** The api process keeps an in-memory map from blob hash to last read time.
- **The flush.**
  - Every 5 minutes, and on graceful shutdown.
  - One statement that stays static SQL: `UPDATE … FROM unnest($1::text[], $2::timestamptz[])`.
  - It writes only rows more than a day stale.
- **Where the code lives.** SQL stays in `lapidary-db`, and the map lives in the api's state.
- **Tests.**
  - Existing tests that read `last_accessed_at` call an explicit flush.
  - A new test shows that 300 reads of one blob cause one write.
  - A new test shows that a row touched within the last day is not rewritten.

### 2. Phantom blob bytes: `fix/phantom-blob-bytes`

- **Scope.** Fix `PurgeReport.quarantined_bytes` and `ReapReport.bytes` together, as the purge spec
  asks.
- **Default rule.** A hash with no content-addressed copy contributes no `stored_bytes` of its own.
  The model file's bytes are counted once, where the file is.
- **Instance storage.** If `instance_storage` / `storage_totals` count the same bytes twice, fix them in
  the same branch.
- **Test.** Re-run the spec's scenario as a test: one 9,684-byte part with one 3,292-byte rung.
  - The purge reports 12,976 bytes.
  - The sweep reports 12,976 bytes.
  - 12,976 bytes leave the store.

### 3. Two jobs, one new path: `fix/new-path-race`

- **The change.** Writing a new part's file must refuse an existing target instead of replacing it.
  - Add a create-new variant beside `put_at`. For example, write the temporary file, then
    `std::fs::hard_link` it to the target, which fails if the target exists, then remove the
    temporary file.
  - Atomic, and no new dependency.
- **What the loser does.** It re-reads the part at that path and decides again, by slice 1 spec §1:
  - same bytes: `skipped`;
  - controlled library: a revision;
  - hobby library: `unkept`.
- **Test.** Two concurrent ingests of different bytes onto one new path. Both byte sets survive, or
  one is reported `unkept`. Nothing is silently replaced.
- **ROADMAP.** Remove the line about this race from Phase 4 slice 1's "Recorded rather than fixed",
  citing this branch's hash.

### 4. Re-parenting: `fix/folder-reparent`

- **The route.** `PATCH /api/folders/{id}` refuses `parent_id`.
  - The message says a category cannot move under another one yet.
  - It says what to do instead: move the parts.
- **Dead code.** `renamedAfterMove` and `wouldCycle` are then unreachable, so remove them and their
  strings instead of testing them.
- **Tests.** One test that the refusal happens, and that nothing on disk or in the database changed.
- **DATA.** Update the "Re-parenting a category" paragraph.

### 5. Watch the mutation-checked tests fail: no branch unless something is found

- **The procedure.** For each slice 1 revision and lock test, break the one line of code it guards.
  Watch it fail, then restore the line.
- **Recording.** Put the result in the ROADMAP record: which tests failed as expected, and which
  didn't.
- **A test that doesn't fail** is a real finding. Fix it on `test/<name>` with a test that does fail.

### 6. Measurements: no branch unless a finding needs code

- **Native stack, as the slice 2 goal describes.** Put numbers in ROADMAP with how they were taken.
- **Phase 1 drop path.**
  - Load the 150-file corpus through the folder input, using CDP `DOM.setFileInputFiles` on the
    `webkitdirectory` input.
  - Time from handing over the files to the batch finishing.
  - Take the grid's search round trips during that ingest; that is the number for "interactive
    immediately".
- **Folder tree fan-out.**
  - Build 100, 1,000 and 10,000 folders through the API.
  - Time the tree fetch and its first render.
  - Record where it degrades.
- **Link open over 400 ms.**
  - Take a CDP performance trace of a link open on SwiftShader and on the GPU (if one is present).
  - Split it into page load, part fetch, viewer chunk, first rung and first frame.
  - Change nothing unless one phase dominates and its fix is small.
- **L0 and L1 quantization.** Only if that trace shows rung transfer or decode dominating. L2 stays
  lossless.

## Before teardown

- A fresh reader reviews this goal's diff: a subagent, or the advisor.
- The ROADMAP record is updated after each merge, not at the end.

## How to work, stop conditions, when done

Exactly as `2026-09-15-phase-4-slice-2-goal.md` states in "How to work", "Stop and ask before" and
"When done". The essentials:
- **Worktree:** your own, from local `main`.
- **TDD:** see each test fail first.
- **Gates:** `cargo xtask export-bindings`, then
  `CARGO_BUILD_JOBS=4 DATABASE_URL=postgres://lapidary:localdev@localhost:55432/lapidary cargo xtask verify slice`
  in the foreground, logging to `target/`.
- **Chain gate, commit and merge with `&&`.**
- **Commits:** conventional, with no AI attribution trailer.
- **Merges:** `git merge --no-ff` into local `main`. Never push.
- **Stop and ask before:**
  - any Docker image build, pull or prune;
  - anything that needs sudo;
  - pushing;
  - touching user data or `deploy/.env`;
  - a decision that contradicts a doc.
