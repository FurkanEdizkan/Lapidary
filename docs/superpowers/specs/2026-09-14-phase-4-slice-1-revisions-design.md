# Revisions, geometric diff, check-out locks and the Linux agent

**Slice:** `2026-09-14-phase-4-slice-1-revisions`
**Closes:** the silent loss of a changed file, and the Phase 4 stubs in `bin/lapidary`.

---

## 0. The gap, precisely

Every ingest writes a new part with revision `'1'` (`insert_part_chain`, `repo.rs`). A file
at a `source_path` the library already indexes, holding new bytes, hits
`part_source_path_unique_per_library`, and `classify_write` (`handler.rs`) settles that as
`Outcome::Skipped`. The batch says "already here". The new bytes are not kept anywhere, in
any library, and nothing says so.

The read side is already revision-aware: the grid and the detail pick the latest revision by
`created_at DESC, id DESC`, rungs and thumbnails key on revision ids, and
`GET /api/revisions/{id}/download` serves any revision's `file.storage_path`. What is
missing is the write.

## 1. Who gets a revision

A file at a path the library already indexes, after BLAKE3:

| The part at that path | Bytes | Library | Outcome |
|---|---|---|---|
| none | — | any | `ingested`, today's path, unchanged |
| live | same hash as the current revision | any | `skipped` |
| live | different | hobby | `unkept`, nothing written |
| live | different | controlled | `revised` |
| soft-deleted | any | any | `skipped`, as today — restore it first |

**A revert is a revision.** Bytes equal to an *older* revision but not to the current one are
history, not a skip.

**`unkept` is a notice, not a failure.** Governance is opt-in (`CLAUDE.md`): a hobby library
keeps no revisions, and it still does not. What changes is that it says so. Nothing broke, the
new bytes are still where they came from, and a retry cannot change the answer, so a failure
row with a retry button would be the wrong shape. The batch line counts them and says what to do:
switch the library to controlled, or give the file a new name to add it as a new part.

**Switching is one-way:** `POST /api/libraries/{id}/controlled`. A hobby library has no history
for the switch to strand. A controlled library switched back would hold revisions no screen
shows, so that direction is not in this slice.

## 2. A revision

- **Label:** the highest numeric label plus one, read under the part's row lock.
  `unique (part_id, rev_label)` backs it.
- **Parent:** the revision that was current when the job looked. If it is no longer current
  under the lock, the write is `RevisionConflict`, retried as `Transient`, and the retry decides
  again from §1.
- **Origin:**
  - `ingest` for a scanned file (`IngestFile`);
  - `upload` for browser bytes (`IngestBlob` without a lock);
  - `agent` for bytes that carry a lock.

  A *new* part keeps `ingest` whatever its route. `insert_part_chain` is not touched by this
  slice, and that is recorded rather than fixed.
- **Author:** null. There are no users yet.
- **Figures and derivatives:** the kernel's output, exactly as ingest writes them, keyed to the
  new revision. The previous revision's rungs and thumbnail stay.

## 3. On disk: newest on top

```
libraries/<library>/<category…>/<model>/
  flange-dn40-lp-3310-02.stl        the current revision, always
  metadata.json                     every revision, oldest first
  revisions/
    1/flange-dn40-lp-3310-02.stl    byte-identical to what revision 1 ingested
```

- **The path rule is the record.** An older revision lives at `revisions/<rev_label>/<name>`,
  so `ManifestFile` gains no field.
- **The file an owner opens is current.** A file manager, a slicer's recent-files list and a
  re-scan all see the newest bytes at the path they already knew.
- **Downloads** read `file.storage_path`, which the revision's transaction rewrites for the
  previous file. `variant=original` stays byte-identical, and the route is unchanged.
- **Moves** rewrite every `storage_path` under the model directory by prefix, `revisions/`
  included.
- **Purge** already quarantines every revision's file: its query joins all revisions and filters
  no role (purge design §3). The reaper removes each file, then removes `revisions/<label>`,
  `revisions` and the model directory, each only if empty, so any reap order leaves nothing behind.

### 3.1 Ordering

`PgRevisions::record_revision` is shaped like `move_to_folder`. The file operations travel into
the transaction as a closure:

1. `BEGIN`; `SELECT … FROM part WHERE id = $1 FOR UPDATE`; read the current revision and its
   source `storage_path` *under that lock*.
2. Refuse a stale parent, before any file is touched.
3. Insert the revision, its source file row and its derivatives. Rewrite the previous file row's
   `storage_path` to `revisions/<the previous revision's label>/<name>`, so `revisions/1/`
   holds revision 1.
4. Run the closure with the current path and that set-aside path. It:
   - creates the set-aside directory;
   - refuses an existing target;
   - renames the current file into it;
   - `put_at`s the new bytes at the current path;
   - renames back if `put_at` failed.
5. If the closure failed, roll back explicitly and return `RenameFailed` (`Transient`).
6. Commit. If the commit fails after the closure ran, undo it: remove the new file and rename
   back. Then `Transient`.

**Why the row lock comes before the files:**
- A move takes the same row (its `UPDATE part`), and so does a second revise of the same part.
- Without the lock, the second job renames the first job's new file onto
  `revisions/<label>/<name>`.
- `std::fs::rename` replaces an existing file without a word, and revision 1's bytes are gone.
- Hence also the closure's own refusal of an existing target, which costs one `stat`.

**The window that remains** is a commit that fails after the rename *and* an undo that also
fails. That takes a dropped connection and a disk error in the same second. The disk is then
ahead of the database: revision 1's row names `<model>/<name>`, which holds the new bytes, and
its own bytes sit in `revisions/<label>/`. Nothing is deleted. The undo's error is logged with
both paths. The folder-tree design's argument for rename-first applies unchanged.

