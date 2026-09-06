-- Slice 7. `quarantined_at` has existed since `0002_parts.sql` and nothing has ever
-- written it; purge does now, and the reaper reads it on a timer.
--
-- The read is the reason for this index. The sweep asks "anything quarantined longer than
-- thirty days?" and on all but a handful of runs the answer is nothing — but without an
-- index that nothing costs a scan of every blob in the library, which on a real corpus is
-- over a million rows, once an hour, forever. A partial index holds only the quarantined
-- ones, so it stays close to empty in the ordinary case and the sweep becomes a lookup
-- that finds no rows rather than a scan that rejects them all.
--
-- Partial and not plain, deliberately: indexing `quarantined_at` across every blob would
-- store an entry per row to answer a question about the few, which is the shape of index
-- that costs more on every ingest than it saves on the sweep.
create index blob_quarantined_at_idx on blob (quarantined_at)
    where quarantined_at is not null;
