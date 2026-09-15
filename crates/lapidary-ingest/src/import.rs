//! Importing a bundle (Phase 4 slice 2 spec §7).
//!
//! The api stores the uploaded ZIP and queues `ImportBundle`. That job reads the whole archive back
//! and checks it before anything is written (`lapidary_targets::bundle::Bundle::open`), then queues
//! one `ImportPart` per part into its own batch, as a scan queues one file per candidate. A part's
//! job replays its revisions through `index`, oldest first, so every label, parent link and figure
//! is this library's own, and the rules a scan follows (skipped, revised, unkept) apply unchanged.

use crate::handler::WorkerHandler;
use lapidary_core::{
    BatchId, BlobHash, JobPayload, LibraryId, LibraryMode, Outcome, RevisionOrigin,
};
use lapidary_db::{PgBlobs, PgJobs, PgParts, PgRevisions};
use lapidary_jobs::HandlerError;
use lapidary_storage::SourceReader;
use lapidary_targets::bundle::Bundle;

fn transient(err: impl std::fmt::Display) -> HandlerError {
    HandlerError::Transient {
        message: err.to_string(),
    }
}

fn permanent(message: String) -> HandlerError {
    HandlerError::Permanent { message }
}

impl WorkerHandler {
    /// Check the bundle whole, then queue its parts into this job's batch.
    pub(crate) async fn import_bundle(
        &self,
        batch: BatchId,
        library: LibraryId,
        blake3: BlobHash,
        path: &str,
    ) -> Result<Outcome, HandlerError> {
        let bundle = self.open_bundle(blake3, path).await?;
        let mut jobs = Vec::with_capacity(bundle.manifest.parts.len());
        for (index, part) in bundle.manifest.parts.iter().enumerate() {
            jobs.push(JobPayload::ImportPart {
                bundle: blake3,
                part: u32::try_from(index).map_err(|_| {
                    permanent(format!(
                        "{path} holds more parts than one import can queue."
                    ))
                })?,
                path: part.source_path.clone(),
            });
        }
        // `enqueue_into`, for `scan_directory`'s reason: the parts belong to the batch the
        // browser is already following.
        PgJobs(self.db.clone())
            .enqueue_into(batch, library, &jobs)
            .await
            .map_err(transient)?;
        Ok(Outcome::Scanned)
    }

    /// One part: its revisions through `index`, oldest first, from wherever this library already is
    /// in that history. A controlled library gets every revision. A hobby library keeps none, so it
    /// gets the newest.
    pub(crate) async fn import_part(
        &self,
        library: LibraryId,
        blake3: BlobHash,
        index: u32,
        path: &str,
    ) -> Result<Outcome, HandlerError> {
        let mut bundle = self.open_bundle(blake3, path).await?;
        let Some(part) = usize::try_from(index)
            .ok()
            .and_then(|index| bundle.manifest.parts.get(index))
            .cloned()
        else {
            return Err(permanent(format!(
                "The bundle holds no part {index} for {path}. Import the bundle again."
            )));
        };
        let controlled = PgParts(self.db.clone())
            .libraries()
            .await
            .map_err(transient)?
            .into_iter()
            .find(|row| row.id == library)
            .map(|row| row.mode == LibraryMode::Controlled.as_str())
            .ok_or_else(|| {
                permanent("The library this import was queued for no longer exists.".to_owned())
            })?;
        let revisions = if controlled {
            part.revisions.clone()
        } else {
            part.revisions.last().cloned().into_iter().collect()
        };

        // Where this library already is in the part's history. A part holding one of the bundle's
        // revisions resumes after it, which makes an import that stopped half way, or one run
        // twice, finish rather than fail. A part holding anything else is another part's history.
        let held = PgRevisions(self.db.clone())
            .current(library, &part.source_path)
            .await
            .map_err(transient)?
            .and_then(|current| current.source_hash)
            .map(|hash| hash.to_hex());
        let start = match held {
            None => 0,
            Some(held) => match revisions
                .iter()
                .position(|revision| revision.blake3 == held)
            {
                Some(at) => at,
                None if !controlled => 0,
                None => {
                    return Err(permanent(format!(
                        "{} already holds a different file in this library, so importing its history would graft it onto another part's. Import the bundle into another library, or move that part aside first.",
                        part.source_path
                    )));
                }
            },
        };

        let mut outcome = Outcome::Skipped;
        for revision in &revisions[start..] {
            let bytes = bundle.bytes(&revision.path).map_err(permanent)?;
            let hash = BlobHash::parse_hex(&revision.blake3).map_err(|_| {
                permanent(format!(
                    "The bundle names {} for {}, which is not a BLAKE3 digest. Export the bundle again.",
                    revision.blake3, revision.path
                ))
            })?;
            let origin = RevisionOrigin::parse(&revision.origin).ok_or_else(|| {
                permanent(format!(
                    "The bundle says {} came by {:?}, which this Lapidary does not know.",
                    revision.path, revision.origin
                ))
            })?;
            let done = self
                .index(library, &part.source_path, bytes, hash, origin, None)
                .await?;
            // The part's own answer: `ingested` for a new part, `revised` when this library already
            // held the start of its history, `skipped` when it held all of it.
            if outcome == Outcome::Skipped {
                outcome = done;
            }
        }
        Ok(outcome)
    }

    /// The uploaded bundle, read back and checked whole. Every refusal is `Permanent`: another
    /// attempt reads the same bytes.
    async fn open_bundle(&self, blake3: BlobHash, path: &str) -> Result<Bundle, HandlerError> {
        let stored = PgBlobs(self.db.clone())
            .blob(&blake3)
            .await
            .map_err(transient)?
            .ok_or_else(|| {
                permanent(format!(
                    "The uploaded bundle {path} is no longer in the blob store. Upload it again."
                ))
            })?;
        // ponytail: the bundle is read whole into memory, up to the upload route's 2 GiB. Read it
        // through a temporary file if the worker's memory cannot hold that.
        let bytes = SourceReader::open(&self.blob_root)
            .get(&blake3, Some(stored.zstd_level))
            .map_err(|err| {
                transient(format!("Could not read the uploaded bundle {path}: {err}"))
            })?;
        if BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes()) != blake3 {
            return Err(permanent(format!(
                "The stored bundle {path} no longer hashes to what was uploaded. Upload it again."
            )));
        }
        Bundle::open(bytes).map_err(permanent)
    }
}
