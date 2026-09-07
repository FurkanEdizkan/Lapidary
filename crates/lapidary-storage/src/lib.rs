//! Content-addressed blob storage. Four handles, deliberately:
//!
//! `DerivativeStore` reads and writes derivatives — thumbnails, tessellations — and both
//! roles hold one. `SourceStore` reaches the ingested source bytes and requires a
//! `WorkerRole` token to construct. `SourceReader` reads those same bytes and can do
//! nothing else — its own doc says why that is not a hole in the rule below.
//! `SourceRelocator` moves a source directory and touches no content at all — its own doc
//! says why a rename earns a fourth handle rather than reusing one of the other three.
//!
//! This is the type half of "the **open** path never touches a source file" — a rule
//! about *opening*: the grid, the viewer, the detail card, the interactive path that
//! must not parse a STEP file to draw a thumbnail. It is not a rule about which process
//! holds the bytes; `deploy/compose.yaml` mounts the blob volume on `api` already. The
//! dependency-graph half cannot express it on its own — `lapidary-api` legitimately
//! depends on this crate for `DerivativeStore`, so the distinction is *which type*, not
//! whether the crates may be connected — so `cargo xtask check-deploy` asserts both
//! halves as a textual backstop against the mistake of importing either type there:
//! `lapidary-api` never names `SourceStore` at all, and names `SourceReader` only in
//! `crates/lapidary-api/src/download.rs`. The crate that actually needs `SourceStore` is
//! `lapidary-ingest`, not `lapidary-api`: ingest was tried as a role-gated route inside
//! `lapidary-api` first, and moved out once it became clear that `lapidary-api` depending on
//! `lapidary-cad` at all — regardless of which routes ever ran — made the `api`
//! container image link the kernel again. See `docs/ARCHITECTURE.md`'s crate graph.

use lapidary_core::BlobHash;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "No blob is stored for {hash_prefix}… . It may have been evicted from the render cache, or quarantined and removed after its 30-day hold. Source blobs are never removed while any part references them, so a missing source blob means the reference itself is stale."
    )]
    NotFound { hash_prefix: String },

    #[error(
        "Could not read or write the blob store at {path}: {source}. Check the volume is mounted and writable."
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

/// Proof the holder is running in the worker role. Zero-sized and unconstructible except
/// through `assume`.
pub struct WorkerRole(());

impl WorkerRole {
    /// Called from `lapidary-ingest`'s scan handler, once per request — the only code in
    /// the workspace that runs solely under `LAPIDARY_ROLE=worker` and is allowed to
    /// reach a source file at all. Nothing checks the process's actual role at the call
    /// site itself: the proof is that this code is reachable, which is true only because
    /// `lapidary-api` (the open path) cannot depend on `lapidary-cad` or construct a
    /// `SourceStore` — `xtask/src/layers.rs`'s `FORBIDDEN_PAIRS` and
    /// `xtask/src/deploy.rs`'s `check_open_path_boundary` enforce that structurally.
    pub fn assume() -> Self {
        WorkerRole(())
    }
}

#[derive(Debug)]
pub struct StoredBlob {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

/// zstd -3 at ingest per DATA.md §1.2. -19 when cold is a later tiering job.
const INGEST_LEVEL: i32 = 3;

fn blob_path(root: &Path, hash: &BlobHash) -> PathBuf {
    let hex = hash.to_hex();
    root.join("blobs")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex)
}

/// Disambiguates temp file names within one process. A `put` never blocks on another —
/// each attempt gets its own counter value, so two writers racing to store the same blob
/// never collide on the temp file, only (harmlessly) on which one's rename wins.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolve a store-relative path, refusing anything that would leave the root.
///
/// `Path::join` resolves nothing and refuses nothing, so every path that reaches this
/// store from *data* — a library slug, a category tree, a slugified part name — goes
/// through here. A path derived from a hash cannot escape and does not.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf, StorageError> {
    lapidary_core::slug::reject_escaping_path(rel).map_err(|e| StorageError::PathRefused {
        detail: e.to_string(),
    })?;
    Ok(root.join(rel))
}

/// Read bytes back from a store-relative path, decoding per `zstd_level` exactly as
/// [`read_blob`] does for a hash-addressed one. Shared by [`SourceStore::get_at`] (the
/// write side) and [`SourceReader::get_at`] (the read-only side) so the two cannot drift
/// into different answers for the same row — see [`SourceReader::get`]'s doc for why the
/// level is always the recorded one and never re-derived.
fn read_blob_at(root: &Path, rel: &str, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
    let path = resolve(root, rel)?;
    let raw = std::fs::read(&path).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;
    if zstd_level.is_some_and(|level| level != 0) {
        zstd::decode_all(raw.as_slice()).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })
    } else {
        Ok(raw)
    }
}

/// Open a store-relative path as a reader, decoding per `zstd_level` exactly as
/// [`read_blob_at`] reads one — the streaming twin of it, and the path-addressed twin of
/// [`open_blob`]. Both legs need both shapes for the same reason: the download route must
/// not hold a whole source file in memory, and a model file is exactly as large as the
/// blob it replaced.
///
/// A missing file is a plain [`StorageError::Io`] and not `NotFound`, matching
/// [`read_blob_at`]: `NotFound` carries a hash prefix, and the caller that reaches here
/// asked by path.
fn open_blob_at(
    root: &Path,
    rel: &str,
    zstd_level: Option<i16>,
) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
    let path = resolve(root, rel)?;
    let file = std::fs::File::open(&path).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let reader = std::io::BufReader::new(file);
    if zstd_level.is_some_and(|level| level != 0) {
        let decoder =
            zstd::stream::read::Decoder::new(reader).map_err(|source| StorageError::Io {
                path: path.display().to_string(),
                source,
            })?;
        Ok(Box::new(decoder))
    } else {
        Ok(Box::new(reader))
    }
}

