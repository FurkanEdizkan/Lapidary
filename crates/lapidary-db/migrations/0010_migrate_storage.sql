-- The `migrate_storage` job needs an outcome of its own, for the reason `0006` gives the
-- scan: `job_done_has_outcome` requires every finished job to say how it finished, and a
-- migration run ingests nothing, skips nothing and renders nothing. It moves bytes a part
-- already had out of `blobs/ab/cd/<hash>` and into that part's own directory, so borrowing
-- `ingested` would report files the user already had as newly added.
--
-- The file half of the move is deliberately NOT here. sqlx runs a migration in one
-- transaction at startup; copying a corpus is neither transactional nor fast, and it needs
-- a worker this migration cannot assume is running. See the design doc §5.2, and
-- `lapidary_ingest::migrate`.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in
           ('ingested', 'skipped', 'rendered', 'scanned', 'migrated'));
