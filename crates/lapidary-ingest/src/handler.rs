//! One file's worth of ingest, as a job. The pipeline below is slice 1's, moved rather
//! than rewritten: read, BLAKE3, library_holds, measure, write, record.
//!
//! **R3 took every derivative out of it, and R5 took the kernel.** Nothing renders, so step 4
//! parses the mesh and measures it and there is no step 5: no thumbnail, no L0 rung, no CAD
//! read. The centre of mass is still recorded here, from the same measurement output, which is
//! what let the historical backfill job go.
//!
//! # Ordering
//!
//! Per file, in this order — the order is the design:
//!
//! 1. read bytes
//! 2. BLAKE3 — hash first, always
//! 3. `blobs.library_holds(library, source_path, hash)` — does the part's *current* revision
//!    hold these bytes? yes -> `Skipped`, no further work at all: not a parse, not a raster,
//!    not a query beyond this one
//!    3b. `revisions.current(library, source_path)` — a part already here, with other bytes:
//!    on to 5a. A deleted part has no rows left, so a file whose part was deleted is not
//!    "already here" and is ingested again as a new part
//!    3a. `parts.library_exists(library)` — the library is still there;
//!    deliberately below the short-circuit, so a re-scan still costs one query
//! 4. `lapidary_cad::parse(bytes, format)` then `measure(&mesh)` — and nothing else
//!    5a. the part's next revision: `revisions.record_revision(...)` locks the part,
//!    sets its current file aside under `revisions/<label>/` and writes these bytes where it
//!    was, then `metadata.json` is rewritten from the rows -> `Revised`. Steps 6 to 10 are
//!    a new part's, not a revision's
//! 6. `blobs.exists(hash)` — whether a `blob` row for these bytes is already there. Read
//!    here rather than beside the insert it decides, so that nothing fallible sits
//!    between the source write and the transaction
//! 7. `model_dir_for(...)` — the category rows this file's directories imply, and the
//!    directory this model lands in
//! 8. `source.put_new_at(storage_path, bytes, AsIs)` writes the file into that directory,
//!    under its own name, *before* the transaction, and never over a file already there:
//!    other bytes there are another job's, and this job is retried to decide again
//! 9. `ingest.record(...)`, or `ingest.link_existing(...)` when step 6 said the row is
//!    there; on a genuine failure the file and the directory this job wrote
//!    are reaped and the failure is returned
//! 10. `metadata.json` beside the file, from the ids the transaction generated
//!
//! Step 9's reap is not optional. The Node prototype wrote its blob and then failed the
//! insert with no cleanup, leaving bytes on disk that nothing referenced and nothing
//! would ever collect — `docs/prototype-notes.md` records it. It is equally not optional
//! that it runs only when the write really failed: `classify_write` turns a unique
//! violation into `Skipped`, which means another worker's part row now describes the file
//! at this path, and reaping it would be silent data loss. That is why the reap is inside
//! an `is_err()` and not on every `Err` arm. For the *source* file that is now settled by
//! step 8: a file this job wrote is its own, and a lost race with other bytes reaps it and
//! is retried, where it used to settle as `Skipped` beside an orphan copy. The rungs used to be
//! this guard's live exposure — two workers meshing one file produced the same rung bytes, both
//! saw `blobs.exists` answer false for them, and a loser that reaped on its way to `Skipped`
//! would have taken the derivatives the winner's committed rows served. Ingest writes no
//! derivative now; the same rule still holds in `derive.rs`'s `store_hashed`, which is the one
//! place left that files derivative bytes.
//!
//! # Where the bytes go, and why both branches write them
//!
//! Source files are path-addressed: `libraries/<library>/<category…>/<model>/cliff.stl`,
//! with a `metadata.json` beside each one. The store is something the owner opens in a
//! file manager, which the content-addressed `blobs/ab/cd/<hash>` layout could never be —
//! see `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md` §0, which
//! reverses `DATA.md` §1.1 on the owner's instruction and states the price: identical
//! bytes in two models are two files, and source dedup is gone.
//!
//! So the old asymmetry is gone with it. `blobs.exists(hash)` used to decide both whether
//! to write bytes and which insert to run; it now decides only the second, which is why
//! it moved above the write, because a
//! model directory that does not hold its own file is a directory that does not hold the
//! model. `blob.ref_count` keeps its meaning — how many `file` rows name this hash — and
//! loses the implication that it counts copies on disk.
//!
//! Derivatives are untouched by any of this. They stay content-addressed, freely
//! evictable, and reachable by hash; only source files moved.
//!
//! # Why the short-circuit is scoped to the library, and to the path
//!
//! It was not, and the consequence was live: `PgBlobs::exists` is keyed on the hash
//! alone, so scanning six real STLs into a brand-new empty library answered
//! `{"ingested":0,"skipped":6,"failed":[]}` and left the library empty. Three things
//! were wrong at once — a content hash decided a per-library write (CLAUDE.md: content
//! addressing is not authorization), the counter said a file row had been linked when
//! none had, and the user got an empty grid with no error anywhere to explain it.
//!
//! `blobs.library_holds(library, source_path, hash)` is the question this handler
//! actually needs: *is this the same file, seen again?* The bytes are still reused — that
//! is the whole point of content addressing, and step 5 reuses them without a second
//! write — but a library that does not have this part gets one.
//!
//! Keying on the source path as well as the hash settles the other half, which the
//! earlier rounds recorded as an open question: two files with identical bytes at two
//! paths are two parts sharing one blob (`ref_count` 2), not one part and one silent
//! omission. A directory of files is a set of files, and a scan that quietly indexes only
//! the first of two is the same shape of lie as the empty second library. Only "same
//! library, same path, same bytes" is a re-scan.
//!
//! **Slice 6a moved this key from the name to the path, and had to.** Until the scan
//! learned to descend, the file stem was unique within a library and stood in for the
//! path perfectly well. It stopped being unique the moment `brackets/bracket.stl` and
//! `plates/bracket.stl` could both exist: a name-keyed lookup calls the second a re-scan
//! of the first and skips it, and a name-keyed unique constraint makes the write that
//! would have caught the mistake report `Skipped` too. Recursion and this key are one
//! change — see `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md` §2.
//!
//! Known limitation, scheduled rather than guessed at: the path is now *recorded*, but
//! nothing compares it across scans, so renaming a file still yields a new part beside
//! the old one rather than a rename. The column is the prerequisite; the comparison
//! belongs to the slice that owns incremental directory sync. A duplicate the user can
//! see is the right failure mode to have in the meantime, against a silent one.
//!
//! # Error classification
//!
//! The parser's errors are `Permanent`: blobs are content-addressed and immutable, so
//! re-parsing the same bytes is guaranteed to produce the same error, and a retry only
//! delays an answer that is already available. Database, blob-store and I/O errors are
//! `Transient` — the file is fine, something else was not, and worth another attempt.
//! When genuinely unsure, this module chooses `Transient`: a retried permanent failure
//! costs one wasted parse, while a non-retried transient failure costs the user a file.

use lapidary_cad::CadError;
use lapidary_core::slug::{disambiguate, disambiguate_with, slugify};
use lapidary_core::{
    BlobHash, FolderId, JobPayload, LibraryId, MeasurementProvenance, Outcome, Provenance,
    RevisionId, RevisionOrigin, source_format,
};
use lapidary_db::{
    DbError, IngestRequest, JobRow, PgBlobs, PgFolders, PgIngest, PgParts, PgPool, PgRevisions,
    RevisionRequest, StoredBlobRow,
};
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_storage::{
    Compression, DerivativeStore, SourceReader, SourceRelocator, SourceStore, StorageError,
    WorkerRole,
};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

