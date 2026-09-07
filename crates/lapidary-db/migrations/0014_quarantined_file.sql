-- Where a purge records the path it is about to forget.
--
-- `purge` deletes the `file` row, and `storage_path` goes with it. Before the folder tree
-- that cost nothing: every source file was at `blobs/<ab>/<cd>/<hash>`, so the hash on the
-- surviving `blob` row was the address. It is not any more. A migrated part keeps its
-- bytes in a model directory of its own, and `blob` cannot hold that path -- it is one row
-- per hash, source dedup is gone (folder-tree design 0), and one hash can be several model
-- files with several independent lifetimes. A per-hash column could describe at most one.
--
-- So this table is the other half of quarantine, keyed on the path. The two are swept by
-- one timer under one retention and are otherwise unrelated: a blob enters quarantine when
-- its recomputed reference count reaches zero and may be shared by parts nobody purged; a
-- file enters when the one part that owned it is purged and is shared with nothing.
--
-- No foreign key to `blob`. The two quarantines are swept in one transaction but they are
-- not one lifetime -- a hash whose last other reference goes away is reaped on its own
-- clock, and a `blake3` that refused to outlive its `blob` row would take the only record
-- of the path with it. The column names the bytes in a log and gives a future restore
-- something to look for; it does not constrain.
--
-- `stored_bytes` is nullable, matching `file.stored_bytes`, which is nullable for a row
-- written before migration 0013 by something outside `insert_part_chain`. A file whose
-- size nobody recorded still has to be removable: it contributes nothing to the sweep's
-- byte total rather than blocking the removal.
create table quarantined_file (
  storage_path    text primary key,
  blake3          text not null,
  stored_bytes    bigint,
  quarantined_at  timestamptz not null default now()
);

-- Plain, and deliberately not partial -- the opposite of `blob_quarantined_at_idx`, whose
-- `where quarantined_at is not null` exists because the quarantined blobs are a handful
-- among millions. Every row in this table is quarantined by construction; that is what the
-- table is. A partial index would hold exactly the rows a full one holds and charge an
-- extra predicate to say so.
create index quarantined_file_quarantined_at_idx on quarantined_file (quarantined_at);
