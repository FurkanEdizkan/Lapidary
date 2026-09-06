//! Moving an existing content-addressed store into the model directories that replaced it.
//!
//! A store ingested before slice 7 holds every source file at `blobs/ab/cd/<hash>`, zstd-3,
//! with `file.storage_path` null. This job walks those rows, writes each file into its
//! model's own directory under its real name with a `metadata.json` beside it, and only
//! then removes the old copy. Migration `0008` states the rule this closes: *a null
//! `storage_path` means the bytes are still at the old content-addressed path*, and every
//! reader of `file` inherits it until this job has drained.
//!
//! # Copy before delete
//!
//! At every instant the bytes are readable at the old path, the new path, or both — never
//! at neither. Per hash, in this order, and the order is the whole design:
//!
//! 1. read the old copy, decoded at the level the `blob` row RECORDS
//! 2. BLAKE3 the result and compare it to the hash the row is keyed on
//! 3. write the new copy (`write_atomic`: temp file, `sync_all`, rename, fsync the parent)
//! 4. write `metadata.json` beside it
//! 5. one transaction: the `file` rows get their paths, the `blob` row drops to level 0
//! 6. **only now** unlink `blobs/ab/cd/<hash>`
//!
//! A crash between any two of those leaves a store that still serves every file. Step 5
//! failing is the one case that must NOT reap what steps 3 and 4 wrote: a `commit` can
//! return an error after the server has already applied it (the connection drops between
//! `COMMIT` and its acknowledgement), and reaping then would delete the file a committed
//! row now points at. A failure in step 3 or 4 is different — nothing is committed yet, the
//! old copy is untouched, and the bytes are reaped so a retry lands on the same directory
//! rather than on a disambiguated one.
//!
//! # Why the batch is a hash and not a file row
//!
//! [`lapidary_db::PgStorageMigration`]'s module doc has this in full. In short: source bytes
//! used to be deduplicated, so one `blob` row can carry several `file` rows, and
//! `zstd_level` is a single column they all read through. Every un-migrated row for one hash
//! settles in one transaction, whatever library it belongs to.
//!
//! # What this job does not move
//!
//! **A quarantined or purged blob.** Purge removes the `file` row, so its bytes are reachable
//! from no row this job can select, and they stay under `blobs/ab/cd/…` after everything
//! else has moved. That is deliberate: those bytes are already scheduled for removal by the
//! purge slice's own 30-day hold, and a job that walked orphaned blobs would be deleting
//! user data on a path whose whole reason to exist is that deletion is explicit and
//! separate. It is stated here so a later reader finds a decision rather than a bug.
//!
//! **Derivatives.** They stay content-addressed, freely evictable and reachable by hash.
//! Only source files moved.

use crate::AppState;
use crate::handler::{WorkerHandler, classify_db, reap_source};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::manifest::{ManifestFile, ManifestPart, ManifestRevision, ModelManifest};
use lapidary_core::slug::slugify;
use lapidary_core::{BatchId, BlobHash, FolderId, LibraryId, Outcome, ScanAccepted};
use lapidary_db::{PendingSource, PgFolders, PgJobs, PgParts, PgStorageMigration};
use lapidary_jobs::HandlerError;
use lapidary_storage::{Compression, SourceStore, WorkerRole};
use std::collections::{HashMap, HashSet};
use std::path::Path as FsPath;

