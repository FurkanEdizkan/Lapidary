-- Task 8b. Nothing enqueued `migrate_storage` until now except its own re-enqueue arm
-- and tests -- an operator could only start one by hand-writing INSERT INTO job. The
-- worker's startup guard (lapidary_db::PgJobs::enqueue_migration_if_absent) closes that,
-- and its whole point is that N workers booting at the same instant against the same
-- un-migrated library must produce exactly one job.
--
-- A bare `INSERT ... SELECT ... WHERE NOT EXISTS (...)` does NOT guarantee that on its
-- own. Verified by hand against Postgres 18.6 before writing this: under READ COMMITTED,
-- a top-level statement takes ONE snapshot at its own start and keeps it for its whole
-- execution -- including a subquery evaluated after the statement blocks on a lock
-- mid-statement, so wrapping the check in an advisory lock taken INSIDE THAT SAME
-- STATEMENT (a CTE ahead of the `WHERE NOT EXISTS`, both part of one INSERT) does not
-- make a blocked transaction's own NOT EXISTS see what unblocked it -- confirmed:
-- reproduced a duplicate row this way, deterministically, before writing this index.
-- This is NOT a claim that advisory locks cannot close the race at all: a lock taken as
-- its OWN, separate statement before the insert -- `reparent`'s shape in folders.rs,
-- one statement to lock, a later one to read and write -- does close it, because the
-- later statement gets a fresh snapshot once the lock is granted. That shape was ruled
-- out here for a different reason: it protects only a caller that remembers to take the
-- lock, where a unique index protects every caller, including a future one that reaches
-- this table directly.
--
-- This index is the actual correctness mechanism: `enqueue_migration_if_absent` and
-- `reenqueue_migration_if_absent` (lapidary-db/src/jobs.rs) both pair
-- `ON CONFLICT (library_id) WHERE kind = 'migrate_storage' AND state = 'pending'
-- DO NOTHING` with it, and unique-index conflict checking is not an MVCC read -- the
-- loser of a genuine race waits on the winner's uncommitted row and then discards
-- itself once the winner commits, rather than reading a stale snapshot.
--
-- Scoped to state = 'pending', not to ('pending', 'running'): `migrate_storage`'s own
-- re-enqueue arm inserts the NEXT run's row while the CURRENT run's row is still
-- 'running', so a library mid-migration briefly holds one running row and one pending
-- row for the same library at once -- and on a worker that hit its shutdown grace
-- period or lost a lease mid-run, that CURRENT row can itself already be back to
-- 'pending' while the same run is still finishing in the background. Either way, a
-- wider index would make that ordinary handoff throw a unique violation instead of
-- chaining.
--
-- A deployment that has been started by hand-writing INSERT INTO job -- the exact
-- workaround this task exists to remove -- may already hold two 'pending'
-- `migrate_storage` rows for one library, and CREATE UNIQUE INDEX would then fail this
-- migration outright. De-duplicate first: keeping the oldest pending row per library and
-- deleting the rest costs nothing a user can see -- these are queue bookkeeping rows,
-- not parts or files, and the survivor does exactly the same work the deleted rows
-- would have.
delete from job j
using (
    select id, row_number() over (partition by library_id order by created_at, id) as rn
      from job
     where kind = 'migrate_storage' and state = 'pending'
) dup
where j.id = dup.id and dup.rn > 1;

create unique index job_migrate_storage_pending_per_library
    on job (library_id)
    where kind = 'migrate_storage' and state = 'pending';
