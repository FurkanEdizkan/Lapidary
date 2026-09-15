-- The if-absent enqueues' own index. `PgJobs::enqueue_if_absent` asks whether a library already has a job of
-- one kind pending or running before it queues another, and `enqueue_migration_if_absent` asks the same of
-- `migrate_storage`. Measured over 10,001 done jobs (goal 6): the first was already answered by combining
-- `job_dequeue_idx` and `job_expired_lease_idx`, and stays at about 0.015 ms; the second, `state IN (...)`,
-- scanned the whole table in 1.4 ms and is now an index-only scan. Partial, as `job_dequeue_idx` is: a
-- finished job is never asked about, and the table keeps those without bound.
create index job_active_idx on job (library_id, kind) where state in ('pending', 'running');
