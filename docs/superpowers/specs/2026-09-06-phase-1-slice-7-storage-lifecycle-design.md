# Slice 7 — delete that is not deletion

**Status:** design, binding for slice 7. **Branch:** `feat/storage-lifecycle`.

Phase 1 shipped a pipeline that only adds. A user can point Lapidary at 1,703 files and
index every one of them; there is no way to remove a single one. That is the gap this
slice closes, and it closes it under the product rule that governs the whole area:

> We never delete user data implicitly. Delete is soft. Purge is separate and explicit.
> Blobs quarantine 30 days before removal. Derivative cache eviction is a different
> action with different wording and must never read as data loss.
> — `CLAUDE.md`, non-negotiable product rules

---

## 0. Two documents disagree about where this belongs, and one number is stale

**`ROADMAP.md:84` puts "Storage tiering job, quarantine, three-step deletion" in Phase 4**,
next to the watcher and the agent. The plan of 2026-09-05 has no slice 7 section at all —
it stops at slice 6. So this slice is a Phase-4 item pulled forward on the owner's
priority call, and what it ships is the *subset a Phase-1 user can hit today*: a library
you can remove things from.

What stays in Phase 4, where the roadmap already puts it: **storage tiering**. Tiering is
what makes quarantine's physical layout matter, and building the layout without the job
that uses it is building for a caller that does not exist.

`ROADMAP.md:84` is amended by this slice to say what remains there, rather than leaving
two documents disagreeing. Part F of the plan carries the amendment.

**The stale number.** The plan's Part D says `blob.ref_count` is "never written". That was
true when it was written and is not true now:

| column | written today? | by what |
|---|---|---|
| `ref_count` | **yes** | `+1` per `file` row and per `derivative` row in `insert_part_chain` (`repo.rs:582`, `:624`); moved on rung replacement (`repo.rs:498`) |
| `last_accessed_at` | **yes** | `PgBlobs::touch_blob` (`repo.rs:302`) |
| `deleted_at` | tests only | no route sets it |
| `quarantined_at` | **no** | zero references outside migration `0002` |

So the slice is not "make `ref_count` work". The counter is live and its arithmetic reads
correct. The slice gives it a decrementer and a collector.

---

## 1. The audit, and why it is not enough to build a reaper on

A reaper that trusts a drifted counter deletes bytes a live row still serves. That is the
one bug in this slice that loses user data, so it was measured before anything was
designed. Recomputing the true count from reachability and comparing it against the stored
one, across the 150-part compose corpus:

```
 agree | disagree | overcounted | undercounted
-------+----------+-------------+--------------
   300 |        0 |           0 |            0
```

**This proves less than it looks like it proves.** The same database:

```
 derivatives | distinct_file_hashes | file_rows | shared_blobs | libraries
-------------+----------------------+-----------+--------------+-----------
         300 |                  150 |       150 |            0 |         1
```

Zero shared blobs means the corpus never took the `link_existing` branch. No rung was ever
re-rendered, so `repo.rs:498`'s move-a-reference arithmetic never ran. One library, so no
cross-library sharing. The agreement covers the simple path and only the simple path — and
`link_existing` is exactly where a reference counter drifts.

The response is not a bigger corpus. It is to **make `ref_count` non-load-bearing for
every destructive decision**, so that the audit's coverage stops gating the slice:

- **Purge does not decrement. It recomputes.** (§3)
- **The reaper re-verifies reachability inside the transaction that removes the bytes.** (§4)

With both, a drifted counter can waste disk (drifted high) or trigger a quarantine the
reaper declines to complete (drifted low). It cannot destroy a reachable blob. The counter
becomes what it should always have been: a fast hint and a number to show the user.

---

## 2. Delete is one `UPDATE`, because the read paths were already written for it

Every read path already filters `part.deleted_at IS NULL`:

