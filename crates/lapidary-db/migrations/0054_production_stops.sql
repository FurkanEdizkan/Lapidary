-- R3: production stops, and similarity goes with it (the owner's pivot, 2026-10-03).
--
-- Lapidary is a visual library: a card shows the part's own picture, the viewer loads the
-- original file when somebody presses **view**, and nothing is rendered. So nothing is queued
-- for rendering either, and this migration settles the rows that would otherwise outlive the
-- code that understood them.
--
-- **Ship this with the binary that goes with it.** A worker on the old binary against this
-- schema fails every `profile_shape` job it claims (`part_shape` is gone from under it); the new
-- binary against the old schema leaves pending rows nobody can explain, because
-- `JobPayload::from_row` no longer parses a `profile_shape` row or a `derive` row naming a
-- render kind. One deploy, both halves.
--
-- What it does NOT do: delete the `derivative` rows themselves. Every `thumbnail`,
-- `tessellation_*`, `structure`, `entities` and `pmi` row is still here and still served if
-- somebody holds its hash. That is R4's migration (0055), which needs `recount`'s predicate and
-- the quarantine path to unlink the blobs safely; a migration cannot unlink a file at all. Every
-- reader of `derivative.kind` filters in SQL or drops what it does not know, so those rows are
-- invisible to this build rather than an error.

-- 1. The queue. A pending or running job of either shape names work no handler can do: the
--    payload would not parse, so the job would fail with a message about a malformed payload
--    rather than about a pipeline that was removed.
--
--    `pending` AND `running`, not pending alone. `PgJobs::dequeue` reclaims a `running` row whose
--    lease has expired — that is the whole design of reclamation without a sweeper — so a running
--    row left here is claimed by the next worker and fails exactly as a pending one would.
--
--    `kind = 'derive'` with a render `produce` only. An export's `derive` row is live work and
--    stays: `export_stl` and `export_3mf` are still built on demand.
DELETE FROM job
 WHERE state IN ('pending', 'running')
   AND (kind = 'profile_shape'
        OR (kind = 'derive'
            AND payload ->> 'produce' IN ('thumbnail', 'tessellation_l0', 'tessellation_l1',
                                          'tessellation_l2', 'structure', 'entities', 'pmi')));

-- 2. Finished history keeps its word, so `job_outcome_known` keeps `profiled`.
--
--    0047 added the value when a shape profile became its own outcome, and a `done` row still
--    holds it (`job_done_has_outcome` requires a finished job to say how it finished). Dropping
--    the value would make every one of those rows unwritable and the constraint a lie about the
--    table's own contents. `Outcome::Profiled` is gone from the Rust enum, so nothing writes one
--    again; `PgJobs::batch_status` counts outcomes by name and never had a `profiled` column, so
--    such a row still totals correctly in the batch it belongs to.
--
--    Stated here rather than done, because the board's rule is that a later restatement of this
--    constraint keeps every value already in it, and the next person to restate it reads this.

-- 3. Shape similarity. Both tables are derived from the L0 tessellation that no longer exists,
--    and the owner's decision is that duplicates are not a feature: identity is the content hash
--    in the background, and no real file is ever refused for sharing a name.
--
--    `part_link` first — it holds a person's decisions about pairs, and `part_shape` holds the
--    profiles those decisions were proposed from. Neither is referenced by anything else (checked
--    against the catalogue: `every_table_referencing_the_part_chain_is_one_purge_deletes_from`
--    reads the same foreign keys), and `PgParts::purge` no longer names them.
DROP TABLE part_link;
DROP TABLE part_shape;

-- 4. The ingest-time thumbnail setting (0005). There is no thumbnail to decide about.
--
--    The column was also the library-existence probe for five callers, which is why the method
--    that read it survives as `PgParts::library_exists` — that is the question each of them was
--    really asking. A library whose owner had turned the setting off looks exactly like one that
--    had not, which is now the only possible state.
ALTER TABLE library DROP COLUMN auto_thumbnail;