pub struct WorkerHandler {
    pub db: PgPool,
    /// The read-only mounted ingest directory ingest_one reads its file from. Never a
    /// hardcoded container path: tests point it at a `TempDir`, and `deploy/compose.yaml`
    /// (Task 12) supplies the real `/ingest` mount.
    pub ingest_dir: PathBuf,
    /// Root of the blob store. This crate is the one place in the workspace allowed to
    /// construct a `SourceStore` over it — see `lib.rs`'s module doc.
    pub blob_root: PathBuf,
    /// Suspends an ingest just before it parses. `None` everywhere a worker really runs.
    ///
    /// **This is all that is left of the `Kernel` trait, and it is all the trait was for.**
    /// `WorkerHandler.cad` was `Option<Arc<dyn Kernel>>` so the OCCT sidecar could be swapped
    /// in; R2 refused those formats and kept the field, renamed, because parsing is synchronous
    /// all the way down — so it is the only instant at which one of two jobs racing over one
    /// new path can be held while the other finishes, and the two slice-1 §3.3 races in
    /// `tests/handler.rs` are staged through it. A trait with one implementation is not worth
    /// keeping to obtain one await point; two `Notify` handles are.
    pub pause: Option<Pause>,
    /// Where the model sites an `import_link` reads from are. `Sites::public()` everywhere a
    /// worker really runs; tests point it at a fixture server.
    pub sites: lapidary_sites::Sites,
}

/// The await point above, and nothing else. A test holds a job here, lets another job run to
/// completion, and then releases this one. The two races in `tests/handler.rs` that need it are
/// the ones over a new path: one with other bytes already there, one with the same bytes. Neither
/// can be written without an instant at which the loser is suspended.
pub struct Pause {
    /// Notified when the job reaches the parse.
    pub reached: Arc<tokio::sync::Notify>,
    /// Awaited before the parse runs.
    pub release: Arc<tokio::sync::Notify>,
}

impl JobHandler for WorkerHandler {
    /// The `kind` COLUMN decides which arm runs. Both failures below are `Permanent`: a
    /// kind this build does not know will not become known on a retry, and a payload that
    /// does not parse holds the same bytes next time. `CoreError` names the kind in both
    /// messages, which is the whole reason this goes through `from_row` rather than
    /// reaching into the JSON — the previous version answered every unrecognised job with
    /// "This job has no file path in its payload", which was false for all of them.
    async fn handle(&self, job: &JobRow) -> Result<Outcome, HandlerError> {
        let payload =
            JobPayload::from_row(&job.kind, &job.payload).map_err(|e| HandlerError::Permanent {
                message: e.to_string(),
            })?;
        match payload {
            JobPayload::IngestFile { path } => self.ingest_one(job.library_id, &path).await,
            JobPayload::Derive { revision, produce } => {
                self.derive_one(job.library_id, revision, produce).await
            }
            // The batch comes from this job's own row, never from the payload: the files
            // a scan finds belong in the batch the browser is already polling. See
            // `scan.rs`'s module doc.
            JobPayload::ScanDirectory => self.scan_directory(job.batch_id, job.library_id).await,
            JobPayload::IngestBlob {
                blake3,
                source_path,
            } => self.ingest_blob(job.library_id, blake3, &source_path).await,
            // The batch comes from the row for `scan_directory`'s reason, and the library
            // for the same one: a migration re-enqueues itself until the library drains,
            // and it has to land in the batch whoever started it is already polling.
            JobPayload::MigrateStorage => self.migrate_storage(job.batch_id, job.library_id).await,
            // A model from the page a person pasted, through ordinary ingest; see `link.rs`.
            JobPayload::ImportLink { url } => self.import_link(job.library_id, &url).await,
            // A bundle unpacks into its own batch, as a scan does; see `import.rs`.
            JobPayload::ImportBundle { blake3, path } => {
                self.import_bundle(job.batch_id, job.library_id, blake3, &path)
                    .await
            }
            JobPayload::ImportPart { bundle, part, path } => {
                self.import_part(job.library_id, bundle, part, &path).await
            }
            JobPayload::DescribePart { part } => self.describe_part(job.library_id, part).await,
        }
    }
}

/// The formats this library refuses, as `source_format` spells them. The scan walk accepts these
/// beside the mesh extensions, so a `.step` file is refused by name rather than skipped in
/// silence — see [`lapidary_core::CAD_FORMATS`].
pub(crate) use lapidary_core::CAD_FORMATS;

impl WorkerHandler {
    /// `metadata.json` written again from the part's rows. Refused for a part this job's library does
    /// not hold. A part whose file is still in the content-addressed store has no directory yet, and
    /// the migration that gives it one writes `metadata.json` from the same rows.
    async fn describe_part(
        &self,
        library: LibraryId,
        part: lapidary_core::PartId,
    ) -> Result<Outcome, HandlerError> {
        let transient = |error: lapidary_db::DbError| HandlerError::Transient {
            message: error.to_string(),
        };
        let revisions = PgRevisions(self.db.clone());
        let history = revisions.history(part).await.map_err(transient)?;
        let model_dir = history
            .iter()
            .find_map(|revision| revision.storage_path.as_deref())
            .and_then(|path| path.rsplit_once('/'))
            .map(|(dir, _)| dir.to_owned());
        let store = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let written = revisions
            .write_manifest(part, |manifest| {
                let Some(manifest) = manifest.filter(|manifest| manifest.part.library == library)
                else {
                    return Err(HandlerError::Permanent {
                        message: format!(
                            "There is no part {part} in this library to describe. It may have been deleted; nothing needs doing."
                        ),
                    });
                };
                let Some(model_dir) = &model_dir else {
                    return Ok(());
                };
                let json = serde_json::to_vec_pretty(&manifest).map_err(|error| {
                    HandlerError::Permanent {
                        message: format!("could not describe part {part} as JSON: {error}"),
                    }
                })?;
                store
                    .put_at(
                        &format!("{model_dir}/metadata.json"),
                        &json,
                        Compression::AsIs,
                    )
                    .map(|_| ())
                    .map_err(|error| HandlerError::Transient {
                        message: format!("could not write {model_dir}/metadata.json: {error}"),
                    })
            })
            .await
            .map_err(transient)?;
        written?;
        Ok(Outcome::Described)
    }

    /// The centre of the volume the kernel measured, onto the revision just committed, marked with the
    /// volume's provenance. Warn-only: the revision is committed and measured, and a centre it
    /// lacks leaves its diff without one.
    ///
    /// **This is why R3 could delete the centre-of-mass backfill.** It runs on the ingest path
    /// from the same measurement output, so every part ingested from here on has its centre
    /// without a job; only the historical sweep died, and that sweep worked by rebuilding an L0
    /// rung to obtain one float.
    pub(crate) async fn record_centre_of_mass(
        &self,
        revision: RevisionId,
        centre_mm: Option<[f64; 3]>,
        source: lapidary_core::Provenance,
        source_path: &str,
    ) {
        let Some(centre_mm) = centre_mm else {
            return;
        };
        if let Err(error) = PgRevisions(self.db.clone())
            .set_centre_of_mass(revision, centre_mm, source)
            .await
        {
            tracing::warn!(
                source_path,
                %error,
                "could not record the file's centre of mass; its revision diff leaves it out"
            );
        }
    }

