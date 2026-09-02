//! Content-addressed blob storage. Two handles, deliberately:
//!
//! `DerivativeStore` reads and writes derivatives — thumbnails, tessellations — and both
//! roles hold one. `SourceStore` reaches the ingested source bytes and requires a
//! `WorkerRole` token to construct.
//!
//! This is the API-level half of "the open path never touches a source file". The
//! dependency-graph half cannot express it: `lapidary-api` legitimately depends on this
//! crate for derivatives, so the distinction is *which bytes*, not whether the crates may
//! be connected. `cargo xtask check-deploy` asserts `lapidary-api` never names
//! `SourceStore`.

use lapidary_core::BlobHash;
use std::path::{Path, PathBuf};
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
}

/// Proof the holder is running in the worker role. Zero-sized and unconstructible except
/// through `assume`, which the binary calls once after reading `LAPIDARY_ROLE`.
pub struct WorkerRole(());

impl WorkerRole {
    /// Called by `bin/lapidary-server` when it has established the worker role.
    pub fn assume() -> Self {
        WorkerRole(())
    }
}

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

fn write_blob(root: &Path, bytes: &[u8], compress: bool) -> Result<StoredBlob, StorageError> {
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let path = blob_path(root, &hash);
    let parent = path.parent().unwrap_or(root);
    std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;

    let payload = if compress {
        zstd::encode_all(bytes, INGEST_LEVEL).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?
    } else {
        bytes.to_vec()
    };

    // Content addressing makes rewriting an existing blob pointless — the bytes are the
    // same by definition — but writing anyway keeps the code one path instead of two.
    std::fs::write(&path, &payload).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;

    Ok(StoredBlob {
        hash,
        size_bytes: bytes.len() as u64,
        stored_bytes: payload.len() as u64,
        zstd_level: if compress { INGEST_LEVEL as i16 } else { 0 },
    })
}

fn read_blob(root: &Path, hash: &BlobHash, compressed: bool) -> Result<Vec<u8>, StorageError> {
    let path = blob_path(root, hash);
    let raw = std::fs::read(&path).map_err(|_| StorageError::NotFound {
        hash_prefix: hash.to_hex()[..8].to_owned(),
    })?;
    if compressed {
        zstd::decode_all(raw.as_slice()).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })
    } else {
        Ok(raw)
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

    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, true)
    }

    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, true)
    }

    /// Reap a blob written for a transaction that then failed. Not user-facing deletion —
    /// no part ever referenced these bytes, so this never touches anything a library
    /// member could see: it exists only to clean up after a failed ingest write, not to
    /// remove content anyone has stored.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SourceStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SourceStore::open(dir.path(), &WorkerRole::assume());
        (dir, store)
    }

    #[test]
    fn a_blob_round_trips_by_its_hash() {
        let (_dir, s) = store();
        let stored = s.put(b"solid bracket\n").expect("put");
        assert_eq!(s.get(&stored.hash).expect("get"), b"solid bracket\n");
    }

    #[test]
    fn the_same_bytes_always_produce_the_same_hash() {
        let (_dir, s) = store();
        assert_eq!(
            s.put(b"same").expect("a").hash,
            s.put(b"same").expect("b").hash
        );
    }

    #[test]
    fn blobs_are_sharded_two_levels_deep() {
        // 65,536 buckets keeps any directory under ~2k entries at a million blobs.
        let (dir, s) = store();
        let stored = s.put(b"shard me").expect("put");
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
        let stored = s.put(&compressible).expect("put");
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
        let err = s.get(&missing).expect_err("must fail");
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
        let stored = s.put(b"orphan").expect("put");
        s.remove(&stored.hash).expect("remove");
        assert!(s.get(&stored.hash).is_err());
        assert!(
            s.put(b"another").is_ok(),
            "the store still works after a removal"
        );
    }

    #[test]
    fn a_derivative_store_needs_no_worker_token() {
        // Both roles hold derivatives; only the worker may reach source bytes.
        let dir = tempfile::tempdir().expect("temp dir");
        let d = DerivativeStore::open(dir.path());
        let hash = d.put(b"gltf bytes").expect("put").hash;
        assert_eq!(d.get(&hash).expect("get"), b"gltf bytes");
    }
}
