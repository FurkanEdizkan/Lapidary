//! One file's worth of ingest, as a job: read, BLAKE3, library_holds, measure, record.
//!
//! **Since M4 nothing here writes a file** (owner, 2026-10-17: "use the directed file path of each
//! file or folder"). A scanned file is read where it is in the owner's folders and its row records
//! that place; an upload or a link import was already written into the inbox, once, before its job
//! was queued. No copy goes into the store, no `metadata.json` is written beside a file, and a
//! changed file's previous bytes are not set aside anywhere — the owner overwrote them, and the
//! previous revision keeps its figures and its hash and is marked no longer on disk.
//!
//! # Ordering
//!
//! Per file, in this order — the order is the design:
//!
//! 1. read bytes, and the size and modification time of the handle they came from
//! 2. BLAKE3 — hash first, always
//! 3. `blobs.library_holds(library, source_path, hash)` — does the part's *current* revision
//!    hold these bytes? yes -> `Skipped`, after bringing the recorded modification time up to
//!    date: a `touch` must not leave every download refused as "changed"
//!    3b. `revisions.current(library, source_path)` — a part already here, with other bytes:
//!    on to 5a. A deleted part has no rows left, so its file is ingested again as a new part
//!    3a. `parts.library_exists(library)` — below the short-circuit, so a re-scan costs one query
//! 4. `lapidary_cad::parse(bytes, format)` then `measure(&mesh)`
//!    5a. `revisions.record_revision(...)` — the part's next revision, at the file's place;
//!    earlier revisions at the same place are marked not on disk -> `Revised`
//! 6. `blobs.exists(hash)` — whether a `blob` row for these bytes is already there
//! 7. `folder_for(...)` — the category rows the file's directories imply
//! 8. `ingest.record(...)`, or `ingest.link_existing(...)` when step 6 said the row is there
//!
//! # Why the short-circuit is scoped to the library, and to the path
//!
//! A content hash must not decide a per-library write (CLAUDE.md: content addressing is not
//! authorization): scanning six real STLs into a new empty library once answered "6 skipped" and
//! left it empty. And two files with identical bytes at two paths are two parts sharing one blob,
//! not one part and a silent omission. Only "same library, same path, same bytes" is a re-scan.
//!
//! # Error classification
//!
//! The parser's errors are `Permanent`: the same bytes produce the same error. Database and I/O
//! errors are `Transient`. When genuinely unsure, this module chooses `Transient`: a retried
//! permanent failure costs one wasted parse, a non-retried transient one costs the user a file.

use lapidary_cad::CadError;
use lapidary_core::slug::slugify;
use lapidary_core::{
    BlobHash, FileRoot, FolderId, JobPayload, LibraryId, MeasurementProvenance, Outcome,
    Provenance, RevisionId, RevisionOrigin, source_format,
};
use lapidary_db::{
    DbError, IngestRequest, JobRow, PgBlobs, PgFolders, PgIngest, PgParts, PgPool, PgRevisions,
    PgSources, RevisionRequest, StoredBlobRow,
};
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_storage::{DerivativeStore, LibraryReader, Roots};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

pub struct WorkerHandler {
    pub db: PgPool,
    /// The owner's folders, mounted read-only: what a scan walks and where a library's files are
    /// read. Never a hardcoded container path — tests point it at a `TempDir`.
    pub ingest_dir: PathBuf,
    /// Where uploads, link imports and the seeded examples are written once (M4). Never inside
    /// `ingest_dir`: the server refuses to start when it is.
    pub inbox_dir: PathBuf,
    /// Lapidary's own store: pictures, exports, bundles, and source files from before M4.
    pub blob_root: PathBuf,
    /// Suspends an ingest just before it parses. `None` everywhere a worker really runs.
    ///
    /// Parsing is synchronous all the way down, so this is the only instant at which one of two
    /// jobs racing over one new path can be held while the other finishes; the races in
    /// `tests/handler.rs` are staged through it.
    pub pause: Option<Pause>,
    /// Where the model sites an `import_link` reads from are. `Sites::public()` everywhere a
    /// worker really runs; tests point it at a fixture server.
    pub sites: lapidary_sites::Sites,
}

