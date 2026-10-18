//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use jiff::Timestamp;
use lapidary_core::{
    BatchId, BatchStatus, DerivativeKind, JobFailure, JobId, JobPayload, LibraryId, Outcome,
    RevisionId,
};
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
pub const FAILED_SAMPLE: i64 = 100;

/// `complete`/`fail`/`reschedule` all guard their `UPDATE` with `AND state = 'running'`
/// (see `complete`'s doc comment for why), so zero rows affected is not an error -- most
/// often it means a second worker already reclaimed and finished this job before this
/// call landed. That is a correct, expected outcome of the design, but it is also a race
/// that is otherwise invisible: nothing else records that it happened. This turns it
/// into something an operator going looking for stalled-worker symptoms can actually
/// find.
///
/// `reschedule` used to carry a second cause -- a `migrate_storage` guard that could leave
/// a row deliberately `running` rather than collide with a pending sibling. Migration
/// `0012` dropped the index that guard existed for and the guard went with it, so the one
/// cause below is the only one left.
fn log_if_stale(id: JobId, verb: &str, rows_affected: u64) {
    if rows_affected == 0 {
        tracing::debug!(
            job = %id,
            verb,
            "this write changed nothing -- another worker already reclaimed and \
             finished this job; not a problem to chase"
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

    /// Forget the finished file jobs of `library`'s earlier batches (A31), leaving `keep` alone. A scan leaves one row
    /// per file, so without this every rescan grew the table by the library's size. Only a batch with nothing left to
    /// do loses its rows, so a tab still watching one is not shown it shrinking; failures stay, because a person reads
    /// them, and so does every scan job, which is how the examples know a library has history.
    pub async fn prune_finished(&self, library: LibraryId, keep: BatchId) -> Result<u64, DbError> {
        let gone = sqlx::query(
            "DELETE FROM job j WHERE j.library_id = $1 AND j.batch_id <> $2 \
               AND j.kind = 'ingest_file' AND j.state = 'done' \
               AND NOT EXISTS (SELECT 1 FROM job o WHERE o.batch_id = j.batch_id \
                               AND o.state IN ('pending', 'running'))",
        )
        .bind(library.as_uuid())
        .bind(keep.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(gone.rows_affected())
    }

    /// Queue a build of `revision`'s `produce`, unless one is already pending or running.
    ///
    /// Returns the batch holding the build and whether this call queued it. A build already on
    /// its way is not a refusal: the caller watches that batch, which is the answer it got the
    /// first time. Best-effort (a `WHERE NOT EXISTS`, not race-free) -- two callers racing can
    /// each see nothing and each insert -- and the second build only rewrites the same row.
    pub async fn enqueue_derive_if_absent(
        &self,
        library: LibraryId,
        revision: RevisionId,
        produce: DerivativeKind,
    ) -> Result<(BatchId, bool), DbError> {
        let payload = JobPayload::Derive { revision, produce };
        self.enqueue_if_absent(library, &payload, true).await
    }

    /// Queue `payload`, unless the same job is already pending, or running when `running_counts`.
    ///
    /// Returns the batch holding the job and whether this call queued it. A running job stands in for a
    /// new one only when what it writes cannot depend on when it read: a rung's build does not, and a
    /// part's description does, since a running one may have read the rows before the change asking
    /// again. Best-effort (a `WHERE NOT EXISTS`, not race-free). `existing` reads the pending and
    /// running jobs alone, however many finished ones the table keeps, off `job_active_idx` (`0036`).
    pub async fn enqueue_if_absent(
        &self,
        library: LibraryId,
        payload: &JobPayload,
        running_counts: bool,
    ) -> Result<(BatchId, bool), DbError> {
        let (batch, queued): (Uuid, bool) = sqlx::query_as(
            "WITH existing AS ( \
                 SELECT batch_id FROM job \
                  WHERE kind = $2 AND library_id = $1 AND payload = $3 \
                    AND (state = 'pending' OR ($4 AND state = 'running')) \
                  LIMIT 1), \
             queued AS ( \
                 INSERT INTO job (id, batch_id, library_id, kind, payload) \
                 SELECT uuidv7(), uuidv7(), $1, $2, $3 \
                  WHERE NOT EXISTS (SELECT 1 FROM existing) \
                 RETURNING batch_id) \
             SELECT batch_id, true FROM queued UNION ALL SELECT batch_id, false FROM existing",
        )
        .bind(library.as_uuid())
        .bind(payload.kind())
        .bind(payload.to_json())
        .bind(running_counts)
        .fetch_one(&self.0)
        .await?;

        if queued {
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(JOB_CHANNEL)
                .execute(&self.0)
                .await?;
        }
        Ok((BatchId::from_uuid(batch), queued))
    }

    /// Queue a rebuild of every export of a `format` source that a different kernel version
    /// wrote, and return how many were queued.
    ///
    /// **Exports only, and that is the whole of R3 in this function.** The ladder used to name
    /// the three tessellations and the CAD `structure` read beside them; nothing produces any of
    /// those now, so leaving them here would re-queue the whole corpus at every worker start,
    /// for ever, for work no handler can do. The rows are still in the table until migration
    /// `0055` (R4) removes them — they are simply not swept, because `ANY($3)` names only what
    /// something can still build.
    ///
    /// A worker calls this as it starts, with the version its own kernel reports for `format`.
    /// `PgParts::derivative_hash` and the download route serve a kind's row whatever wrote it, so
    /// an export from before a pipeline change is served forever unless something rewrites it.
    /// One batch per library; an export whose rebuild is already queued is left to that job;
    /// deleted parts are skipped. The source format is the revision's newest source file's, the
    /// file `PgParts::revision_source` hands a derive.
    // ponytail: a large library queues every stale export at once and worker concurrency bounds
    // the work. Trickle it if the rebuild starves ingest.
    pub async fn enqueue_stale_derivatives(
        &self,
        format: &str,
        kernel_version: &str,
    ) -> Result<u64, DbError> {
        let rebuildable: Vec<&str> = DerivativeKind::ALL.map(DerivativeKind::as_str).to_vec();
        let queued = sqlx::query(
            "WITH stale AS ( \
                 SELECT p.library_id, \
                        jsonb_build_object('revision', d.revision_id, 'produce', d.kind) AS payload \
                   FROM derivative d \
                   JOIN revision r ON r.id = d.revision_id \
                   JOIN part p ON p.id = r.part_id \
                   JOIN LATERAL (SELECT format FROM file \
                                  WHERE revision_id = r.id AND role = 'source' \
                                  ORDER BY created_at DESC, id DESC LIMIT 1) s ON true \
                  WHERE d.kind = ANY($3) AND s.format = $1 AND d.kernel_version <> $2), \
             batches AS ( \
                 SELECT library_id, uuidv7() AS batch_id \
                   FROM (SELECT DISTINCT library_id FROM stale) l) \
             INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT uuidv7(), b.batch_id, s.library_id, 'derive', s.payload \
               FROM stale s JOIN batches b USING (library_id) \
              WHERE NOT EXISTS (SELECT 1 FROM job j \
                                 WHERE j.kind = 'derive' AND j.library_id = s.library_id \
                                   AND j.payload = s.payload \
                                   AND j.state IN ('pending', 'running'))",
        )
        .bind(format)
        .bind(kernel_version)
        .bind(&rebuildable)
        .execute(&self.0)
        .await?
        .rows_affected();

        if queued > 0 {
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(JOB_CHANNEL)
                .execute(&self.0)
                .await?;
        }
        Ok(queued)
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
    ///
    /// `profiled` is not here and must not come back: R3 deleted `Outcome::Profiled` with shape
    /// similarity, while `job_outcome_known` keeps the value because finished rows hold it.
    pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError> {
        let outcome_str = match outcome {
            Outcome::Ingested => "ingested",
            Outcome::Skipped => "skipped",
            Outcome::Rendered => "rendered",
            Outcome::Scanned => "scanned",
            Outcome::Migrated => "migrated",
            Outcome::Described => "described",
            Outcome::Revised => "revised",
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
             WHERE id = $1 AND state = 'running'",
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
    /// One `UPDATE`, every kind this worker holds, at once: a graceful shutdown should
    /// not leave any of it -- an `ingest_file` mid-scan, a `derive` mid-render, a
    /// `migrate_storage` mid-move -- leased and waiting out its lease when a single pass
    /// could hand all of it back immediately.
    pub async fn release_leases(&self, worker_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'pending', run_after = now(), leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE leased_by = $1 AND state = 'running'",
        )
        .bind(worker_id)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected())
    }

    /// What a scan turned into. `None` when the batch has no jobs -- an id never issued
    /// and a scan that enqueued nothing are indistinguishable, and both mean "no status
    /// resource" (Task 11 turns this into a 404).
    /// Has anything ever run in this library?
    ///
    /// The seeder's question, and deliberately not "does it have parts". A library with no
    /// parts is not the same as a library nothing has ever happened to: a user who deleted
    /// every part has scanned before, and seeding them again on the next restart would put
    /// back data they removed. A `job` row is the durable record that something happened
    /// here, and it outlives every part it created.
    ///
    /// `EXISTS`, not a count: the answer is a boolean and the table is the busiest one in
    /// the schema.
    pub async fn library_has_history(&self, library: LibraryId) -> Result<bool, DbError> {
        let found: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM job WHERE library_id = $1 LIMIT 1")
                .bind(library.as_uuid())
                .fetch_optional(&self.0)
                .await?;
        Ok(found.is_some())
    }

    /// The seeder's record that it ran: one finished `scan_directory` row in a batch of its own,
    /// which is what [`Self::library_has_history`] reads. Finished, so no worker claims it and no
    /// queue count shows it; in a batch nobody was handed, so no status route reaches it.
    pub async fn record_seeded(&self, library: LibraryId) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
             VALUES (uuidv7(), uuidv7(), $1, 'scan_directory', '{}'::jsonb, 'done', 'ingested')",
        )
        .bind(library.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(())
    }

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
                    count(*) FILTER (WHERE outcome = 'revised'), \
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
            revised,
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

        let failed = self.failures(library, batch, None, FAILED_SAMPLE).await?;

        Ok(Some(BatchStatus {
            batch_id: batch,
            library_id: library,
            total: total as u32,
            pending: pending as u32,
            running: running as u32,
            ingested: ingested as u32,
            skipped: skipped as u32,
            rendered: rendered as u32,
            revised: revised as u32,
            scanned: scanned as u32,
            migrated: migrated as u32,
            migrating: migrating as u32,
            failed_total: failed_total as u32,
            failed,
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

    /// One page of a batch's failed jobs, in enqueue order, after the job `after`.
    ///
    /// `batch_status` carries the first [`FAILED_SAMPLE`] of these; the failure list pages
    /// past them by the last job it holds. The cursor's position is read back off that job's
    /// row whatever state it is in now, so a job retried out of the list still marks where
    /// the list got to.
    pub async fn failures(
        &self,
        library: LibraryId,
        batch: BatchId,
        after: Option<JobId>,
        limit: i64,
    ) -> Result<Vec<JobFailure>, DbError> {
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
        let rows: Vec<(Uuid, Option<String>, String, i32)> = sqlx::query_as(
            "SELECT j.id, COALESCE(j.payload->>'path', p.name, ''), j.last_error, j.attempts \
             FROM job j \
             LEFT JOIN revision rv \
                    ON rv.id = CASE WHEN j.kind = 'derive' \
                                     THEN (j.payload->>'revision')::uuid END \
             LEFT JOIN part p ON p.id = rv.part_id AND p.library_id = j.library_id \
             WHERE j.batch_id = $1 AND j.library_id = $2 AND j.state = 'failed' \
               AND ($4::uuid IS NULL \
                    OR (j.created_at, j.id) > (SELECT created_at, id FROM job WHERE id = $4)) \
             ORDER BY j.created_at, j.id LIMIT $3",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(limit)
        .bind(after.map(|job| job.as_uuid()))
        .fetch_all(&self.0)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, path, reason, attempts)| JobFailure {
                job: JobId::from_uuid(id),
                path: path.unwrap_or_default(),
                reason,
                attempts: attempts.max(0) as u32,
            })
            .collect())
    }

    /// Puts a batch's failed jobs back in the queue — all of them, or only `job` — and says
    /// how many went back.
    ///
    /// `attempts` starts again from zero: whoever presses Retry has usually changed something
    /// (replaced the file, remounted the directory), and a changed input deserves a fresh
    /// job's tries. `run_after` is now, or a row left behind a backoff would wait it out.
    /// `last_error` stays, as `reschedule` keeps it — until the job runs again, the last
    /// reason is still the truth.
    ///
    /// The batch reopens by arithmetic rather than by a flag: `batch_status` reports a finish
    /// only while nothing is pending, so the next read of it sees the batch running again.
    ///
    /// **And a row no handler can run is left out, which R3 is why.** Migration `0054` deleted the
    /// *pending and running* `profile_shape` and render-kind `derive` rows, deliberately leaving
    /// the **failed** ones where they are — a finished batch keeps its totals. Retrying one would
    /// put it back in the queue for a handler that cannot parse its payload, and the person who
    /// pressed Retry would be handed "this job's payload is malformed" about work that was
    /// removed from the product. So the filter is positive: a kind `JobPayload` still names, and
    /// for a `derive`, a `produce` `DerivativeKind` still names. Both lists come off the types,
    /// so deleting another variant is a compile error here rather than a row set pending that
    /// nothing can run.
    pub async fn retry(
        &self,
        library: LibraryId,
        batch: BatchId,
        job: Option<JobId>,
    ) -> Result<u64, DbError> {
        let runnable: Vec<&str> = vec![
            JobPayload::INGEST_FILE,
            JobPayload::DERIVE,
            JobPayload::SCAN_DIRECTORY,
            JobPayload::INGEST_INBOX,
            JobPayload::IMPORT_BUNDLE,
            JobPayload::IMPORT_PART,
        ];
        let buildable: Vec<&str> = DerivativeKind::ALL.map(DerivativeKind::as_str).to_vec();
        let retried = sqlx::query(
            "UPDATE job SET state = 'pending', attempts = 0, run_after = now(), \
                            leased_by = NULL, lease_expires_at = NULL, updated_at = now() \
             WHERE batch_id = $1 AND library_id = $2 AND state = 'failed' \
               AND kind = ANY($4) \
               AND (kind <> 'derive' OR payload->>'produce' = ANY($5)) \
               AND ($3::uuid IS NULL OR id = $3)",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(job.map(|job| job.as_uuid()))
        .bind(&runnable)
        .bind(&buildable)
        .execute(&self.0)
        .await?
        .rows_affected();

        if retried > 0 {
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(JOB_CHANNEL)
                .execute(&self.0)
                .await?;
        }
        Ok(retried)
    }

    /// A dedicated connection listening for enqueue notifications. Outside the pool by
    /// necessity: a LISTEN occupies its connection for as long as it is listening.
    pub async fn listener(&self) -> Result<PgListener, DbError> {
        let mut listener = PgListener::connect_with(&self.0).await?;
        listener.listen(JOB_CHANNEL).await?;
        Ok(listener)
    }
}