/// Put `payload` at `path`, creating the directories above it: a uniquely-named temp file
/// in the *same* directory, flushed to disk, then an atomic rename.
///
/// A same-directory rename is atomic on POSIX filesystems — readers see either the old
/// state or the complete new file, never a partial write. A temp file in a different
/// directory (e.g. the system temp dir) could sit on a different filesystem, where rename
/// degrades to a copy and loses that guarantee.
///
/// `sync_all` before the rename, because the whole ingest ordering rests on the bytes
/// being on disk before a row points at them: a rename that lands before its contents are
/// durable turns a power cut into a `file` row naming an empty file. One discipline, both
/// callers — the content-addressed [`write_blob`] and the path-addressed
/// [`SourceStore::put_at`] — so neither can drift into a weaker one.
fn write_atomic(path: &Path, payload: &[u8]) -> Result<(), StorageError> {
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| StorageError::PathRefused {
        detail: format!("{} names no directory to write into", path.display()),
    })?;
    std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("blob"),
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(payload)?;
        file.sync_all()
    };
    if let Err(source) = write() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(StorageError::Io {
            path: tmp_path.display().to_string(),
            source,
        });
    }
    if let Err(source) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(StorageError::Io {
            path: path.display().to_string(),
            source,
        });
    }

    // The rename itself has to be durable, not only the bytes it renames. `sync_all` above
    // puts the temp file's CONTENTS on disk; the directory entry that gives them their real
    // name is a separate write, and a power cut between the two loses the file while the
    // `file` row naming it survives — the exact failure this whole write-then-record
    // ordering exists to prevent, one level up. The `migrate_storage` job is where it
    // stops being theoretical: it unlinks the content-addressed copy once the row points
    // at this one, so a lost rename there is the only copy.
    //
    // Best-effort, and it must be: Windows cannot open a directory as a file at all, and a
    // filesystem that refuses the handle has still performed the rename. There is no
    // portable way to assert this line from a test — no test here claims to.
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn write_blob(root: &Path, bytes: &[u8], compress: bool) -> Result<StoredBlob, StorageError> {
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let path = blob_path(root, &hash);

    // Content addressing means the bytes at `path` are already correct if it exists — the
    // hash is a function of the content, so writing again would be pure waste. It would
    // also be actively unsafe: two workers ingesting the same file compute the same hash
    // and race to write the same path. `std::fs::write` truncates before writing, so one
    // writer's truncate landing between another's writes leaves a zero-filled hole, and a
    // concurrent read can observe the file mid-truncation. Slice 2's job queue runs
    // multiple workers by design, and two overlapping scans can race today — so this is
    // reachable, not theoretical. Returning here, before doing any compression or I/O,
    // closes that hazard for the common case; the temp-file-then-rename below closes it
    // for the race on first write.
    match std::fs::metadata(&path) {
        Ok(existing) => {
            return Ok(StoredBlob {
                hash,
                size_bytes: bytes.len() as u64,
                // The on-disk size, not a freshly recomputed compressed length — nothing
                // was written on this call, so there is no fresh length to report.
                stored_bytes: existing.len(),
                zstd_level: if compress { INGEST_LEVEL as i16 } else { 0 },
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::Io {
                path: path.display().to_string(),
                source,
            });
        }
    }

    // Compressed straight into the temp file rather than into a `Vec` first. `encode_all`
    // allocated a second full copy of the file and held it alongside the caller's slice
    // for the whole write: on the 380 MB STL in the owner's corpus that is 380 MB plus
    // ~185 MB resident at once, inside a worker capped at 2 GB running two jobs. The
    // uncompressed branch had the same shape for no reason at all — `bytes.to_vec()`
    // copied the slice only to hand it straight to `write`.
    //
    // `stored_bytes` comes from the file rather than from a buffer's length, which is the
    // same number by a shorter route: it is what the blob actually occupies.
    //
    // Nothing to verify: the hash was computed from these exact bytes ten lines up.
    let stored_bytes = stage_and_rename(
        &path,
        |file| {
            if compress {
                zstd::stream::copy_encode(bytes, file, INGEST_LEVEL)?;
            } else {
                std::io::Write::write_all(file, bytes)?;
            }
            Ok(hash)
        },
        None,
    )?;

    Ok(StoredBlob {
        hash,
        size_bytes: bytes.len() as u64,
        stored_bytes,
        zstd_level: if compress { INGEST_LEVEL as i16 } else { 0 },
    })
}

/// Write into a uniquely-named temp file in the *same* directory as `path`, then rename
/// it into place. Both blob writers go through here.
///
/// A same-directory rename is atomic on POSIX filesystems — a reader sees either the old
/// state (nothing, since this is a new blob) or the complete new file, never a partial
/// write. A temp file in a different directory, the system temp dir being the obvious
/// mistake, could sit on a different filesystem, where rename degrades to a copy and
/// loses that guarantee.
///
/// `fill` writes the bytes and returns the hash they actually had. `expect` is the hash a
/// caller *promised* those bytes would have, checked here rather than by the caller
/// because the check has to land between the write and the rename: verifying afterwards
/// means a blob that failed verification was, for a moment, the blob at that path, and
/// verifying beforehand means reading the source twice. A mismatch removes the temp file
/// and stores nothing.
///
/// The temp file is named for the *destination* hash in both cases. For an unverified
/// write that is the hash of the bytes; for a verified one it is the claim, which is the
/// only path the bytes could ever land at, so a mismatch leaves no stray file anywhere
/// else. `TMP_COUNTER` disambiguates two writers racing the same blob, so they collide
/// only (harmlessly) on which rename wins.
fn stage_and_rename(
    path: &Path,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<BlobHash>,
    expect: Option<&BlobHash>,
) -> Result<u64, StorageError> {
    let parent = path.parent().unwrap_or(path);
    // Here and not in each caller. It was in each caller, and the merge that brought the
    // folder tree alongside the streaming write dropped one of the two copies -- every
    // derivative write then failed with `NotFound` on its own temp file, because the
    // fan-out directory it names had never been created. The staging is what needs the
    // directory, so the staging is what creates it.
    std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;
    let tmp_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("blob"),
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let written = (|| -> std::io::Result<(BlobHash, u64)> {
        let mut file = std::fs::File::create(&tmp_path)?;
        let actual = fill(&mut file)?;
        // Before the rename, so a reader that observes the renamed path observes complete
        // bytes rather than whatever the page cache had flushed.
        file.sync_all()?;
        let len = file.metadata()?.len();
        Ok((actual, len))
    })();

    let (actual, stored_bytes) = match written {
        Ok(pair) => pair,
        Err(source) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(StorageError::Io {
                path: tmp_path.display().to_string(),
                source,
            });
        }
    };

    if let Some(expected) = expect
        && actual != *expected
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(StorageError::HashMismatch {
            expected: expected.to_hex(),
            actual: actual.to_hex(),
        });
    }

    if let Err(source) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(StorageError::Io {
            path: path.display().to_string(),
            source,
        });
    }
    Ok(stored_bytes)
}