/// The await point above, and nothing else.
pub struct Pause {
    /// Notified when the job reaches the parse.
    pub reached: Arc<tokio::sync::Notify>,
    /// Awaited before the parse runs.
    pub release: Arc<tokio::sync::Notify>,
}

/// Where a file is, as [`WorkerHandler::index`] records it: which root, the path under it, and
/// the modification time of the handle its bytes were read from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FileAt<'a> {
    pub root: FileRoot,
    pub path: &'a str,
    pub mtime_ns: i64,
}

impl JobHandler for WorkerHandler {
    /// The `kind` COLUMN decides which arm runs. Both failures below are `Permanent`: a kind
    /// this build does not know will not become known on a retry, and a payload that does not
    /// parse holds the same bytes next time.
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
            // The batch comes from this job's own row, never from the payload: the files a scan
            // finds belong in the batch the browser is already polling. See `scan.rs`.
            JobPayload::ScanDirectory => self.scan_directory(job.batch_id, job.library_id).await,
            JobPayload::IngestInbox { path, source_path } => {
                self.ingest_upload(job.batch_id, job.library_id, &path, &source_path)
                    .await
            }
            // A model from the page a person pasted, through ordinary ingest; see `link.rs`.
            JobPayload::ImportLink { url } => self.import_link(job.library_id, &url).await,
            // "Fetch details": a part described from its page, nothing downloaded (A2).
            JobPayload::DescribePage { path, url } => {
                self.describe_one(job.library_id, &path, &url).await
            }
            // A bundle unpacks into its own batch, as a scan does; see `import.rs`.
            JobPayload::ImportBundle { blake3, path } => {
                self.import_bundle(job.batch_id, job.library_id, blake3, &path)
                    .await
            }
            JobPayload::ImportPart { bundle, part, path } => {
                self.import_part(job.library_id, bundle, part, &path).await
            }
        }
    }
}

/// What `index_body` indexes: a mesh's bytes, or a pack's size and what it holds (A13).
enum Body {
    Mesh(Vec<u8>),
    Pack {
        size: u64,
        contents: serde_json::Value,
    },
}

/// The formats this library refuses, as `source_format` spells them. The scan walk accepts these
/// beside the mesh extensions, so a `.step` file is refused by name rather than skipped in
/// silence — see [`lapidary_core::CAD_FORMATS`].
pub(crate) use lapidary_core::CAD_FORMATS;

/// The whole process's parse budget, in MiB of estimated peak memory (A32). Half the container's 3 GiB, which leaves
/// the server, its caches and its database pool their room; measured against the owner's corpus, whose large 3MF kits
/// peaked about 1.1 GiB each (F3).
// ponytail: one process-wide budget and fixed factors; make it a setting if a deployment raises its memory ceiling.
const PARSE_BUDGET_MIB: u32 = 1536;
static PARSE_BUDGET: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(PARSE_BUDGET_MIB as usize);

/// A blocking step that did not finish. A panic is the file's doing, the same on every attempt, so it fails the job
/// at once rather than reading a large file three times over (A39); anything else is the runtime's, and retried.
fn stopped(source_path: &str, doing: &str, error: &tokio::task::JoinError) -> HandlerError {
    if error.is_panic() {
        HandlerError::Permanent {
            message: format!(
                "{doing} {source_path} failed inside Lapidary's reader. The file may be malformed; export it again from your tool and rescan."
            ),
        }
    } else {
        HandlerError::Transient {
            message: format!("{doing} {source_path} stopped: {error}"),
        }
    }
}

/// What reading and measuring a mesh of `size` bytes is expected to hold at its peak, in MiB, by format: the bytes and
/// what they parse into. A 3MF is a zip of XML that inflates as it is read, so it costs most. At least 1, and never
/// more than the whole budget, so the largest file runs alone rather than never.
pub(crate) fn parse_cost_mib(source_path: &str, size: u64) -> u32 {
    let factor = match source_format(source_path).as_str() {
        "3mf" => 6,
        "obj" => 3,
        _ => 2,
    };
    let mib = size.saturating_mul(factor).div_ceil(1 << 20);
    u32::try_from(mib)
        .unwrap_or(u32::MAX)
        .clamp(1, PARSE_BUDGET_MIB)
}