/// `POST /api/libraries/{id}/migrate-storage` -- enqueue one `migrate_storage` job for
/// `library`, guarded exactly like the worker's own startup sweep
/// (`bin/lapidary-server`, which calls this same `PgJobs::enqueue_migration_if_absent`
/// once per library it finds un-migrated on boot). This route exists so an operator
/// does not have to wait for the next worker restart, or hand-write `INSERT INTO job`,
/// to start one.
///
/// Deliberately identical in shape to this crate's own `scan`: `queued: 0` when
/// `library` already has a migration pending or running, or when this call lost the
/// race to one that landed first, is a success and not an error -- the batch id in that
/// case names nothing real and exists only so the response shape never has to be
/// optional, matching `ScanAccepted`'s own convention.
pub async fn migrate(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgJobs(state.db.clone())
        .enqueue_migration_if_absent(library)
        .await
    {
        Ok(Some(batch_id)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted {
                batch_id,
                queued: 1,
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted {
                batch_id: BatchId::new(),
                queued: 0,
            }),
        )
            .into_response(),
        Err(source) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "message": format!(
                    "Could not queue a storage migration for this library: {source}. \
                     Nothing was queued, so it is safe to try again once the database is \
                     reachable."
                )
            })),
        )
            .into_response(),
    }
}

/// How many distinct blobs one run moves before it hands the queue back and re-enqueues
/// itself.
///
/// Not a tuning knob for throughput — a bound on how long one job holds its lease. A run
/// that tried to drain a 23 GB corpus in one go would outlive its lease, be reclaimed
/// mid-copy by a second worker, and have both of them writing the same directories.
const HASHES_PER_RUN: i64 = 200;

impl WorkerHandler {
    /// One slice of the move, and a re-enqueue if anything is left.
    ///
    /// Returns [`Outcome::Migrated`] — including for a run that found nothing, because a
    /// drained store is a success and re-running the job is a no-op rather than an error.
    /// See the module doc for the per-hash ordering and why it is that ordering.
    pub(crate) async fn migrate_storage(
        &self,
        batch: BatchId,
        library: LibraryId,
    ) -> Result<Outcome, HandlerError> {
        let migrations = PgStorageMigration(self.db.clone());
        let pending = migrations
            .pending_sources(library, HASHES_PER_RUN)
            .await
            .map_err(classify_db)?;
        if pending.is_empty() {
            return Ok(Outcome::Migrated);
        }

        // Before any directory is resolved, and for every library this page touches — not
        // just the one whose job is running. A page can pull in another library's rows
        // through a shared hash, and `model_dir_for` builds those rows' paths out of THAT
        // library's folder slugs.
        let mut libraries: Vec<LibraryId> = pending.iter().map(|row| row.library).collect();
        libraries.sort_by_key(LibraryId::as_uuid);
        libraries.dedup();
        for library in libraries {
            self.reslug_back_filled_categories(library).await?;
        }

        let source = SourceStore::open(&self.blob_root, &WorkerRole::assume());
        let mut moved = 0usize;
        let mut refused: Option<HandlerError> = None;
        // Rows arrive ordered by hash, so consecutive equal hashes are the whole group.
        for group in pending.chunk_by(|a, b| a.hash == b.hash) {
            match self.migrate_one_hash(&source, &migrations, group).await {
                Ok(()) => moved += group.len(),
                Err(error) => {
                    tracing::warn!(
                        hash = %group.first().map(|row| row.hash.to_hex()).unwrap_or_default(),
                        reason = ?error,
                        "could not move a blob into its model directory; its bytes are \
                         still readable where they were"
                    );
                    if displaces(refused.as_ref(), &error) {
                        refused = Some(error);
                    }
                }
            }
        }

        // Nothing moved and something refused: report it rather than re-enqueue. A run that
        // makes no progress and queues itself again is a queue that never drains — the
        // refusal repeats, the batch never settles, and the operator sees a scan bar that
        // moves forever. Progress is what earns another run.
        if moved == 0
            && let Some(error) = refused
        {
            return Err(error);
        }

        if migrations.any_pending(library).await.map_err(classify_db)? {
            // Guarded, not a plain insert: this run's own row is still `running` while
            // this executes, and a worker that hit its shutdown grace period or lost a
            // lease mid-run can already have put it back to `pending` in the
            // background (`lapidary_jobs::worker`'s `SHUTDOWN_GRACE`) -- either way, a
            // successor may already be queued. `reenqueue_migration_if_absent`'s own
            // doc has the full reasoning; the short version is that a plain `INSERT`
            // here would occasionally throw the unique violation
            // `job_migrate_storage_pending_per_library` exists to prevent straight into
            // this `map_err`, misreporting a benign double-enqueue as "could not reach
            // the database."
            PgJobs(self.db.clone())
                .reenqueue_migration_if_absent(batch, library)
                .await
                .map_err(|e| HandlerError::Transient {
                    message: format!(
                        "Moved {moved} file(s) into their model directories, but could not \
                         queue the next batch: {e}. Wait for the database to come back and \
                         start the migration again — everything already moved is skipped, \
                         never moved twice."
                    ),
                })?;
        }
        Ok(Outcome::Migrated)
    }