| method | line |
|---|---|
| `PartRepository::page` (the grid) | `repo.rs:1235` |
| `PgParts::detail` | `repo.rs:801` |
| `PgParts::library_of` | `repo.rs:915` |
| `PgBlobs::source_for_download` | `repo.rs:1035` |
| `PgParts::storage_totals` | `repo.rs:1100`–`:1109` |

So "delete hides the part" needs no query changes. `DELETE /api/parts/{id}` sets
`deleted_at = now()`, and the part leaves the grid, the detail route, the download route
and the storage totals at once. `POST /api/parts/{id}/restore` clears it.

**`PgBlobs::library_holds` must keep *not* filtering `deleted_at`** (`repo.rs:335`, where
the decision is already documented). It is the load-bearing exception, and it answers the
question this slice must get right:

> The user deletes `brackets/lm8uu-holder.stl`. The file is still on disk. The next scan
> finds it. What happens?

Nothing happens, and nothing is what the user asked for. Deleting from the index is a
decision; a scan re-adding it would make delete useless — the next scan would resurrect
everything. Both branches already produce that today:

- **Same bytes** → `library_holds` returns true regardless of `deleted_at`, the hash
  short-circuit settles the file as `Skipped`.
- **Changed bytes** → the insert violates `part_source_path_unique_per_library`, and
  `classify_write` maps that violation to `Skipped`
  (`crates/lapidary-ingest/src/handler.rs:503`).

Zero code, in both branches. It is a property of what 6a built, and it is untested — this
slice tests it, because it is the first thing anyone will try and nothing currently stops
a later change from breaking it silently.

Undelete stays explicit, for the reason delete does: we do not un-delete implicitly either.

---

## 3. Purge recomputes rather than decrementing

Purge is the second step and a different action with different wording. It removes the
part chain — `part`, its `revision` rows, their `file` and `derivative` rows — and it is
only offered for a part that is already deleted.

The reference update is where this design earns its keep. **Not** `ref_count - 1`:

```sql
WITH doomed AS (
    SELECT DISTINCT blake3 FROM (
        SELECT f.blake3 FROM file f JOIN revision r ON r.id = f.revision_id WHERE r.part_id = $1
        UNION ALL
        SELECT d.blake3 FROM derivative d JOIN revision r ON r.id = d.revision_id
        WHERE r.part_id = $1 AND d.blake3 IS NOT NULL
    ) h
)
-- ... delete the part chain ...
UPDATE blob b SET ref_count =
      (SELECT count(*) FROM file f WHERE f.blake3 = b.blake3)
    + (SELECT count(*) FROM derivative d WHERE d.blake3 = b.blake3)
WHERE b.blake3 IN (SELECT blake3 FROM doomed);
```

Three properties, all of them the point:

1. **The doomed hashes are collected before the delete.** After the part chain is gone
   there is no path from the part to its blobs.
2. **The recompute is self-healing.** Whatever drift `link_existing` or a rung replacement
   left behind is corrected the moment a purge touches that hash. The destructive path is
   the one path that cannot act on a wrong number.
3. **It is not a race.** `UPDATE blob SET ref_count = (subquery)` takes the same row lock
   that ingest's `ref_count + 1` takes, so a concurrent ingest of the same bytes and a
   purge serialize on the `blob` row. Either order is correct: recompute-then-increment
   counts the new `file` row, increment-then-recompute counts it too. The mixed scheme
   (arithmetic on ingest, recompute on purge) is safe because of that lock, not in spite
   of it.

A hash that lands at `0` gets `quarantined_at = now()` in the same transaction. A hash
that lands above `0` is still serving another part and is left alone — which is what makes
purging one of two identical files safe.

---

## 4. Quarantine is a timestamp, and the reaper distrusts it

**This deviates from `DATA.md` §1.6**, which says quarantine "moves the blob to
`quarantine/`". It does not move. `quarantined_at` is set, the bytes stay at
`blobs/ab/cd/<hash>`, and:

- **Restore is free.** Clearing the column restores the blob. A physical move needs the
  reverse move, and needs every reader (`SourceReader`, the derivative store) to know a
  second location, which is a second lookup on the hot path to serve a case that is
  supposed to be rare.