/// A path from a payload or a row, refused if it would leave the directory it is joined to.
/// `Permanent`: a payload holds the same bytes on every attempt.
fn refuse_escaping(path: &str) -> Result<(), HandlerError> {
    lapidary_core::slug::reject_escaping_path(path).map_err(|e| HandlerError::Permanent {
        message: e.to_string(),
    })
}

impl WorkerHandler {
    /// The three roots a `file` row can name.
    pub(crate) fn roots(&self) -> Roots {
        Roots {
            library: self.ingest_dir.clone(),
            inbox: self.inbox_dir.clone(),
            store: self.blob_root.clone(),
        }
    }

    /// The centre of the volume measured, onto the revision just committed. Warn-only: the
    /// revision is committed and measured, and a centre it lacks leaves its diff without one.
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

    /// One file a scan found in the models folder, read where it is: `source_path` is its path under
    /// the folder (A2), and once it is filed it gets what the person gave the file or folder it was
    /// added through — their pictures, and the page's details when they asked for them.
    pub async fn ingest_one(
        &self,
        library: LibraryId,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        refuse_escaping(source_path)?;
        if lapidary_core::is_pack_file(source_path) {
            return self
                .ingest_pack(
                    library,
                    source_path,
                    FileRoot::Library,
                    source_path,
                    RevisionOrigin::Ingest,
                )
                .await;
        }
        let at = source_path.to_owned();
        // 0. Unchanged since it was indexed (A30's measurement, A31): the same size and modification time as the
        // part's current revision at this path is a file not read again, as a pack is not. A rescan of a library is
        // then a stat per file rather than every byte of it.
        let stat = LibraryReader::open(&self.ingest_dir)
            .stat_at(&at)
            .map_err(|e| HandlerError::Transient {
                message: format!("Could not read {source_path} in your folders: {e}"),
            })?;
        if self
            .unchanged(library, source_path, FileRoot::Library, &at, &stat)
            .await?
        {
            return Ok(Outcome::Skipped);
        }
        // 1. Read bytes, and 2. BLAKE3 -- hash first, always; everything below branches on it. Off the async threads,
        // since a mesh can be hundreds of megabytes. A missing file may mean the mount is not ready yet, so this is
        // Transient — unlike every step below it, this one has nothing to do with the bytes.
        let (bytes, stat, hash) = {
            let dir = self.ingest_dir.clone();
            let rel = at.clone();
            tokio::task::spawn_blocking(move || {
                let (bytes, stat) = LibraryReader::open(&dir).read_at(&rel)?;
                let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
                Ok::<_, lapidary_storage::StorageError>((bytes, stat, hash))
            })
            .await
            .map_err(|e| stopped(source_path, "Reading", &e))?
            .map_err(|e| HandlerError::Transient {
                message: format!("Could not read {source_path} in your folders: {e}"),
            })?
        };
        let at = FileAt {
            root: FileRoot::Library,
            path: &at,
            mtime_ns: stat.mtime_ns,
        };
        let filed = self
            .index(
                library,
                source_path,
                bytes,
                hash,
                RevisionOrigin::Ingest,
                at,
            )
            .await?;
        if matches!(filed, Outcome::Ingested | Outcome::Revised) {
            self.as_added(library, source_path).await;
        }
        Ok(filed)
    }

    /// What the person gave the file or folder this part was added through (A2): each of their
    /// pictures, once, after the part's own, and the page's details when they asked for them.
    /// Warn-only: the part is filed, and what it is given is not a reason to fail it.
    async fn as_added(&self, library: LibraryId, source_path: &str) {
        let source = match PgSources(self.db.clone())
            .covering(library, source_path)
            .await
        {
            Ok(Some(source)) => source,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(source_path, %error, "could not read what this file was added with; it has no pictures from it");
                return;
            }
        };
        let images = match PgSources(self.db.clone()).images(source.id).await {
            Ok(images) => images,
            Err(error) => {
                tracing::warn!(source_path, %error, "could not read the pictures this file was added with");
                Vec::new()
            }
        };
        if !images.is_empty() {
            match PgRevisions(self.db.clone())
                .current(library, source_path)
                .await
            {
                Ok(Some(current)) => {
                    for (n, webp) in images.iter().enumerate() {
                        // A source URL of its own, so filing the part again adds nothing twice.
                        let mark = format!("lapidary:source/{}/{n}", source.id);
                        self.adopt_picture(
                            current.part,
                            webp,
                            "uploaded",
                            Some(&mark),
                            source_path,
                        )
                        .await;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(source_path, %error, "could not find the part just filed; it has no pictures from what it was added with")
                }
            }
        }
        if source.fetch_details
            && let Some(url) = source.page_url.as_deref()
            && let Err(error) = self.describe_one(library, source_path, url).await
        {
            tracing::warn!(source_path, url, %error, "could not fetch the page's details; the part is filed without them");
        }
    }