    /// Every un-migrated `file` row that names one hash, moved together.
    async fn migrate_one_hash(
        &self,
        source: &SourceStore,
        migrations: &PgStorageMigration,
        group: &[PendingSource],
    ) -> Result<(), HandlerError> {
        let Some(first) = group.first() else {
            return Ok(());
        };
        let hash = first.hash;

        // The level the row RECORDS, never one re-derived from the format — `SourceReader`'s
        // rule, and it applies with more force here than anywhere else: this is the read
        // whose result gets written back under a new name and whose original is then
        // unlinked. `None` and `Some(0)` both mean stored as-is.
        let compression = if first.zstd_level.is_some_and(|level| level != 0) {
            Compression::Zstd
        } else {
            Compression::AsIs
        };
        let bytes = source
            .get(&hash, compression)
            .map_err(|e| HandlerError::Transient {
                message: format!(
                    "Could not read the stored copy of {} before moving it: {e}. Check that \
                     the blob volume is mounted and readable, then start the migration \
                     again.",
                    first.name
                ),
            })?;

        // The one line between "the recorded level was wrong" and "wrote a zstd frame as
        // cliff.stl, then deleted the original". Nothing decides anything on the recomputed
        // hash except this: bytes that do not hash to the row's own key are not the bytes
        // that row describes, and copying them somewhere else would launder the corruption
        // into a file the user opens.
        //
        // Permanent: the same blob decodes to the same bytes on every attempt, so three
        // retries would produce three identical refusals.
        let read = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        if read != hash {
            return Err(HandlerError::Permanent {
                message: format!(
                    "The stored copy of {} does not match the hash recorded for it — it \
                     reads as {}… where the database says {}… . The blob may have been \
                     corrupted or written by something other than Lapidary; re-scan this \
                     part from its source file. Nothing was moved or removed.",
                    first.name,
                    &read.to_hex()[..8],
                    &hash.to_hex()[..8],
                ),
            });
        }

        let mut written: Vec<(String, String)> = Vec::with_capacity(group.len());
        let mut moved = Vec::with_capacity(group.len());
        for row in group {
            match self.copy_into_model_dir(source, row, &bytes).await {
                Ok((model_dir, storage_path)) => {
                    moved.push((row.file_id, storage_path.clone()));
                    written.push((model_dir, storage_path));
                }
                Err(error) => {
                    // Safe to reap only because nothing has been committed yet: every row
                    // in this group still says NULL, so the bytes these paths hold are
                    // bytes nothing points at, and the old copy is still where it was. See
                    // the module doc for why the same reap after `settle` would be data
                    // loss.
                    for (model_dir, storage_path) in &written {
                        reap_copy(source, model_dir, storage_path);
                    }
                    return Err(error);
                }
            }
        }

        let unreferenced = migrations
            .settle(&hash, &moved)
            .await
            .map_err(classify_db)?;

        // The delete half, and the only place it happens. `unreferenced` was read inside
        // the transaction that just committed, so a row that still needs the old copy — an
        // un-migrated sibling in a library this page did not reach — keeps it.
        if unreferenced && let Err(error) = source.remove(&hash) {
            tracing::warn!(
                hash = %hash.to_hex(),
                %error,
                "could not remove the content-addressed copy after moving it; the file is \
                 stored twice until it is removed by hand"
            );
        }
        Ok(())
    }

