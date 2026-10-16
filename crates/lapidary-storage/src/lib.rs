//! Where bytes are, and the three handles onto them (M4).
//!
//! A source file lives under one of three roots, and its `file` row says which
//! (`lapidary_core::FileRoot`):
//!
//! - **the owner's folders** — the ingest mount. Lapidary reads them where they are and never
//!   writes, renames or removes anything under them. [`LibraryReader`] is the only handle onto
//!   them, and it has no method that writes: that is the type half of the rule, and the `:ro`
//!   mount in `deploy/compose.yaml` is the backstop.
//! - **the inbox** — where an upload, a link import or a seeded example is written once by
//!   [`InboxWriter`], and then read like any other file. It is never under the owner's folders;
//!   the server refuses to start if it is configured there.
//! - **the store** — Lapidary's own blob root. Pictures, exports and imported bundles go here
//!   through [`DerivativeStore`]. Source files indexed before M4 are here too, and stay where
//!   they are: a path under the store, or `blobs/ab/cd/<hash>` zstd-compressed at the level
//!   their row recorded.
//!
//! Until M4 a scan copied every file into the store under `libraries/<slug>/<category>/<model>/`
//! and wrote a `metadata.json` beside it, a revision renamed the previous file into
//! `revisions/<label>/`, and a move renamed the model's directory. None of that exists any more:
//! a category is the library's own grouping, and moving a part between two moves nothing on disk.

pub mod images;

use lapidary_core::{BlobHash, FileRoot};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "No blob is stored for {hash_prefix}… . It may have been evicted from the render cache and removed after its 30-day hold. Source blobs are never removed while any part references them, so a missing source blob means the reference itself is stale."
    )]
    NotFound { hash_prefix: String },

    #[error(
        "Could not read or write {path}: {source}. Check the folder is mounted and readable (and, for Lapidary's own store and inbox, writable)."
    )]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "These bytes do not match the hash they were uploaded under: expected {expected}, got {actual}. The transfer was corrupted, or the file changed while it was being sent. Upload it again."
    )]
    HashMismatch { expected: String, actual: String },

    #[error("{detail}")]
    PathRefused { detail: String },
}

#[derive(Debug)]
pub struct StoredBlob {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

/// A file's size and modification time, as a download compares them against what was indexed.
///
/// Nanoseconds, never a `timestamptz`: Postgres keeps microseconds, the filesystem nanoseconds,
/// and a value rounded on the way in would differ from every stat taken on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    pub mtime_ns: i64,
}

impl FileStat {
    fn of(metadata: &std::fs::Metadata) -> Self {
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|since| i64::try_from(since.as_nanos()).ok())
            .unwrap_or(0);
        FileStat {
            size: metadata.len(),
            mtime_ns,
        }
    }
}

fn blob_path(root: &Path, hash: &BlobHash) -> PathBuf {
    let hex = hash.to_hex();
    root.join("blobs")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex)
}

/// Disambiguates temp file names within one process.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolve a root-relative path, refusing anything that would leave the root.
///
/// `Path::join` resolves nothing and refuses nothing, so every path that reaches a root from
/// *data* — a row's `storage_path`, a library's root, a path a browser reported — goes through
/// here. A path derived from a hash cannot escape and does not.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf, StorageError> {
    lapidary_core::slug::reject_escaping_path(rel).map_err(|e| StorageError::PathRefused {
        detail: e.to_string(),
    })?;
    Ok(root.join(rel))
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StorageError + '_ {
    move |source| StorageError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// A reader over `file`, decoding a zstd frame when the recorded level says it is one.
///
/// Every level other than 0 decodes, negatives included: zstd's `--fast=N` levels are spelled
/// as negative numbers and still produce a frame. `None` is the column's nullable absence and
/// reads as uncompressed. Only a `store` row from before M4 has anything but 0.
fn decoded(
    file: std::fs::File,
    zstd_level: Option<i16>,
    path: &Path,
) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
    let reader = std::io::BufReader::new(file);
    if zstd_level.is_some_and(|level| level != 0) {
        Ok(Box::new(
            zstd::stream::read::Decoder::new(reader).map_err(io(path))?,
        ))
    } else {
        Ok(Box::new(reader))
    }
}