    /// One file Lapidary wrote into the inbox — an upload — indexed from there. `path` is where
    /// it is under the inbox; `source_path` is the part it belongs to.
    /// An uploaded file's job: an archive is opened (A10), anything else indexed.
    async fn ingest_upload(
        &self,
        batch: lapidary_core::BatchId,
        library: LibraryId,
        path: &str,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        refuse_escaping(path)?;
        refuse_escaping(source_path)?;
        let format = source_format(path);
        if crate::archive::ARCHIVES.contains(&format.as_str()) {
            return self.unpack_upload(batch, library, path, source_path).await;
        }
        // A RAR can't be opened, so it is kept as a pack (A13) rather than refused.
        if crate::archive::REFUSED_ARCHIVES.contains(&format.as_str()) {
            return self
                .ingest_pack(
                    library,
                    source_path,
                    FileRoot::Inbox,
                    path,
                    RevisionOrigin::Upload,
                )
                .await;
        }
        self.ingest_inbox(library, path, source_path).await
    }

    pub(crate) async fn ingest_inbox(
        &self,
        library: LibraryId,
        path: &str,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        refuse_escaping(path)?;
        refuse_escaping(source_path)?;
        let (bytes, stat) = LibraryReader::open(&self.inbox_dir)
            .read_at(path)
            .map_err(|e| HandlerError::Transient {
                message: format!(
                    "Could not read the uploaded {source_path} back from the inbox ({path}): {e}"
                ),
            })?;
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        let at = FileAt {
            root: FileRoot::Inbox,
            path,
            mtime_ns: stat.mtime_ns,
        };
        self.index(
            library,
            source_path,
            bytes,
            hash,
            RevisionOrigin::Upload,
            at,
        )
        .await
    }

