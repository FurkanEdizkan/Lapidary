//! One derivative, rebuilt from a revision's source bytes.
//!
//! The other half of the trade `handler.rs` makes: ingest now writes L0 and, if the
//! library wants one, a thumbnail, and everything else arrives here when something asks
//! for it (design §3.1). Same procedure whatever is being made — resolve the source blob,
//! read it, parse it, produce exactly one thing, upsert — which is why there is one
//! `derive` job kind discriminated by a `DerivativeKind` rather than one kind per rung
//! (§3.6).
//!
//! # The revision is taken, never resolved
//!
//! `revision_source(job.library_id, payload.revision)`, never `latest_revision(part)`.
//! The payload names the revision precisely so that nothing resolves "latest" a second
//! time (§3.7): a job enqueued against revision A while a second revision lands must not
//! render onto B and report success. That is also why nothing here reads `ingest_dir` —
//! the bytes come from the blob store, which is the only place they are still guaranteed
//! to be. The library comes from the job's own column rather than the payload, and the
//! query is scoped by it: a revision id is a uuid a caller might hold from anywhere, and
//! an unscoped resolve renders one library's part onto another's job.
//!
//! # Error classification
//!
//! `handler.rs`'s rules, applied to a shorter pipeline. A revision this library cannot
//! reach — no such revision, or no source file on it — is `Permanent`: there are no bytes
//! to re-read and there never will be. A blob store that
//! will not give up the bytes is `Transient`: the store may be a mount that is not ready.
//! A parse failure is `Permanent`, because the bytes are immutable and already parsed
//! once at ingest, so a second answer would be the same answer.

use crate::handler::{WorkerHandler, reap, transient_db};
use lapidary_cad::{Kernel, KernelParams, MeshKernel};
use lapidary_core::{DerivativeKind, LibraryId, Outcome, RevisionId};
use lapidary_db::{DerivativeBytes, PgBlobs, PgIngest, PgParts, StoredBlobRow};
use lapidary_jobs::HandlerError;
use lapidary_storage::{Compression, DerivativeStore, SourceStore, WorkerRole};

impl WorkerHandler {
    /// Produce `want` for `revision` and upsert it, replacing whatever was there.
    ///
    /// Returns `Outcome::Rendered` — not `Ingested`. A derive job creates no part and no
    /// source blob; reporting it as an ingest would inflate a batch's `ingested` count
    /// with work that indexed nothing.
    pub(crate) async fn derive_one(
        &self,
        library: LibraryId,
        revision: RevisionId,
        want: DerivativeKind,
    ) -> Result<Outcome, HandlerError> {
        let kind = want.as_str();
        let Some((hash, format)) = PgParts(self.db.clone())
            .revision_source(library, revision)
            .await
            .map_err(transient_db)?
        else {
            return Err(HandlerError::Permanent {
                message: format!(
                    "Could not build the {kind} for revision {revision}: library {library} \
                     has no such revision with a source file to re-read. Check that the \
                     revision belongs to this library, and re-scan the part if it does."
                ),
            });
        };

        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let bytes = source
            .get(&hash, Compression::for_source_format(&format))
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })?;

        let kernel = MeshKernel;
        let params = KernelParams {
            linear_deflection_mm: None,
            format,
            produce: vec![want],
        };
        let version = kernel.version(&params);
        let kernel_version = format!("{} {}", version.implementation, version.version);
        let output =
            kernel
                .process(&bytes, &params)
                .await
                .map_err(|e| HandlerError::Permanent {
                    message: e.to_string(),
                })?;

        let ingest = PgIngest(self.db.clone());
        // `want` is what gets written, never a kind read back off the output: the kernel
        // was asked for exactly one thing, and a rung filed under a level nobody asked for
        // is a cache entry that can never be hit.
        match want {
            DerivativeKind::Thumbnail => {
                let Some(webp) = output.thumbnail_webp else {
                    return Err(missing(kind, revision));
                };
                ingest
                    .upsert_derivative(
                        revision,
                        want,
                        DerivativeBytes::Inline(&webp),
                        &kernel_version,
                    )
                    .await
                    .map_err(transient_db)?;
            }
            DerivativeKind::TessellationL0
            | DerivativeKind::TessellationL1
            | DerivativeKind::TessellationL2 => {
                let Some(rung) = output.tessellations.first() else {
                    return Err(missing(kind, revision));
                };
                // The bytes go to disk before the row that points at them, exactly as
                // ingest's ladder does: a filesystem write cannot be rolled back by
                // Postgres.
                let derivatives = DerivativeStore::open(&self.blob_root);
                let stored = derivatives
                    .put(&rung.glb)
                    .map_err(|e| HandlerError::Transient {
                        message: e.to_string(),
                    })?;
                // Only bytes this job introduced may be reaped. A rung whose bytes some
                // revision already stores is bytes that revision is still serving, and
                // removing them would be silent data loss -- the same rule, asked of the
                // same authority, as `ingest_one`'s ladder.
                let reapable = !PgBlobs(self.db.clone())
                    .exists(&stored.hash)
                    .await
                    .map_err(transient_db)?;
                let blob = StoredBlobRow {
                    hash: stored.hash,
                    size_bytes: stored.size_bytes,
                    stored_bytes: stored.stored_bytes,
                    zstd_level: stored.zstd_level,
                };
                if let Err(db_err) = ingest
                    .upsert_derivative(
                        revision,
                        want,
                        DerivativeBytes::Hashed {
                            blob: &blob,
                            grid: rung.grid,
                        },
                        &kernel_version,
                    )
                    .await
                {
                    if reapable {
                        reap(&derivatives, &[blob.hash]);
                    }
                    return Err(transient_db(db_err));
                }
            }
        }
        Ok(Outcome::Rendered)
    }
}

/// The kernel was asked for one thing and returned nothing. Unreachable through
/// `MeshKernel`, which produces whatever `produce` names or fails, but the output type
/// permits it and a silent `Ok` here would report a derivative that was never written.
fn missing(kind: &str, revision: RevisionId) -> HandlerError {
    HandlerError::Permanent {
        message: format!(
            "The kernel produced no {kind} for revision {revision} despite being asked for \
             one. This is a bug in Lapidary; please report it with the part's file."
        ),
    }
}