    /// One file on the ingest mount, start to finish. See this module's doc for the
    /// ordering, why each step is where it is, and the full reasoning behind the
    /// library-and-path short-circuit.
    ///
    /// Steps 1 and 2 only. Everything from the short-circuit onwards is [`index`], which
    /// this shares with [`ingest_blob`] — the two differ in where the bytes come from and
    /// in nothing else, and a second copy of the pipeline is a second place for the
    /// reap rules to drift.
    ///
    /// [`index`]: Self::index
    /// [`ingest_blob`]: Self::ingest_blob
    pub async fn ingest_one(
        &self,
        library: LibraryId,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        // The payload is a path relative to `ingest_dir`, and since slice 6a it may have
        // more than one segment. `Path::join` resolves nothing and refuses nothing, so
        // `../../etc/passwd` would escape the mount and `/etc/passwd` would replace it
        // outright. `DATA.md` §5.4 already states this rule for archive entries; it
        // belongs on every path that reaches a filesystem from a payload, and
        // `lapidary_core::slug::reject_escaping_path` is the one guard every such path
        // runs — see its own doc for why it lives there rather than here.
        //
        // `Permanent`, not `Transient`: a payload holds the same bytes on every attempt,
        // so three retries of a traversal would produce three identical refusals and only
        // delay an answer already available.
        lapidary_core::slug::reject_escaping_path(source_path).map_err(|e| {
            HandlerError::Permanent {
                message: e.to_string(),
            }
        })?;
        let path = self.ingest_dir.join(source_path);

        // 1. Read bytes. A missing file may mean the mount is not ready yet, so this is
        // Transient rather than Permanent -- unlike every step below it, this one has
        // nothing to do with the bytes themselves.
        let bytes = std::fs::read(&path).map_err(|e| HandlerError::Transient {
            message: format!("Could not read {source_path}: {e}"),
        })?;

        // 2. BLAKE3 -- hash first, always. Everything below branches on this.
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        self.index(library, source_path, bytes, hash, RevisionOrigin::Ingest)
            .await
    }

    /// One blob already in the store, start to finish: the upload route's half of the
    /// pipeline.
    ///
    /// Usually the api wrote these bytes and verified them against the hash the client
    /// claimed (`SourceWriter::put_file`), and inserted the `blob` row that keeps them from
    /// being an invisible orphan while this job waits. Not always: when the probe found the
    /// bytes already held, the browser sends none and the commit stages nothing, and a
    /// scanned file's only copy is the one filed in its model directory. So steps 1 and 2
    /// are a read back out of the store — the staged copy, else a filed one — and the bytes
    /// are hashed again before anything is indexed, because a filed copy lives in a folder
    /// the owner edits and its path is no proof of what it holds.
    ///
    /// `SourceReader` rather than `SourceStore`, though this crate may construct either:
    /// this arm never writes a source blob and never reaps one — the bytes were not its
    /// to write — and the read-only handle is the one that says so. The staged copy's
    /// `zstd_level` comes from the `blob` row for the reason its own doc gives, which
    /// matters more here than anywhere: the api chose that level, in another process,
    /// possibly on another build. A filed copy's comes from its own `file` row, which
    /// migration `0013` made the authority for files in the folder tree.
    ///
    /// See the slice 6a design, §4.1 and §4.2.
    pub(crate) async fn ingest_blob(
        &self,
        library: LibraryId,
        hash: BlobHash,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        // Not a filesystem join here — the path never touches one — but it becomes
        // `part.source_path`, and from there both a `Content-Disposition` filename on the
        // download route and the model directory this file is written into. Same refusal,
        // different surface, and the same shared guard as `ingest_one` above.
        lapidary_core::slug::reject_escaping_path(source_path).map_err(|e| {
            HandlerError::Permanent {
                message: e.to_string(),
            }
        })?;

        // A blob row this job cannot find is Permanent: the row is written in the same
        // request that wrote the bytes, so its absence is not a race that resolves, and
        // the level needed to decode the file is only in that row.
        let stored = PgBlobs(self.db.clone())
            .blob(&hash)
            .await
            .map_err(classify_db)?
            .ok_or_else(|| HandlerError::Permanent {
                message: format!(
                    "The uploaded bytes for {source_path} are no longer in the blob store. \
                     The upload may have been rolled back after this job was queued; \
                     upload the file again."
                ),
            })?;

        // The staged copy first, at the level the api recorded on the row. On `NotFound`
        // alone, the copies already filed in part directories, newest first — where a
        // re-upload's bytes are when the probe called them known. Any other failure is the
        // store misbehaving rather than the bytes being absent, and worth another attempt.
        let reader = SourceReader::open(&self.blob_root);
        let unchanged = |bytes: Vec<u8>| {
            (BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes()) == hash).then_some(bytes)
        };
        let staged = match reader.get(&hash, Some(stored.zstd_level)) {
            Ok(bytes) => unchanged(bytes),
            Err(StorageError::NotFound { .. }) => None,
            Err(e) => {
                return Err(HandlerError::Transient {
                    message: format!("Could not read the uploaded bytes for {source_path}: {e}"),
                });
            }
        };
        let bytes = match staged {
            Some(bytes) => bytes,
            None => {
                let copies = PgBlobs(self.db.clone())
                    .source_copies(&hash)
                    .await
                    .map_err(classify_db)?;
                let mut found = None;
                let mut unreadable = None;
                for (rel, level) in &copies {
                    match reader.get_at(rel, *level) {
                        // A copy that no longer hashes to these bytes was edited where it is
                        // filed, and one that is gone was moved or deleted there. Both are
                        // passed over rather than retried: another attempt reads the same
                        // file.
                        Ok(bytes) => {
                            found = unchanged(bytes);
                            if found.is_some() {
                                break;
                            }
                        }
                        Err(StorageError::Io { source, .. })
                            if source.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => unreadable = Some(e),
                    }
                }
                match (found, unreadable) {
                    (Some(bytes), _) => bytes,
                    // A copy is there and could not be read for some reason other than its
                    // absence. When genuinely unsure, this module chooses `Transient`.
                    (None, Some(e)) => {
                        return Err(HandlerError::Transient {
                            message: format!(
                                "Could not read a stored copy of the bytes for {source_path}: {e}"
                            ),
                        });
                    }
                    (None, None) => {
                        return Err(HandlerError::Permanent {
                            message: format!(
                                "Could not add {source_path}: this server already holds a file \
                                 with the same contents, but no stored copy of it is still \
                                 readable and unchanged. Each copy is kept in its part's folder \
                                 inside the storage folder; if one was edited or deleted there, \
                                 put the original file back and upload again."
                            ),
                        });
                    }
                }
            }
        };

