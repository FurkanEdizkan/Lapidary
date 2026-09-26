//! A part's shape profile, computed in the worker from the L0 tessellation it already stored
//! (Phase 6; design in `docs/goals/phase-6.md` § Shape profile).
//!
//! # Why this is not on the open path, and not a kernel call
//!
//! Opening a part reads metadata and derivatives only (`CLAUDE.md`), and the L0 rung is a
//! derivative — 5k triangles whatever the part was, already written at ingest. So the whole profile
//! costs a decode and a fixed number of float operations, and it happens here, in the worker,
//! behind the queue. Nothing about it touches the source file or the CAD kernel, which is also why
//! an algorithm change (a `SHAPE_VERSION` bump) is a sweep over stored rungs rather than a
//! re-tessellation of a corpus.
//!
//! # Three ways in, one code path
//!
//! [`profile_shape`](WorkerHandler::profile_shape) does the work, and every caller goes through it:
//!
//! - **ingest and a revision**, through [`record_shape`](WorkerHandler::record_shape), which logs
//!   what it cannot do instead of failing. A part with no profile is a part missing from the
//!   duplicate queue; a part missing from the library because its profile would not compute is a
//!   file the user gave us and cannot see. The first is a gap, the second is a bug.
//! - **a rebuilt L0**, the same way, from `derive.rs`: the rung the stored profile was computed
//!   from no longer exists, so the row is stale the moment the new one is written.
//! - **the `profile_shape` job**, which is how a backfill catches up — parts ingested before Phase
//!   6, parts whose rung a newer kernel rebuilt, parts whose `SHAPE_VERSION` moved. There its
//!   failures are the job's, reported and retried by the queue's own rules.

use crate::handler::{WorkerHandler, classify_db};
use lapidary_core::{JobPayload, LibraryId, Outcome, RevisionId};
use lapidary_db::{PgJobs, PgShapes};
use lapidary_jobs::HandlerError;
use lapidary_storage::DerivativeStore;

/// How many stale parts one worker start queues. The sweep runs again at the next start, so a
/// corpus larger than this catches up over several; the cap is what keeps a first start after a
/// `SHAPE_VERSION` bump from inserting a job per part in one statement.
// ponytail: a fixed cap, and the queue drains it before the next sweep. Trickle it per library if
// a backfill ever starves ingest.
const PER_START: i64 = 5_000;

impl WorkerHandler {
    /// `revision`'s L0 tessellation, decoded and profiled, and the profile recorded against its
    /// part.
    ///
    /// `Permanent` when this library has no such revision, its part is deleted, or the revision has
    /// no L0 — none of those become true on a retry, and the message says which it was. `Transient`
    /// when the stored rung could not be read: the derivative store may be a mount that is not
    /// ready, and an evicted rung is rebuilt by the blob route rather than by this job.
    ///
    /// A rung that decodes but has no surface is `Permanent`: the bytes are immutable, so the
    /// second answer is the first answer. It never reaches `record`, which is deliberate —
    /// `part_shape` requires `size_mm > 0`, so a profile of a shapeless mesh would fail at the
    /// insert and be retried for ever.
    pub(crate) async fn profile_shape(
        &self,
        library: LibraryId,
        revision: RevisionId,
    ) -> Result<Outcome, HandlerError> {
        let shapes = PgShapes(self.db.clone());
        let Some((part, l0)) = shapes
            .l0_of_revision(library, revision)
            .await
            .map_err(classify_db)?
        else {
            return Err(HandlerError::Permanent {
                message: format!(
                    "Could not work out the shape of revision {revision}: library {library} has \
                     no such revision with a viewer mesh to read. Check that the revision belongs \
                     to this library and is not a deleted part's, and re-scan the part if it is."
                ),
            });
        };
        let bytes = DerivativeStore::open(&self.blob_root)
            .get(&l0)
            .map_err(|error| HandlerError::Transient {
                message: format!(
                    "Could not read the viewer mesh revision {revision}'s shape is worked out \
                     from — {error}. It may have been evicted from the derivative cache, in which \
                     case opening the part rebuilds it and this is tried again."
                ),
            })?;
        let (positions, indices) =
            lapidary_cad::read_triangles(&bytes).map_err(|error| HandlerError::Permanent {
                message: format!(
                    "Could not read revision {revision}'s stored viewer mesh — {error}. Rebuild \
                     the part's derivatives from the part's page; the stored bytes will be \
                     refused the same way every time."
                ),
            })?;
        let profile = lapidary_cad::profile(&positions, &indices).map_err(|error| {
            HandlerError::Permanent {
                message: format!(
                    "Could not work out the shape of revision {revision} — {error}. The part is \
                     indexed and searchable; it is left out of the near-duplicate review, which \
                     is all a shape is used for."
                ),
            }
        })?;
        // `false` is not a failure: `record` refuses a profile of an older revision than the row
        // already holds, which is exactly what should happen when a backfill finishes after the
        // next revision's own profile landed.
        shapes
            .record(part, revision, l0, &profile)
            .await
            .map_err(classify_db)?;
        Ok(Outcome::Profiled)
    }

    /// [`profile_shape`](Self::profile_shape) with its failures logged rather than returned, for
    /// the two places that call it after a transaction has already committed.
    ///
    /// Warn-only, exactly as `record_topology` is and for the same reason: the part is ingested,
    /// measured and in the grid, and failing the job would tell the user their file did not
    /// ingest when it did. What it costs is one part missing from the duplicate review until the
    /// next worker start sweeps it up, which is what `enqueue_stale_shapes` is for.
    pub(crate) async fn record_shape(
        &self,
        library: LibraryId,
        revision: RevisionId,
        source_path: &str,
    ) {
        if let Err(error) = self.profile_shape(library, revision).await {
            let message = match &error {
                HandlerError::Permanent { message } | HandlerError::Transient { message } => {
                    message.as_str()
                }
            };
            tracing::warn!(
                source_path,
                %revision,
                error = message,
                "could not work out this part's shape; it is left out of the near-duplicate review \
                 until a worker start sweeps it up"
            );
        }
    }

    /// Queue a `profile_shape` for every part whose latest revision has an L0 tessellation but no
    /// current profile of it. A worker runs this as it starts, beside the stale-derivative sweep.
    ///
    /// Never fails, for that sweep's reason: a database that will not answer at startup costs a
    /// backfill delayed to the next start, not a worker that never came up.
    pub(crate) async fn enqueue_stale_shapes(&self) {
        let stale = match PgShapes(self.db.clone()).stale_revisions(PER_START).await {
            Ok(stale) => stale,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "could not check which parts are missing a shape profile; the next worker \
                     start tries again"
                );
                return;
            }
        };
        let jobs = PgJobs(self.db.clone());
        let mut queued = 0usize;
        for part in &stale {
            let payload = JobPayload::ProfileShape {
                revision: part.revision,
            };
            // `running_counts`: a running job read the rung this one would read, and the rung is
            // immutable, so its answer cannot depend on when it read. Same rule as a derive's.
            match jobs.enqueue_if_absent(part.library, &payload, true).await {
                Ok((_, true)) => queued += 1,
                Ok((_, false)) => {}
                Err(error) => tracing::warn!(
                    part = %part.part,
                    %error,
                    "could not queue this part's shape profile; the next worker start tries again"
                ),
            }
        }
        if queued > 0 {
            tracing::info!(
                queued,
                stale = stale.len(),
                "queued shape profiles for parts that have none, or whose viewer mesh or profile \
                 version moved"
            );
        }
    }
}
