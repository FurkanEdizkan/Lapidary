-- Slice 6a. The scan becomes recursive, and that alone would lose files.
--
-- `part_name` is the file stem, so descending into subdirectories makes
-- `brackets/bracket.stl` and `plates/bracket.stl` both the part `bracket`. The second
-- violates `part_name_unique_per_library`, and `classify_write` maps a violation of that
-- constraint to `Outcome::Skipped` -- so the file reports as "already here" and is never
-- indexed. Silent, and on a real corpus it is silent for thousands of files.
--
-- So identity moves from the name to the path. The name stays what a person reads in the
-- grid, duplicates and all: two parts called `bracket` in two folders is the truth, and
-- the path is what tells them apart. There is no part-number convention yet to do it
-- instead.

alter table part add column source_path text;

-- Every existing row is flat by construction -- the scan that created it could not
-- descend -- so its original filename is exactly the stem plus the source format. This
-- reconstructs it rather than guessing.
--
-- The lateral, and not a join: a part with no source `file` row (a live part with zero
-- revisions is a state this schema permits and the slice 5 handoff records) must still
-- get a value, or the NOT NULL below strands it. Those fall back to the bare name, which
-- is what they would have been called anyway.
update part p
set source_path = coalesce(
    (
        select p.name || '.' || f.format
        from revision r
        join file f on f.revision_id = r.id
        where r.part_id = p.id and f.role = 'source'
        order by f.created_at desc, f.id desc
        limit 1
    ),
    p.name
)
where source_path is null;

alter table part alter column source_path set not null;

-- The swap, and it must be a swap rather than an addition. Keeping the name unique would
-- reintroduce the collision this migration exists to remove, on the very first nested
-- scan.
--
-- `0003_jobs.sql` records the coupling this inherits: the key must agree with
-- `PgBlobs::library_holds`, or that query returns false and this constraint throws on a
-- path with no reason to expect it. Both move to the path in the same commit, and
-- `classify_write` matches this constraint's name.
alter table part drop constraint part_name_unique_per_library;
alter table part add constraint part_source_path_unique_per_library
    unique (library_id, source_path);
