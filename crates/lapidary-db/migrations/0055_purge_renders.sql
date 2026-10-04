-- R4: the rendered derivatives leave the table, and the reaper takes their blobs (2026-10-04).
--
-- R1 moved the card to the part's own picture, R2 took the CAD kernel out, R3 stopped
-- production. So every `thumbnail`, `tessellation_l0/l1/l2`, `structure`, `entities` and `pmi`
-- row here names bytes **nothing can read and nothing can rebuild**: `DerivativeKind` cannot
-- spell those kinds any more, so no route serves one and no job writes one. They are rows
-- waiting out the lifetime of the installation. The owner asked for the space back.
--
-- **The exports stay.** `export_stl` and `export_3mf` are the two kinds a worker still builds,
-- on demand, from the source mesh — so the rule this migration leaves behind is that every row
-- in `derivative` is a kind something can still build. Deleting an export would not be free the
-- way deleting a render is: it is cache a person re-earns by asking again, and there is already
-- a user-triggered, 90-day-age-gated path for evicting one (`PgParts::free_render_cache`, the
-- "free cache space" button), whose promise is that nothing a person loses was theirs. An
-- unconditional sweep of rebuildable rows from a migration would make that button's wording a
-- lie, and that wording is a product rule: eviction must never read as data loss.
--
-- `kind NOT IN (…)` rather than naming the seven dead kinds, so a row of any kind that was ever
-- written — nine have existed, the seven above plus the two exports — is settled by one
-- predicate, and so the statement says what the goal says: only what something can still build
-- survives. The two strings are frozen here on purpose: a migration describes the world at the
-- moment it ran, and `DerivativeKind::as_str` is free to grow a third kind afterwards without
-- changing what this one did.
--
-- # Why the blobs are quarantined rather than deleted
--
-- These derivatives are **our** data, not the user's, so the thirty-day hold is not owed to
-- them. The quarantine path is used because it is the only *safe* unlink, not out of ceremony:
-- `PgBlobs::reap` is the only code in the workspace that unlinks a blob, and it is the only
-- code carrying the unlink-before-commit ordering that makes a concurrent re-ingest of the same
-- bytes safe, and the foreign-key decline that keeps one wrongly-quarantined row from rolling
-- back every legitimate removal beside it. A migration cannot unlink a file at all. So this
-- marks the rows and leaves the unlinking to the hourly sweep, which needs no new code.
--
-- # The guard, and it is copied rather than retyped
--
-- The second statement below is `recount` (`repo.rs`) with its `unnest($1::text[]) AS h`
-- changed to this migration's temporary table and nothing else touched. Its `file` half is the
-- whole guard: **a blob a `file` row still references is untouched**, because a source file and
-- a thumbnail can be the same bytes and because `ref_count` is recomputed here, not
-- decremented. Its `derivative` half is what spares a blob a surviving export still names.
--
-- The omission that predicate carries is worth knowing about and is deliberately not widened
-- here: `part_image.blake3` also references `blob` (`0015`), and `recount` counts only `file`
-- and `derivative`. It cannot bite this migration — nothing has ever written a `part_image` row
-- from a render (`origin = 'rendered'` has no producer), so no blob a dead derivative names is
-- also a gallery picture — but a blob shared between a gallery image and a purged part would be
-- quarantined by `PgParts::purge` and then decline forever in the reaper. That is a
-- pre-existing bug in `recount`, reported with this goal, and fixing it here would mean
-- shipping a different predicate from the one the application runs.
--
-- # Two statements, and the reason it cannot be one
--
-- Every sub-statement of one query sees the **same snapshot**, so a data-modifying CTE's
-- deletions are invisible to the counting subqueries beside it. Measured before this was
-- written, on the four cases the test seeds: folded into one statement the recount reads the
-- rows it has just deleted, every count comes back as it was, **nothing is quarantined at all**
-- and the migration silently does half its job. The temporary table is what puts the delete in
-- the past before the count is taken.
--
-- # What this does not do
--
-- It does not return any disk. The hash-addressed bytes wait out the thirty days and the hourly
-- reaper unlinks them; the inline thumbnails were `bytea` in this table, so their space comes
-- back only with `VACUUM FULL derivative`, which takes an `ACCESS EXCLUSIVE` lock and cannot
-- run inside a transaction — it is a post-deploy command, with the bytes measured either side
-- of it. **Never run the reaper's sweep at retention 0 to get the rest sooner**: the same sweep
-- collects the user's own source files, purged yesterday.
--
-- `thumb_bytes` and its `derivative_storage_is_exclusive` check are left in place. Dropping the
-- column would need the constraint restated and would return no space without the same
-- `VACUUM FULL`, so it buys nothing this migration does not already buy.

-- The hashes whose reference this migration takes away, captured as the rows go. `DISTINCT`
-- because one blob is legitimately named by several rows — a part whose L0, L1 and L2 encoded
-- to the same bytes has one blob and three rows — and `recount` must see each hash once.
-- `blake3 IS NULL` is the inline thumbnail: its bytes are the row, there is no blob to settle.
CREATE TEMP TABLE purged_render_blob (h text PRIMARY KEY);

WITH doomed AS (
    DELETE FROM derivative WHERE kind NOT IN ('export_stl', 'export_3mf')
    RETURNING blake3
)
INSERT INTO purged_render_blob (h)
SELECT DISTINCT blake3 FROM doomed WHERE blake3 IS NOT NULL;

-- `recount`, verbatim but for its source of hashes. `coalesce(b.quarantined_at, now())` is what
-- stops this from restarting a clock that is already running, and the `ELSE NULL` arm is what
-- un-quarantines a blob whose count came back above zero.
WITH counts AS (
    SELECT h AS blake3,
           (SELECT count(*) FROM file f WHERE f.blake3 = h)
         + (SELECT count(*) FROM derivative d WHERE d.blake3 = h) AS actual
    FROM purged_render_blob
)
UPDATE blob b SET
    ref_count = c.actual,
    quarantined_at = CASE WHEN c.actual = 0
                          THEN coalesce(b.quarantined_at, now())
                          ELSE NULL END
FROM counts c
WHERE b.blake3 = c.blake3;

DROP TABLE purged_render_blob;