/// A file on disk into the blob store, hashing, compressing and writing in one pass.
///
/// The caller has bytes it did not produce — an upload — so it names the hash it was
/// promised and this refuses to store anything else. Reading the file to hash it and then
/// reading it again to compress it would be two passes over as much as 2 GB for a check
/// the single pass already makes.
///
/// `size_bytes` is the staged file's length, not a running count, because the two must
/// agree and the filesystem is the authority on one of them.
fn write_blob_from_file(
    root: &Path,
    staged: &Path,
    expect: &BlobHash,
    compress: bool,
) -> Result<StoredBlob, StorageError> {
    let path = blob_path(root, expect);

    let size_bytes = std::fs::metadata(staged)
        .map_err(|source| StorageError::Io {
            path: staged.display().to_string(),
            source,
        })?
        .len();

    // Unlike `write_blob`, an existing blob at this path is *not* an early return: the
    // claim has not been checked yet, and answering "already stored" to bytes that hash
    // to something else would let a client register any path against any blob it can
    // name. Content addressing is not authorization, and a claim is not a hash. The
    // rename below is a no-op overwrite in that case, of identical bytes.
    let stored_bytes = stage_and_rename(
        &path,
        |file| {
            let source = std::fs::File::open(staged)?;
            let mut reader = HashingReader {
                inner: std::io::BufReader::new(source),
                hasher: blake3::Hasher::new(),
            };
            if compress {
                zstd::stream::copy_encode(&mut reader, file, INGEST_LEVEL)?;
            } else {
                std::io::copy(&mut reader, file)?;
            }
            Ok(BlobHash::from_bytes(*reader.hasher.finalize().as_bytes()))
        },
        Some(expect),
    )?;

    Ok(StoredBlob {
        hash: *expect,
        size_bytes,
        stored_bytes,
        zstd_level: if compress { INGEST_LEVEL as i16 } else { 0 },
    })
}

/// Hashes what passes through it. The point is that the bytes are hashed on their way
/// into the compressor rather than on a separate pass over the same file.
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

/// The bytes as they go to disk. Borrowed when nothing compresses them — a source file is
/// up to 2 GiB and copying it to hand it to a writer is a copy nobody asked for.
fn compressed<'a>(
    bytes: &'a [u8],
    compress: bool,
    path: &Path,
) -> Result<std::borrow::Cow<'a, [u8]>, StorageError> {
    if !compress {
        return Ok(std::borrow::Cow::Borrowed(bytes));
    }
    let packed = zstd::encode_all(bytes, INGEST_LEVEL).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(std::borrow::Cow::Owned(packed))
}

/// Distinguish "no blob at this path" from every other I/O failure. Collapsing every
/// failure into `NotFound` sends whoever hits a permissions error, a bad mount, or a file
/// caught mid-rewrite off to hunt for a blob that is sitting right there with the wrong
/// mode bits. `lapidary-db` made exactly this mistake once — every `connect()` failure
/// mapped to `Unreachable`, so a wrong password read as an unreachable database — and now
/// classifies by SQLSTATE instead. Same fix, applied here: only a real `NotFound` from the
/// OS becomes `StorageError::NotFound`; everything else is `StorageError::Io`, which
/// already carries the path and the underlying error.
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

/// The whole blob, decompressed, in memory.
///
/// Decodes *from the file* rather than from a `Vec` of the file. The previous shape read
/// the compressed bytes whole and then decoded them into a second buffer, so both were
/// resident at the peak — 565 MB for the 380 MB STL in the owner's corpus, inside an `api`
/// container capped at 512 MB. That was not a slow path, it was an OOM on a real file.
///
/// This still holds the decompressed result whole, which is right for a caller that needs
/// the bytes (the kernel) and wrong for one that only forwards them. [`open_blob`] is the
/// forwarding case.
fn read_blob(root: &Path, hash: &BlobHash, compressed: bool) -> Result<Vec<u8>, StorageError> {
    let path = blob_path(root, hash);
    let file =
        std::fs::File::open(&path).map_err(|source| classify_read_error(source, hash, &path))?;
    let mut out = Vec::new();
    if compressed {
        zstd::stream::copy_decode(file, &mut out).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?;
    } else {
        std::io::Read::read_to_end(&mut std::io::BufReader::new(file), &mut out).map_err(
            |source| StorageError::Io {
                path: path.display().to_string(),
                source,
            },
        )?;
    }
    Ok(out)
}

/// The blob as a reader, decompressing as it is read, holding no full copy of anything.
///
/// For a caller that forwards bytes rather than inspecting them — today the download
/// route, whose memory used to scale with the file it served multiplied by the number of
/// people asking for it at once.
///
/// Blocking, deliberately: the callers that stream this hand it to `spawn_blocking`, and
/// an async decompressor would be a new dependency to avoid an ordinary thread.
fn open_blob(
    root: &Path,
    hash: &BlobHash,
    compressed: bool,
) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
    let path = blob_path(root, hash);
    let file =
        std::fs::File::open(&path).map_err(|source| classify_read_error(source, hash, &path))?;
    let reader = std::io::BufReader::new(file);
    if compressed {
        let decoder =
            zstd::stream::read::Decoder::new(reader).map_err(|source| StorageError::Io {
                path: path.display().to_string(),
                source,
            })?;
        Ok(Box::new(decoder))
    } else {
        Ok(Box::new(reader))
    }
}

/// Derivatives: never compressed (they are already packed and sit on the hot open path),
/// freely evictable, and readable by both roles.
pub struct DerivativeStore {
    root: PathBuf,
}

impl DerivativeStore {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, false)
    }

    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, false)
    }

    /// Reap a derivative written for a transaction that then failed, exactly as
    /// [`SourceStore::remove`] does for source bytes. The caller is responsible for only
    /// calling it on bytes this job created: a rung shared with another revision is bytes
    /// somebody else is still serving.
    pub fn remove(&self, hash: &BlobHash) -> Result<(), StorageError> {
        remove_blob(&self.root, hash)
    }
}

/// Whether a source blob is compressed on the way in.
///
/// `DATA.md` §1.2's table, in one place with its reasoning, rather than a boolean at each
/// call site where the next reader cannot tell what `true` meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Zstd,
    AsIs,
}

impl Compression {
    /// STEP, STL and OBJ compress 2–10×. 3MF is already a deflate ZIP, so re-compressing
    /// it spends CPU on every ingest to make the file very slightly larger.
    ///
    /// An unrecognised format compresses: that wastes a little CPU on something already
    /// packed, where the other default would waste disk on everything else.
    pub fn for_source_format(format: &str) -> Self {
        match format.to_ascii_lowercase().as_str() {
            "3mf" => Compression::AsIs,
            _ => Compression::Zstd,
        }
    }

    fn compresses(self) -> bool {
        matches!(self, Compression::Zstd)
    }
}

/// Source bytes: compressed hard, never deleted while referenced, and reachable only
/// from the worker role.
pub struct SourceStore {
    root: PathBuf,
}

