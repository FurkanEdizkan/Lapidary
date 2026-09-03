//! One file's worth of ingest, as a job. The pipeline below is slice 1's, moved rather
//! than rewritten: read, BLAKE3, library_holds, kernel, link-or-put. That ordering is the
//! design and its reasoning lives in `scan.rs`'s module doc.
//!
//! # Error classification
//!
//! The kernel's errors are `Permanent`: blobs are content-addressed and immutable, so
//! re-parsing the same bytes is guaranteed to produce the same error, and a retry only
//! delays an answer that is already available. Database, blob-store and I/O errors are
//! `Transient` — the file is fine, something else was not, and worth another attempt.
//! When genuinely unsure, this module chooses `Transient`: a retried permanent failure
//! costs one wasted parse, while a non-retried transient failure costs the user a file.

use lapidary_cad::MeshKernel;
use lapidary_core::{BlobHash, LibraryId, Outcome};
use lapidary_db::{DbError, IngestRequest, JobRow, PgBlobs, PgIngest, PgPool, StoredBlobRow};
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_storage::{SourceStore, WorkerRole};
use std::path::{Path as FsPath, PathBuf};

pub struct IngestHandler {
    pub db: PgPool,
    /// The read-only mounted ingest directory ingest_one reads its file from. Never a
    /// hardcoded container path: tests point it at a `TempDir`, and `deploy/compose.yaml`
    /// (Task 12) supplies the real `/ingest` mount.
    pub ingest_dir: PathBuf,
    /// Root of the blob store. This crate is the one place in the workspace allowed to
    /// construct a `SourceStore` over it — see `lib.rs`'s module doc.
    pub blob_root: PathBuf,
}

impl JobHandler for IngestHandler {
    async fn handle(&self, job: &JobRow) -> Result<Outcome, HandlerError> {
        let Some(file_name) = job.payload.get("path").and_then(|p| p.as_str()) else {
            return Err(HandlerError::Permanent {
                message: "This job has no file path in its payload. It was not written by \
                          Lapidary's scan endpoint."
                    .to_owned(),
            });
        };
        self.ingest_one(job.library_id, file_name).await
    }
}

impl IngestHandler {
    /// One file, start to finish. See this module's doc for the ordering and why each
    /// step is where it is; see `scan.rs`'s module doc for the design's full reasoning.
    pub(crate) async fn ingest_one(
        &self,
        library: LibraryId,
        file_name: &str,
    ) -> Result<Outcome, HandlerError> {
        let path = self.ingest_dir.join(file_name);
        let kernel = MeshKernel;
        let version = kernel.version();
        let kernel_version = format!("{} {}", version.implementation, version.version);
        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let blobs = PgBlobs(self.db.clone());
        let ingest = PgIngest(self.db.clone());

        // 1. Read bytes. A missing file may mean the mount is not ready yet, so this is
        // Transient rather than Permanent -- unlike every step below it, this one has
        // nothing to do with the bytes themselves.
        let bytes = std::fs::read(&path).map_err(|e| HandlerError::Transient {
            message: format!("Could not read {file_name}: {e}"),
        })?;

        // 2. BLAKE3 -- hash first, always. Everything below branches on this.
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        let name = part_name(file_name);

        // 3. The same file, seen again -- same library, same name, same bytes --
        // short-circuits parse, raster and every write entirely. Scoped to the library on
        // purpose: a hash this library has never seen is a part it does not have,
        // whatever some other library holds. See `scan.rs`'s module doc.
        if blobs
            .library_holds(library, name, &hash)
            .await
            .map_err(transient_db)?
        {
            return Ok(Outcome::Skipped);
        }

        // 4. Parse + measure + rasterize. Nothing has been written yet, so a failure here
        // needs no cleanup. This runs even when the bytes are already in the blob store,
        // because the new part needs its own measurements and its own thumbnail; only the
        // bytes are shared, and they are already in memory from step 1. The bytes are
        // immutable, so this error is the final answer about them.
        let output = kernel.ingest(&bytes).map_err(|e| HandlerError::Permanent {
            message: e.to_string(),
        })?;

        // 5a. Some library already holds these bytes. Reuse them exactly as they are: no
        // second copy on disk, no second `blob` row, and -- the part that matters -- no
        // reap on failure, because those bytes are referenced by a part this job did not
        // create.
        if blobs.exists(&hash).await.map_err(transient_db)? {
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
                    blob: &blob,
                    measurements: &output.measurements,
                    thumbnail_webp: &output.thumbnail_webp,
                    kernel_version: &kernel_version,
                })
                .await
            {
                Ok(_) => Ok(Outcome::Ingested),
                Err(db_err) => classify_write(db_err),
            };
        }

        // 5b. New bytes. The blob is written before the transaction. `stored.hash` is
        // recomputed from `bytes` inside `put` and is definitionally the same as `hash`
        // above; `hash` is used below rather than `stored.hash` so there is exactly one
        // hash variable in scope.
        let stored = source.put(&bytes).map_err(|e| HandlerError::Transient {
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
                blob: &blob,
                measurements: &output.measurements,
                thumbnail_webp: &output.thumbnail_webp,
                kernel_version: &kernel_version,
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
                classify_write(db_err)
            }
        }
    }
}

/// The part name shown in the grid. Slice 1 has no part-numbering convention to draw on,
/// so the file's stem (its name without the `.stl` extension) is the whole story; falls
/// back to the full file name on the pathological case where a candidate file (already
/// proven to have a `.stl` extension by `is_stl_candidate`) somehow has no stem.
pub(crate) fn part_name(file_name: &str) -> &str {
    FsPath::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name)
}

fn transient_db(error: DbError) -> HandlerError {
    HandlerError::Transient {
        message: error.to_string(),
    }
}

/// A unique violation on `part_name_unique_per_library` is not a failure: another worker
/// won the race for this file after a lease expiry, and the part exists. Mapping it to
/// `Skipped` is what makes at-least-once delivery effectively-once -- see the design doc,
/// section 3.5.
fn classify_write(error: DbError) -> Result<Outcome, HandlerError> {
    if let DbError::Query(sqlx::Error::Database(db)) = &error
        && db.constraint() == Some("part_name_unique_per_library")
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
}
