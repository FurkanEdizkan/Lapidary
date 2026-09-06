//! One file's worth of ingest, as a job. The pipeline below is slice 1's, moved rather
//! than rewritten: read, BLAKE3, library_holds, kernel, link-or-put.
//!
//! # Ordering
//!
//! Per file, in this order — the order is the design:
//!
//! 1. read bytes
//! 2. BLAKE3 — hash first, always
//! 3. `blobs.library_holds(library, source_path, hash)`? yes -> `Skipped`, no further work at
//!    all: not a parse, not a raster, not a query beyond this one
//!    3a. `parts.auto_thumbnail(library)` — what this library wants produced;
//!    deliberately below the short-circuit, so a re-scan still costs one query
//! 4. `kernel.process(bytes, params)` — parse + measure + rasterize + cluster
//! 5. does any library already hold these bytes (`blobs.exists(hash)`)?
//!    - yes -> `ingest.link_existing(...)`: the blob stays exactly where it is, and this
//!      library gets its own part pointing at it. No write, so nothing to reap.
//!    - no  -> `source.put(bytes, compression)` writes the blob *before* the transaction,
//!      then `ingest.record(...)`; on error, `source.remove(hash)` reaps the blob just
//!      written and the failure is returned
//!
//! Step 5's reap is not optional. The Node prototype wrote its blob and then failed the
//! insert with no cleanup, leaving bytes on disk that nothing referenced and nothing
//! would ever collect — `docs/prototype-notes.md` records it. It is equally not optional
//! that the reap runs only on the branch that *wrote* something: reaping a blob another
//! library's part references would be silent data loss, which is why the two branches
//! are separate rather than one call with a flag.
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
//! The kernel's errors are `Permanent`: blobs are content-addressed and immutable, so
//! re-parsing the same bytes is guaranteed to produce the same error, and a retry only
//! delays an answer that is already available. Database, blob-store and I/O errors are
//! `Transient` — the file is fine, something else was not, and worth another attempt.
//! When genuinely unsure, this module chooses `Transient`: a retried permanent failure
//! costs one wasted parse, while a non-retried transient failure costs the user a file.

use lapidary_cad::{Kernel, KernelParams, MeshKernel};
use lapidary_core::{BlobHash, DerivativeKind, JobPayload, LibraryId, Outcome, source_format};
use lapidary_db::{
    DbError, IngestRequest, JobRow, PgBlobs, PgIngest, PgParts, PgPool, StoredBlobRow,
    TessellationRow,
};
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_storage::{Compression, DerivativeStore, SourceReader, SourceStore, WorkerRole};
use std::path::{Path as FsPath, PathBuf};

pub struct WorkerHandler {
    pub db: PgPool,
    /// The read-only mounted ingest directory ingest_one reads its file from. Never a
    /// hardcoded container path: tests point it at a `TempDir`, and `deploy/compose.yaml`
    /// (Task 12) supplies the real `/ingest` mount.
    pub ingest_dir: PathBuf,
    /// Root of the blob store. This crate is the one place in the workspace allowed to
    /// construct a `SourceStore` over it — see `lib.rs`'s module doc.
    pub blob_root: PathBuf,
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
        }
    }
}

impl WorkerHandler {
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
        // belongs on every path that reaches a filesystem from a payload.
        reject_escaping_path(source_path)?;
        let path = self.ingest_dir.join(source_path);

        // 1. Read bytes. A missing file may mean the mount is not ready yet, so this is
        // Transient rather than Permanent -- unlike every step below it, this one has
        // nothing to do with the bytes themselves.
        let bytes = std::fs::read(&path).map_err(|e| HandlerError::Transient {
            message: format!("Could not read {source_path}: {e}"),
        })?;

