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
//! once at ingest, so a second answer would be the same answer. A write refused by
//! `upsert_derivative`'s two shape guards is `Permanent` too — `classify_db` decides
//! that, per variant, so a new call site cannot get it wrong by picking the mapper.

use crate::handler::{CAD_FORMATS, WorkerHandler, classify_cad, classify_db, reap};
use crate::scan::MESH_EXTENSIONS;
use lapidary_cad::KernelParams;
use lapidary_core::{DerivativeKind, LibraryId, Outcome, RevisionId};
use lapidary_db::{DerivativeBytes, PgBlobs, PgIngest, PgJobs, PgParts, StoredBlobRow};
use lapidary_jobs::HandlerError;
use lapidary_storage::{Compression, DerivativeStore, SourceStore, WorkerRole};

impl WorkerHandler {
    /// Queue a rebuild of every rung, and every CAD read, an older kernel wrote. A worker runs this as
    /// it starts.
    ///
    /// The version is asked per format exactly as [`derive_one`](Self::derive_one) asks it, so
    /// a rung is stale when rebuilding it here would record a different version. A CAD format on
    /// a worker without a CAD kernel is skipped, since this worker could not rebuild it. Never
    /// fails: a database that will not answer at startup costs a rebuild delayed to the next
    /// start, not a worker that never came up.
    pub async fn enqueue_stale_derivatives(&self) {
        let jobs = PgJobs(self.db.clone());
        for format in MESH_EXTENSIONS.iter().chain(&CAD_FORMATS) {
            let Ok(kernel) = self.kernel_for(format) else {
                continue;
            };
            let params = KernelParams {
                linear_deflection_mm: None,
                format: (*format).to_owned(),
                produce: vec![DerivativeKind::TessellationL0],
            };
            let version = kernel.version(&params);
            let kernel_version = format!("{} {}", version.implementation, version.version);
            match jobs
                .enqueue_stale_derivatives(format, &kernel_version)
                .await
            {
                Ok(0) => {}
                Ok(queued) => tracing::info!(
                    format = %format,
                    queued,
                    kernel_version = %kernel_version,
                    "queued rebuilds of derivatives an older kernel wrote"
                ),
                Err(error) => tracing::warn!(
                    format = %format,
                    %error,
                    "could not check for derivatives an older kernel wrote; the next worker \
                     start tries again"
                ),
            }
        }
    }

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
        let Some(stored) = PgParts(self.db.clone())
            .revision_source(library, revision)
            .await
            .map_err(classify_db)?
        else {
            return Err(HandlerError::Permanent {
                message: format!(
                    "Could not build the {kind} for revision {revision}: library {library} \
                     has no such revision with a source file to re-read. Check that the \
                     revision belongs to this library, and re-scan the part if it does."
                ),
            });
        };

        // Two ways of naming the same file, and the row says which applies. A part
        // ingested since the store became a folder tree carries the path its bytes are
        // actually at; one from before that carries NULL and its bytes are still at
        // `blobs/ab/cd/<hash>` until the `migrate_storage` job reaches them (migration
        // `0009`). Reading the path when it is there is not an optimisation — nothing
        // writes the content-addressed copy any more, so the fallback alone would fail on
        // every part ingested from now on.
        //
        // The recorded `zstd_level`, never `for_source_format`, for the path-addressed
        // read: that is `SourceReader::get`'s rule and it applies here for the same
        // reason. The content-addressed fallback keeps the call it has always made, so
        // rows written before `blob.zstd_level` was worth consulting read exactly as they
        // did yesterday.
        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let format = stored.format;
        let bytes = match stored.storage_path.as_deref() {
            Some(rel) => source.get_at(rel, stored.zstd_level),
            None => source.get(&stored.hash, Compression::for_source_format(&format)),
        }
        .map_err(|e| HandlerError::Transient {
            message: e.to_string(),
        })?;

        // A CAD read is one bridge run that reads the tree, the entities and the PMI together, so
        // asking for one of them is asking for all three.
        let produce = match want {
            DerivativeKind::Structure | DerivativeKind::Entities | DerivativeKind::Pmi => vec![
                DerivativeKind::Structure,
                DerivativeKind::Entities,
                DerivativeKind::Pmi,
            ],
            other => vec![other],
        };
        let params = KernelParams {
            linear_deflection_mm: None,
            format,
            produce,
        };
        let kernel = self.kernel_for(&params.format)?;
        let version = kernel.version(&params);
        let kernel_version = format!("{} {}", version.implementation, version.version);
        let output = kernel
            .process(&bytes, &params)
            .await
            .map_err(classify_cad)?;