    /// An uploaded zip or 7z (A10): its models unpacked beside it, each queued into this batch as an
    /// uploaded file is. See `archive.rs`.
    async fn unpack_upload(
        &self,
        batch: lapidary_core::BatchId,
        library: LibraryId,
        path: &str,
        source_path: &str,
    ) -> Result<Outcome, HandlerError> {
        let inbox = self.inbox_dir.clone();
        let at = path.to_owned();
        let unpacked = tokio::task::spawn_blocking(move || {
            crate::archive::unpack(&inbox, &at, crate::archive::LIMITS)
        })
        .await
        .map_err(|e| stopped(source_path, "Unpacking", &e))?
        .map_err(|message| HandlerError::Permanent { message })?;
        if !unpacked.skipped.is_empty() {
            tracing::warn!(source_path, skipped = ?unpacked.skipped, "left out of an uploaded archive");
        }
        if unpacked.placed.is_empty() {
            let left = if unpacked.skipped.is_empty() {
                String::new()
            } else {
                format!(" Left out: {}.", unpacked.skipped.join(", "))
            };
            return Err(HandlerError::Permanent {
                message: format!("{source_path} holds no stl, obj or 3mf file.{left}"),
            });
        }
        let under = crate::archive::without_extension(source_path);
        let jobs: Vec<JobPayload> = unpacked
            .placed
            .into_iter()
            .map(|(inside, placed)| JobPayload::IngestInbox {
                path: placed,
                source_path: format!("{under}/{inside}"),
            })
            .collect();
        // `enqueue_into`, as a bundle's parts are: they belong to the batch the browser follows.
        lapidary_db::PgJobs(self.db.clone())
            .enqueue_into(batch, library, &jobs)
            .await
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })?;
        Ok(Outcome::Scanned)
    }

    /// Steps 3 through 8: short-circuit, parse, and the one transaction. Writes no file.
    pub(crate) async fn index(
        &self,
        library: LibraryId,
        source_path: &str,
        bytes: Vec<u8>,
        hash: BlobHash,
        origin: RevisionOrigin,
        at: FileAt<'_>,
    ) -> Result<Outcome, HandlerError> {
        // Its share of the parse budget, held until it is filed and no longer (A32, A39): several large meshes at once
        // is what nears the container's memory ceiling, so a large one waits for room while small ones keep flowing.
        // Here, because every mesh is filed through this — a scan's, an upload's, an import's — and released on
        // return, before the part is given its pictures and its page, which may wait on another site.
        let _budget = PARSE_BUDGET
            .acquire_many(parse_cost_mib(source_path, bytes.len() as u64))
            .await
            .map_err(|e| HandlerError::Transient {
                message: format!("Could not start reading {source_path}: {e}"),
            })?;
        self.index_body(library, source_path, Body::Mesh(bytes), hash, origin, at)
            .await
    }

    /// A pack in the models folder (A13): hashed as it streams past, never read whole — a creator's
    /// release runs to gigabytes — and its contents read from the archive's index. Unchanged since it was
    /// indexed (the same size and modified time), it is not hashed again: a rescan of a folder of packs
    /// would otherwise read hundreds of gigabytes to learn nothing.
    ///
    /// `path` is where the file is under `root`: the part's own path in the models folder for a scan, the
    /// inbox path for an uploaded RAR.
    async fn ingest_pack(
        &self,
        library: LibraryId,
        source_path: &str,
        root: FileRoot,
        path: &str,
        origin: RevisionOrigin,
    ) -> Result<Outcome, HandlerError> {
        let dir = if root == FileRoot::Inbox {
            &self.inbox_dir
        } else {
            &self.ingest_dir
        };
        let stat = LibraryReader::open(dir)
            .stat_at(path)
            .map_err(|e| HandlerError::Transient {
                message: format!("Could not read {source_path} ({path}): {e}"),
            })?;
        let at = FileAt {
            root,
            path,
            mtime_ns: stat.mtime_ns,
        };
        if self
            .unchanged(library, source_path, root, path, &stat)
            .await?
        {
            return Ok(Outcome::Skipped);
        }
        let file = dir.join(path);
        let format = source_format(source_path);
        let (hash, size, contents) = tokio::task::spawn_blocking(move || {
            let mut hasher = blake3::Hasher::new();
            let size = hasher.update_reader(std::fs::File::open(&file)?)?.count();
            let contents = crate::archive::contents(&file, &format);
            Ok::<_, std::io::Error>((
                BlobHash::from_bytes(*hasher.finalize().as_bytes()),
                size,
                contents,
            ))
        })
        .await
        .map_err(|e| stopped(source_path, "Reading", &e))?
        .map_err(|e| HandlerError::Transient {
            message: format!("Could not read {source_path} in your folders: {e}"),
        })?;
        self.index_body(
            library,
            source_path,
            Body::Pack { size, contents },
            hash,
            origin,
            at,
        )
        .await
    }

    /// Whether the part at `source_path` already holds this file as it is now: its current revision is stored at
    /// `path` with this size and modification time. Then nothing is read (A13 for packs, A31 for meshes).
    async fn unchanged(
        &self,
        library: LibraryId,
        source_path: &str,
        root: FileRoot,
        path: &str,
        stat: &lapidary_storage::FileStat,
    ) -> Result<bool, HandlerError> {
        Ok(PgRevisions(self.db.clone())
            .current(library, source_path)
            .await
            .map_err(classify_db)?
            .is_some_and(|current| current.is_file(root, path, stat.size, stat.mtime_ns)))
    }

    async fn index_body(
        &self,
        library: LibraryId,
        source_path: &str,
        body: Body,
        hash: BlobHash,
        origin: RevisionOrigin,
        at: FileAt<'_>,
    ) -> Result<Outcome, HandlerError> {
        refuse_escaping(source_path)?;
        refuse_escaping(at.path)?;
        let blobs = PgBlobs(self.db.clone());
        let ingest = PgIngest(self.db.clone());
        let name = part_name(source_path);

        // 3. The same file, seen again -- same library, same path, the bytes its current
        // revision holds. Its modification time is brought up to date and nothing else happens.
        if blobs
            .library_holds(library, source_path, &hash)
            .await
            .map_err(classify_db)?
        {
            self.refresh_seen(library, source_path, at).await;
            return Ok(Outcome::Skipped);
        }

        // 3b. A part already at this path, with other bytes, becomes its next revision at 5a.
        let existing = PgRevisions(self.db.clone())
            .current(library, source_path)
            .await
            .map_err(classify_db)?;
        // The same bytes, committed by another job between step 3's question and this one.
        if existing
            .as_ref()
            .is_some_and(|existing| existing.source_hash == Some(hash))
        {
            self.refresh_seen(library, source_path, at).await;
            return Ok(Outcome::Skipped);
        }

        // 3a. The library has to exist. Read *after* the short-circuit, so a re-scan still
        // costs exactly one query.
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
        let format = source_format(source_path);
        refuse_b_rep(&format)?;
        if let Some(pause) = &self.pause {
            pause.reached.notify_one();
            pause.release.notified().await;
        }

        // 4. Parse + measure. Nothing has been written, so a failure needs no cleanup. A pack (A13) has
        // nothing to parse: no figures, no picture, and its contents instead.
        let (measurements, centre_of_mass_mm, embedded, size, contents) = match body {
            Body::Mesh(bytes) => {
                // Off the async threads, since a large mesh is seconds of work (A32), and in the order that holds
                // least at once: the thumbnail while the file's bytes are needed anyway, then the parse, then the bytes
                // go before the triangles are measured.
                let (fmt, path) = (format.clone(), source_path.to_owned());
                let (mesh, embedded, size) = tokio::task::spawn_blocking(move || {
                    let size = bytes.len() as u64;
                    // The picture a slicer saved inside a 3MF, attached once the part exists (F1). Never a
                    // reason to fail: a part with no picture is a part.
                    let embedded = if fmt == "3mf" {
                        lapidary_cad::thumbnail_3mf(&bytes).unwrap_or_else(|error| {
                            tracing::warn!(source_path = %path, %error, "could not read this 3MF's thumbnail; the part has no picture from it");
                            None
                        })
                    } else {
                        None
                    };
                    let mesh = lapidary_cad::parse(&bytes, &fmt);
                    drop(bytes);
                    let mesh = mesh.map(|mesh| lapidary_cad::measure(&mesh));
                    (mesh, embedded, size)
                })
                .await
                .map_err(|e| stopped(source_path, "Reading", &e))?;
                let (measurements, centre_of_mass_mm) = mesh.map_err(classify_cad)?;
                (Some(measurements), centre_of_mass_mm, embedded, size, None)
            }
            Body::Pack { size, contents } => (None, None, None, size, Some(contents)),
        };
        let blob = StoredBlobRow {
            hash,
            size_bytes: size,
            // No content-addressed copy is written, so the blob row claims none (`0026`).
            stored_bytes: size,
            zstd_level: 0,
        };

        // 5a. The next revision of the part at step 3b, with its file where it is now.
        if let Some(existing) = existing {
            let revision = PgRevisions(self.db.clone())
                .record_revision(RevisionRequest {
                    part: existing.part,
                    parent: existing.revision,
                    origin,
                    root: at.root,
                    storage_path: at.path,
                    mtime_ns: Some(at.mtime_ns),
                    blob: &blob,
                    measurements: measurements.as_ref(),
                    // Every figure a mesh gives is tessellated, and a mesh is all this library
                    // takes (R2).
                    provenance: MeasurementProvenance::TESSELLATED,
                    format: &format,
                })
                .await
                // Never `classify_write`: a unique violation here is a lost race for a label, and
                // the retry re-reads what is current and decides again.
                .map_err(|error| HandlerError::Transient {
                    message: error.to_string(),
                })?;
            self.record_centre_of_mass(
                revision,
                centre_of_mass_mm,
                Provenance::Tessellated,
                source_path,
            )
            .await;
            self.record_contents(revision, contents.as_ref(), source_path)
                .await;
            if let Some(raw) = embedded {
                self.adopt_picture(existing.part, &raw, "embedded", None, source_path)
                    .await;
            }
            return Ok(Outcome::Revised);
        }

        // 6. Is there already a `blob` row for these bytes? It decides which insert runs.
        let blob_row_exists = blobs.exists(&hash).await.map_err(classify_db)?;
        // 7. The category rows the file's directories imply. After the kernel, so a file that
        // will never parse creates no folder rows.
        let folder = self.folder_for(library, source_path).await?;
        let request = IngestRequest {
            // The route these bytes came by: `ingest` for a scan, `upload` for the browser.
            origin,
            library,
            name,
            source_path,
            folder,
            storage_path: Some(at.path),
            root: at.root,
            mtime_ns: Some(at.mtime_ns),
            blob: &blob,
            measurements: measurements.as_ref(),
            provenance: MeasurementProvenance::TESSELLATED,
            format: &format,
        };
        // 8. One transaction. A unique violation on the path is a race this job lost to another
        // job with the same path: with the same bytes it settles as `Skipped`, and with other
        // bytes it is decided again on the next attempt — against the winner's part, as its
        // next revision. Nothing was written, so there is nothing to undo either way.
        let written = if blob_row_exists {
            ingest.link_existing(request).await
        } else {
            ingest.record(request).await
        };
        let part = match written {
            Ok(part) => part,
            Err(error) => {
                let settled = classify_write(error);
                if settled.is_ok()
                    && !blobs
                        .library_holds(library, source_path, &hash)
                        .await
                        .map_err(classify_db)?
                {
                    return Err(HandlerError::Transient {
                        message: format!(
                            "Another job indexed other bytes at {source_path} first. This file is \
                             decided again against the part that job recorded."
                        ),
                    });
                }
                return settled;
            }
        };

        match PgParts(self.db.clone()).latest_revision(part).await {
            Ok(Some(revision)) => {
                self.record_centre_of_mass(
                    revision,
                    centre_of_mass_mm,
                    Provenance::Tessellated,
                    source_path,
                )
                .await;
                self.record_contents(revision, contents.as_ref(), source_path)
                    .await;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(
                %part,
                %error,
                "could not read back the revision just written; its centre of mass is not recorded"
            ),
        }
        if let Some(raw) = embedded {
            self.adopt_picture(part, &raw, "embedded", None, source_path)
                .await;
        }
        Ok(Outcome::Ingested)
    }

    /// A pack's contents onto the revision just committed (A13). Warn-only, as the centre of mass is: the
    /// pack is indexed, and a list it lacks leaves its page saying so.
    async fn record_contents(
        &self,
        revision: RevisionId,
        contents: Option<&serde_json::Value>,
        source_path: &str,
    ) {
        let Some(contents) = contents else { return };
        if let Err(error) = PgRevisions(self.db.clone())
            .set_contents(revision, contents)
            .await
        {
            tracing::warn!(source_path, %error, "could not record what this pack holds; its page lists nothing");
        }
    }

    /// `imports/<owner>`, where this library's imports are written (A38): one folder per person, as `uploads/` is, so
    /// a folder another person adds whole does not take them in.
    pub(crate) async fn imports_dir(&self, library: LibraryId) -> Result<String, HandlerError> {
        match PgParts(self.db.clone())
            .library_owner_name(library)
            .await
            .map_err(classify_db)?
        {
            Some(owner) => Ok(format!("imports/{owner}")),
            // A library nobody has claimed is one from before any account existed: there is nobody else to keep out.
            None => Ok("imports".to_owned()),
        }
    }

    /// Step 3's other half. Warn-only: the file is already indexed, and a stale time costs a
    /// download refusal that the next scan clears.
    async fn refresh_seen(&self, library: LibraryId, source_path: &str, at: FileAt<'_>) {
        if let Err(error) = PgIngest(self.db.clone())
            .refresh_seen(library, source_path, at.root, at.path, at.mtime_ns)
            .await
        {
            tracing::warn!(source_path, %error, "could not record the file's new modification time; its download may be refused until the next scan");
        }
    }

    /// A picture the application found — a 3MF's own thumbnail (`embedded`, F1) or a model page's
    /// (`url_supplied`, F3) — as the part's picture when it has none. Through the same
    /// `normalize` every upload takes, and stored the way the gallery stores one: on its row under
    /// the inline limit, in the store above.
    ///
    /// **A person's picture always wins.** One with no `source_url` — a thumbnail — goes only to a
    /// part with no picture (`adopt_part_image`); one from a page is appended after whatever the part
    /// has, once per URL (`add_page_image`), so a set grows and nothing a person chose moves.
    /// Warn-only throughout: the part is committed, and a picture it lacks is not a failure.
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
            match DerivativeStore::open(&self.blob_root).put(&image.webp) {
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
        let row = lapidary_db::NewPartImage {
            bytes: match &blob {
                Some(hash) => lapidary_db::ImageBytes::Blob(hash),
                None => lapidary_db::ImageBytes::Inline(&image.webp),
            },
            origin,
            source_url,
        };
        let parts = PgParts(self.db.clone());
        let adopted = match source_url {
            Some(_) => parts.add_page_image(part, row).await,
            None => parts.adopt_part_image(part, row).await,
        };
        if let Err(error) = adopted {
            tracing::warn!(source_path, %error, "could not attach a found picture; the part has no picture from it");
        }
    }

    /// The category this model is filed under: the folder rows its directories imply, created as
    /// needed. A category is the library's own grouping — it names no directory of Lapidary's.
    ///
    /// After the `library_holds` short-circuit, never during the walk: a re-scan of a directory
    /// whose models were all moved to other categories must not re-create the originals. And
    /// after the kernel, so a file that will never parse creates no folder rows.
    pub(crate) async fn folder_for(
        &self,
        library: LibraryId,
        source_path: &str,
    ) -> Result<Option<FolderId>, HandlerError> {
        // `get_or_create` is insert-then-select against a unique constraint, so two workers
        // racing the same directory is safe.
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
        Ok(parent)
    }
}