        // 2. BLAKE3 -- hash first, always. Everything below branches on this.
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        self.index(library, source_path, bytes, hash).await
    }

    /// One blob already in the store, start to finish: the upload route's half of the
    /// pipeline.
    ///
    /// The api wrote these bytes and verified them against the hash the client claimed
    /// (`SourceWriter::put_file`), and inserted the `blob` row that keeps them from being
    /// an invisible orphan while this job waits. So steps 1 and 2 are a read back out of
    /// the store rather than off the mount, and nothing here re-decides the hash: it is
    /// the row's primary key and the name of the file the bytes were read from.
    ///
    /// `SourceReader` rather than `SourceStore`, though this crate may construct either:
    /// this arm never writes a source blob and never reaps one — the bytes were not its
    /// to write — and the read-only handle is the one that says so. Its `zstd_level`
    /// comes from the `blob` row for the reason its own doc gives, which matters more
    /// here than anywhere: the api chose that level, in another process, possibly on
    /// another build.
    ///
    /// See the slice 6a design, §4.1 and §4.2.
    pub(crate) async fn ingest_blob(
        &self,
        library: LibraryId,
        hash: BlobHash,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        // Not a filesystem join here — the path never touches one — but it becomes
        // `part.source_path`, and from there a `Content-Disposition` filename on the
        // download route. Same refusal, different surface. See `path_escapes`.
        reject_escaping_path(source_path)?;

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

        let bytes = SourceReader::open(&self.blob_root)
            .get(&hash, Some(stored.zstd_level))
            .map_err(|e| HandlerError::Transient {
                message: format!("Could not read the uploaded bytes for {source_path}: {e}"),
            })?;

        self.index(library, source_path, bytes, hash).await
    }

    /// Steps 3 through 6: short-circuit, kernel, rungs, and the one transaction.
    ///
    /// Both byte sources land here with the same two facts — these bytes, and their hash
    /// — and everything below depends on nothing else about where they came from. Step 5
    /// still asks `blobs.exists(&hash)` rather than being told: for an upload the answer
    /// is always yes, because the api inserted the row, and that is exactly the branch
    /// that links without writing and reaps no source blob. The right behaviour arrived
    /// at by the existing question rather than by a new flag.
    async fn index(
        &self,
        library: LibraryId,
        source_path: &str,
        bytes: Vec<u8>,
        hash: BlobHash,
    ) -> Result<Outcome, HandlerError> {
        let kernel = MeshKernel;
        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        // First production use. No `WorkerRole` proof: derivatives are readable by both
        // roles, which is what lets `lapidary-api` serve a rung without ever being able
        // to name `SourceStore`.
        let derivatives = DerivativeStore::open(&self.blob_root);
        let blobs = PgBlobs(self.db.clone());
        let ingest = PgIngest(self.db.clone());
        let name = part_name(source_path);

        // 3. The same file, seen again -- same library, same name, same bytes --
        // short-circuits parse, raster and every write entirely. Scoped to the library on
        // purpose: a hash this library has never seen is a part it does not have,
        // whatever some other library holds. See `scan.rs`'s module doc.
        if blobs
            .library_holds(library, source_path, &hash)
            .await
            .map_err(classify_db)?
        {
            return Ok(Outcome::Skipped);
        }

        // 3a. What this library wants made. Read *after* the short-circuit, so a re-scan
        // still costs exactly one query -- L1 and L2 are no longer ingest's to produce
        // (design section 3.1), and the thumbnail is the library's choice (section 3.2).
        // A library that does not exist is Permanent: the row will not reappear, and
        // three retries reporting a bare row-count error tell an operator nothing.
        let auto_thumbnail = PgParts(self.db.clone())
            .auto_thumbnail(library)
            .await
            .map_err(classify_db)?
            .ok_or_else(|| HandlerError::Permanent {
                message: format!(
                    "There is no library {library} to ingest {source_path} into. The library \
                     may have been removed after this job was queued; re-scan the library \
                     you meant."
                ),
            })?;
        let mut produce = Vec::with_capacity(2);
        if auto_thumbnail {
            produce.push(DerivativeKind::Thumbnail);
        }
        produce.push(DerivativeKind::TessellationL0);
        let params = KernelParams {
            linear_deflection_mm: None,
            format: source_format(source_path),
            produce,
        };
        let version = kernel.version(&params);
        let kernel_version = format!("{} {}", version.implementation, version.version);

        // 4. Parse + measure + rasterize. Nothing has been written yet, so a failure here
        // needs no cleanup. This runs even when the bytes are already in the blob store,
        // because the new part needs its own measurements and its own thumbnail; only the
        // bytes are shared, and they are already in memory from step 1. The bytes are
        // immutable, so this error is the final answer about them.
        let output =
            kernel
                .process(&bytes, &params)
                .await
                .map_err(|e| HandlerError::Permanent {
                    message: e.to_string(),
                })?;

        // 5. The rungs go to disk before either branch's transaction, for the same reason
        // the source blob does: a filesystem write cannot be rolled back by Postgres, so
        // the bytes must be there before a row is allowed to point at them.
        let mut rungs = Vec::with_capacity(output.tessellations.len());
        let mut reapable = Vec::new();
        for rung in &output.tessellations {
            let stored = derivatives
                .put(&rung.glb)
                .map_err(|e| HandlerError::Transient {
                    message: e.to_string(),
                })?;
            // Only bytes this job introduced may be reaped if the transaction fails. A
            // rung whose bytes some revision already stores is bytes that revision is
            // still serving -- the same rule the source blob follows above, asked of the
            // same authority. `put` is content-addressed, so writing them again was a
            // no-op rather than a second copy.
            if !blobs.exists(&stored.hash).await.map_err(classify_db)? {
                reapable.push(stored.hash);
            }
            rungs.push(TessellationRow {
                kind: rung.lod.as_kind(),
                blob: StoredBlobRow {
                    hash: stored.hash,
                    size_bytes: stored.size_bytes,
                    stored_bytes: stored.stored_bytes,
                    zstd_level: stored.zstd_level,
                },
                grid: rung.grid,
            });
        }

        // 5a. Some library already holds these bytes. Reuse them exactly as they are: no
        // second copy on disk, no second `blob` row, and -- the part that matters -- no
        // reap on failure, because those bytes are referenced by a part this job did not
        // create.
        if blobs.exists(&hash).await.map_err(classify_db)? {
            let blob = StoredBlobRow {
                hash,
                // `link_existing` reads only `hash` and `size_bytes` (the `file` row);
                // the `blob` row, and with it the stored size and compression level,
                // already exists and is not rewritten.
                size_bytes: bytes.len() as u64,
                stored_bytes: bytes.len() as u64,
                zstd_level: 0,
            };
            return match ingest
                .link_existing(IngestRequest {
                    library,
                    name,
                    source_path,
                    blob: &blob,
                    measurements: &output.measurements,
                    thumbnail_webp: output.thumbnail_webp.as_deref(),
                    kernel_version: &kernel_version,
                    format: &params.format,
                    tessellations: &rungs,
                })
                .await
            {
                Ok(_) => Ok(Outcome::Ingested),
                Err(db_err) => {
                    // This branch wrote no source blob and must not reap one. It did
                    // write rungs, so it reaps exactly those.
                    reap(&derivatives, &reapable);
                    classify_write(db_err)
                }
            };
        }

        // 5b. New bytes. The blob is written before the transaction. `stored.hash` is
        // recomputed from `bytes` inside `put` and is definitionally the same as `hash`
        // above; `hash` is used below rather than `stored.hash` so there is exactly one
        // hash variable in scope.
        let stored = source
            .put(&bytes, Compression::for_source_format(&params.format))
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })?;
        let blob = StoredBlobRow {
            hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        };

        // 6. One transaction. On failure, reap the blob `put` just wrote -- nothing
        // references it, and nothing else ever will, so it must not be left on disk. The
        // Node prototype's exact miss (docs/prototype-notes.md): a successful blob write
        // followed by a failed DB insert, with no cleanup.
        match ingest
            .record(IngestRequest {
                library,
                name,
                source_path,
                blob: &blob,
                measurements: &output.measurements,
                thumbnail_webp: output.thumbnail_webp.as_deref(),
                kernel_version: &kernel_version,
                format: &params.format,
                tessellations: &rungs,
            })
            .await
        {
            Ok(_) => Ok(Outcome::Ingested),
            Err(db_err) => {
                // Best-effort: the DB error is the one worth reporting to the caller
                // either way, and a failed reap does not change what they need to know
                // about this file. But a failed reap is not nothing -- it leaves bytes
                // behind that nothing will ever reference and nothing will ever collect,
                // which is exactly the leak this whole reap exists to close.
                // `SourceStore::remove` already treats a missing file as success, so this
                // only fires on a real I/O problem with the store itself -- the one place
                // in the pipeline that knowingly leaves bytes behind, so it is the one
                // place that says so.
                if let Err(reap_err) = source.remove(&hash) {
                    tracing::warn!(
                        hash = %hash.to_hex(),
                        error = %reap_err,
                        "failed to reap a blob after a failed ingest write; it may now be an orphan on disk"
                    );
                }
                reap(&derivatives, &reapable);
                classify_write(db_err)
            }
        }
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