        let ingest = PgIngest(self.db.clone());
        // `want` is what gets written, never a kind read back off the output: the kernel
        // was asked for exactly one thing, and a rung filed under a level nobody asked for
        // is a cache entry that can never be hit.
        match want {
            // All three from the one read, and `structure` last: the stale sweep finds a CAD read by
            // its `structure` row's version, so a job that stops partway is found again. A file with
            // no analytic surface has no entities row, and one that specifies no PMI has no PMI row,
            // exactly as ingest writes them.
            DerivativeKind::Structure | DerivativeKind::Entities | DerivativeKind::Pmi => {
                let unserializable = |e: serde_json::Error| HandlerError::Permanent {
                    message: format!(
                        "Could not store what the CAD kernel read for revision {revision} — {e}. \
                         This is a bug in Lapidary; please report it with the part's file."
                    ),
                };
                let Some(structure) = output.structure.as_ref() else {
                    return Err(missing(DerivativeKind::Structure.as_str(), revision));
                };
                let structure = serde_json::to_vec(structure).map_err(unserializable)?;
                let entities = (!output.entities.is_empty())
                    .then(|| serde_json::to_vec(&output.entities))
                    .transpose()
                    .map_err(unserializable)?;
                let pmi = output
                    .pmi
                    .as_ref()
                    .map(serde_json::to_vec)
                    .transpose()
                    .map_err(unserializable)?;
                // The counts come from the same read, so a revision read before the bridge counted
                // faces and edges gets them too, before `structure` marks the read current.
                self.record_topology(revision, output.topology, &format!("revision {revision}"))
                    .await;
                // And its centre of mass, which a bridge before 8 did not write.
                self.record_centre_of_mass(
                    revision,
                    output.centre_of_mass_mm,
                    output.provenance.volume,
                    &format!("revision {revision}"),
                )
                .await;
                for (read, json) in [
                    (DerivativeKind::Entities, entities),
                    (DerivativeKind::Pmi, pmi),
                    (DerivativeKind::Structure, Some(structure)),
                ] {
                    if let Some(json) = json {
                        self.store_hashed(revision, read, &json, None, &kernel_version)
                            .await?;
                    }
                }
            }
            // A file for another tool, written from the part's mesh, and served as `*.lapidary.*`.
            DerivativeKind::ExportStl | DerivativeKind::Export3mf => {
                if let Some(refused) = output.unproduced.iter().find(|u| u.kind == want) {
                    return Err(HandlerError::Permanent {
                        message: refused.reason.clone(),
                    });
                }
                let Some((_, bytes)) = output.exports.iter().find(|(made, _)| *made == want) else {
                    return Err(missing(kind, revision));
                };
                self.store_hashed(revision, want, bytes, None, &kernel_version)
                    .await?;
            }
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
                    .map_err(classify_db)?;
            }
            DerivativeKind::TessellationL0
            | DerivativeKind::TessellationL1
            | DerivativeKind::TessellationL2 => {
                let Some(rung) = output.tessellations.first() else {
                    return Err(missing(kind, revision));
                };
                self.store_hashed(revision, want, &rung.glb, rung.grid, &kernel_version)
                    .await?;
            }
        }
        Ok(Outcome::Rendered)
    }

    /// `bytes` into the derivative store, then the row for `kind` pointing at them.
    async fn store_hashed(
        &self,
        revision: RevisionId,
        kind: DerivativeKind,
        bytes: &[u8],
        grid: Option<u32>,
        kernel_version: &str,
    ) -> Result<(), HandlerError> {
        // The bytes go to disk before the row that points at them, exactly as ingest's ladder
        // does: a filesystem write cannot be rolled back by Postgres.
        let derivatives = DerivativeStore::open(&self.blob_root);
        let stored = derivatives
            .put(bytes)
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })?;
        // Only bytes this job introduced may be reaped. Bytes some revision already stores are
        // bytes that revision is still serving, and removing them would be silent data loss --
        // the same rule, asked of the same authority, as `ingest_one`'s ladder.
        let reapable = !PgBlobs(self.db.clone())
            .exists(&stored.hash)
            .await
            .map_err(classify_db)?;
        let blob = StoredBlobRow {
            hash: stored.hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        };
        if let Err(db_err) = PgIngest(self.db.clone())
            .upsert_derivative(
                revision,
                kind,
                DerivativeBytes::Hashed { blob: &blob, grid },
                kernel_version,
            )
            .await
        {
            if reapable {
                reap(&derivatives, &[blob.hash]);
            }
            return Err(classify_db(db_err));
        }
        Ok(())
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