/// Distinguish "no blob at this path" from every other I/O failure: only a real `NotFound` from
/// the OS becomes `StorageError::NotFound`, and a permissions error stays an `Io` naming the path.
fn classify_read_error(source: std::io::Error, hash: &BlobHash, path: &Path) -> StorageError {
    if source.kind() == std::io::ErrorKind::NotFound {
        StorageError::NotFound {
            hash_prefix: hash.to_hex()[..8].to_owned(),
        }
    } else {
        StorageError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

fn read_all(mut reader: impl std::io::Read, path: &Path) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out).map_err(io(path))?;
    Ok(out)
}

/// A uniquely-named temp file in `dir`, for a write that is renamed or linked into place.
fn temp_in(dir: &Path, name: &Path) -> PathBuf {
    dir.join(format!(
        ".{}.tmp-{}-{}",
        name.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("blob"),
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Fill a temp file beside `path`, `sync_all` it, and return its path, the hash `fill` reported
/// and the length written. The caller decides how it is put in place — a rename that replaces,
/// or a hard link that refuses to — and removes the temp file on every path.
///
/// Same directory, so the rename or link is one filesystem operation: a temp file in the system
/// temp directory could be on another filesystem, where a rename degrades to a copy.
fn stage(
    path: &Path,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<BlobHash>,
) -> Result<(PathBuf, BlobHash, u64), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::PathRefused {
        detail: format!("{} names no directory to write into", path.display()),
    })?;
    std::fs::create_dir_all(parent).map_err(io(parent))?;
    let tmp = temp_in(parent, path);
    let written = (|| -> std::io::Result<(BlobHash, u64)> {
        let mut file = std::fs::File::create(&tmp)?;
        let hash = fill(&mut file)?;
        file.sync_all()?;
        Ok((hash, file.metadata()?.len()))
    })();
    match written {
        Ok((hash, len)) => Ok((tmp, hash, len)),
        Err(source) => {
            let _ = std::fs::remove_file(&tmp);
            Err(StorageError::Io {
                path: tmp.display().to_string(),
                source,
            })
        }
    }
}

/// The rename or link that placed a file has to be durable, not only its bytes. Best-effort:
/// a filesystem that refuses a directory handle has still performed the operation.
fn sync_dir(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
}

/// Copy `from` through a hasher into `file`, returning the hash of what was copied.
fn copy_hashing(from: &Path, file: &mut std::fs::File) -> std::io::Result<BlobHash> {
    let mut reader = HashingReader {
        inner: std::io::BufReader::new(std::fs::File::open(from)?),
        hasher: blake3::Hasher::new(),
    };
    std::io::copy(&mut reader, file)?;
    Ok(BlobHash::from_bytes(*reader.hasher.finalize().as_bytes()))
}

/// Hashes what passes through it, so a copy is verified on the one pass that makes it.
struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
}

impl<R: std::io::Read> std::io::Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.hasher.update(&buf[..read]);
        Ok(read)
    }
}

fn hash_of(bytes: &[u8]) -> BlobHash {
    BlobHash::from_bytes(*blake3::hash(bytes).as_bytes())
}

/// Put a staged temp file at `path`: a rename that replaces when `replace`, else a hard link
/// that fails with `AlreadyExists` when the name is taken. The temp file is gone afterwards
/// either way.
fn place(tmp: &Path, path: &Path, replace: bool) -> Result<(), std::io::Error> {
    let placed = if replace {
        std::fs::rename(tmp, path)
    } else {
        // ponytail: a filesystem without hard links (FAT, exFAT, some network mounts) gets a
        // check then a rename, which narrows the race rather than closing it;
        // `renameat2(RENAME_NOREPLACE)` closes it.
        std::fs::hard_link(tmp, path).or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists || path.exists() {
                Err(std::io::ErrorKind::AlreadyExists.into())
            } else {
                std::fs::rename(tmp, path)
            }
        })
    };
    let _ = std::fs::remove_file(tmp);
    if placed.is_ok() {
        sync_dir(path);
    }
    placed
}

/// Lapidary's own content-addressed store: derivatives (exports), pictures over the inline
/// limit, and imported bundles. Never compressed, freely evictable, readable by every route.
pub struct DerivativeStore {
    root: PathBuf,
}

