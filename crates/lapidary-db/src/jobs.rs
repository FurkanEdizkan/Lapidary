//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use jiff::Timestamp;
use lapidary_core::{BatchId, BatchStatus, JobFailure, JobId, JobPayload, LibraryId, Outcome};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use std::time::Duration;
use uuid::Uuid;

/// The `LISTEN`/`NOTIFY` channel. A wake-up only: the payload is empty and nothing reads
/// it. Notification is a latency optimization over the worker's polling floor and must
/// never become the mechanism the queue depends on -- a NOTIFY fires into the void when
/// nothing is listening, so a worker that starts after an enqueue would never learn about
/// that work. See the design doc, section 3.4, and the test that disables the listener.
pub const JOB_CHANNEL: &str = "lapidary_jobs";

pub struct PgJobs(pub PgPool);

/// Rebuild a `jiff::Timestamp` from the microseconds the query selected. sqlx 0.9 has no
/// `jiff` feature, so every timestamp in this workspace crosses the boundary this way;
/// `PgParts::page` does the same, including reporting a corrupt row rather than clamping.
fn to_timestamp(column: &'static str, micros: i64) -> Result<Timestamp, DbError> {
    Timestamp::from_microsecond(micros).map_err(|_| DbError::TimestampOutOfRange {
        column,
        value: micros,
    })
}

/// The most failures one status response carries. A thousand-file disaster must return a
/// readable payload, and `failed_total` still reports the real number.
const FAILED_SAMPLE: i64 = 100;

/// `complete`/`fail`/`reschedule` all guard their `UPDATE` with `AND state = 'running'`
/// (see `complete`'s doc comment for why), so zero rows affected is not an error -- most
/// often it means a second worker already reclaimed and finished this job before this
/// call landed. That is a correct, expected outcome of the design, but it is also a race
/// that is otherwise invisible: nothing else records that it happened. This turns it
/// into something an operator going looking for stalled-worker symptoms can actually
/// find.
///
/// Since fix round 2, `reschedule` alone can also match zero rows for a second, distinct
/// reason with nothing stale about it: its own `migrate_storage` guard (see that
/// method's doc) can leave a row deliberately `running`, still held by the very same
/// caller, because moving it to `pending` would collide with a pending or racing
/// sibling. `rows_affected` alone cannot tell that apart from a genuinely stale write,
/// so the message below no longer claims either cause specifically -- claiming "another
/// worker already reclaimed and finished it" for a row nobody touched would be exactly
/// the kind of measurement that lies this project's error-message rule already forbids
/// in user-facing text, even here where the audience is `tracing::debug!` and an
/// operator, not an end user.
fn log_if_stale(id: JobId, verb: &str, rows_affected: u64) {
    if rows_affected == 0 {
        tracing::debug!(
            job = %id,
            verb,
            "this write changed nothing -- either another worker already reclaimed \
             and finished this job, or (reschedule only) its own migrate_storage guard \
             left the row running on purpose; neither is a problem to chase"
        );
    }
}

/// One claimed job, as `dequeue` hands it to the worker loop.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: JobId,
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub kind: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

impl PgJobs {
    /// Enqueue any mix of job kinds under one fresh batch. One statement regardless of N:
    /// a thousand files is one insert, because this runs inside the HTTP request.
    ///
    /// The `kind` and `payload` columns both come from the same `JobPayload` --
    /// `kind()` for the column, `to_json()` for the payload -- so the two cannot
    /// disagree by construction. A signature that took `kind` and `payloads` apart would
    /// let a caller pass a kind that mismatched its own payloads.
    pub async fn enqueue(
        &self,
        library: LibraryId,
        jobs: &[JobPayload],
    ) -> Result<(BatchId, u32), DbError> {
        let batch = BatchId::new();
        let queued = self.enqueue_into(batch, library, jobs).await?;
        Ok((batch, queued))
    }

