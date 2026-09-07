# A purge that removes the model directory

**Slice:** `2026-09-07-purge-removes-the-model-directory`
**Closes:** the gap pinned by
`crates/lapidary-ingest/tests/reap.rs::a_purged_model_directory_outlives_the_sweep_that_reports_removing_it`,
written up in `docs/superpowers/plans/2026-09-07-after-the-folder-tree-what-is-next.md` §2.

---

## 0. The gap, precisely

`PgBlobs::reap` unlinks `blobs/<ab>/<cd>/<hash>`. That was every source file's address
until the folder tree; since then it is only the address of files `migrate_storage` has
not reached. A migrated part keeps its bytes at `file.storage_path`, inside a model
directory of its own, with a `metadata.json` beside them.

`PgParts::purge` deletes the `file` row. `storage_path` goes with it, and it is the only
record of where the bytes are — `blob` is keyed per hash and a hash can name several model
files, so the path cannot be recovered from it. Thirty days later the sweep unlinks a
content-addressed path that holds nothing, reports the hash as removed (a missing file is
success to a reaper, deliberately), and the model directory stays on disk for good.

**Nothing is lost**, which is the right way round for this area to fail. What breaks is
the promise: `strings.parts.purgeConfirm` tells a person their bytes are kept for 30 days
and then deleted, and for a migrated part the second half never happens. Every part
ingested from now on is migrated, so this is the ordinary case rather than an edge one.

## 1. Shape

A second quarantine, keyed on the path instead of the hash, swept by the same timer under
the same 30-day constant.

Not a column on `blob`, and the reason is the whole design: `blob` is one row per hash and
source dedup is gone (folder-tree design §0), so one hash can be several model files with
several independent lifetimes. A per-hash column could describe at most one of them.

The two quarantines answer different questions and neither subsumes the other:

| | `blob.quarantined_at` | `quarantined_file` |
|---|---|---|
| Keyed on | hash | store-relative path |
| Enters when | recomputed `ref_count` reaches 0 | the part that owned the file is purged |
| Shared? | yes — several parts may reference one blob | no — one model file, one part |
| Guard before removal | no `file`/`derivative` row names the hash | no live `file` row names the path |
| Bytes it names | `blobs/<ab>/<cd>/<hash>` | the model file, plus its `metadata.json` |

## 2. Schema — migration `0014_quarantined_file.sql`

```sql
create table quarantined_file (
  storage_path    text primary key,
  blake3          text not null,
  stored_bytes    bigint,
  quarantined_at  timestamptz not null default now()
);

create index quarantined_file_quarantined_at_idx on quarantined_file (quarantined_at);
```

**No foreign key to `blob`.** The two quarantines are swept in one transaction but they
are not one lifetime: a hash whose last *other* reference goes away is reaped on its own
clock, and a `blake3` column that refused to outlive its `blob` row would make the file
row disappear with it — losing the only record of the path. The column is here to name the
bytes in a log and to give a future restore something to look for, not to constrain.

**The index is plain, not partial**, which is the opposite of
`blob_quarantined_at_idx`'s deliberate `where quarantined_at is not null`. Every row in
this table is quarantined by construction — that is what the table *is* — so a partial
index would hold exactly the rows a full one holds and cost an extra predicate to say so.

**`stored_bytes` is nullable**, matching `file.stored_bytes`, which is nullable for a row
written before migration `0013` by something outside `insert_part_chain`. A file whose
size was never recorded still has to be removable; it contributes nothing to the sweep's
byte total rather than blocking the removal.

## 3. Purge writes the row

`PgParts::purge` already collects the doomed hashes *before* deleting the chain, because
afterwards there is no path from the part to its blobs. The paths are collected in the
same place and for exactly the same reason.

```sql
SELECT f.storage_path, f.blake3, f.stored_bytes
  FROM file f JOIN revision r ON r.id = f.revision_id
 WHERE r.part_id = $1 AND f.storage_path IS NOT NULL
```

**No `role` filter**, unlike `storage_totals` and the grid's source LATERAL. Those two ask
"what is this part's source file"; this one asks "what has this part put on disk". A row
with a `storage_path` is a file in a model directory whatever its role, and after the
purge nothing references it. Only `source` rows carry a path today — `put_at` has one
caller — so the clause is inert now and correct when that stops being true.

A part whose files have no `storage_path` writes no rows here. That is the un-migrated
case, and the blob quarantine already covers it; the two are complementary, not
alternatives.

### 3.1 On conflict, the clock restarts

```sql
ON CONFLICT (storage_path) DO UPDATE
   SET blake3 = excluded.blake3,
       stored_bytes = excluded.stored_bytes,
       quarantined_at = now()
```

This is the opposite of what purge does for a blob, which keeps the running clock
(`coalesce(b.quarantined_at, now())`), and the difference is not an inconsistency — it is
the same rule applied to a different key. *The clock belongs to the bytes.* A hash
identifies its bytes, so a second purge touching an already-quarantined blob is touching
the same bytes and must not restart their hold. A path does not identify its bytes: the
file at `libraries/default/vee-block/vee-block.stl` today need not be the file that was
there when the row was written.

Reaching the conflict at all takes a person: `model_dir_for` disambiguates against a
directory that exists, so a re-ingest after a purge lands at `<model>_<hash6>` and not on
the quarantined path. What puts it back in play is the store being **browsable** — the
whole point of the layout. Delete the model directory in a file manager, re-scan, purge
again, and the same path arrives holding different bytes. Restarting is the safe answer to
that, and it is the reaper's own stated principle: losing bytes is worse than keeping them.