    /// One row's file and its manifest, written into the directory its model owns. Returns
    /// that directory and the store-relative path the `file` row will name.
    ///
    /// `model_dir_for` rather than a second copy of the layout rule: it is `ingest_one`'s
    /// own resolver, and two implementations of "where does this model go" would drift the
    /// moment either changed — a store that then fails to re-scan.
    ///
    /// `metadata.json` is written from the database rows, never read back and rewritten, so
    /// `ModelManifest::is_future_schema` has nothing to guard here: there is no round trip
    /// in which a newer build's unknown fields could be dropped. The only manifest that can
    /// already exist at this path is one this same build wrote on an earlier attempt at
    /// this same file — a newer build's directory would have made `model_dir_for`
    /// disambiguate away from it, and a row a newer build had already migrated would not
    /// have a null `storage_path` to be selected by.
    async fn copy_into_model_dir(
        &self,
        source: &SourceStore,
        row: &PendingSource,
        bytes: &[u8],
    ) -> Result<(String, String), HandlerError> {
        let (_folder, model_dir) = self
            .model_dir_for(row.library, &row.source_path, &row.name, &row.hash)
            .await?;
        // The name the user gave it, exactly as ingest writes it: the promise is that the
        // directory holds the file they recognise.
        let file_name = FsPath::new(&row.source_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&row.name);
        let storage_path = format!("{model_dir}/{file_name}");

        // `AsIs`, and the `blob` row is dropped to level 0 by the same transaction that
        // records this path. A model directory is something the owner opens in a file
        // manager, and a zstd frame named `cliff.stl` is not that — but the recorded level
        // is what every reader follows, so writing the file uncompressed and leaving the
        // row saying 3 would make every later read decode bytes that were never encoded.
        source
            .put_at(&storage_path, bytes, Compression::AsIs)
            .map_err(|e| HandlerError::Transient {
                message: format!(
                    "Could not write {storage_path}: {e}. Check that the blob volume is \
                     mounted and writable, then start the migration again — the original \
                     copy has not been touched."
                ),
            })?;

        let manifest = manifest_for(row, file_name);
        let written = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| e.to_string())
            .and_then(|json| {
                source
                    .put_at(
                        &format!("{model_dir}/metadata.json"),
                        &json,
                        Compression::AsIs,
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        // Not warn-only, unlike ingest's. Ingest writes its manifest after the part is
        // committed and a retry would settle as `Skipped`, so failing there would report a
        // file as not ingested when it was. Nothing is committed here, so a retry really
        // does get another attempt — and a model directory whose manifest is missing is
        // exactly the orphan this migration exists to stop producing.
        if let Err(error) = written {
            reap_copy(source, &model_dir, &storage_path);
            return Err(HandlerError::Transient {
                message: format!(
                    "Could not write {model_dir}/metadata.json: {error}. Check that the blob \
                     volume is mounted and writable, then start the migration again — the \
                     original copy has not been touched."
                ),
            });
        }
        Ok((model_dir, storage_path))
    }

    /// Give every category in `library` the slug its name actually slugifies to.
    ///
    /// Migration `0008`'s back-fill wrote `folder.slug = name`, unslugged. That was defensible
    /// for the ingest directory, which we only ever read — but a slug names a directory in
    /// OUR store, which we create, so a back-filled category called `Rocks?` yields a path
    /// no Windows client can hold. `slugify` lives in Rust, which is why the fix lands here
    /// rather than in the plpgsql that produced it.
    ///
    /// Siblings that slug alike (`Rocks?` and `Rocks*` both become `Rocks-`) would violate
    /// `folder_slug_unique_per_parent`, so the loser takes the last six hex digits of its own
    /// id — the shape `slug::disambiguate` uses for a model directory, with the folder's id
    /// standing in for a source hash the folder does not have. The LAST six, not the first:
    /// a uuidv7's leading digits are a millisecond timestamp, and two folders back-filled by
    /// one statement share them.
    ///
    /// No intermediate state can violate the constraint, so the updates need no transaction
    /// of their own: a folder is only changed when `slug == name != slugify(name)`, so its
    /// current slug is not a slugify output, while every slug assigned here is — and a slug
    /// being assigned can therefore never collide with one still waiting to be replaced.
    /// Siblings cannot share a name (`folder_name_unique_per_parent`), so no two changing
    /// folders compete for the same target on that route either.
    async fn reslug_back_filled_categories(&self, library: LibraryId) -> Result<(), HandlerError> {
        let folders = PgFolders(self.db.clone());
        let tree = folders.tree(library).await.map_err(classify_db)?;

        // Folders already carrying a correct slug own it. They are the ones with a directory
        // on disk under that name, so a colliding back-filled sibling is the one that moves.
        let mut taken: HashMap<Option<FolderId>, HashSet<String>> = HashMap::new();
        for row in &tree {
            if row.slug == slugify(&row.name) {
                taken
                    .entry(row.parent_id)
                    .or_default()
                    .insert(row.slug.clone());
            }
        }

        let mut changes = Vec::new();
        for row in &tree {
            let want = slugify(&row.name);
            if row.slug == want {
                continue;
            }
            let siblings = taken.entry(row.parent_id).or_default();
            let mut slug = want;
            if !siblings.insert(slug.clone()) {
                let id = row.id.as_uuid().simple().to_string();
                slug = format!("{slug}_{}", &id[id.len() - 6..]);
                if !siblings.insert(slug.clone()) {
                    // Both the target and its disambiguated form are already held by
                    // siblings that legitimately slug to them. Leaving this one alone costs
                    // one category a hostile directory name; renaming it anyway raises a
                    // bare `folder_slug_unique_per_parent` violation that propagates out of
                    // `migrate_storage` before a single file moves, stalling the whole
                    // library's migration behind a message no operator can act on.
                    tracing::warn!(
                        folder = %row.id,
                        name = %row.name,
                        wanted = %slug,
                        "left a category's slug as it is: the name it slugs to, and its \
                         disambiguated form, are both already taken by sibling categories. \
                         Rename one of them and start the migration again to give this one \
                         a filesystem-safe directory"
                    );
                    continue;
                }
            }
            changes.push((row.id, row.name.clone(), row.slug.clone(), slug));
        }
        if changes.is_empty() {
            return Ok(());
        }

        // Every old path is read BEFORE any rename lands, so an ancestor that is itself
        // about to change still reports the directory that is really on disk.
        let library_slug = PgParts(self.db.clone())
            .library_slug(library)
            .await
            .map_err(classify_db)?
            .ok_or_else(|| HandlerError::Permanent {
                message: format!(
                    "There is no library {library} whose categories could be re-slugged. The \
                     library may have been removed after this job was queued; start the \
                     migration again for the library you meant."
                ),
            })?;
        let mut on_disk = Vec::with_capacity(changes.len());
        for (id, ..) in &changes {
            let path = folders.slug_path(*id).await.map_err(classify_db)?;
            on_disk.push(
                FsPath::new(&self.blob_root)
                    .join(format!("libraries/{library_slug}/{path}"))
                    .exists(),
            );
        }

        for ((id, name, was, slug), existed) in changes.into_iter().zip(on_disk) {
            if existed {
                // Renaming the directory too, and rewriting every `storage_path` beneath
                // it, is what a category move does — Task 10's job, not a second
                // implementation here. Nothing is lost either way: the rows already written
                // still name the directory the files are really in, and they still read.
                tracing::warn!(
                    folder = %id,
                    from = %was,
                    to = %slug,
                    "re-slugged a category that already has a directory under its old name; \
                     files already written stay there and stay readable, and new ones land \
                     under the new slug until the category is moved"
                );
            }
            folders
                .rename(id, &name, &slug)
                .await
                .map_err(classify_db)?;
        }
        Ok(())
    }
}

/// Whether `next` should replace the refusal this run is already holding.
///
/// The first refusal stands, except that a permanent one displaces a transient one. It
/// matters only when a run makes no progress at all, because then the refusal it returns is
/// what decides whether the queue retries — and three backoffs spent rediscovering a corrupt
/// blob is exactly the delay `classify_db` already refuses to introduce for a guard refusal.
fn displaces(refused: Option<&HandlerError>, next: &HandlerError) -> bool {
    matches!(
        (refused, next),
        (None, _)
            | (
                Some(HandlerError::Transient { .. }),
                HandlerError::Permanent { .. }
            )
    )
}

/// Remove a model directory this job wrote for a move that then failed before anything was
/// committed. `handler::reap_source`'s rule, plus the manifest, because a directory that
/// still holds one cannot be removed and a retry would then disambiguate around it.
fn reap_copy(source: &SourceStore, model_dir: &str, storage_path: &str) {
    if let Err(error) = source.remove_at(&format!("{model_dir}/metadata.json")) {
        tracing::warn!(
            model_dir,
            %error,
            "failed to reap a metadata.json after a failed move; it may now be an orphan on disk"
        );
    }
    reap_source(source, storage_path, model_dir);
}

/// The `metadata.json` for one migrated file, built entirely from the rows that describe it.
///
/// One revision, not the part's whole history: the manifest describes the directory it sits
/// in, and a directory holds one revision's file. That matches what ingest writes for a
/// freshly ingested model, so a migrated store and an ingested one read the same.
fn manifest_for(row: &PendingSource, file_name: &str) -> ModelManifest {
    ModelManifest {
        schema: ModelManifest::SCHEMA,
        part: ManifestPart {
            id: row.part,
            library: row.library,
            name: row.name.clone(),
            part_number: row.part_number.clone(),
            classification: row.classification.clone(),
            source_path: row.source_path.clone(),
            metadata: row.metadata.clone(),
        },
        revisions: vec![ManifestRevision {
            id: row.revision,
            rev_label: row.rev_label.clone(),
            origin: row.origin.clone(),
            volume_mm3: row.volume_mm3,
            volume_source: row.volume_source.clone(),
            bbox_mm: row.bbox_mm,
            triangle_count: row.triangle_count,
            is_watertight: row.is_watertight,
            units: row.units.clone(),
            files: vec![ManifestFile {
                role: row.role.clone(),
                format: row.format.clone(),
                blake3: row.hash,
                size_bytes: row.size_bytes,
                file_name: file_name.to_owned(),
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transient() -> HandlerError {
        HandlerError::Transient {
            message: "the volume went away".to_owned(),
        }
    }

    fn permanent() -> HandlerError {
        HandlerError::Permanent {
            message: "the blob does not match its hash".to_owned(),
        }
    }

    /// A run where several hashes refuse reports one of them, and which one decides whether
    /// the queue spends three backoffs rediscovering an answer it already had.
    #[test]
    fn a_permanent_refusal_displaces_a_transient_one_but_not_the_other_way() {
        assert!(
            displaces(None, &transient()),
            "the first refusal always stands"
        );
        assert!(displaces(None, &permanent()));
        assert!(
            displaces(Some(&transient()), &permanent()),
            "a refusal that will never succeed must not wait behind one that might"
        );
        assert!(
            !displaces(Some(&permanent()), &transient()),
            "and must not then be displaced back"
        );
        assert!(
            !displaces(Some(&transient()), &transient()),
            "otherwise the reported refusal changes for no reason between two equal ones"
        );
        assert!(!displaces(Some(&permanent()), &permanent()));
    }
}