**This path never goes through `classify_write`.** A unique violation there means "already
here", which is the silence this slice removes. A `rev_label` violation here is a lost race,
and it is `Transient`.

### 3.2 `metadata.json`

Rewritten after the commit, from the database rows, listing every revision oldest first, so
`revisions[0]` stays the original. It is warn-only, for ingest step 10's reason: the revision is
committed, and failing the job would say a file did not arrive when it did.

### 3.3 A gap left open

Two *different* jobs can race different bytes onto one *new* path. Both see no part. The loser's
`put_at` replaces the winner's file before its insert fails and settles as `skipped`. That is
today's behaviour. This slice does not widen it and does not fix it.

## 4. Geometric diff

Between any two revisions of one part (DATA §6.1, the subset the stored figures support):

- volume, surface area, bbox x/y/z, triangle count;
- each delta has from, to, the absolute change, and the percent when `from` is not zero;
- **a delta is approximate if either operand is.** The provenance columns already say which
  values are mesh-derived, and `Approximate<T>` carries that to the UI's ≈;
- **a missing operand gives no delta, never zero.** A figure that was not measured did not change
  by nothing.

It is pure code in `lapidary-vcs` over rows. The kernel is never involved, so the diff honours
the open path's rule.

## 5. Check-out locks

- A `part_lock` table. A partial unique index on `part_id WHERE released_at IS NULL` allows one
  active lock per part.
- **Holder:** free text, `$USER@$HOSTNAME` from the agent. There are no users.
- **A lock id is an identifier, not a secret.** There is no auth. Anyone who can reach the api
  can release any lock. A forced release is recorded (`forced`, `released_by`) and shown, and
  that record is the whole of the protection in this slice.
- **Hobby libraries refuse a lock**, with the switch from §1.
- **Enforced only when held:**
  - No active lock: any path revises, a scan included.
  - An active lock, and a job that does not carry it: refused as a Permanent failure naming the
    holder and since when. Check it in, or release it, then retry.
  - A job carrying a lock that is no longer active: refused, naming who released it and when.

  Checked inside `record_revision`'s transaction, under the part's row lock. These *are* failures:
  the bytes were not kept for a reason somebody can act on, and a retry after they do will work.
- **`revision.locked_by` and `locked_at` are dropped.** Nothing reads them, and a lock on a
  revision is the wrong grain: a check-out is taken before the next revision exists.

## 6. The agent, Linux only

- `lapidary checkout <part>`:
  - takes the lock;
  - downloads `variant=original` to `<workspace>/<part_number or name>_<rev>/<file name>` (DATA
    §6.2's flat layout);
  - writes `.lapidary-checkout.json` beside it, holding the server, library, part, base
    revision, lock, file name, hash and holder.
- `lapidary checkin <folder>` releases the lock and leaves every file in place. It renames
  `.lapidary-checkout.json` to `.lapidary-checked-in.json`, so the agent stops watching and the
  record stays. A lock somebody already released is reported, and the folder is still marked.
- `lapidary agent` watches every checkout.

**Polling, not OS events.**
- **Rules:**
  - The agent polls the one checked-out file every 500 ms.
  - A new size or mtime starts the settle.
  - Two seconds unchanged ends it.
  - Then BLAKE3; the last known hash means nothing happened.
- **Why polling:**
  - It needs no new dependency.
  - It watches one file per checkout.
  - An editor's write-temp-then-rename reaches the watched path as a new mtime anyway.
- **§6.2's ignore list holds by construction.** Only the named file is watched, so `.tmp`, `.bak`,
  `~$*` and `.lck` never enter.
- **When to change:** move to inotify/FSEvents/`ReadDirectoryChangesW` when a checkout holds more
  than a handful of files, or on macOS and Windows.

A changed hash is uploaded through probe, chunks and commit carrying the lock. The agent follows
the batch and records the new base revision and hash in the checkout file. A refusal prints the
server's message.

**Only the file handed out comes back.** A tool that saves another name or format (FreeCAD's
`.FCStd`, a slicer's project file) is not picked up. DATA §6.3's honesty rule covers saying
that.

## 7. Not in this slice

- The overlay diff and the Hausdorff heatmap.
- The `lapidary://` scheme and launching a tool.
- macOS and Windows watchers.
- The `Target` trait.
- Storage tiering.
- Face and edge count deltas, which need STEP entities and therefore OCCT to check.
- Mass and centre of mass, since nothing stores a density.
- Switching a controlled library back to hobby.
- Users, and auth on locks.
- States and approvals, which are Phase 8.
- Origin for a new part's first revision.

## 8. Testing

- **Handler, controlled library:**
  - Changed bytes make revision 2 with its parent. `revisions/1/<name>` is byte-identical, the
    new bytes are on top, and `metadata.json` lists both.
  - The same bytes are skipped.
  - A revert makes revision 3.
  - Moving the part keeps both downloadable.
  - A purge leaves no file or directory.
- **Handler, hobby library:** a changed file is `unkept`, writes nothing and adds no failure row.
- **db:**
  - A stale parent is a conflict, and the closure never runs.
  - A failing closure leaves no revision.
  - History is newest first.
  - Lock rules.
- **api:** history, diff (a revision of another part is a 404), the one-way switch, and lock
  refusals.
- **vcs:** provenance propagation, missing operands, a zero base.
- **Agent:** unit tests for the watcher state machine (debounce, settle, touch without change,
  change during settle), the checkout file round-trip and the folder name. The HTTP half is the
  exit check's.
- **web:** the History section appears only with two or more revisions; the compare table and its
  ≈; the lock line.