impl DerivativeStore {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// `bytes` at `blobs/ab/cd/<hash>`. Bytes already there are correct by construction, so
    /// nothing is written again — which is also what keeps two workers storing the same blob
    /// from racing a truncate.
    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError> {
        let hash = hash_of(bytes);
        let path = blob_path(&self.root, &hash);
        if let Ok(existing) = std::fs::metadata(&path) {
            return Ok(StoredBlob {
                hash,
                size_bytes: bytes.len() as u64,
                stored_bytes: existing.len(),
                zstd_level: 0,
            });
        }
        let (tmp, _, len) = stage(&path, |file| {
            std::io::Write::write_all(file, bytes)?;
            Ok(hash)
        })?;
        place(&tmp, &path, true).map_err(io(&path))?;
        Ok(StoredBlob {
            hash,
            size_bytes: bytes.len() as u64,
            stored_bytes: len,
            zstd_level: 0,
        })
    }

    /// A staged file into the store under the hash it was uploaded as, refused if the bytes
    /// disagree — an imported bundle, which is Lapidary's own to keep. One pass: the bytes are
    /// hashed on their way into the temp file, and a mismatch stores nothing.
    pub fn put_file(&self, staged: &Path, expect: &BlobHash) -> Result<StoredBlob, StorageError> {
        let path = blob_path(&self.root, expect);
        let (tmp, actual, len) = stage(&path, |file| copy_hashing(staged, file))?;
        if actual != *expect {
            let _ = std::fs::remove_file(&tmp);
            return Err(StorageError::HashMismatch {
                expected: expect.to_hex(),
                actual: actual.to_hex(),
            });
        }
        place(&tmp, &path, true).map_err(io(&path))?;
        Ok(StoredBlob {
            hash: *expect,
            size_bytes: len,
            stored_bytes: len,
            zstd_level: 0,
        })
    }

    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError> {
        LibraryReader::open(&self.root).get(hash, Some(0))
    }

    /// Reap a blob the reaper found unreferenced after its hold, or one written for a
    /// transaction that then failed. A missing file is success.
    pub fn remove(&self, hash: &BlobHash) -> Result<(), StorageError> {
        let path = blob_path(&self.root, hash);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }
}

/// Read-only access to one root: the owner's folders, the inbox, or the store.
///
/// **It has no method that writes, renames or removes, and must never grow one.** It is the only
/// handle in the workspace that reaches the owner's folders, so "Lapidary never writes there" is a
/// fact about this type rather than a promise each caller keeps.
pub struct LibraryReader {
    root: PathBuf,
}

impl LibraryReader {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The whole file, and its size and modification time taken from the same open handle — so
    /// the stat describes the bytes that were read, not a file replaced in between.
    pub fn read_at(&self, rel: &str) -> Result<(Vec<u8>, FileStat), StorageError> {
        let path = resolve(&self.root, rel)?;
        let file = std::fs::File::open(&path).map_err(io(&path))?;
        let stat = FileStat::of(&file.metadata().map_err(io(&path))?);
        let bytes = read_all(file, &path)?;
        Ok((bytes, stat))
    }

    /// The file's size and modification time now.
    pub fn stat_at(&self, rel: &str) -> Result<FileStat, StorageError> {
        let path = resolve(&self.root, rel)?;
        Ok(FileStat::of(&std::fs::metadata(&path).map_err(io(&path))?))
    }

    /// The file at `rel`, decoded per `zstd_level` — 0 for everything but a pre-M4 store row.
    pub fn get_at(&self, rel: &str, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
        let path = resolve(&self.root, rel)?;
        read_all(self.stream_at(rel, zstd_level)?, &path)
    }

    /// The same bytes as [`get_at`](Self::get_at), as a reader that holds no full copy: the
    /// download route must not hold a 380 MB STL in memory to send it.
    pub fn stream_at(
        &self,
        rel: &str,
        zstd_level: Option<i16>,
    ) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
        let path = resolve(&self.root, rel)?;
        let file = std::fs::File::open(&path).map_err(io(&path))?;
        decoded(file, zstd_level, &path)
    }

    /// A content-addressed blob, `blobs/ab/cd/<hash>` — a store row from before the store was a
    /// folder tree, or a bundle. Decoded per the level its row recorded.
    pub fn get(&self, hash: &BlobHash, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
        let path = blob_path(&self.root, hash);
        read_all(self.stream(hash, zstd_level)?, &path)
    }

    /// The same bytes as [`get`](Self::get), streamed.
    pub fn stream(
        &self,
        hash: &BlobHash,
        zstd_level: Option<i16>,
    ) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
        let path = blob_path(&self.root, hash);
        let file = std::fs::File::open(&path)
            .map_err(|source| classify_read_error(source, hash, &path))?;
        decoded(file, zstd_level, &path)
    }
}