- **The store's layout is about to move anyway.** The folder-tree work relocates it. A
  `quarantine/` tree built now is built against a layout with a pending change — the same
  reason the `part_image` gallery was deferred out of 6b.

`DATA.md` §1.6 is amended to describe the column. The three steps and the 30 days do not
change; only where the bytes sit during step three.

**The reaper** (`PgBlobs::reap`, called by a job) removes a blob when
`quarantined_at < now() - interval '30 days'`. Inside the transaction that deletes the
row and the file, it re-asks reachability:

```sql
DELETE FROM blob b WHERE b.blake3 = $1
  AND b.quarantined_at < $2
  AND NOT EXISTS (SELECT 1 FROM file f WHERE f.blake3 = b.blake3)
  AND NOT EXISTS (SELECT 1 FROM derivative d WHERE d.blake3 = b.blake3)
```

The `NOT EXISTS` pair is the safety property from §1, and it is deliberately redundant with
`ref_count`: it is the reason a counter this slice cannot fully audit is still safe to
ship. If a row appeared since quarantine — the bytes were re-ingested, `link_existing`
pointed a new `file` row at them — the `DELETE` matches nothing, and the reaper clears
`quarantined_at` instead. Re-ingest un-quarantines by existing.

**On day one the reaper removes nothing.** No row is 30 days old. The destructive path is
proved by tests with the cutoff injected, never by a wall clock — a test that waits 30 days
is a test that does not run.

---

## 5. Eviction says something else

Derivative cache eviction is not in this slice. What is in this slice is the **wording
boundary**, because `CLAUDE.md` requires eviction never to read as data loss, and the
strings are cheaper to get right before there are two of them.

Three actions, three vocabularies, in `web/src/lib/strings.ts`:

| action | says | undo |
|---|---|---|
| delete | "Remove from library" — *"Hidden from the library. Nothing on disk changes."* | Restore, indefinitely |
| purge | "Purge permanently" — names the part, requires confirmation | Bytes recoverable for 30 days |
| evict (Phase 4) | "Free cache space" — *"Thumbnails and previews regenerate on demand."* | Nothing to undo |

Eviction is not a deletion with softer words. It removes only bytes we produced and can
reproduce, which is why it is the one of the three that needs no undo. No eviction policy,
no eviction route, no eviction job here.

---

## 6. `last_accessed_at` and `HEAD`

Carried from slice 6a: `HEAD` on a blob does not warm `last_accessed_at`, because only
`get(handler)` calls `touch_blob`. It is a lifecycle column, so it lands here: `HEAD` warms
it too. A client that checks a blob is present is a client using that blob, and a tiering
job that reads the column in Phase 4 would otherwise tier out bytes that are in active use.

---

## 7. Acceptance

1. `DELETE /api/parts/{id}` hides the part from the grid, the detail route, the download
   route and the storage totals. `POST /api/parts/{id}/restore` brings it back with its
   revisions, files and thumbnail intact.
2. Re-scanning a deleted part's path does not resurrect it — proved for **both** branches:
   unchanged bytes (hash short-circuit) and changed bytes (constraint → `classify_write`).
3. Purge of a part whose blob is shared with a second part leaves the blob at `ref_count`
   1 and un-quarantined; the second part still downloads byte-identical bytes.
4. Purge of the last part naming a blob lands it at `0` with `quarantined_at` set, and the
   bytes are still on disk and still readable by hash.
5. A blob whose stored `ref_count` was corrupted to a wrong value is corrected by the next
   purge that touches it, and the reaper does not remove it while a `file` row exists.
6. The reaper removes a blob quarantined past the injected cutoff, and clears
   `quarantined_at` instead of removing one that was re-ingested after quarantine.
7. `HEAD` on a blob advances `last_accessed_at`.
8. `cargo xtask verify slice` passes all 14 gates. Every task's mutation is reverted
   byte-identically.
