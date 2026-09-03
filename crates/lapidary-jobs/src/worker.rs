//! The worker loop. Owns delivery and policy; knows nothing about meshes.

use crate::{JobHandler, JobsError, Next, next_state};
use lapidary_db::{PgJobs, PgListener};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// What a claimed-but-exhausted job is failed with. It blames the worker that vanished,
/// not the file, because the file was never the problem.
const ABANDONED: &str = "This file was claimed three times and never finished. The worker holding it stopped \
     responding each time. Check the worker's logs for a crash, then scan again.";

pub struct WorkerConfig {
    pub worker_id: String,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub concurrency: usize,
    /// When false, no LISTEN connection is opened and the loop runs on its polling floor
    /// alone. The floor is the correctness mechanism; this flag exists so a test can
    /// prove that, and so a database that refuses an extra connection degrades to slower
    /// rather than to broken.
    pub listen: bool,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: default_worker_id(),
            lease: Duration::from_secs(60),
            poll_interval: Duration::from_secs(5),
            concurrency: 4,
            listen: true,
        }
    }
}

fn default_worker_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
    format!("{host}-{}", std::process::id())
}

pub async fn run<H: JobHandler>(
    jobs: PgJobs,
    handler: Arc<H>,
    config: WorkerConfig,
    shutdown: CancellationToken,
) -> Result<(), JobsError> {
    let permits = Arc::new(Semaphore::new(config.concurrency));
    let mut listener = if config.listen {
        match jobs.listener().await {
            Ok(listener) => Some(listener),
            // Degrade to the polling floor rather than refusing to start. A worker that
            // polls is slower; a worker that will not start ingests nothing.
            Err(error) => {
                tracing::warn!(%error, "could not open the job listener; polling only");
                None
            }
        }
    } else {
        None
    };

    let jobs = Arc::new(jobs);

    loop {
        if shutdown.is_cancelled() {
            break;
        }

        // The permit is acquired BEFORE the dequeue, never after. Leasing a job we have
        // no capacity to start would burn lease time while it waits its turn, and a
        // lease that expires in a queue is indistinguishable from a crashed worker --
        // manufacturing the exact failure the lease exists to detect.
        //
        // At this crate's default and tested concurrency (2 in the worker tests, 4 by
        // default) no test can observe this ordering directly: the queue never has more
        // ready jobs than permits, so a permit is always available and "acquire before"
        // vs. "acquire after" behave identically. Verified by hand for this task: moving
        // the acquire to after `dequeue` still passes every test in this file. The
        // property this ordering protects -- a worker with no free capacity leaving a
        // job leased and idle until its lease expires -- needs a job slower than a lease
        // to observe, which arrives with Phase 2. Recorded here rather than claimed by a
        // test that does not exist.
        let permit = match Arc::clone(&permits).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };

        match jobs.dequeue(&config.worker_id, config.lease).await {
            Ok(Some(job)) if job.attempts > job.max_attempts => {
                if let Err(error) = jobs.fail(job.id, ABANDONED).await {
                    tracing::warn!(%error, job = %job.id, "could not record an abandoned job");
                }
                drop(permit);
            }
            Ok(Some(job)) => {
                let jobs = Arc::clone(&jobs);
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let result = handler.handle(&job).await;
                    let outcome = next_state(result, job.attempts, job.max_attempts);
                    let recorded = match outcome {
                        Next::Complete(outcome) => jobs.complete(job.id, outcome).await,
                        Next::Fail { reason } => jobs.fail(job.id, &reason).await,
                        Next::Retry { reason, backoff } => {
                            jobs.reschedule(job.id, &reason, backoff).await
                        }
                    };
                    if let Err(error) = recorded {
                        // The lease will lapse and another worker will reclaim it. That
                        // is safe: the ingest is idempotent by the part uniqueness
                        // constraint, so a redo becomes Skipped rather than a duplicate.
                        tracing::warn!(%error, job = %job.id, "could not record a job result");
                    }
                    drop(permit);
                });
            }
            Ok(None) => {
                drop(permit);
                // Checked again here, not only at the top of the loop: without this, a
                // shutdown that arrives while we are asleep in `wait_for_work` would sit
                // unnoticed until the next poll tick or NOTIFY wakes us, which could be
                // the full `poll_interval` away. This is purely a prompt-exit nicety --
                // the top-of-loop check is what actually stops new dequeues, and that one
                // must not move: spec S4.4 says shutdown stops dequeuing and lets only
                // in-flight work finish, and a worker that keeps draining a backlog after
                // SIGTERM will hit SIGKILL before finishing, skipping release_leases
                // entirely and degrading the graceful path into the crash path it exists
                // to avoid.
                wait_for_work(&mut listener, config.poll_interval, &shutdown).await;
                if shutdown.is_cancelled() {
                    break;
                }
            }
            Err(error) => {
                drop(permit);
                tracing::warn!(%error, "could not reach the job queue; retrying");
                wait_for_work(&mut listener, config.poll_interval, &shutdown).await;
                if shutdown.is_cancelled() {
                    break;
                }
            }
        }
    }

    // Hand back whatever is still leased so a restart resumes at once.
    if let Err(error) = jobs.release_leases(&config.worker_id).await {
        tracing::warn!(%error, "could not release this worker's leases on shutdown");
    }
    Ok(())
}

/// Sleep until there is plausibly work, the poll interval elapses, or we are shutting
/// down. The poll interval is the floor and the only thing correctness rests on.
async fn wait_for_work(
    listener: &mut Option<PgListener>,
    poll_interval: Duration,
    shutdown: &CancellationToken,
) {
    match listener {
        Some(listener) => {
            tokio::select! {
                _ = listener.recv() => {}
                _ = tokio::time::sleep(poll_interval) => {}
                _ = shutdown.cancelled() => {}
            }
        }
        None => {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {}
                _ = shutdown.cancelled() => {}
            }
        }
    }
}