/// Refuse a relative path that would leave `ingest_dir`.
///
/// Absolute paths and `..` segments both escape a `Path::join`, which resolves nothing and
/// refuses nothing: `ingest_dir.join("/etc/passwd")` *is* `/etc/passwd`, and
/// `ingest_dir.join("../../etc/passwd")` walks out of the mount. A Windows-style prefix
/// (`C:\`, `\\server\share`) is caught by the same `is_absolute` check on Windows and is
/// harmless as a literal filename elsewhere.
///
/// `Permanent`, because a payload holds the same bytes on every attempt: three retries of
/// a traversal produce three identical refusals and delay an answer already available.
///
/// Empty is refused too. It joins to `ingest_dir` itself, which reads as a directory and
/// would fail later with a confusing I/O error instead of the real reason.
fn reject_escaping_path(source_path: &str) -> Result<(), HandlerError> {
    if lapidary_core::path_escapes(source_path) {
        return Err(HandlerError::Permanent {
            message: format!(
                "Refused the file path {source_path:?}: it points outside the ingest \
                 directory. Paths are relative to the ingest mount and may not be \
                 absolute or contain `..`."
            ),
        });
    }
    Ok(())
}

/// The part name shown in the grid. Slice 1 has no part-numbering convention to draw on,
/// so the file's stem (its name without the extension) is the whole story; falls back to
/// the full file name on the pathological case where a candidate file (already proven by
/// `is_mesh_candidate` to have one of the mesh extensions) somehow has no stem.
pub(crate) fn part_name(file_name: &str) -> &str {
    FsPath::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name)
}