/// Remove derivative bytes a job wrote for a transaction that then failed. Best-effort and
/// warn-only: the database error is what the caller needs to hear about either way.
pub(crate) fn reap(derivatives: &DerivativeStore, hashes: &[BlobHash]) {
    for hash in hashes {
        if let Err(reap_err) = derivatives.remove(hash) {
            tracing::warn!(
                hash = %hash.to_hex(),
                error = %reap_err,
                "failed to reap a derivative blob after a failed write; it may now be an orphan on disk"
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

    /// A39: a reader that panics panics again on every attempt, so the job fails at once.
    #[tokio::test]
    async fn a_reader_that_panics_fails_the_job_rather_than_retrying_it() {
        let panicked = tokio::task::spawn_blocking(|| panic!("index out of bounds"))
            .await
            .expect_err("it panicked");
        assert!(matches!(
            stopped("broken.3mf", "Reading", &panicked),
            HandlerError::Permanent { .. }
        ));
    }

    #[test]
    fn a_meshs_parse_cost_follows_its_format_and_never_exceeds_the_budget() {
        const MIB: u64 = 1 << 20;
        assert_eq!(
            parse_cost_mib("brackets/bracket-lp-1042-03.stl", 10 * MIB),
            20
        );
        assert_eq!(
            parse_cost_mib("kits/Knight_Lancer.3mf", 174 * MIB),
            1044,
            "a 3MF inflates as it parses"
        );
        assert_eq!(
            parse_cost_mib("idler-bracket-lp-2210-01.obj", 1),
            1,
            "at least one"
        );
        assert_eq!(
            parse_cost_mib("Immortal.3mf", 400 * MIB),
            PARSE_BUDGET_MIB,
            "the largest runs alone rather than never"
        );
    }

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

    /// A lost race for one path is `Skipped`: the winner's row is committed and describes the same
    /// file. The end-to-end version is `losing_the_race_for_a_file_is_a_skip_rather_than_a_failure`;
    /// this pins the mapping with no database and no scheduler involved.
    #[test]
    fn only_a_lost_race_for_the_same_path_is_a_skip_rather_than_an_error() {
        let lost_race = classify_write(violation("part_source_path_unique_per_library"));
        assert_eq!(
            lost_race.as_ref().ok(),
            Some(&Outcome::Skipped),
            "another worker won the race for this path; its row is committed"
        );

        // Every other refusal is a failure. A unique
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
