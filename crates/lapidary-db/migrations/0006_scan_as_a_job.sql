-- The directory walk becomes a job (slice 5, task 6): the api route enqueues one
-- `scan_directory` job, and the worker walks the ingest mount and enqueues the per-file
-- jobs into that same batch. `job_done_has_outcome` requires every finished job to say
-- how it finished, and a scan job ingests nothing, skips nothing and renders nothing --
-- so it needs an outcome of its own rather than borrowing one that would misreport it.
-- `skipped` reads to a user as "this file was already here", `rendered` makes the grid
-- read the whole batch as a preview render, and `ingested` claims a part that does not
-- exist. See `lapidary_core::Outcome`.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in ('ingested', 'skipped', 'rendered', 'scanned'));