        self.index(library, source_path, bytes, hash, RevisionOrigin::Upload)
            .await
    }

    /// Steps 3 through 6: short-circuit, kernel, rungs, and the one transaction.
    ///
    /// Both byte sources land here with the same two facts — these bytes, and their hash
    /// — and everything below depends on nothing else about where they came from. Step 5
    /// still asks `blobs.exists(&hash)` rather than being told: for an upload the answer
    /// is always yes, because the api inserted the row, and that is exactly the branch
    /// that links without writing and reaps no source blob. The right behaviour arrived
    /// at by the existing question rather than by a new flag.
    pub(crate) async fn index(
        &self,
        library: LibraryId,
        source_path: &str,
        bytes: Vec<u8>,
        hash: BlobHash,
        origin: RevisionOrigin,
    ) -> Result<Outcome, HandlerError> {
        // Here as well as in the routes that read a file first: a scan, an upload and a bundle's
        // part all name the path a part is filed under, and this is where all three pass.
        lapidary_core::slug::reject_escaping_path(source_path).map_err(|e| {
            HandlerError::Permanent {
                message: e.to_string(),
            }
        })?;
        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let blobs = PgBlobs(self.db.clone());
        let ingest = PgIngest(self.db.clone());
        let name = part_name(source_path);

        // 3. The same file, seen again -- same library, same path, the bytes its current
        // revision holds -- short-circuits parse, raster and every write entirely. Scoped to
        // the library on purpose: a hash this library has never seen is a part it does not
        // have, whatever some other library holds. See `scan.rs`'s module doc.
        if blobs
            .library_holds(library, source_path, &hash)
            .await
            .map_err(classify_db)?
        {
            return Ok(Outcome::Skipped);
        }

        // 3b. A part already at this path, with other bytes (Phase 4 slice 1 spec §1), goes on to
        // become its next revision at step 5a; everything this decides against, it decides
        // before the kernel, so it writes nothing.
        let existing = PgRevisions(self.db.clone())
            .current(library, source_path)
            .await
            .map_err(classify_db)?;
        if let Some(existing) = &existing {
            // The same bytes, committed by another job between step 3's question and this one. Two
            // queries leave that window open, and without this a lease-expiry duplicate of one file
            // was told it was a change nobody keeps.
            if existing.source_hash == Some(hash) {
                return Ok(Outcome::Skipped);
            }
            if existing.storage_path.is_none() {
                return Err(HandlerError::Transient {
                    message: format!(
                        "{source_path} changed, and its part's file is still in the old storage \
                         layout, so there is no folder to keep the previous file in yet. The \
                         storage migration moves it on its own; this change is retried after."
                    ),
                });
            }
        }

        // 3a. The library has to exist. Read *after* the short-circuit, so a re-scan still
        // costs exactly one query. This was `auto_thumbnail`, which was the setting *and* the
        // existence probe; R3 dropped the setting and kept the probe (`library_exists`).
        // A library that does not exist is Permanent: the row will not reappear, and
        // three retries reporting a bare row-count error tell an operator nothing.
        if !PgParts(self.db.clone())
            .library_exists(library)
            .await
            .map_err(classify_db)?
        {
            return Err(HandlerError::Permanent {
                message: format!(
                    "There is no library {library} to ingest {source_path} into. The library \
                     may have been removed after this job was queued; re-scan the library \
                     you meant."
                ),
            });
        }
        // **Nothing else is produced.** Measurements are the whole of what ingest needs: the
        // card shows the part's own picture, the viewer loads the original file when somebody
        // asks, and the two exports are built on demand. Ingest used to write an L0 rung always
        // and a thumbnail if the library wanted one.
        let format = source_format(source_path);
        refuse_b_rep(&format)?;
        if let Some(pause) = &self.pause {
            pause.reached.notify_one();
            pause.release.notified().await;
        }

        // 4. Parse + measure. Nothing has been written yet, so a failure here
        // needs no cleanup. This runs even when the bytes are already in the blob store,
        // because the new part needs its own measurements; only the
        // bytes are shared, and they are already in memory from step 1. The bytes are
        // immutable, so every parse failure is the final answer about them (`classify_cad`).
        let mesh = lapidary_cad::parse(&bytes, &format).map_err(classify_cad)?;
        let (measurements, centre_of_mass_mm) = lapidary_cad::measure(&mesh);
        // The picture a slicer saved inside a 3MF, read now while the bytes are in hand and
        // attached once the part exists (F1). Never a reason to fail: a part with no picture is a
        // part, so a broken thumbnail is a log line.
        let embedded = if format == "3mf" {
            lapidary_cad::thumbnail_3mf(&bytes).unwrap_or_else(|error| {
                tracing::warn!(source_path, %error, "could not read this 3MF's thumbnail; the part has no picture from it");
                None
            })
        } else {
            None
        };

        // 5a. The next revision of the part at step 3b (spec §3.1). It has its folder and its
        // directory already, and its transaction writes the blob row itself, so steps 6 to 9
        // are not its. The files move inside that transaction, after the part row is locked:
        // `set_aside` sets the current file aside and writes these bytes where it was.
        if let Some(existing) = existing {
            let blob = StoredBlobRow {
                hash,
                size_bytes: bytes.len() as u64,
                // `Compression::AsIs`, for step 8's reason: this is the file the owner opens.
                stored_bytes: bytes.len() as u64,
                zstd_level: 0,
            };
            let relocator = SourceRelocator::open(&self.blob_root);
            let mut moved: Option<(String, String)> = None;
            let recorded = PgRevisions(self.db.clone())
                .record_revision(
                    RevisionRequest {
                        part: existing.part,
                        parent: existing.revision,
                        origin,
                        blob: &blob,
                        measurements: &measurements,
                        // Every figure a mesh gives is tessellated, and a mesh is all this
                        // library takes (R2). The kernel used to report this per figure
                        // because a B-rep read some of them off the solid.
                        provenance: MeasurementProvenance::TESSELLATED,
                        format: &format,
                    },
                    |current, aside| {
                        set_aside(&relocator, &source, &self.blob_root, current, aside, &bytes)?;
                        moved = Some((current.to_owned(), aside.to_owned()));
                        Ok(())
                    },
                )
                .await;
            if let Ok(revision) = &recorded {
                self.record_centre_of_mass(
                    *revision,
                    centre_of_mass_mm,
                    Provenance::Tessellated,
                    source_path,
                )
                .await;
            }
            if let Err(error) = recorded {
                // Never `classify_write`: a unique violation here is a lost race for a label,
                // not "already here", and every failure on this path is worth another attempt
                // — the retry re-reads what is current and decides again.
                if let Some((current, aside)) = &moved {
                    put_back(&relocator, &source, current, aside);
                }
                return Err(HandlerError::Transient {
                    message: error.to_string(),
                });
            }

            // `metadata.json` from the rows, every revision listed. Warn-only, for step 10's
            // reason: the revision is committed, and failing the job would say it was not.
            if let Some((current, _)) = &moved
                && let Some((model_dir, _)) = current.rsplit_once('/')
            {
                let written = PgRevisions(self.db.clone())
                    .write_manifest(existing.part, |manifest| {
                        let manifest = manifest
                            .ok_or_else(|| "the part could not be found again".to_owned())?;
                        let json =
                            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
                        source
                            .put_at(
                                &format!("{model_dir}/metadata.json"),
                                &json,
                                Compression::AsIs,
                            )
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Err(error) = written {
                    tracing::warn!(
                        source_path,
                        error,
                        "could not rewrite metadata.json after a revision; it lists the revisions before this one until it is rewritten"
                    );
                }
            }
            if let Some(raw) = embedded {
                self.adopt_picture(existing.part, &raw, "embedded", None, source_path)
                    .await;
            }
            return Ok(Outcome::Revised);
        }

        // 6. Is there already a `blob` row for these bytes? Asked here, before anything is
        // written, and not beside the insert it decides: every query between the source
        // write and the transaction is a way to fail with the file already on disk and no
        // reap to run, which would leave the retry of this same file walking around a
        // directory it wrote itself. Widening the window changes nothing -- a row another
        // worker inserts in it makes `record`'s `ON CONFLICT (blake3) DO NOTHING` a no-op.
        let blob_row_exists = blobs.exists(&hash).await.map_err(classify_db)?;

        // 7. Where this model lives: its category rows, and the directory that holds it.
        // See `model_dir_for` for why this is here and not earlier.
        let (folder, mut model_dir) = self
            .model_dir_for(library, source_path, name, &hash)
            .await?;
        // The file keeps the name the user gave it, not the slugged part name: the whole
        // promise is that the directory holds the file they recognise. `part_name` is the
        // fallback for the pathological case of a candidate file with no file name at all.
        let file_name = FsPath::new(source_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(name);
        let mut storage_path = format!("{model_dir}/{file_name}");

        // 8. The bytes, written before the transaction exactly as they always were -- a
        // filesystem write cannot be rolled back by Postgres, so they must be on disk
        // before a row is allowed to point at them. What changed is only the name they are
        // written under, and that *both* branches below now need this write: a
        // path-addressed store holds one copy per model, so a second library ingesting
        // bytes it already holds gets its own file rather than a second reference to one.
        //
        // `Compression::AsIs`, not `for_source_format`: this file is the one the owner
        // opens in a file manager, and a zstd frame named `cliff.stl` is not that. The
        // recorded `zstd_level` of 0 is what every reader follows, so re-introducing
        // compression here is a one-argument change (`DATA.md` §1.3's opt-out, sub-project
        // 4) rather than a format nobody can read.
        //
        // `stored.hash` is recomputed from `bytes` inside `put_new_at` and is definitionally
        // the same as `hash` above; `hash` is used below so there is exactly one hash
        // variable in scope.
        //
        // A new part's file never replaces one already there (slice 1 spec §3.3): another job
        // may have written other bytes at this path a moment ago, and its row is about to point
        // at them. The same bytes already there are this file, left by an attempt that stopped
        // before its row or by a job racing it with the same file, so they are used as written
        // and, being possibly another's, never reaped by this attempt. `ours` says which.
        //
        // Other bytes that are a purged part's file, waiting out its 30 days in quarantine at a name
        // this model resolves to again, are never got past by retrying (ROADMAP, goal 2's record). So
        // this file takes a longer name beside them: 12 digits of its hash, then all of them. Other
        // bytes that are not quarantined may be another job's, whose row is about to point at them,
        // and this file is decided again as before.
        let mut longer = [12, 64].into_iter();
        let (stored, ours) = loop {
            match source.put_new_at(&storage_path, &bytes, Compression::AsIs) {
                Ok(stored) => break (stored, true),
                Err(lapidary_storage::StorageError::AlreadyExists { .. }) => {
                    let there = source.get_at(&storage_path, Some(0)).map_err(|e| {
                        HandlerError::Transient {
                            message: e.to_string(),
                        }
                    })?;
                    if BlobHash::from_bytes(*blake3::hash(&there).as_bytes()) == hash {
                        let stored = lapidary_storage::StoredBlob {
                            hash,
                            size_bytes: bytes.len() as u64,
                            stored_bytes: bytes.len() as u64,
                            zstd_level: 0,
                        };
                        break (stored, false);
                    }
                    // The rungs stay, as they do for a lost race at step 9: the job that wrote
                    // this file may be serving the same ones.
                    let purged = blobs
                        .waits_in_quarantine(&storage_path)
                        .await
                        .map_err(classify_db)?;
                    let longer_name = longer.next().filter(|_| purged);
                    let (Some(digits), Some((base, _))) = (longer_name, model_dir.rsplit_once('/'))
                    else {
                        return Err(HandlerError::Transient {
                            message: if !purged {
                                format!(
                                    "Another job wrote other bytes to {storage_path} while this file \
                                     was being read, and they were left as they are. This file is \
                                     decided again against the part that job records."
                                )
                            } else {
                                format!(
                                    "Purged parts' quarantined files are at {storage_path} and under \
                                     each longer name tried, and they were left as they are. This file \
                                     is tried again; move those files out of the storage folder if it \
                                     keeps failing."
                                )
                            },
                        });
                    };
                    model_dir = format!(
                        "{base}/{}",
                        disambiguate_with(&slugify(name), &hash, digits)
                    );
                    storage_path = format!("{model_dir}/{file_name}");
                }
                Err(e) => {
                    return Err(HandlerError::Transient {
                        message: e.to_string(),
                    });
                }
            }
        };
        let blob = StoredBlobRow {
            hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        };
        let request = IngestRequest {
            // The route these bytes came by: `ingest` for a scan, `upload` for the browser.
            origin,
            library,
            name,
            source_path,
            folder,
            storage_path: Some(&storage_path),
            blob: &blob,
            measurements: &measurements,
            // Tessellated, for the revision branch's reason: a mesh is all this library takes.
            provenance: MeasurementProvenance::TESSELLATED,
            format: &format,
        };

        // 9. One transaction, and nothing fallible between it and the write above. The
        // question read at step 6 decides one thing only, now that both branches write
        // their own file: whether the `blob` row for these bytes has to be inserted, or
        // already exists because another part shares the hash.
        //
        // It decides nothing about compression. `link_existing` leaves the existing `blob`
        // row alone — it has to, because that row still describes a legacy copy an
        // un-migrated sibling reads — and the level of *this* file rides on the `file` row
        // it inserts (migration `0013`). Before that column existed this branch left a
        // level-3 row over the raw file it had just written, and the download route
        // zstd-decoded an STL and 500ed.
        let written = if blob_row_exists {
            ingest.link_existing(request).await
        } else {
            ingest.record(request).await
        };
        let part = match written {
            Ok(part) => part,
            Err(db_err) => {
                // Reap only what a *failure* wrote. `classify_write` turns a unique
                // violation into `Skipped`, and that is not a failure: another worker won
                // the race for this file after a lease expiry, its part row is committed,
                // and the bytes at `storage_path` are the bytes that row describes.
                // Reaping them would delete a part that ingested perfectly well -- the
                // exact mistake the old branch-specific reap existed to avoid, which
                // survives here as a condition rather than as two code paths.
                let settled = classify_write(db_err);
                if settled.is_err() {
                    if ours
                        && self
                            .named_by_no_part(library, source_path, &storage_path)
                            .await
                    {
                        reap_source(&source, &storage_path, &model_dir);
                    }
                    return settled;
                }
                // A lost race for this path. The rungs stay either way, because the winner may
                // serve the same ones.
                //
                // The same bytes are the winner's part, and this file is skipped. A copy this
                // attempt wrote under another name is no part's, so it goes; one the winner adopted
                // is at the winner's path, so it stays.
                if blobs
                    .library_holds(library, source_path, &hash)
                    .await
                    .map_err(classify_db)?
                {
                    if ours
                        && self
                            .named_by_no_part(library, source_path, &storage_path)
                            .await
                    {
                        reap_source(&source, &storage_path, &model_dir);
                    }
                    return settled;
                }
                // Other bytes are decided again on the next attempt, which reads the winner's part
                // (spec §1: a revision, or unkept). This attempt's own file is no part's: a winner
                // adopts only bytes equal to its own.
                if ours {
                    reap_source(&source, &storage_path, &model_dir);
                }
                return Err(HandlerError::Transient {
                    message: format!(
                        "Another job filed other bytes at {source_path} first. This file is \
                         decided again against the part that job recorded."
                    ),
                });
            }
        };

        // An adopted file may be gone: the job that wrote it can fail and reap it between this
        // attempt's write and its commit. This part's row names it now, and these are its bytes, so
        // they go back.
        if !ours
            && !FsPath::new(&self.blob_root).join(&storage_path).exists()
            && let Err(error) = source.put_new_at(&storage_path, &bytes, Compression::AsIs)
            && !matches!(error, lapidary_storage::StorageError::AlreadyExists { .. })
        {
            tracing::error!(
                storage_path,
                %error,
                "a part's file was removed by a failed job racing it and could not be written back; upload the file again"
            );
        }

        // 10. `metadata.json`, beside the file it describes. This is the whole of
        // re-adoption: delete the database and each directory still says what it is.
        //
        // After the commit, from the rows it wrote, and warn-only for the same reason the
        // spec (§1) makes a missing manifest an orphan
        // the walk reports and skips rather than a failure: the part is already committed
        // and already in the grid, a retry would settle as `Skipped` and never reach this
        // line again, and reporting the job as failed would tell the user a file did not
        // ingest when it did.
        match PgParts(self.db.clone()).latest_revision(part).await {
            Ok(Some(revision)) => {
                self.record_centre_of_mass(
                    revision,
                    centre_of_mass_mm,
                    Provenance::Tessellated,
                    source_path,
                )
                .await;
                // From the rows, while the part's row is held, as a revision and a custom value write theirs:
                // a value committed meanwhile waits for this file rather than being written over by it.
                let written = PgRevisions(self.db.clone())
                    .write_manifest(part, |manifest| {
                        let manifest = manifest.ok_or_else(|| {
                            "the part this ingest just recorded could not be found again".to_owned()
                        })?;
                        let json =
                            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
                        source
                            .put_at(
                                &format!("{model_dir}/metadata.json"),
                                &json,
                                Compression::AsIs,
                            )
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Err(error) = written {
                    tracing::warn!(
                        %error,
                        model_dir,
                        "could not write metadata.json; this model directory will read as an orphan until it is rewritten"
                    );
                }
            }
            Ok(None) => tracing::warn!(
                %part,
                model_dir,
                "the revision this ingest just wrote could not be found again, so metadata.json was not written; this model directory will read as an orphan until it is rewritten"
            ),
            Err(error) => tracing::warn!(
                %part,
                model_dir,
                %error,
                "could not read back the revision just written, so metadata.json was not written; this model directory will read as an orphan until it is rewritten"
            ),
        }

        if let Some(raw) = embedded {
            self.adopt_picture(part, &raw, "embedded", None, source_path)
                .await;
        }
        Ok(Outcome::Ingested)
    }

    /// A picture the application found — a 3MF's own thumbnail (`embedded`, F1) or a model page's
    /// (`url_supplied`, F3) — as the part's picture when it has none. Through the same
    /// `normalize` every upload takes — decoded under limits, bounded, re-encoded to WebP — and
    /// stored the way the gallery stores one: on its row under the inline limit, in the store above.
    ///
    /// **A person's picture always wins**: `adopt_part_image` adds nothing to a part that has one,
    /// so a revised 3MF never replaces what somebody chose. Warn-only throughout, for the reason
    /// `record_centre_of_mass` is: the part is committed, and a picture it lacks is not a failure.
    pub(crate) async fn adopt_picture(
        &self,
        part: lapidary_core::PartId,
        raw: &[u8],
        origin: &str,
        source_url: Option<&str>,
        source_path: &str,
    ) {
        let image = match lapidary_storage::images::normalize(raw) {
            Ok(image) => image,
            Err(error) => {
                tracing::warn!(source_path, origin, %error, "a found picture is not one Lapidary keeps; the part has none from it");
                return;
            }
        };
        let blob = if image.webp.len() < lapidary_storage::images::INLINE_LIMIT {
            None
        } else {
            match lapidary_storage::DerivativeStore::open(&self.blob_root).put(&image.webp) {
                Ok(stored) => {
                    let row = StoredBlobRow {
                        hash: stored.hash,
                        size_bytes: stored.size_bytes,
                        stored_bytes: stored.stored_bytes,
                        zstd_level: stored.zstd_level,
                    };
                    if let Err(error) = PgBlobs(self.db.clone()).record_unreferenced(&row).await {
                        tracing::warn!(source_path, %error, "could not record a found picture; the part has no picture from it");
                        return;
                    }
                    Some(row.hash)
                }
                Err(error) => {
                    tracing::warn!(source_path, %error, "could not store a found picture; the part has no picture from it");
                    return;
                }
            }
        };
        let adopted = PgParts(self.db.clone())
            .adopt_part_image(
                part,
                lapidary_db::NewPartImage {
                    bytes: match &blob {
                        Some(hash) => lapidary_db::ImageBytes::Blob(hash),
                        None => lapidary_db::ImageBytes::Inline(&image.webp),
                    },
                    origin,
                    source_url,
                },
            )
            .await;
        if let Err(error) = adopted {
            tracing::warn!(source_path, %error, "could not attach a found picture; the part has no picture from it");
        }
    }

    /// The category rows and the directory one model lands in: its leaf `folder_id`, and
    /// the store-relative path of the directory itself.
    ///
    /// Both of the ordering rules that constrain it are about *when* it is called, so they
    /// are stated where the constraint lives rather than at the call site alone:
    ///
    /// - **After the `library_holds` short-circuit**, never during the walk. A re-scan of a
    ///   directory whose models have all been moved away must not silently re-create the
    ///   now-empty originals, on disk or in the tree (spec §6).
    /// - **After the kernel**, so a file that will never parse creates no folder rows and
    ///   no directory. "Categories are created only for files that actually ingest" is the
    ///   same sentence read one step further.
    ///
    /// One method with two callers rather than one rule written twice: the
    /// `migrate_storage` job resolves the directory for a part that ingested before this
    /// layout existed, and a second implementation of "where does this model go" would
    /// drift from this one the first time either changed.
    ///
    /// The collision rule is the spec's (§2): slugify the part name, and if that directory
    /// already exists in the target category, append `_` and six hex characters of the
    /// source hash. Deterministic, so re-ingesting the same bytes lands on the same name.
    /// The existence check is a plain `exists()` on a path built entirely from slugs --
    /// `slugify` removes every separator, so nothing here can name a directory outside the
    /// store, and `put_at` re-checks that regardless.
    pub(crate) async fn model_dir_for(
        &self,
        library: LibraryId,
        source_path: &str,
        name: &str,
        hash: &BlobHash,
    ) -> Result<(Option<FolderId>, String), HandlerError> {
        let library_slug = PgParts(self.db.clone())
            .library_slug(library)
            .await
            .map_err(classify_db)?
            .ok_or_else(|| HandlerError::Permanent {
                message: format!(
                    "There is no library {library} to store {source_path} in. The library \
                     may have been removed after this job was queued; re-scan the library \
                     you meant."
                ),
            })?;

        // The tree mirrors the ingest directory's own nesting. `get_or_create` is
        // insert-then-select against a unique constraint, so two workers racing the same
        // directory is safe, and `mkdir -p` races harmlessly for the filesystem half.
        let folders = PgFolders(self.db.clone());
        let mut parent: Option<FolderId> = None;
        let segments: Vec<&str> = FsPath::new(source_path)
            .parent()
            .and_then(|dir| dir.to_str())
            .filter(|dir| !dir.is_empty())
            .map(|dir| dir.split('/').collect())
            .unwrap_or_default();
        for segment in segments {
            parent = Some(
                folders
                    .get_or_create(library, parent, segment, &slugify(segment))
                    .await
                    .map_err(classify_db)?,
            );
        }

        // Read back rather than joined from the segments above: `slug_path` is what every
        // later reader of this tree uses, and a folder that already existed carries the
        // slug it was created with, which a re-slug of today's segment need not match.
        let category = match parent {
            Some(folder) => folders.slug_path(folder).await.map_err(classify_db)?,
            None => String::new(),
        };
        let base = format!("libraries/{library_slug}/{category}");
        let base = base.trim_end_matches('/');

        let mut model_dir = slugify(name);
        if FsPath::new(&self.blob_root)
            .join(base)
            .join(&model_dir)
            .exists()
        {
            model_dir = disambiguate(&model_dir, hash);
        }
        Ok((parent, format!("{base}/{model_dir}")))
    }

    /// Whether no live part at `source_path` names the file at `storage_path`. A job racing this one
    /// with the same bytes adopts a file already there instead of writing its own (step 8), and its
    /// committed row may then name the file this attempt wrote.
    ///
    /// A query that fails answers no, so the file stays: an orphan is recoverable, and deleting the
    /// bytes a committed row names is not.
    ///
    /// ponytail: a check, then a reap, so a racer that commits between the two still loses the file.
    /// The adopter puts it back after its commit (step 9), which narrows the window from the other
    /// side; a lock held across both jobs' inserts and reaps would close it.
    async fn named_by_no_part(
        &self,
        library: LibraryId,
        source_path: &str,
        storage_path: &str,
    ) -> bool {
        match PgRevisions(self.db.clone())
            .current(library, source_path)
            .await
        {
            Ok(current) => {
                current.and_then(|current| current.storage_path).as_deref() != Some(storage_path)
            }
            Err(error) => {
                tracing::warn!(
                    source_path,
                    storage_path,
                    %error,
                    "could not check whether another job's part names this file, so it is left in place"
                );
                false
            }
        }
    }
}

/// The filesystem half of a revision (spec §3.1), run inside its transaction: the current
/// file set aside at `aside`, then the new bytes written at `current`. A failed write puts the
/// previous file back, so a refusal leaves the model directory holding what it held.
fn set_aside(
    relocator: &SourceRelocator,
    source: &SourceStore,
    blob_root: &FsPath,
    current: &str,
    aside: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let (aside_dir, _) = aside
        .rsplit_once('/')
        .ok_or_else(|| format!("`{aside}` names no directory"))?;
    // `std::fs::rename` replaces an existing file without a word, and a file already there is
    // bytes this revision would destroy — whoever put them there.
    if blob_root.join(aside).exists() {
        return Err(format!(
            "`{aside}` already holds a file, and setting the current file aside there would \
             replace it. Move that file out of the storage folder, then retry."
        ));
    }
    relocator
        .create_dir(aside_dir)
        .map_err(|error| error.to_string())?;
    relocator
        .rename(current, aside)
        .map_err(|error| error.to_string())?;
    if let Err(error) = source.put_at(current, bytes, Compression::AsIs) {
        put_back(relocator, source, current, aside);
        return Err(error.to_string());
    }
    Ok(())
}

/// Undo [`set_aside`]: the new file removed, the previous one renamed back.
///
/// `error`, not `warn`, and both paths named: if this fails, the disk is ahead of the
/// database — the previous revision's row names `current`, and its bytes are at `aside`.
/// Nothing is deleted either way; this is the line an operator needs to put them back.
fn put_back(relocator: &SourceRelocator, source: &SourceStore, current: &str, aside: &str) {
    if let Err(error) = source.remove_at(current) {
        tracing::error!(
            current,
            aside,
            %error,
            "a revision failed and its new file could not be removed; the previous revision's bytes are at `aside`, and its row names `current`"
        );
        return;
    }
    if let Err(error) = relocator.rename(aside, current) {
        tracing::error!(
            current,
            aside,
            %error,
            "a revision failed and the previous file could not be put back; its bytes are at `aside`, and its row names `current`"
        );
    }
}

/// Remove the source file, and the directory that held it, after a transaction that
/// failed.
///
/// The empty directory is not tidiness. `model_dir_for` decides a model's name by asking
/// whether the directory is already taken, so one left behind by a failed write makes the
/// *retry* of that same file think its name is taken and rename the model — a transient
/// database error would permanently change where a part lives. `remove_dir_if_empty`
/// refuses a directory that still holds something, which is the case where another writer
/// got there first and the directory is not ours to remove.
///
/// Best-effort and warn-only, exactly as the derivative reap below is: the database error
/// is what the caller needs to hear about either way. A failed reap is still worth a line,
/// because it is the one place in the pipeline that knowingly leaves bytes behind.
pub(crate) fn reap_source(source: &SourceStore, storage_path: &str, model_dir: &str) {
    if let Err(reap_err) = source.remove_at(storage_path) {
        tracing::warn!(
            storage_path,
            error = %reap_err,
            "failed to reap a source file after a failed ingest write; it may now be an orphan on disk"
        );
    }
    if let Err(reap_err) = source.remove_dir_if_empty(model_dir) {
        tracing::warn!(
            model_dir,
            error = %reap_err,
            "left a model directory behind after a failed ingest write; a retry of this file will store it under a disambiguated name"
        );
    }
}

/// Remove derivative bytes this job wrote for a transaction that then failed.
///
/// Best-effort and warn-only, exactly as the source blob's reap is: the database error is
/// what the caller needs to hear about either way. A failed reap is still worth a line,
/// because it is the one place in the pipeline that knowingly leaves bytes behind.
pub(crate) fn reap(derivatives: &DerivativeStore, hashes: &[BlobHash]) {
    for hash in hashes {
        if let Err(reap_err) = derivatives.remove(hash) {
            tracing::warn!(
                hash = %hash.to_hex(),
                error = %reap_err,
                "failed to reap a derivative blob after a failed ingest write; it may now be an orphan on disk"
            );
        }
    }
}

/// The part name shown in the grid. Slice 1 has no part-numbering convention to draw on,
/// so the file's stem (its name without the extension) is the whole story; falls back to
/// the full file name on the pathological case where a candidate file (already proven by
/// `is_model_candidate` to have one of the mesh extensions) somehow has no stem.
pub(crate) fn part_name(file_name: &str) -> &str {
    FsPath::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name)
}

/// A database error, sorted into "try again" and "never". Every `DbError` a handler can see
/// now is a connection or a query that may work on the next attempt, so every one is
/// `Transient`.
///
/// **It had two exceptions and R3 took both with their errors.** `ThumbnailNotInline` and
/// `EmptyDerivative` were `PgIngest::upsert_derivative` refusing a derivative shape before it
/// wrote anything, and both described the inline `thumb_bytes` path — which no longer has a
/// producer. The function stays, rather than collapsing into `HandlerError::Transient { .. }`
/// at every call site, because this is where that judgement belongs: the next `DbError` worth
/// refusing permanently gets one arm here instead of a decision at each of nineteen callers.
pub(crate) fn classify_db(error: DbError) -> HandlerError {
    HandlerError::Transient {
        message: error.to_string(),
    }
}

/// Every `CadError` is `Permanent`, sorted the way `classify_db` sorts a database one.
///
/// **It had three exceptions and R2 took all three with the sidecar.** A crash, a timeout and a
/// kernel that would not start were the worker's trouble rather than the file's, so they were
/// retried; nothing out-of-process reads a file now, so what is left is four parser and writer
/// verdicts on immutable bytes — three attempts would reach the same one three times. The
/// function stays, rather than collapsing into `HandlerError::Permanent { .. }` at every call
/// site, because this is where that judgement belongs: a `CadError` worth retrying would get
/// one arm here instead of a decision at each caller.
pub(crate) fn classify_cad(error: CadError) -> HandlerError {
    HandlerError::Permanent {
        message: error.to_string(),
    }
}

/// The refusal a STEP or IGES file gets, for a file a scan found.
///
/// `Permanent`, and that is the point: the bytes are immutable and there is no parser to build
/// in, so a retry would reach exactly the same answer. The sentence names the format and what
/// this library does take; it lives in `lapidary-core` because the upload route refuses the
/// same files before a byte moves and must say the same thing.
pub(crate) fn refuse_b_rep(format: &str) -> Result<(), HandlerError> {
    if CAD_FORMATS.contains(&format) {
        return Err(HandlerError::Permanent {
            message: lapidary_core::cad_refusal(format),
        });
    }
    Ok(())
}

/// A unique violation on `part_source_path_unique_per_library` is not a failure: another
/// worker won the race for this file after a lease expiry, and the part exists. Mapping it
/// to `Skipped` is what makes at-least-once delivery effectively-once -- see the design
/// doc, section 3.5.
///
/// Was `part_name_unique_per_library` until slice 6a, and the rename is the whole point
/// rather than a tidy-up: with the scan descending, a name collides whenever two folders
/// hold the same filename, and mapping *that* to `Skipped` would report a file as already
/// here and never index it. Only a genuine re-scan of the same path may be skipped.
fn classify_write(error: DbError) -> Result<Outcome, HandlerError> {
    if let DbError::Query(sqlx::Error::Database(db)) = &error
        && db.constraint() == Some("part_source_path_unique_per_library")
    {
        return Ok(Outcome::Skipped);
    }
    Err(HandlerError::Transient {
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_name_strips_the_stl_extension() {
        assert_eq!(part_name("bracket-lp-1042-03.stl"), "bracket-lp-1042-03");
    }

    /// **Every one of the four, not a representative one.** A file this build cannot read is
    /// the file's own verdict on immutable bytes: if any of these ever reads `Transient` again,
    /// the queue retries a parse that is refused deterministically -- three attempts, three
    /// identical refusals, and a failure recorded four backoffs after it was already known.
    /// The three that *were* retried (a crash, a timeout, a bridge that would not start) went
    /// with the sidecar they described.
    #[test]
    fn every_parser_verdict_is_permanent_rather_than_retried() {
        let verdicts = [
            CadError::MalformedMesh {
                format: "stl".to_owned(),
                detail: "the header declares 48 triangles and the file holds 3".to_owned(),
            },
            CadError::ArchiveRefused {
                format: "3mf".to_owned(),
                detail: "an entry expands past the ratio cap".to_owned(),
            },
            CadError::UnsupportedFormat {
                format: "step".to_owned(),
            },
            CadError::ExportFailed {
                format: "stl".to_owned(),
                detail: "more triangles than a binary STL's count can name".to_owned(),
            },
        ];
        for error in verdicts {
            let text = error.to_string();
            match classify_cad(error) {
                HandlerError::Permanent { message } => assert_eq!(message, text),
                other => panic!("a parser verdict must not be retried, got {other:?}"),
            }
        }
    }

    /// Every `DbError` is a retry now. The two that were not — `ThumbnailNotInline` and
    /// `EmptyDerivative`, both about the inline thumbnail shape — went with the renders, and the
    /// case that pinned them went with them rather than being bent to a different error.
    #[test]
    fn a_query_failure_is_still_transient() {
        let err = DbError::Query(sqlx::Error::PoolClosed);
        assert!(matches!(classify_db(err), HandlerError::Transient { .. }));
    }

    /// A `DatabaseError` carrying one constraint name and nothing else.
    ///
    /// `sqlx` will not let a `PgDatabaseError` be built outside its own crate, and the
    /// test below has to ask `classify_write` about a *named constraint* rather than about
    /// a live database. Everything under `constraint` is the trait's required surface,
    /// untouched by the code under test.
    #[derive(Debug)]
    struct ViolatedConstraint(&'static str);

    impl std::fmt::Display for ViolatedConstraint {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "duplicate key value violates unique constraint \"{}\"",
                self.0
            )
        }
    }

    impl std::error::Error for ViolatedConstraint {}

    impl sqlx::error::DatabaseError for ViolatedConstraint {
        fn message(&self) -> &str {
            "duplicate key value violates a unique constraint"
        }
        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::UniqueViolation
        }
        fn constraint(&self) -> Option<&str> {
            Some(self.0)
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
    }

    fn violation(constraint: &'static str) -> DbError {
        DbError::Query(sqlx::Error::Database(Box::new(ViolatedConstraint(
            constraint,
        ))))
    }

    /// The condition the reap is guarded by, pinned where it can be pinned deterministically.
    ///
    /// `ingest_one` reaps its source file, its model directory and its rungs only when
    /// `classify_write(...).is_err()`, because the one error it turns into `Ok(Skipped)` is
    /// a race this worker lost — a race whose winner's committed row describes the very
    /// bytes a reap would delete. The end-to-end version of that is
    /// `losing_the_race_for_a_file_is_a_skip_rather_than_a_failure`, and it depends on two
    /// concurrent handlers actually overlapping: measured over 20 runs against a restored
    /// bug it caught it twice. This asserts the same invariant as a fact about one
    /// function, so a change to the mapping, or a flip of the `is_err()`, fails on every
    /// run of the suite with no database and no scheduler involved.
    #[test]
    fn only_a_lost_race_for_the_same_path_is_a_skip_rather_than_an_error() {
        let lost_race = classify_write(violation("part_source_path_unique_per_library"));
        assert_eq!(
            lost_race.as_ref().ok(),
            Some(&Outcome::Skipped),
            "another worker won the race for this path; its row is committed. This is also \
             the one outcome whose rungs must not be reaped -- `index`'s failure arm reaps \
             behind exactly this value's `is_err()`, and decides the source file by its bytes"
        );

        // Every other refusal is a failure, and a failure reaps what it wrote. A unique
        // violation on some *other* constraint is the case worth naming: it is the same
        // shape of error, and reporting it as `Skipped` would tell the user a file is
        // already indexed when nothing indexed it.
        for other in [
            violation("folder_name_unique_per_parent"),
            violation("derivative_kind_unique_per_revision"),
            DbError::Query(sqlx::Error::PoolClosed),
        ] {
            let settled = classify_write(other);
            assert!(
                settled.is_err(),
                "only a lost race for this path may settle as an outcome, got {settled:?}"
            );
        }
    }
}