    /// Enqueue into a batch that already exists, rather than minting a fresh one.
    ///
    /// This is what a `scan_directory` job uses for the per-file jobs its walk finds, and
    /// the reason it exists at all: `enqueue` mints a `BatchId` on every call, so a scan
    /// job that enqueued its children into a new batch would leave the browser polling
    /// the scan's own batch, seeing `1 of 1` settled, and reporting a finished scan while
    /// a hundred and fifty files were still ingesting.
    ///
    /// Growing a batch after the fact is safe because `batch_status` stores no total: it
    /// counts rows by `batch_id` on every read, so the total moves as rows land. Nothing
    /// caches it, and nothing may start to.
    pub async fn enqueue_into(
        &self,
        batch: BatchId,
        library: LibraryId,
        jobs: &[JobPayload],
    ) -> Result<u32, DbError> {
        if jobs.is_empty() {
            // No rows, and deliberately no NOTIFY: waking every worker to find nothing
            // is the one case where the optimization is pure cost.
            return Ok(0);
        }

        let ids: Vec<Uuid> = (0..jobs.len()).map(|_| JobId::new().as_uuid()).collect();
        let kinds: Vec<&'static str> = jobs.iter().map(JobPayload::kind).collect();
        let payloads: Vec<serde_json::Value> = jobs.iter().map(JobPayload::to_json).collect();

        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT id, $2, $3, kind, payload \
             FROM unnest($1::uuid[], $4::text[], $5::jsonb[]) AS t(id, kind, payload)",
        )
        .bind(&ids)
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(&kinds)
        .bind(&payloads)
        .execute(&self.0)
        .await?;

        sqlx::query("SELECT pg_notify($1, '')")
            .bind(JOB_CHANNEL)
            .execute(&self.0)
            .await?;

