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
use lapidary_db::{NewPartSource, PgBlobs, PgJobs, PgParts, PgRevisions};
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
    ///
    /// Whatever the answer, nothing will ever point at the bundle's own bytes: its parts are
    /// replayed into files of their own. So its blob is released into the 30-day quarantine here,
    /// which outlasts the part jobs that still read it, instead of staying on disk uncounted.
    pub(crate) async fn import_bundle(
        &self,
        batch: BatchId,
        library: LibraryId,
        blake3: BlobHash,
        path: &str,
    ) -> Result<Outcome, HandlerError> {
        let bundle = match self.open_bundle(blake3, path, true).await {
            Ok(bundle) => bundle,
            Err(refused) => {
                self.release_bundle(blake3).await;
                return Err(refused);
            }
        };
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
        self.release_bundle(blake3).await;
        Ok(Outcome::Scanned)
    }

    /// A release that fails leaves a blob out of quarantine: disk not reclaimed, which is not worth
    /// failing an import over.
    async fn release_bundle(&self, blake3: BlobHash) {
        if let Err(err) = PgBlobs(self.db.clone()).release(&blake3).await {
            tracing::warn!(error = %err, bundle = %blake3.to_hex(), "could not release an imported bundle's bytes into quarantine");
        }
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
        let mut bundle = self.open_bundle(blake3, path, false).await?;
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

        // Where this library already is in the part's history. A controlled part whose revisions are
        // the bundle's first ones, in order, resumes after them, which makes an import that stopped
        // half way, or one run twice, finish rather than fail, reverts included. A part with any
        // other history is another part's, and is refused rather than grafted onto. A hobby library
        // keeps one revision, and `index` decides between skipped and unkept.
        let revisions_db = PgRevisions(self.db.clone());
        let held: Vec<String> = match revisions_db
            .current(library, &part.source_path)
            .await
            .map_err(transient)?
        {
            None => Vec::new(),
            Some(current) => {
                let mut rows = revisions_db
                    .history(current.part)
                    .await
                    .map_err(transient)?;
                rows.reverse();
                rows.into_iter()
                    .map(|row| {
                        row.source_hash
                            .map(|hash| hash.to_hex())
                            .unwrap_or_default()
                    })
                    .collect()
            }
        };
        let start = if held.is_empty() || !controlled {
            0
        } else if held.len() <= revisions.len()
            && held
                .iter()
                .zip(&revisions)
                .all(|(held, revision)| *held == revision.blake3)
        {
            held.len()
        } else {
            return Err(permanent(format!(
                "{} already holds a different history in this library, so importing this one would graft it onto another part's. Import the bundle into another library, or move that part aside first.",
                part.source_path
            )));
        };

        let mut outcome = Outcome::Skipped;
        for revision in &revisions[start..] {
            let bytes = bundle.bytes(&revision.path).map_err(permanent)?;
            // The unpacking job hashed every file; a part's job hashes its own, since the stored
            // bundle could change between the two.
            if bytes.len() as u64 != revision.size_bytes
                || blake3::hash(&bytes).to_hex().as_str() != revision.blake3
            {
                return Err(permanent(format!(
                    "The file at {} in the bundle is not the one its manifest names. Import the bundle again.",
                    revision.path
                )));
            }
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

        // What a person typed travels with the bytes: the part number, the tags, and where the part
        // came from — which is how its licence reaches whoever imports it. Only for a part this
        // import created: one this library already held may have been edited here since, and the
        // bundle's older values must never quietly replace that.
        if outcome == Outcome::Ingested
            && (part.part_number.is_some() || !part.tags.is_empty() || !part.sources.is_empty())
        {
            let imported = revisions_db
                .current(library, &part.source_path)
                .await
                .map_err(transient)?
                .ok_or_else(|| {
                    transient(format!(
                        "{} was imported but could not be found again.",
                        part.source_path
                    ))
                })?
                .part;
            let parts = PgParts(self.db.clone());
            if let Some(number) = part.part_number.as_deref() {
                parts
                    .set_part_number(imported, Some(number))
                    .await
                    .map_err(transient)?;
            }
            if !part.tags.is_empty() {
                parts
                    .set_tags(imported, &part.tags)
                    .await
                    .map_err(transient)?;
            }
            for source in &part.sources {
                parts
                    .add_part_source(
                        imported,
                        NewPartSource {
                            url: source.url.as_deref(),
                            vendor: source.vendor.as_deref(),
                            external_id: source.external_id.as_deref(),
                            title: source.title.as_deref(),
                            license: source.license.as_deref(),
                            // Prices stay out of a bundle, so there is none to write here.
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(transient)?;
            }
        }
        Ok(outcome)
    }

    /// The uploaded bundle, read back and checked whole. Every refusal is `Permanent`: another
    /// attempt reads the same bytes.
    async fn open_bundle(
        &self,
        blake3: BlobHash,
        path: &str,
        hash_every_file: bool,
    ) -> Result<Bundle, HandlerError> {
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
        if hash_every_file {
            Bundle::open(bytes).map_err(permanent)
        } else {
            Bundle::read(bytes).map_err(permanent)
        }
    }
}