impl SourceStore {
    pub fn open(root: &Path, _proof: &WorkerRole) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn put(&self, bytes: &[u8], compression: Compression) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, compression.compresses())
    }

    /// Write source bytes at a store-relative path — `libraries/<lib>/<category…>/<model>/
    /// cliff.stl`, where [`SourceStore::put`] writes `blobs/ab/cd/<hash>`. Ingest's writer
    /// since the store became a folder the user opens in a file manager.
    ///
    /// `rel` is built from data — a library slug, a category tree, a slugified part name —
    /// so it is refused if it would leave the root, and the bytes go down through the same
    /// temp-file-then-`sync_all`-then-rename discipline `put` uses. Nothing about the
    /// hash changed: the returned [`StoredBlob`] still carries it, because `file.blake3`
    /// and re-adoption both still verify bytes against it. Only the name they are written
    /// under is different.
    ///
    /// Unlike `put` there is no "already stored" short-circuit, and there cannot be: a
    /// path says nothing about the content at it, so the only way to know whether these
    /// bytes are there is to write them.
    pub fn put_at(
        &self,
        rel: &str,
        bytes: &[u8],
        compression: Compression,
    ) -> Result<StoredBlob, StorageError> {
        let path = resolve(&self.root, rel)?;
        let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
        let payload = compressed(bytes, compression.compresses(), &path)?;
        write_atomic(&path, &payload)?;
        Ok(StoredBlob {
            hash,
            size_bytes: bytes.len() as u64,
            stored_bytes: payload.len() as u64,
            zstd_level: if compression.compresses() {
                INGEST_LEVEL as i16
            } else {
                0
            },
        })
    }

    pub fn get(&self, hash: &BlobHash, compression: Compression) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, compression.compresses())
    }

    /// Read source bytes back from `file.storage_path`.
    ///
    /// `zstd_level` is the level recorded on the `blob` row, never
    /// `Compression::for_source_format` — the same rule, and the same reasoning,
    /// [`SourceReader::get`] states: ingest-time policy is a moving target, and a reader
    /// that re-derived it would hand out a zstd frame as though it were the file the day
    /// the policy moved. `None` is the column's nullable absence and reads as
    /// uncompressed, exactly like level 0.
    pub fn get_at(&self, rel: &str, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
        read_blob_at(&self.root, rel, zstd_level)
    }

    /// Reap a source file written for a transaction that then failed — the path-addressed
    /// half of [`SourceStore::remove`], and held to the same rule: this removes bytes no
    /// part ever referenced, never content a library member could see. A missing file is
    /// success, because the reap's job is that the bytes are not there afterwards.
    pub fn remove_at(&self, rel: &str) -> Result<(), StorageError> {
        let path = resolve(&self.root, rel)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    /// Remove a model directory the same failed write created, if nothing is left in it.
    ///
    /// Non-recursive on purpose. An empty directory left behind is an orphan a re-adoption
    /// walk reports and skips, and worse, one the next attempt at this same file walks
    /// around — it sees the directory, decides the name is taken, and disambiguates,
    /// so a transient database error would rename a model permanently. A directory that
    /// still holds something is a directory another writer is using, and a reap that
    /// removes one of those is the data loss reaping exists to prevent: `remove_dir`
    /// refuses it, and this reports rather than insists.
    pub fn remove_dir_if_empty(&self, rel: &str) -> Result<(), StorageError> {
        let path = resolve(&self.root, rel)?;
        match std::fs::remove_dir(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    /// Reap a blob written for a transaction that then failed. Not user-facing deletion —
    /// no part ever referenced these bytes, so this never touches anything a library
    /// member could see: it exists only to clean up after a failed ingest write, not to
    /// remove content anyone has stored.
    pub fn remove(&self, hash: &BlobHash) -> Result<(), StorageError> {
        remove_blob(&self.root, hash)
    }
}

/// Source bytes, read-only: no `put`, no `remove`, and no `WorkerRole` to construct.
///
/// It exists for `lapidary-api`'s download route, which hands a user the exact bytes they
/// asked for. That is not the open path — it parses nothing, draws nothing, and no route
/// that renders a part reads through here — and `CLAUDE.md`'s other rule, *"`variant=original`
/// returns byte-identical ingested bytes"*, cannot be satisfied without it.
///
/// Not `SourceStore`, because the alternatives were worse: gating this behind `WorkerRole`
/// means handing the api `put` and `remove` on source bytes to buy a read, and the write
/// surface is the half of that type worth spending a token on. Proxying the download
/// through the worker instead buys nothing — the api already has the bytes — and puts every
/// download behind the 2 GiB ceiling that exists so the worker can mesh. See
/// `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md` §1.2.
///
/// What keeps that from spreading: `xtask/src/deploy.rs`'s `check_open_path_boundary`
/// allows `lapidary-api` to name this type in `download.rs` and nowhere else. A read-only
/// handle in one named route is a decision; the same handle in six files is the mistake the
/// `SourceStore` grep exists to catch, arrived at by copy-paste.
pub struct SourceReader {
    root: PathBuf,
}

impl SourceReader {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// `zstd_level` is `blob.zstd_level` as stored, read from the same row that carried the
    /// hash — never `Compression::for_source_format`. That is ingest-time policy and slice 7
    /// is about to change it, so a reader that re-derived it would start handing out zstd
    /// frames as though they were the file the day the policy moved. `None` is the column's
    /// nullable absence (`0002_parts.sql`) and reads as uncompressed, exactly like level 0.
    ///
    /// Every other level decodes, negatives included: zstd's `--fast=N` levels are spelled
    /// as negative numbers and still produce a zstd *frame*, so a `> 0` test would hand a
    /// user a compressed frame as their file. Nothing writes one today — `Compression`
    /// cannot produce one — but slice 7's tiering is where level spellings get picked.
    pub fn get(&self, hash: &BlobHash, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, zstd_level.is_some_and(|level| level != 0))
    }

    /// The same bytes as [`get`](Self::get), as a reader that holds no full copy.
    ///
    /// The download route's memory used to be the size of the file it served, times the
    /// number of people asking at once, inside an `api` container capped at 512 MB — so a
    /// single 380 MB STL from the owner's corpus was an OOM rather than a slow request.
    ///
    /// The `zstd_level` argument means exactly what it means in `get`, including why a
    /// negative level still decodes.
    pub fn stream(
        &self,
        hash: &BlobHash,
        zstd_level: Option<i16>,
    ) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
        open_blob(&self.root, hash, zstd_level.is_some_and(|level| level != 0))
    }

    /// Read source bytes back from `file.storage_path` — the path-addressed twin of
    /// [`SourceReader::get`], for a row whose bytes have already migrated off the
    /// content-addressed store (migration `0009`). `rel` is resolved through the same
    /// [`reject_escaping_path`](lapidary_core::slug::reject_escaping_path) guard every
    /// other path-addressed method in this crate runs, because a `storage_path` reaching
    /// here came off a database row, not off a hash — data, in the sense `resolve`'s own
    /// doc means it.
    ///
    /// `zstd_level` is the same rule as `get`, restated because it is the rule this method
    /// exists to not get wrong: the recorded level off the same `blob` row that carried
    /// the hash, never re-derived from the format or from the fact that this is the
    /// path-addressed leg rather than the hash-addressed one. Ingest writes every
    /// path-addressed file as-is today (`handler.rs`), which makes `Some(0)` the only
    /// value this method sees in production — but the column does not promise that, and a
    /// reader that assumed it would be the thing that breaks first when it stops being
    /// true.
    pub fn get_at(&self, rel: &str, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError> {
        read_blob_at(&self.root, rel, zstd_level)
    }

    /// The same bytes as [`get_at`](Self::get_at), as a reader that holds no full copy —
    /// what [`stream`](Self::stream) is to [`get`](Self::get), for the path-addressed leg.
    ///
    /// Both legs stream or neither does. The download route picks between them on whether
    /// `file.storage_path` is set, and that column flips under a library as
    /// `migrate_storage` drains it: a route that streamed one and buffered the other would
    /// have the 512 MB api container survive or OOM on the same part depending on how far
    /// a background job had got.
    pub fn stream_at(
        &self,
        rel: &str,
        zstd_level: Option<i16>,
    ) -> Result<Box<dyn std::io::Read + Send>, StorageError> {
        open_blob_at(&self.root, rel, zstd_level)
    }
}

/// Source bytes, write-only: no `get`, no `remove`, and no `WorkerRole` to construct.
///
/// The mirror of [`SourceReader`], and it exists for the mirror reason. `lapidary-api`'s
/// upload route holds bytes a user just handed it and has to put them somewhere; the
/// worker's `/ingest` mount is read-only, and staging them on a volume the worker also
/// mounts would buy a mount, a second root for `IngestFile` to join against, and the
/// failure mode of the worker opening a file the api has not finished writing — all to
/// move bytes the api is already sitting on. `deploy/compose.yaml` mounts the blob volume
/// on `api` read-write today, deliberately, because the boundary is a type and not a
/// mount flag.
///
/// Not `SourceStore`, for the reason `SourceReader` is not: gating this behind
/// `WorkerRole` would hand the api `get` and `remove` on every source blob in the store in
/// order to buy a single `put`. Read is the half worth spending a token on when the caller
/// already holds the bytes in its own request body.
///
/// What keeps that from spreading, exactly as next door: `xtask/src/deploy.rs`'s
/// `check_open_path_boundary` allows `lapidary-api` to name this type in `upload.rs` and
/// nowhere else. See `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
/// §4.1.
pub struct SourceWriter {
    root: PathBuf,
}

impl SourceWriter {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Store a staged file under the hash it was uploaded as, refusing it if the bytes
    /// disagree.
    ///
    /// The verification is not the caller's to skip: the hash is the dedup key for the
    /// whole store, so a wrong one accepted here silently attaches one library's part to
    /// another library's bytes. `DATA.md` §5.2 states it as a rule and this is where the
    /// rule is enforced, in one pass over the file rather than two.
    pub fn put_file(
        &self,
        staged: &Path,
        expect: &BlobHash,
        compression: Compression,
    ) -> Result<StoredBlob, StorageError> {
        write_blob_from_file(&self.root, staged, expect, compression.compresses())
    }
}

/// Move a model or category directory, and nothing else.
///
/// The third handle onto source paths, and deliberately the narrowest: no read, no write
/// of contents, no delete. It exists because a move is a rename plus a row update, and
/// neither `SourceStore` (which demands a `WorkerRole`) nor `SourceReader` (read-only) can
/// rename.
///
/// Giving `lapidary-api` this rather than routing an O(1) syscall through the job queue is
/// a decision the spec argues (§10): the open-path boundary exists so the interactive path
/// never *parses* a source file to draw something, and a rename parses nothing, reads no
/// bytes and invokes no kernel. `xtask/src/deploy.rs` keeps it to one module, the same way
/// it keeps `SourceReader` to `download.rs`.
///
/// If this ever grows a method that reads or writes contents, it has become `SourceStore`
/// and the route belongs in `lapidary-ingest`.
pub struct SourceRelocator {
    root: PathBuf,
}

impl SourceRelocator {
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Resolve a store-relative path, refusing anything that would leave the root. The
    /// same guard `SourceStore`'s path-addressed methods run — see [`resolve`].
    fn resolve(&self, rel: &str) -> Result<PathBuf, StorageError> {
        resolve(&self.root, rel)
    }

    pub fn create_dir(&self, rel: &str) -> Result<(), StorageError> {
        let path = self.resolve(rel)?;
        std::fs::create_dir_all(&path).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    /// A same-filesystem rename: atomic, and O(1) however large the subtree. The parent of
    /// `to` must exist — the caller creates the category before moving into it.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let from_path = self.resolve(from)?;
        let to_path = self.resolve(to)?;
        std::fs::rename(&from_path, &to_path).map_err(|source| StorageError::Io {
            path: from_path.display().to_string(),
            source,
        })
    }
}

/// A missing file is success: the reap's job is that the bytes are not on disk
/// afterwards, and a `put` that failed before its rename leaves nothing to remove.
fn remove_blob(root: &Path, hash: &BlobHash) -> Result<(), StorageError> {
    let path = blob_path(root, hash);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SourceStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SourceStore::open(dir.path(), &WorkerRole::assume());
        (dir, store)
    }

    /// A staged file, as the upload route produces one.
    fn staged(dir: &tempfile::TempDir, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join("staged.part");
        std::fs::write(&path, bytes).expect("staged file writes");
        path
    }

    #[test]
    fn a_staged_file_stores_under_the_hash_it_claims_and_reads_back() {
        let (dir, _s) = store();
        let bytes = b"solid bracket-lp-1042-03\nendsolid bracket-lp-1042-03\n";
        let path = staged(&dir, bytes);
        let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());

        let writer = SourceWriter::open(dir.path());
        let stored = writer
            .put_file(&path, &hash, Compression::Zstd)
            .expect("a truthful claim stores");
        assert_eq!(stored.hash, hash);
        assert_eq!(stored.size_bytes, bytes.len() as u64);

        // The reader is the worker's half: same root, same hash, the level off the
        // returned row rather than re-derived from a file extension.
        let read = SourceReader::open(dir.path())
            .get(&hash, Some(stored.zstd_level))
            .expect("the blob reads back");
        assert_eq!(
            read, bytes,
            "the bytes must survive the compression round trip"
        );
    }

    #[test]
    fn a_staged_file_that_does_not_match_its_claim_stores_nothing() {
        let (dir, _s) = store();
        let path = staged(&dir, b"these are not the bytes that were promised");
        // A hash of something else entirely — the shape of a corrupted transfer, and the
        // shape of a client trying to attach its own path to another library's blob.
        let claimed = BlobHash::from_bytes(*blake3::hash(b"the promised bytes").as_bytes());

        let err = SourceWriter::open(dir.path())
            .put_file(&path, &claimed, Compression::Zstd)
            .expect_err("a false claim is refused");
        assert!(
            matches!(err, StorageError::HashMismatch { .. }),
            "expected a hash mismatch, got: {err}"
        );

        // The point of the refusal: nothing was stored under the claimed hash, and no
        // temp file was left beside it. A blob that failed verification must never have
        // existed at that path even briefly.
        assert!(
            SourceReader::open(dir.path())
                .get(&claimed, Some(3))
                .is_err(),
            "the claimed hash must name nothing"
        );
        let shard = dir.path().join("blobs").join(&claimed.to_hex()[0..2]);
        let leftovers: Vec<_> = std::fs::read_dir(&shard)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|shard| std::fs::read_dir(shard.path()))
            .flatten()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "a refused write must leave no temp file behind, found: {leftovers:?}"
        );
    }

    #[test]
    fn an_uncompressed_staged_file_stores_verbatim() {
        // 3MF is already a deflate ZIP, so `Compression::AsIs` is the branch a real
        // upload of one takes -- and it is a different code path through `put_file`.
        let (dir, _s) = store();
        let bytes = b"PK\x03\x04 not really a 3mf, but the branch is the point";
        let path = staged(&dir, bytes);
        let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());

        let stored = SourceWriter::open(dir.path())
            .put_file(&path, &hash, Compression::AsIs)
            .expect("stores");
        assert_eq!(stored.zstd_level, 0);
        assert_eq!(stored.stored_bytes, bytes.len() as u64);
        assert_eq!(
            SourceReader::open(dir.path())
                .get(&hash, None)
                .expect("reads back"),
            bytes
        );
    }

    #[test]
    fn a_blob_round_trips_by_its_hash() {
        let (_dir, s) = store();
        let stored = s.put(b"solid bracket\n", Compression::Zstd).expect("put");
        assert_eq!(
            s.get(&stored.hash, Compression::Zstd).expect("get"),
            b"solid bracket\n"
        );
    }

    #[test]
    fn the_same_bytes_always_produce_the_same_hash() {
        let (_dir, s) = store();
        assert_eq!(
            s.put(b"same", Compression::Zstd).expect("a").hash,
            s.put(b"same", Compression::Zstd).expect("b").hash
        );
    }

    #[test]
    fn blobs_are_sharded_two_levels_deep() {
        // 65,536 buckets keeps any directory under ~2k entries at a million blobs.
        let (dir, s) = store();
        let stored = s.put(b"shard me", Compression::Zstd).expect("put");
        let hex = stored.hash.to_hex();
        let path = dir
            .path()
            .join("blobs")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex);
        assert!(path.exists(), "expected {}", path.display());
    }

    #[test]
    fn source_bytes_are_compressed_and_the_stored_size_reflects_it() {
        let (_dir, s) = store();
        let compressible = "solid ".repeat(4096).into_bytes();
        let stored = s.put(&compressible, Compression::Zstd).expect("put");
        assert_eq!(stored.size_bytes, compressible.len() as u64);
        assert!(
            stored.stored_bytes < stored.size_bytes,
            "zstd should shrink this"
        );
        assert_eq!(stored.zstd_level, 3);
    }

    #[test]
    fn derivatives_are_never_compressed_because_they_are_hot_path_and_already_packed() {
        // docs/DATA.md §1.2: derivatives (thumbnails, tessellations) are already packed
        // (meshopt, WebP) and sit on the hot open path — every grid render, every viewer
        // load. Compressing them would buy ~2% space for a decode stage paid on every
        // one of those reads, which is the opposite of what the inline-thumbnail design
        // exists for. Asserted on observable facts, not the internal flag: input shaped
        // to visibly shrink under zstd (mirrors the source-store test above) must come
        // back the same size, uncompressed — a compressed store would shrink it.
        let dir = tempfile::tempdir().expect("temp dir");
        let d = DerivativeStore::open(dir.path());
        let compressible = "solid ".repeat(4096).into_bytes();
        let stored = d.put(&compressible).expect("put");
        assert_eq!(
            stored.stored_bytes, stored.size_bytes,
            "derivatives must be stored as-is, not shrunk by compression"
        );
        assert_eq!(stored.zstd_level, 0);
    }

    #[test]
    fn getting_an_unknown_hash_says_which_hash_and_what_that_means() {
        let (_dir, s) = store();
        let missing = lapidary_core::BlobHash::from_bytes([0x11; 32]);
        let err = s.get(&missing, Compression::Zstd).expect_err("must fail");
        let msg = err.to_string();
        assert!(
            msg.contains(&missing.to_hex()[..8]),
            "names the hash: {msg}"
        );
        assert!(
            msg.contains("quarantine") || msg.contains("evicted"),
            "suggests a cause: {msg}"
        );
    }

    #[test]
    fn removing_a_blob_leaves_the_store_usable() {
        // Ingest reaps a blob when the transaction that would have referenced it fails.
        let (_dir, s) = store();
        let stored = s.put(b"orphan", Compression::Zstd).expect("put");
        s.remove(&stored.hash).expect("remove");
        assert!(s.get(&stored.hash, Compression::Zstd).is_err());
        assert!(
            s.put(b"another", Compression::Zstd).is_ok(),
            "the store still works after a removal"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_second_put_of_identical_bytes_does_not_rewrite_the_file() {
        // Two concurrent writers of the same blob must not race a truncate+write on the
        // same path. This pins the fix on this process alone: a second `put` of bytes
        // already on disk must not touch the file at all. Comparing inode numbers, not
        // just contents, is the point — a rewrite-in-place, or an unlink-then-recreate,
        // both leave the *content* unchanged but would show up here as a new inode
        // (rename onto an existing path replaces the inode; the early-return path this
        // test is pinning never runs rename at all).
        use std::os::unix::fs::MetadataExt;
        let (_dir, s) = store();
        let first = s.put(b"idempotent", Compression::Zstd).expect("first put");
        let path = blob_path(&s.root, &first.hash);
        let before = std::fs::metadata(&path).expect("stat before").ino();

        let second = s.put(b"idempotent", Compression::Zstd).expect("second put");

        let after = std::fs::metadata(&path).expect("stat after").ino();
        assert_eq!(
            before, after,
            "a second put of identical bytes must not rewrite the file"
        );
        assert_eq!(second.stored_bytes, first.stored_bytes);
    }

    #[test]
    #[cfg(unix)]
    fn a_permission_denied_read_surfaces_as_io_not_not_found() {
        // A permissions error, a bad mount, or a file caught mid-rewrite must not be
        // reported as NotFound — that sends whoever is debugging it to hunt for a file
        // that is sitting right there with the wrong mode bits. Denies read access on the
        // blob itself (not just its directory) so `std::fs::read` fails with
        // PermissionDenied, not NotFound, and asserts the store reports it as such.
        //
        // Assumes the test process is not root — root bypasses file mode entirely, which
        // would make this assertion vacuous. Verified non-root in CI and in this
        // environment; if that ever stops holding, this test starts failing loudly
        // (`result` becomes `Ok`, which the match below rejects) rather than passing
        // having proven nothing.
        use std::os::unix::fs::PermissionsExt;
        let (_dir, s) = store();
        let stored = s.put(b"guarded", Compression::Zstd).expect("put");
        let path = blob_path(&s.root, &stored.hash);
        let original_mode = std::fs::metadata(&path).expect("stat").permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let result = s.get(&stored.hash, Compression::Zstd);

        // Restore permissions unconditionally, before asserting, so a failed assertion
        // still leaves the temp directory removable by its own Drop.
        std::fs::set_permissions(&path, original_mode).expect("restore permissions");

        match result {
            Err(StorageError::Io { .. }) => {}
            other => {
                panic!("a permission-denied read must surface as StorageError::Io, not {other:?}")
            }
        }
    }

    #[test]
    fn a_derivative_store_needs_no_worker_token() {
        // Both roles hold derivatives; only the worker may reach source bytes.
        let dir = tempfile::tempdir().expect("temp dir");
        let d = DerivativeStore::open(dir.path());
        let hash = d.put(b"gltf bytes").expect("put").hash;
        assert_eq!(d.get(&hash).expect("get"), b"gltf bytes");
    }

    #[test]
    fn the_compression_policy_follows_the_data_doc_table() {
        // DATA.md §1.2: STEP, STL and OBJ compress; 3MF is already a deflate ZIP.
        assert_eq!(Compression::for_source_format("stl"), Compression::Zstd);
        assert_eq!(Compression::for_source_format("obj"), Compression::Zstd);
        assert_eq!(Compression::for_source_format("step"), Compression::Zstd);
        assert_eq!(Compression::for_source_format("3mf"), Compression::AsIs);
        // Case is not the caller's problem: `source_format` lowercases, but a policy that
        // silently compressed an uppercase 3MF would be a very quiet bug.
        assert_eq!(Compression::for_source_format("3MF"), Compression::AsIs);
        // An unknown format compresses. Spending CPU is the safe wrong answer; storing an
        // already-packed format uncompressed costs only space.
        assert_eq!(Compression::for_source_format("wrl"), Compression::Zstd);
    }

    #[test]
    fn an_as_is_blob_round_trips_and_records_equal_sizes() {
        let (_dir, s) = store();
        // Deliberately compressible, so a stored size equal to the real size proves the
        // policy was honoured rather than proving the bytes were incompressible.
        let bytes = vec![0u8; 64 * 1024];
        let stored = s.put(&bytes, Compression::AsIs).expect("stores");
        assert_eq!(stored.stored_bytes, stored.size_bytes);
        assert_eq!(stored.zstd_level, 0);
        assert_eq!(
            s.get(&stored.hash, Compression::AsIs).expect("reads"),
            bytes
        );
    }

    #[test]
    fn a_source_reader_reads_back_what_a_source_store_wrote() {
        // The download route's claim in miniature: bytes ingest stored come back
        // byte-identical through a handle that cannot write them. The level is threaded
        // from the `StoredBlob` the write returned rather than written as a literal,
        // because that is the coupling the route has — it reads `blob.zstd_level` out of
        // the same row that carried the hash.
        let (dir, s) = store();
        let reader = SourceReader::open(dir.path());

        let stl = "facet normal 0 0 1\nvertex 12.0 4.5 0.0\n"
            .repeat(512)
            .into_bytes();
        let compressed = s.put(&stl, Compression::Zstd).expect("stores the STL");
        assert!(
            compressed.stored_bytes < compressed.size_bytes,
            "the STL must actually be compressed on disk, or the decode leg proves nothing"
        );
        assert_eq!(
            reader
                .get(&compressed.hash, Some(compressed.zstd_level))
                .expect("reads the STL back"),
            stl
        );

        // 3MF is a deflate ZIP, so DATA.md §1.2 stores it as-is — the leg where a reader
        // that decoded unconditionally would fail on a frame that was never a frame.
        let threemf = b"PK\x03\x04\x14\x00\x00\x00\x08\x00".repeat(2048);
        let raw = s.put(&threemf, Compression::AsIs).expect("stores the 3MF");
        assert_eq!(
            raw.stored_bytes, raw.size_bytes,
            "deliberately compressible bytes, so equal sizes prove they were stored raw"
        );
        assert_eq!(
            reader
                .get(&raw.hash, Some(raw.zstd_level))
                .expect("reads the 3MF back"),
            threemf
        );
        // The column is nullable, and an absent level must read as uncompressed rather
        // than sending raw bytes through the decoder.
        assert_eq!(
            reader
                .get(&raw.hash, None)
                .expect("reads with a null level"),
            threemf
        );
        // A negative level is zstd's --fast=N, which still writes a frame, so it has to
        // take the decode branch. These bytes are not a frame, so refusing is the proof:
        // a `> 0` test would return them unchanged and call that success.
        assert!(
            reader.get(&raw.hash, Some(-3)).is_err(),
            "a negative level must decode, not pass raw bytes through"
        );
    }

    #[test]
    fn a_source_reader_reads_a_path_addressed_file_back_by_its_recorded_level() {
        // The download route's `Some(storage_path)` branch, proven at the layer that does
        // not need a database. Ingest only ever writes a path-addressed file `AsIs`
        // (`handler.rs`), so this fixture is deliberately compressed anyway: `get_at` must
        // follow whatever `zstd_level` the row says, not assume "path-addressed" means
        // "uncompressed" — the same mistake the hash-addressed twin above exists to catch
        // for `get`.
        let (dir, s) = store();
        let reader = SourceReader::open(dir.path());
        let rel = "libraries/default/Bases/round-32mm/round-32mm.stl";
        let bytes = "solid round-32mm\n".repeat(64).into_bytes();
        let compressed = s.put_at(rel, &bytes, Compression::Zstd).expect("put_at");
        assert!(
            compressed.stored_bytes < compressed.size_bytes,
            "the fixture must really be compressed on disk, or the decode leg proves nothing"
        );
        assert_eq!(
            reader
                .get_at(rel, Some(compressed.zstd_level))
                .expect("reads the compressed file back"),
            bytes
        );

        // The shape ingest actually writes today: `AsIs`, recorded as level 0.
        let as_is_rel = "libraries/default/Terrain/Rocks/cliff/cliff.stl";
        let raw = s
            .put_at(as_is_rel, b"solid cliff\n", Compression::AsIs)
            .expect("put_at");
        assert_eq!(raw.zstd_level, 0);
        assert_eq!(
            reader
                .get_at(as_is_rel, Some(raw.zstd_level))
                .expect("reads the as-is file back"),
            b"solid cliff\n"
        );
        // The column is nullable, and an absent level must read as uncompressed rather
        // than sending raw bytes through the decoder — same rule as `get`.
        assert_eq!(
            reader
                .get_at(as_is_rel, None)
                .expect("reads with a null level"),
            b"solid cliff\n"
        );
    }

    #[test]
    fn a_reader_cannot_be_talked_out_of_the_store_by_a_path_either() {
        // The read-only twin of `a_path_addressed_write_cannot_be_talked_out_of_the_store`:
        // `get_at` takes a path off a database row, not off a hash, so it runs the same
        // `reject_escaping_path` guard rather than trusting the caller scoped it.
        let dir = tempfile::tempdir().expect("temp dir");
        let reader = SourceReader::open(dir.path());
        assert!(matches!(
            reader.get_at("/etc/passwd", None),
            Err(StorageError::PathRefused { .. })
        ));
        assert!(matches!(
            reader.get_at("../../etc/passwd", None),
            Err(StorageError::PathRefused { .. })
        ));
    }

    #[test]
    fn a_zstd_blob_still_shrinks() {
        let (_dir, s) = store();
        let bytes = vec![0u8; 64 * 1024];
        let stored = s.put(&bytes, Compression::Zstd).expect("stores");
        assert!(stored.stored_bytes < stored.size_bytes);
        assert_eq!(
            s.get(&stored.hash, Compression::Zstd).expect("reads"),
            bytes
        );
    }

    #[test]
    fn a_source_file_lands_at_the_path_it_was_given_under_its_own_name() {
        // The whole reversal in one assertion: bytes reachable by opening a folder, not
        // by knowing a hash. The directories above it are created on the way.
        let (dir, s) = store();
        let rel = "libraries/default/Terrain/Rocks/cliff/cliff.stl";
        let stored = s
            .put_at(rel, b"solid cliff\n", Compression::AsIs)
            .expect("put_at");

        assert_eq!(
            std::fs::read(dir.path().join(rel)).expect("reads"),
            b"solid cliff\n",
            "the file a user opens in a file manager is the file we ingested"
        );
        assert_eq!(stored.zstd_level, 0);
        assert_eq!(stored.size_bytes, stored.stored_bytes);
        // The hash is still computed and still returned: `file.blake3` and re-adoption
        // both verify against it, whatever name the bytes are stored under.
        assert_eq!(
            s.get_at(rel, Some(stored.zstd_level)).expect("get_at"),
            b"solid cliff\n"
        );
        assert_eq!(
            stored.hash,
            BlobHash::from_bytes(*blake3::hash(b"solid cliff\n").as_bytes())
        );
        // No temp file survives the rename.
        let leftovers: Vec<_> =
            std::fs::read_dir(dir.path().join("libraries/default/Terrain/Rocks/cliff"))
                .expect("read dir")
                .filter_map(|e| e.ok().map(|e| e.file_name()))
                .filter(|name| name != "cliff.stl")
                .collect();
        assert!(
            leftovers.is_empty(),
            "left a temp file behind: {leftovers:?}"
        );
    }

    #[test]
    fn a_compressed_source_file_round_trips_through_the_level_it_recorded() {
        // Reading back must follow the recorded level rather than re-deriving one from
        // the format — the rule `SourceReader::get` states, applied to the path-addressed
        // reader the derive job uses.
        let (_dir, s) = store();
        let rel = "libraries/default/Bases/round-32mm/round-32mm.stl";
        let bytes = b"solid round-32mm\n".repeat(64);
        let stored = s.put_at(rel, &bytes, Compression::Zstd).expect("put_at");
        assert!(stored.stored_bytes < stored.size_bytes);
        assert_eq!(
            s.get_at(rel, Some(stored.zstd_level)).expect("get_at"),
            bytes
        );
    }

    #[test]
    fn a_reaped_model_directory_leaves_nothing_and_takes_nothing_that_is_not_ours() {
        // The failed-transaction path. The file goes, the empty directory goes with it —
        // an empty directory left behind would make the retry think the name is taken and
        // rename the model — but a directory holding somebody else's file stays exactly
        // as it is.
        let (dir, s) = store();
        let model = "libraries/default/Terrain/Rocks/cliff";
        let rel = format!("{model}/cliff.stl");
        s.put_at(&rel, b"solid cliff\n", Compression::AsIs)
            .expect("put_at");

        s.remove_at(&rel).expect("reaps the file");
        s.remove_dir_if_empty(model).expect("reaps the directory");
        assert!(!dir.path().join(model).exists());

        // Twice is success: a reap's job is that the bytes are gone afterwards.
        s.remove_at(&rel).expect("a missing file is success");
        s.remove_dir_if_empty(model)
            .expect("a missing directory is success");

        // And a directory another writer is using is refused, not emptied.
        s.put_at(&rel, b"solid cliff\n", Compression::AsIs)
            .expect("put_at");
        assert!(s.remove_dir_if_empty(model).is_err());
        assert!(dir.path().join(&rel).exists());
    }

    #[test]
    fn a_path_addressed_write_cannot_be_talked_out_of_the_store() {
        let (_dir, s) = store();
        assert!(matches!(
            s.put_at("../escape.stl", b"x", Compression::AsIs),
            Err(StorageError::PathRefused { .. })
        ));
        assert!(matches!(
            s.get_at("/etc/passwd", None),
            Err(StorageError::PathRefused { .. })
        ));
        assert!(matches!(
            s.remove_at("../../etc/passwd"),
            Err(StorageError::PathRefused { .. })
        ));
        assert!(matches!(
            s.remove_dir_if_empty("../.."),
            Err(StorageError::PathRefused { .. })
        ));
    }

    #[test]
    fn a_relocator_moves_a_directory_and_its_contents() {
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        r.create_dir("libraries/default/Terrain/cliff")
            .expect("mkdir");
        std::fs::write(
            dir.path().join("libraries/default/Terrain/cliff/cliff.stl"),
            b"solid\n",
        )
        .expect("write");
        r.create_dir("libraries/default/Bases").expect("mkdir");

        r.rename(
            "libraries/default/Terrain/cliff",
            "libraries/default/Bases/cliff",
        )
        .expect("rename");

        assert!(
            dir.path()
                .join("libraries/default/Bases/cliff/cliff.stl")
                .exists()
        );
        assert!(!dir.path().join("libraries/default/Terrain/cliff").exists());
    }

    #[test]
    fn a_relocator_refuses_a_path_that_escapes_the_root() {
        // The whole point of the narrow handle: it cannot be talked into touching
        // anything outside the store. Asserting the variant, not just `is_err()`, matters
        // here: an unguarded `from` or `to` still fails, but with a filesystem ENOENT
        // (neither "a" nor "../a" exists in a fresh temp dir), not the guard's own error —
        // `is_err()` alone would pass just as well against a relocator missing the check
        // on either argument.
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        assert!(matches!(
            r.create_dir("../escaped"),
            Err(StorageError::PathRefused { .. })
        ));
        assert!(
            matches!(r.rename("../a", "b"), Err(StorageError::PathRefused { .. })),
            "the guard must run on `from`"
        );
        assert!(
            matches!(
                r.rename("a", "/etc/lapidary"),
                Err(StorageError::PathRefused { .. })
            ),
            "the guard must run on `to` as well as `from`"
        );
    }

    #[test]
    fn renaming_a_missing_directory_says_which_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        let err = r
            .rename(
                "libraries/default/Terrain/gone",
                "libraries/default/Bases/gone",
            )
            .expect_err("must fail");
        assert!(
            err.to_string().contains("Terrain/gone"),
            "names the path: {err}"
        );
    }
}