/// The three directories a `file` row's `storage_path` can be relative to.
#[derive(Debug, Clone)]
pub struct Roots {
    /// The ingest mount: the owner's folders. Read only.
    pub library: PathBuf,
    /// Where uploads, link imports and the seeded examples are written once.
    pub inbox: PathBuf,
    /// Lapidary's own blob root.
    pub store: PathBuf,
}

impl Roots {
    pub fn dir(&self, root: FileRoot) -> &Path {
        match root {
            FileRoot::Library => &self.library,
            FileRoot::Inbox => &self.inbox,
            FileRoot::Store => &self.store,
        }
    }

    pub fn reader(&self, root: FileRoot) -> LibraryReader {
        LibraryReader::open(self.dir(root))
    }

    /// A source file as its row names it: at `storage_path` under `root`, or — a store row from
    /// before the folder tree, whose path is NULL — at `blobs/ab/cd/<hash>`.
    pub fn open(
        &self,
        root: FileRoot,
        storage_path: Option<&str>,
        hash: &BlobHash,
        zstd_level: Option<i16>,
    ) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
        match storage_path {
            Some(rel) => self.reader(root).stream_at(rel, zstd_level),
            None => self.reader(FileRoot::Store).stream(hash, zstd_level),
        }
    }

    /// [`open`](Self::open), read whole.
    pub fn get(
        &self,
        root: FileRoot,
        storage_path: Option<&str>,
        hash: &BlobHash,
        zstd_level: Option<i16>,
    ) -> Result<Vec<u8>, StorageError> {
        let named = self.dir(root).join(storage_path.unwrap_or("blobs"));
        read_all(self.open(root, storage_path, hash, zstd_level)?, &named)
    }
}

/// The one writer of source files, and it writes only into the inbox (M4).
///
/// An upload, a link import and a seeded example are written here once and then indexed like any
/// other file. The inbox is configured apart from the owner's folders and the server refuses to
/// start when it is inside them, so nothing this writes can land in a folder the owner keeps.
pub struct InboxWriter {
    root: PathBuf,
}

/// Where [`InboxWriter`] put a file, and what it measured there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Inbox-relative, `/`-separated.
    pub path: String,
    pub stat: FileStat,
}

/// How many other names an upload tries before it gives up: `name (2).stl` to `name (999).stl`.
const MAX_ALTERNATIVES: u32 = 999;

impl InboxWriter {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// A staged upload into the inbox at `rel`, or — when a file is already there — at the first
    /// free `name (2).ext`, `name (3).ext`… beside it. **Never over a file already there**: an
    /// earlier upload's row names it.
    ///
    /// Copied rather than renamed, because the staging volume and the inbox are two mounts and a
    /// rename across them fails. Hashed on the one pass that copies, and refused — nothing placed —
    /// when the bytes are not the ones the browser promised.
    pub fn put_new_file(
        &self,
        rel: &str,
        staged: &Path,
        expect: &BlobHash,
    ) -> Result<Placed, StorageError> {
        let first = resolve(&self.root, rel)?;
        let (tmp, actual, _) = stage(&first, |file| copy_hashing(staged, file))?;
        if actual != *expect {
            let _ = std::fs::remove_file(&tmp);
            return Err(StorageError::HashMismatch {
                expected: expect.to_hex(),
                actual: actual.to_hex(),
            });
        }
        for n in 1..=MAX_ALTERNATIVES {
            let candidate = if n == 1 {
                rel.to_owned()
            } else {
                alternative(rel, n)
            };
            let path = resolve(&self.root, &candidate)?;
            if std::fs::hard_link(&tmp, &path).is_ok() {
                let _ = std::fs::remove_file(&tmp);
                sync_dir(&path);
                return Ok(Placed {
                    path: candidate,
                    stat: stat(&path)?,
                });
            }
            if path.exists() {
                continue;
            }
            // No hard links on this filesystem: `place`'s fallback, with its ceiling.
            return match place(&tmp, &path, false) {
                Ok(()) => Ok(Placed {
                    path: candidate,
                    stat: stat(&path)?,
                }),
                Err(source) => Err(StorageError::Io {
                    path: path.display().to_string(),
                    source,
                }),
            };
        }
        let _ = std::fs::remove_file(&tmp);
        Err(StorageError::PathRefused {
            detail: format!(
                "The inbox already holds {rel} and {MAX_ALTERNATIVES} other files named after it, \
                 so this upload was not written. Rename the file and upload it again."
            ),
        })
    }