/// A database error, sorted into "try again" and "never". Almost every `DbError` a
/// handler can see is a connection or a query that may work on the next attempt, so
/// `Transient` is the default -- but two of them are refusals, not failures.
///
/// `ThumbnailNotInline` and `EmptyDerivative` are `PgIngest::upsert_derivative` rejecting
/// a derivative shape before it writes anything. Nothing about the next attempt is
/// different: the same handler will offer the same bytes in the same shape and be refused
/// again, so `Transient` would have the queue retry a job that cannot succeed three times
/// and only then record a failure whose text has been true since the first try. That is
/// the bug ruling T7-B named in the old unknown-kind path, and it is `Permanent` here for
/// the same reason `classify_write` below is not a blanket mapping either.
pub(crate) fn classify_db(error: DbError) -> HandlerError {
    let message = error.to_string();
    match error {
        DbError::ThumbnailNotInline { .. } | DbError::EmptyDerivative { .. } => {
            HandlerError::Permanent { message }
        }
        _ => HandlerError::Transient { message },
    }
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

    /// A refused derivative shape is not a database that might be back in a moment. If
    /// this ever reads `Transient` again, the queue will retry a write that is refused
    /// deterministically -- three attempts, three identical refusals, and a failure
    /// recorded four backoffs after it was already known.
    #[test]
    fn a_refused_derivative_shape_is_permanent_not_a_retry() {
        let revision = lapidary_core::RevisionId::new();
        let refusals = [
            DbError::ThumbnailNotInline { revision },
            DbError::EmptyDerivative {
                kind: "thumbnail",
                revision,
            },
        ];
        for refusal in refusals {
            let text = refusal.to_string();
            match classify_db(refusal) {
                // The operator's remedy has to survive the classification: it is the only
                // place the reason for the refusal is written down.
                HandlerError::Permanent { message } => assert_eq!(message, text),
                other => panic!("a guard refusal must not be retried, got {other:?}"),
            }
        }
    }

    /// The other direction, so the match above cannot quietly become a blanket
    /// `Permanent` and strand a job the next attempt would have run.
    #[test]
    fn a_query_failure_is_still_transient() {
        let err = DbError::Query(sqlx::Error::PoolClosed);
        assert!(matches!(classify_db(err), HandlerError::Transient { .. }));
    }
}