        Ok(jobs.len() as u32)
    }

    /// Enqueue one `ingest_file` job per path under a fresh batch. A thin wrapper over
    /// `enqueue`, kept as its own method because every scan caller wants exactly this
    /// shape.
    pub async fn enqueue_scan(
        &self,
        library: LibraryId,
        paths: &[String],
    ) -> Result<(BatchId, u32), DbError> {
        let jobs: Vec<JobPayload> = paths
            .iter()
            .cloned()
            .map(|path| JobPayload::IngestFile { path })
            .collect();
        self.enqueue(library, &jobs).await
    }

    /// One fresh `migrate_storage` job for `library`, under a batch of its own, unless
    /// `library` already has one pending or running -- or unless a concurrent caller
    /// wins the race to create the first one.
    ///
    /// This is the worker startup guard (`bin/lapidary-server`, worker role only): for
    /// every library `PgStorageMigration::libraries_needing_migration` still finds
    /// un-migrated rows in, it calls this once, so an operator upgrading an old store
    /// never has to hand-write `INSERT INTO job`. It is also what the optional manual
    /// trigger route (`lapidary_ingest::migrate::migrate`) calls.
    ///
    /// A single statement, not a read then a write -- deliberately, and it has to be
    /// for the property this exists to guarantee: two workers booting at the same
    /// instant against the same un-migrated library each take their own READ COMMITTED
    /// snapshot before either commits, so a bare `WHERE NOT EXISTS` here is NOT
    /// race-free by itself. Verified by hand against a real Postgres 18.6 before this
    /// was written, including the tempting-looking fix of wrapping the check in an
    /// advisory lock taken INSIDE THIS SAME STATEMENT (a CTE calling
    /// `pg_advisory_xact_lock` ahead of the `INSERT ... SELECT`, both part of one
    /// INSERT): that specific shape is not race-free either. Blocking on a lock
    /// mid-statement does not give the rest of that same statement a fresher snapshot --
    /// the whole statement, including a `WHERE NOT EXISTS` evaluated after the block,
    /// still reads the snapshot it started with, so the loser's check comes back
    /// "nothing exists yet" even though the winner has, by then, already committed.
    /// This is NOT a claim that an advisory lock cannot close the race by any means --
    /// one taken as its OWN, separate statement before the insert (`PgFolders::reparent`'s
    /// shape: one statement to lock, a later one to read and write within the same
    /// transaction) does close it, since that later statement gets a fresh snapshot once
    /// the lock is granted. That shape was set aside here because it protects only a
    /// caller that remembers to take the lock, where a unique index protects every
    /// caller. `job_migrate_storage_pending_per_library` (migration 0010) is the actual
    /// guarantee this method relies on: `ON CONFLICT ... DO NOTHING` only has something
    /// to conflict with because that index exists, and Postgres makes the loser's INSERT
    /// wait on the winner's uncommitted row and then discard itself once the winner
    /// commits -- no stale snapshot involved, because unique-index conflict checking is
    /// not an MVCC read.
    ///
    /// The `WHERE NOT EXISTS` clause is still load-bearing, just not for that race: it
    /// checks `state IN ('pending', 'running')`, where the index (see its migration for
    /// why) covers `'pending'` alone. Removing this clause would still leave the
    /// concurrency race closed by the index, but a library whose migration is already
    /// RUNNING -- with no pending successor queued yet -- would no longer be recognised,
    /// and this method would queue a second, redundant chain beside it.
    ///
    /// Returns the batch this job was queued under, or `None` if nothing was queued.
    pub async fn enqueue_migration_if_absent(
        &self,
        library: LibraryId,
    ) -> Result<Option<BatchId>, DbError> {
        let batch: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT uuidv7(), uuidv7(), $1, 'migrate_storage', '{}'::jsonb \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM job \
                  WHERE kind = 'migrate_storage' AND library_id = $1 \
                    AND state IN ('pending', 'running')) \
             ON CONFLICT (library_id) WHERE kind = 'migrate_storage' AND state = 'pending' \
                 DO NOTHING \
             RETURNING batch_id",
        )
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;

        let batch = batch.map(BatchId::from_uuid);
        if batch.is_some() {
            // Same optimization `enqueue_into` makes, and the same reason: waking every
            // worker to find one migration job is cheap once, and not worth skipping
            // for the no-op case above, which already returns before reaching here.
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(JOB_CHANNEL)
                .execute(&self.0)
                .await?;
        }
        Ok(batch)
    }

    /// `migrate_storage`'s own re-enqueue arm, guarded the same way
    /// `enqueue_migration_if_absent` is, and backed by the same
    /// `job_migrate_storage_pending_per_library` index -- but checking
    /// `state = 'pending'` ONLY, never `'running'`.
    ///
    /// That difference is not an oversight. The caller of this method IS the currently
    /// RUNNING `migrate_storage` job for `library` -- a `NOT EXISTS` that also excluded
    /// `'running'` rows would see that caller's own row on every single run and refuse
    /// to queue a successor, silently stopping the chain from ever draining. What this
    /// guards against instead is a successor some OTHER path already queued:
    /// `lapidary_jobs::worker`'s shutdown-grace release can put this same job's own row
    /// back to `'pending'` while this handler is still finishing in the background (see
    /// that module's `SHUTDOWN_GRACE` doc), and a second worker can reclaim an expired
    /// lease and run this same re-enqueue concurrently. Either way, without this guard
    /// the plain `INSERT` this replaced would throw the index's unique violation
    /// straight into the caller, which `lapidary_ingest::migrate` would then report as
    /// "could not queue the next batch" -- true of a database outage, and false of a
    /// benign double-enqueue this method exists to make harmless instead.
    ///
    /// Inserts into `batch` -- the chain's own, existing batch -- rather than minting a
    /// fresh one, for `enqueue_into`'s reason: the browser (or whatever else is
    /// watching) is already polling this batch, and a new one would orphan its count.
    ///
    /// Returns whether this call actually queued the next run. `false` is not a
    /// failure -- it means the chain's continuation is already queued by someone else.
    pub async fn reenqueue_migration_if_absent(
        &self,
        batch: BatchId,
        library: LibraryId,
    ) -> Result<bool, DbError> {
        let queued: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT uuidv7(), $1, $2, 'migrate_storage', '{}'::jsonb \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM job \
                  WHERE kind = 'migrate_storage' AND library_id = $2 AND state = 'pending') \
             ON CONFLICT (library_id) WHERE kind = 'migrate_storage' AND state = 'pending' \
                 DO NOTHING \
             RETURNING id",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;

        let queued = queued.is_some();
        if queued {
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(JOB_CHANNEL)
                .execute(&self.0)
                .await?;
        }
        Ok(queued)
    }

    /// The batch id of `library`'s currently active `migrate_storage` chain, pending or
    /// running -- whichever row already existed (or a concurrent caller just created)
    /// when `enqueue_migration_if_absent` returned `None`. Every row in one migration's
    /// chain shares its first row's `batch_id` (`reenqueue_migration_if_absent` always
    /// inserts into the SAME batch, never a fresh one), so picking any one active row's
    /// batch id is unambiguous -- there is exactly one live chain per library at a time.
    ///
    /// Exists for the manual trigger route (`lapidary_ingest::migrate::migrate`): a
    /// `queued: 0` response still needs a real batch id to hand back when a migration
    /// IS running, unlike a render sweep's `queued: 0`, which means nothing is running
    /// at all. Reporting a fabricated id there would send an operator polling a batch
    /// that can never resolve -- a 404 for a migration genuinely in progress.
    pub async fn active_migration_batch(
        &self,
        library: LibraryId,
    ) -> Result<Option<BatchId>, DbError> {
        let batch: Option<Uuid> = sqlx::query_scalar(
            "SELECT batch_id FROM job \
              WHERE kind = 'migrate_storage' AND library_id = $1 \
                AND state IN ('pending', 'running') \
              LIMIT 1",
        )
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        Ok(batch.map(BatchId::from_uuid))
    }

    /// Claim one job, or reclaim one whose lease expired.
    ///
    /// Reclamation is folded in here rather than given to a sweeper, so there is no
    /// sweeper process to be the thing that died. `attempts` increments on reclamation
    /// exactly as it does on retry, which is what caps the poison-pill case: a file that
    /// panics the worker before it can record anything is tried `max_attempts` times and
    /// then abandoned by the caller, rather than re-leased forever.
    ///
    /// Exhausted rows are deliberately NOT excluded here. Filtering them out in SQL would
    /// leave them 'running' with a dead lease, invisible to this query and to any cleanup
    /// -- which is how this table would grow a permanent population of zombies. The
    /// caller claims them and fails them (see `lapidary_jobs::worker`).
    pub async fn dequeue(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> Result<Option<JobRow>, DbError> {
        let row: Option<(Uuid, Uuid, Uuid, String, serde_json::Value, i32, i32)> = sqlx::query_as(
            "UPDATE job SET state = 'running', \
                                attempts = attempts + 1, \
                                leased_by = $1, \
                                lease_expires_at = now() + make_interval(secs => $2), \
                                updated_at = now() \
                 WHERE id = ( \
                     SELECT id FROM job \
                     WHERE (state = 'pending' AND run_after <= now()) \
                        OR (state = 'running' AND lease_expires_at < now()) \
                     ORDER BY run_after \
                     FOR UPDATE SKIP LOCKED \
                     LIMIT 1 \
                 ) \
                 RETURNING id, batch_id, library_id, kind, payload, attempts, max_attempts",
        )
        .bind(worker_id)
        .bind(lease.as_secs_f64())
        .fetch_optional(&self.0)
        .await?;

        Ok(row.map(
            |(id, batch_id, library_id, kind, payload, attempts, max_attempts)| JobRow {
                id: JobId::from_uuid(id),
                batch_id: BatchId::from_uuid(batch_id),
                library_id: LibraryId::from_uuid(library_id),
                kind,
                payload,
                attempts,
                max_attempts,
            },
        ))
    }

    /// Terminal, successful. `last_error` is deliberately left as-is: a job that failed
    /// transiently, retried and then succeeded keeps the reason it retried. That is
    /// diagnostically useful, invisible to users (`BatchStatus` only reports `last_error`
    /// for rows in state `failed`), and `attempts` alone tells you it retried but not why.
    ///
    /// `AND state = 'running'` guards against the scenario spec §3.2 itself calls out: a
    /// worker stalls past its lease, a second worker reclaims and finishes the same job,
    /// and the first worker -- still alive, just slow -- eventually calls back in. Without
    /// this guard the stale write clobbers whatever the reclaiming worker recorded; with
    /// it, the row is no longer `running` by the time the stale caller arrives, so the
    /// `UPDATE` matches zero rows instead of overwriting a real result. "Measurement must
    /// not lie" (CLAUDE.md) -- a part that ingested must never be reported failed because
    /// a second, abandoned attempt finished after it.
    pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError> {
        let outcome_str = match outcome {
            Outcome::Ingested => "ingested",
            Outcome::Skipped => "skipped",
            Outcome::Rendered => "rendered",
            Outcome::Scanned => "scanned",
            Outcome::Migrated => "migrated",
        };
        let result = sqlx::query(
            "UPDATE job SET state = 'done', outcome = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running'",
        )
        .bind(id.as_uuid())
        .bind(outcome_str)
        .execute(&self.0)
        .await?;
        log_if_stale(id, "complete", result.rows_affected());
        Ok(())
    }

    /// Terminal. `reason` is the handler's own message and is shown to a person.
    ///
    /// `AND state = 'running'` -- see `complete`'s doc comment for why: this is the same
    /// stale-writer guard, so a worker that stalled past its lease and is only now
    /// reporting failure cannot overwrite a row another worker already finished.
    pub async fn fail(&self, id: JobId, reason: &str) -> Result<(), DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'failed', last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running'",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .execute(&self.0)
        .await?;
        log_if_stale(id, "fail", result.rows_affected());
        Ok(())
    }

    /// Back to the queue behind a backoff. `last_error` is kept -- not cleared -- so a job
    /// that is still retrying can say what went wrong last time. This is only legal
    /// because `job_failed_has_reason` is an implication (`state <> 'failed' or last_error
    /// is not null`), not a biconditional: a `pending` row carrying a `last_error` violates
    /// nothing.
    ///
    /// `AND state = 'running'` -- see `complete`'s doc comment for why: the same
    /// stale-writer guard applies here too.
    ///
    /// `AND NOT (kind = 'migrate_storage' AND EXISTS (...))` guards against a second
    /// collision `job_migrate_storage_pending_per_library` (migration 0010) can throw,
    /// one an `INSERT ... ON CONFLICT DO NOTHING` cannot resolve because this is an
    /// UPDATE: if this row is a `migrate_storage` job and its library already has a
    /// pending successor -- queued by another attempt at the SAME job racing this one,
    /// or by a worker that reclaimed its expired lease -- moving THIS row to `pending`
    /// too would put two pending rows under one library and abort with a raw `23505`.
    /// Skipping it here is not a compromise: that library's continuation is already
    /// queued, so leaving this row `running` to lapse by lease expiry -- exactly what
    /// happens to any job whose worker vanishes -- is the correct outcome. See
    /// `release_leases` below for why this cannot simply be scoped to `kind <>
    /// 'migrate_storage'` in the caller instead: a single `UPDATE` can touch rows of
    /// every kind at once, and a raw constraint violation aborts the WHOLE statement,
    /// not just the offending row.
    pub async fn reschedule(
        &self,
        id: JobId,
        reason: &str,
        backoff: Duration,
    ) -> Result<(), DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'pending', \
                            run_after = now() + make_interval(secs => $3), \
                            last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running' \
               AND NOT (job.kind = 'migrate_storage' AND EXISTS ( \
                   SELECT 1 FROM job successor \
                    WHERE successor.kind = 'migrate_storage' \
                      AND successor.library_id = job.library_id \
                      AND successor.state = 'pending'))",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .bind(backoff.as_secs_f64())
        .execute(&self.0)
        .await?;
        log_if_stale(id, "reschedule", result.rows_affected());
        Ok(())
    }

    /// Graceful shutdown: hand back whatever this worker still holds so a restart
    /// resumes at once rather than waiting out every lease. A crash does not get this,
    /// which is what lease expiry is for -- the two paths are separate because only one
    /// of them can run cleanup code.
    ///
    /// One `UPDATE`, every kind this worker holds, at once -- which is exactly why the
    /// `migrate_storage` exclusions below (see `reschedule`'s doc for the pending-
    /// successor half of the reasoning) have to live in this statement rather than in a
    /// second pass over just that kind: a `migrate_storage` row that cannot become
    /// `pending` colliding with `job_migrate_storage_pending_per_library` (migration
    /// 0010) would abort this UPDATE in its entirety, silently leaving every OTHER job
    /// of any kind this worker held -- an `ingest_file` mid-scan, a `derive` mid-render
    /// -- leased and un-released, degrading a graceful shutdown into the crash path for
    /// all of them, not just the one migration.
    ///
    /// Fix round 2: excluding a row with a pending successor is not enough on its own.
    /// This worker can hold TWO `running` `migrate_storage` rows for the SAME library at
    /// once, with no pending row yet -- `reenqueue_migration_if_absent` fires
    /// `pg_notify` on success, and this worker's own dequeue loop (default concurrency
    /// 4) can claim and start running the fresh successor on another slot before the
    /// predecessor's `complete()` call lands, or before a `Transient` retry lands, or
    /// while the predecessor is still legitimately busy past `SHUTDOWN_GRACE` on a large
    /// corpus. Releasing BOTH in the same `UPDATE` would move two rows for one library
    /// to `pending` in one statement -- a self-collision against the same index, from
    /// this statement alone, with no pending successor involved at all. The second
    /// `EXISTS` below closes that: it excludes a `migrate_storage` row whenever ANOTHER
    /// `running` `migrate_storage` row for the same library, held by this SAME worker,
    /// sorts before it (`id` is uuidv7 and therefore time-ordered, so this is a stable,
    /// deterministic "oldest wins" tie-break, not an arbitrary one) -- so at most ONE
    /// such row moves per library per call. The rest stay `running` to lapse by lease
    /// expiry, for the identical reason the pending-successor exclusion already gives:
    /// their work is either already queued or about to be, so losing the lease costs
    /// nothing a retry does not already cover.
    pub async fn release_leases(&self, worker_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'pending', run_after = now(), leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE leased_by = $1 AND state = 'running' \
               AND NOT (job.kind = 'migrate_storage' AND ( \
                   EXISTS ( \
                       SELECT 1 FROM job successor \
                        WHERE successor.kind = 'migrate_storage' \
                          AND successor.library_id = job.library_id \
                          AND successor.state = 'pending') \
                   OR EXISTS ( \
                       SELECT 1 FROM job other \
                        WHERE other.kind = 'migrate_storage' \
                          AND other.library_id = job.library_id \
                          AND other.state = 'running' \
                          AND other.leased_by = $1 \
                          AND other.id < job.id)))",
        )
        .bind(worker_id)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected())
    }

    /// What a scan turned into. `None` when the batch has no jobs -- an id never issued
    /// and a scan that enqueued nothing are indistinguishable, and both mean "no status
    /// resource" (Task 11 turns this into a 404).
    pub async fn batch_status(
        &self,
        library: LibraryId,
        batch: BatchId,
    ) -> Result<Option<BatchStatus>, DbError> {
        // `library_id = $2` is the ownership check, not a performance filter: it is what
        // stops a batch id alone from being a capability -- content addressing is not
        // authorization (CLAUDE.md).
        // `min(created_at)` is itself an aggregate, so it is just as capable of coming
        // back NULL over zero rows as the counts are -- decoding it as a bare i64
        // panics on the empty-batch case instead of falling into the `total == 0`
        // guard below. Both aggregate timestamp columns are `Option<i64>` for exactly
        // this reason.
        #[allow(clippy::type_complexity)]
        let counts: Option<(
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            Option<i64>,
            Option<i64>,
        )> = sqlx::query_as(
            "SELECT count(*), \
                    count(*) FILTER (WHERE state = 'pending'), \
                    count(*) FILTER (WHERE state = 'running'), \
                    count(*) FILTER (WHERE outcome = 'ingested'), \
                    count(*) FILTER (WHERE outcome = 'skipped'), \
                    count(*) FILTER (WHERE outcome = 'rendered'), \
                    count(*) FILTER (WHERE outcome = 'scanned'), \
                    count(*) FILTER (WHERE outcome = 'migrated'), \
                    count(*) FILTER (WHERE kind = 'migrate_storage'), \
                    count(*) FILTER (WHERE state = 'failed'), \
                    (extract(epoch FROM min(created_at)) * 1000000)::bigint, \
                    CASE WHEN count(*) FILTER (WHERE state IN ('pending','running')) = 0 \
                         THEN (extract(epoch FROM max(updated_at)) * 1000000)::bigint \
                    END \
             FROM job WHERE batch_id = $1 AND library_id = $2",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;

        // An aggregate over zero rows still returns one row, with count 0 -- so "no
        // jobs" is detected on the count, not on fetch_optional returning None.
        let Some((
            total,
            pending,
            running,
            ingested,
            skipped,
            rendered,
            scanned,
            migrated,
            migrating,
            failed_total,
            started,
            finished,
        )) = counts
        else {
            return Ok(None);
        };
        if total == 0 {
            return Ok(None);
        }

        // `, id` is load-bearing, not decoration: `enqueue` inserts a whole batch
        // in one statement, and Postgres's `now()` is constant for the duration of a
        // transaction, so every job in a real batch shares the exact same
        // `created_at` -- `ORDER BY created_at` alone never actually discriminates
        // between same-batch failures and the list would reshuffle under a polling
        // reader from one request to the next (spec §7 promises it does not). `JobId`
        // is uuidv7 and `enqueue` generates ids in insertion order, so ordering
        // by `id` as the tiebreaker reproduces enqueue order, which -- since Task 10
        // sorts paths before enqueueing -- is the alphabetical order a person expects.
        // Do not simplify this back to `ORDER BY created_at`.
        //
        // An `ingest_file` failure names its own path; a `derive` failure has none, so it
        // falls through to the part its revision belongs to. Decoding the path column as
        // `Option<String>` -- even though `COALESCE`'s final `''` means it can never
        // actually come back SQL NULL -- is what stops a row this join fails to match
        // from ever becoming a decode error again: that decode error is exactly the 500
        // a `derive` job with no matching `path` key used to trigger.
        //
        // `p.library_id = j.library_id` is the same reachability check `batch_status`
        // itself is scoped by (`library_id = $2` above), pushed down into this join
        // rather than left for a caller to remember: content addressing is not
        // authorization (CLAUDE.md), and a `derive` payload's `revision` is a uuid a
        // caller might hold from anywhere. A `derive` job naming another library's
        // revision still shows up as a failure -- the job did fail -- but the join
        // misses and `COALESCE` falls through to `''`, so the response never leaks what
        // that other library calls its own part.
        let failures: Vec<(Option<String>, String, i32)> = sqlx::query_as(
            "SELECT COALESCE(j.payload->>'path', p.name, ''), j.last_error, j.attempts \
             FROM job j \
             LEFT JOIN revision rv \
                    ON rv.id = CASE WHEN j.kind = 'derive' \
                                     THEN (j.payload->>'revision')::uuid END \
             LEFT JOIN part p ON p.id = rv.part_id AND p.library_id = j.library_id \
             WHERE j.batch_id = $1 AND j.library_id = $2 AND j.state = 'failed' \
             ORDER BY j.created_at, j.id LIMIT $3",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(FAILED_SAMPLE)
        .fetch_all(&self.0)
        .await?;

        Ok(Some(BatchStatus {
            batch_id: batch,
            library_id: library,
            total: total as u32,
            pending: pending as u32,
            running: running as u32,
            ingested: ingested as u32,
            skipped: skipped as u32,
            rendered: rendered as u32,
            scanned: scanned as u32,
            migrated: migrated as u32,
            migrating: migrating as u32,
            failed_total: failed_total as u32,
            failed: failures
                .into_iter()
                .map(|(path, reason, attempts)| JobFailure {
                    path: path.unwrap_or_default(),
                    reason,
                    attempts: attempts.max(0) as u32,
                })
                .collect(),
            // Microseconds in, jiff out -- the conversion belongs here, at the database
            // boundary, so no wire type ever carries a raw epoch integer. `started` is
            // provably `Some` here: `total != 0` means at least one row exists, and
            // `min(created_at)` over a non-empty set cannot be NULL. `unwrap_or_default`
            // (not `unwrap()` -- CLAUDE.md bans that outside tests) documents that this
            // is an invariant, not a real fallback.
            started_at: to_timestamp("job.created_at", started.unwrap_or_default())?,
            finished_at: finished
                .map(|us| to_timestamp("job.updated_at", us))
                .transpose()?,
        }))
    }

    /// A dedicated connection listening for enqueue notifications. Outside the pool by
    /// necessity: a LISTEN occupies its connection for as long as it is listening.
    pub async fn listener(&self) -> Result<PgListener, DbError> {
        let mut listener = PgListener::connect_with(&self.0).await?;
        listener.listen(JOB_CHANNEL).await?;
        Ok(listener)
    }
}