    /// `bytes` at `rel` in Lapidary's own part of the inbox — a link import, an imported bundle's
    /// part or a seeded example — replacing what is there. The same bytes already there are left
    /// alone, so a re-import of an unchanged model keeps its file, and its recorded modification
    /// time, as they were.
    pub fn put(&self, rel: &str, bytes: &[u8]) -> Result<Placed, StorageError> {
        let path = resolve(&self.root, rel)?;
        let unchanged = std::fs::read(&path)
            .map(|there| hash_of(&there) == hash_of(bytes))
            .unwrap_or(false);
        if !unchanged {
            let (tmp, _, _) = stage(&path, |file| {
                std::io::Write::write_all(file, bytes)?;
                Ok(hash_of(bytes))
            })?;
            place(&tmp, &path, true).map_err(io(&path))?;
        }
        Ok(Placed {
            path: rel.to_owned(),
            stat: stat(&path)?,
        })
    }
}

fn stat(path: &Path) -> Result<FileStat, StorageError> {
    Ok(FileStat::of(&std::fs::metadata(path).map_err(io(path))?))
}

/// `uploads/brackets/bracket.stl` as `uploads/brackets/bracket (n).stl`.
fn alternative(rel: &str, n: u32) -> String {
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let named = format!("{stem} ({n}){ext}");
    if dir.is_empty() {
        named
    } else {
        format!("{dir}/{named}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for (rel, bytes) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, bytes).expect("write");
        }
        dir
    }

    #[test]
    fn a_reader_reads_a_file_where_it_is_with_the_stat_of_what_it_read() {
        let dir = tree(&[("Brackets/steel/LP-1042-03.stl", b"solid bracket\n")]);
        let reader = LibraryReader::open(dir.path());
        let (bytes, stat) = reader
            .read_at("Brackets/steel/LP-1042-03.stl")
            .expect("reads");
        assert_eq!(bytes, b"solid bracket\n");
        assert_eq!(stat.size, 14);
        assert_eq!(
            stat,
            reader
                .stat_at("Brackets/steel/LP-1042-03.stl")
                .expect("stats"),
            "nothing changed between the two, so they agree to the nanosecond"
        );
    }

    #[test]
    fn a_reader_cannot_be_talked_out_of_its_root_by_a_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let reader = LibraryReader::open(dir.path());
        for escaping in ["/etc/passwd", "../../etc/passwd"] {
            assert!(matches!(
                reader.get_at(escaping, None),
                Err(StorageError::PathRefused { .. })
            ));
            assert!(matches!(
                reader.read_at(escaping),
                Err(StorageError::PathRefused { .. })
            ));
        }
    }

    /// A store row from before M4 may be a zstd frame, at a path or at its hash, and still
    /// downloads: the recorded level decides, a negative one included.
    #[test]
    fn a_compressed_store_file_still_reads_back_by_its_recorded_level() {
        let stl = "facet normal 0 0 1\nvertex 12.0 4.5 0.0\n"
            .repeat(512)
            .into_bytes();
        let frame = zstd::encode_all(stl.as_slice(), 3).expect("compresses");
        let hash = hash_of(&stl);
        let hex = hash.to_hex();
        let by_hash = format!("blobs/{}/{}/{hex}", &hex[0..2], &hex[2..4]);
        let dir = tree(&[
            ("libraries/default/Bases/round-32mm/round-32mm.stl", &frame),
            (&by_hash, &frame),
            (
                "libraries/default/Terrain/cliff/cliff.stl",
                b"solid cliff\n",
            ),
        ]);
        let store = LibraryReader::open(dir.path());
        assert_eq!(
            store
                .get_at("libraries/default/Bases/round-32mm/round-32mm.stl", Some(3))
                .expect("decodes"),
            stl
        );
        assert_eq!(store.get(&hash, Some(3)).expect("decodes"), stl);
        assert_eq!(
            store
                .get_at("libraries/default/Terrain/cliff/cliff.stl", None)
                .expect("a null level reads as is"),
            b"solid cliff\n"
        );
        assert!(
            store
                .get_at("libraries/default/Terrain/cliff/cliff.stl", Some(-3))
                .is_err(),
            "a negative level is a frame too, and these bytes are not one"
        );
    }

    #[test]
    fn getting_an_unknown_hash_says_which_hash() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = BlobHash::from_bytes([0x11; 32]);
        let msg = LibraryReader::open(dir.path())
            .get(&missing, None)
            .expect_err("must fail")
            .to_string();
        assert!(
            msg.contains(&missing.to_hex()[..8]),
            "names the hash: {msg}"
        );
    }

    #[test]
    fn derivatives_round_trip_uncompressed_and_are_written_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let d = DerivativeStore::open(dir.path());
        let compressible = "solid ".repeat(4096).into_bytes();
        let stored = d.put(&compressible).expect("put");
        assert_eq!(stored.stored_bytes, stored.size_bytes);
        assert_eq!(stored.zstd_level, 0);
        assert_eq!(d.get(&stored.hash).expect("get"), compressible);
        let hex = stored.hash.to_hex();
        assert!(
            dir.path()
                .join("blobs")
                .join(&hex[0..2])
                .join(&hex[2..4])
                .join(&hex)
                .exists()
        );
        d.remove(&stored.hash).expect("remove");
        assert!(d.get(&stored.hash).is_err());
    }

    #[test]
    fn a_staged_bundle_stores_under_its_hash_and_a_false_claim_stores_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let staged = dir.path().join("staged.part");
        std::fs::write(&staged, b"PK\x03\x04 workshop bundle").expect("stage");
        let d = DerivativeStore::open(dir.path());
        let hash = hash_of(b"PK\x03\x04 workshop bundle");
        assert_eq!(d.put_file(&staged, &hash).expect("stores").hash, hash);
        let claimed = hash_of(b"something else");
        assert!(matches!(
            d.put_file(&staged, &claimed),
            Err(StorageError::HashMismatch { .. })
        ));
        assert!(d.get(&claimed).is_err(), "the claimed hash names nothing");
    }

    #[test]
    fn an_upload_never_lands_on_a_file_already_in_the_inbox() {
        let inbox = tree(&[("uploads/brackets/bracket.stl", b"an earlier upload")]);
        let staging = tempfile::tempdir().expect("temp dir");
        let staged = staging.path().join("x.part");
        std::fs::write(&staged, b"solid bracket v2\n").expect("stage");
        let writer = InboxWriter::open(inbox.path());
        let v2 = hash_of(b"solid bracket v2\n");
        let placed = writer
            .put_new_file("uploads/brackets/bracket.stl", &staged, &v2)
            .expect("placed");
        assert_eq!(placed.path, "uploads/brackets/bracket (2).stl");
        assert_eq!(placed.stat.size, 17);
        assert_eq!(
            std::fs::read(inbox.path().join("uploads/brackets/bracket.stl")).expect("read"),
            b"an earlier upload",
            "the earlier file is untouched"
        );
        let again = writer
            .put_new_file("uploads/brackets/bracket.stl", &staged, &v2)
            .expect("placed");
        assert_eq!(again.path, "uploads/brackets/bracket (3).stl");
        assert!(matches!(
            writer.put_new_file("uploads/x.stl", &staged, &hash_of(b"not these")),
            Err(StorageError::HashMismatch { .. })
        ));
        assert!(!inbox.path().join("uploads/x.stl").exists());
        assert!(matches!(
            writer.put_new_file("../outside.stl", &staged, &v2),
            Err(StorageError::PathRefused { .. })
        ));
    }

    #[test]
    fn an_import_replaces_only_bytes_that_changed() {
        let inbox = tempfile::tempdir().expect("temp dir");
        let writer = InboxWriter::open(inbox.path());
        let rel = "imports/printables/someone/throttle-stop/stop.stl";
        let first = writer.put(rel, b"solid stop\n").expect("writes");
        let same = writer.put(rel, b"solid stop\n").expect("writes");
        assert_eq!(
            first, same,
            "the same bytes are left as they were, mtime and all"
        );
        let changed = writer.put(rel, b"solid stop v2\n").expect("writes");
        assert_eq!(changed.stat.size, 14);
        assert_eq!(
            std::fs::read(inbox.path().join(rel)).expect("read"),
            b"solid stop v2\n"
        );
    }

    #[test]
    fn an_alternative_name_keeps_the_folder_and_the_extension() {
        assert_eq!(alternative("uploads/a/b.stl", 2), "uploads/a/b (2).stl");
        assert_eq!(alternative("b.3mf", 3), "b (3).3mf");
        assert_eq!(alternative("uploads/README", 2), "uploads/README (2)");
        assert_eq!(alternative("uploads/.hidden", 2), "uploads/.hidden (2)");
    }
}