## 4. The sweep removes it

`PgBlobs::reap` grows a second half in the **same transaction**, taking a second closure.

```sql
DELETE FROM quarantined_file q
 WHERE q.quarantined_at < now() - make_interval(secs => $1)
   AND NOT EXISTS (SELECT 1 FROM file f WHERE f.storage_path = q.storage_path)
RETURNING q.storage_path, q.stored_bytes
```

The `NOT EXISTS` is the mirror of the blob half's two, and it does the job that half gets
from a foreign key: `file.blake3` references `blob`, so a referenced blob row cannot be
deleted whatever a query says, and the reachability check merely lets the sweep *decline*
rather than fail. There is no foreign key from `file.storage_path` to anything, so here
the check is the whole guard rather than a politeness in front of one.

### 4.1 Unlink before commit, and only the file is fatal

The blob half unlinks while the transaction still holds the deleted row. The file half
keeps that ordering — a crash between an unlink and a commit that never lands leaves a row
naming bytes that are gone, and a sweep that committed first would leave bytes with no row
at all, which nothing would ever collect.

Three removals per row, in this order:

1. the file at `storage_path` — **fatal on error**, aborting the sweep, so the row and the
   bytes survive together and the next hour tries again;
2. `<model directory>/metadata.json` — fatal for the same reason. A missing one is already
   success (`remove_at` maps `NotFound` to `Ok`);
3. the model directory, if empty — **not fatal**. `remove_dir_if_empty` answers
   `DirectoryNotEmpty` for a directory the owner has put something of their own into, and
   the store is a directory the owner opens. Removing what they left is the data loss
   reaping exists to prevent; failing the sweep over it stops every *other* removal for as
   long as that file sits there. It is logged and skipped.

The order matters for a crash, not for a success. File first means every partial state is
"the bytes are gone, the tidying is not finished", which the next sweep completes. Removing
`metadata.json` first would leave a directory holding bytes and no manifest — which the
folder-tree spec calls an orphan, and which reads differently from an empty one to anything
that walks the store.

`remove_dir_if_empty` is left alone rather than taught to answer `Ok` on `DirectoryNotEmpty`.
Its doc says it "reports rather than insists", its existing caller in `handler.rs` logs and
continues, and moving the decision into the function would take it away from both.

### 4.2 The other half: a path claimed again

Mirroring the blob half's un-quarantine, and not conditional on the cutoff:

```sql
DELETE FROM quarantined_file q
 WHERE EXISTS (SELECT 1 FROM file f WHERE f.storage_path = q.storage_path)
RETURNING q.storage_path
```

A row is dropped rather than un-flagged, because unlike a blob there is no `ref_count` to
correct and no state to return to — the path is claimed, so the record of it being doomed
is simply wrong.

## 5. What the report says

`ReapReport` gains `removed_files: Vec<String>`; `bytes` sums both halves, and
`un_quarantined` counts both. Those bytes genuinely left the volume, which is what the
field has always meant.

## 6. Non-goals

- **`PurgeReport.quarantined_bytes` is not corrected.** For a migrated source it counts
  `blob.stored_bytes` — a figure describing `blobs/<ab>/<cd>/<hash>`, which
  `migrate_storage` emptied. That is a pre-existing wrinkle in what the *blob* row means
  after the folder tree, not something this slice introduces, and correcting it means
  deciding what a `blob` row's size means for a hash with no content-addressed copy left.
  Left for the slice that answers that.
- **No restore.** `DATA.md` §1.6 calls a quarantined blob "restorable"; nothing implements
  restore for either quarantine today, and this slice does not start.
- **No instance-wide storage view.** A quarantined file's library *is* knowable — the
  slug is in its path — where a quarantined blob's is not. `library_id` is deliberately
  not recorded anyway: adding it would invite a library's storage panel to count bytes the
  panel is documented not to count, and Phase 4's instance-wide view is where that belongs.

## 7. `DATA.md` §1.6 changes

- The open question "How long the hold should be for path-addressed sources" is answered:
  **the same thirty days**, one constant (`reap::QUARANTINE`) for both quarantines. A
  second retention would be a second promise to explain in the confirmation dialog, and
  nothing has argued the two should differ.
- "Reachable by hash" is true of a quarantined blob and false of a quarantined file, whose
  bytes sit at a path no route serves once the `file` row is gone.

## 8. Testing

`crates/lapidary-ingest/tests/reap.rs`, against the real store:

1. **The tripwire, inverted.** `a_purged_model_directory_is_removed_once_its_thirty_days_are_up`
   replaces `a_purged_model_directory_outlives_the_sweep_that_reports_removing_it` — same
   fixture, opposite assertion, plus `metadata.json` and the directory itself.
2. **Inside the hold, nothing moves.** The production `QUARANTINE`, against a file
   quarantined seconds ago — what every sweep on a new installation does.
3. **A path claimed again is declined, not unlinked.** A live `file` row at the quarantined
   path: the row goes, the bytes stay. This is the guard, and it is the one assertion that
   fails loudly if the `NOT EXISTS` is ever dropped.
4. **A directory holding something else survives, and the sweep does not fail.** The
   owner's own file beside the model: the model file goes, their file stays, the directory
   stays, and the sweep still reports success and removes everything else it was going to.
5. **An un-migrated part still reaps by hash.** The existing tests cover it; this slice
   must not regress them.
