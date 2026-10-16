//! `GET /api/revisions/{id}/download?variant=original`.
//!
//! Two kinds of fixture. Most of this file is about a part indexed **before M4**, whose file is in
//! Lapidary's own store — at its hash, or at a path under the store, possibly a zstd frame — and
//! still downloads byte-identical (`legacy` writes those exactly as the old store did). The tests at
//! the end are about a file in the owner's folders, read where it is, and refused with a sentence
//! when it changed there since it was indexed.
//!
//! The compressed leg and the `AsIs` leg are not a coverage pair: only the first can detect a
//! reader that stopped decoding, which is the drift the byte-identity claim exists to catch.

/// The store as Lapidary wrote it before M4, for fixtures only: nothing writes this layout now.
mod legacy {
    use lapidary_core::BlobHash;
    use std::path::Path;

    pub struct Stored {
        pub hash: BlobHash,
        pub size_bytes: u64,
        pub stored_bytes: u64,
        pub zstd_level: i16,
    }

    #[derive(Clone, Copy)]
    pub enum Compression {
        Zstd,
        AsIs,
    }

    impl Compression {
        /// `DATA.md` §1.2 as it stood: 3MF as is, everything else at zstd 3.
        pub fn for_source_format(format: &str) -> Self {
            if format.eq_ignore_ascii_case("3mf") {
                Compression::AsIs
            } else {
                Compression::Zstd
            }
        }
    }

    fn write(path: &Path, bytes: &[u8], compression: Compression) -> Stored {
        let (payload, zstd_level) = match compression {
            Compression::Zstd => (zstd::encode_all(bytes, 3).expect("compresses"), 3),
            Compression::AsIs => (bytes.to_vec(), 0),
        };
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
        std::fs::write(path, &payload).expect("writes");
        Stored {
            hash: BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()),
            size_bytes: bytes.len() as u64,
            stored_bytes: payload.len() as u64,
            zstd_level,
        }
    }

    /// At `blobs/ab/cd/<hash>`.
    pub fn put(root: &Path, bytes: &[u8], compression: Compression) -> Result<Stored, String> {
        let hex = blake3::hash(bytes).to_hex();
        Ok(write(
            &root
                .join("blobs")
                .join(&hex[0..2])
                .join(&hex[2..4])
                .join(hex.as_str()),
            bytes,
            compression,
        ))
    }

    /// At a path under the store.
    pub fn put_at(
        root: &Path,
        rel: &str,
        bytes: &[u8],
        compression: Compression,
    ) -> Result<Stored, String> {
        Ok(write(&root.join(rel), bytes, compression))
    }
}

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, RevisionId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// A part name carrying all three characters `DATA.md` §5.1 names as the reason this
/// header shape exists: ş, ğ and ı. A shaft bearing cover, which is a thing that exists
/// and has a part number.
const TURKISH_NAME: &str = "Şaft yatak kapağı, LP-3120-05";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [62.0, 62.0, 12.0],
        triangle_count: 360,
        surface_area_mm2: 14_320.5,
        volume_mm3: Some(36_216.0),
        is_watertight: true,
    }
}

/// A real ASCII STL — a fan of facets around a 62 mm cover — and real for one load-bearing
/// reason: `legacy::Compression::for_source_format("stl")` compresses, so these bytes reach disk
/// as a zstd frame and come back only if the route decodes. A 3MF fixture would take the
/// `AsIs` branch and pass whether the route decoded or not.
///
/// Indented one space per level rather than the conventional two: `cargo xtask
/// check-strings` flags a run of three or more spaces inside a string literal, which is
/// what a mangled line continuation leaves behind, and an STL is whitespace-insensitive.
fn ascii_stl() -> Vec<u8> {
    let mut stl = String::from("solid LP-3120-05\n");
    for step in 0..360 {
        let angle = f64::from(step).to_radians();
        let (x, y) = (31.0 * angle.cos(), 31.0 * angle.sin());
        stl.push_str("facet normal 0.0 0.0 1.0\n");
        stl.push_str(" outer loop\n");
        stl.push_str("  vertex 0.000 0.000 12.000\n");
        stl.push_str(&format!("  vertex {x:.3} {y:.3} 12.000\n"));
        stl.push_str(&format!("  vertex {x:.3} {y:.3} 0.000\n"));
        stl.push_str(" endloop\n");
        stl.push_str("endfacet\n");
    }
    stl.push_str("endsolid LP-3120-05\n");
    stl.into_bytes()
}

/// Stands in for a 3MF: a ZIP local file header over already-deflated payload, which is
/// why `DATA.md` §1.2 stores the format as-is. Nothing parses these bytes, and a fixture
/// that parsed would invite a later reader to believe something did.
const THREE_MF: &[u8] = b"PK\x03\x04\x14\x00\x08\x08\x08\x00\x9d\x71\x3d\x5a\x00\x00\x00\x00\
\x00\x00\x00\x00\x00\x00\x00\x00\x13\x00\x00\x003D/3dmodel.model\xed\x9b\x4d\x6f\xdb\x38\
\x10\x86\xef\xfb\x2b\x04\x9d\x8b\x2d\xdb\x49\x9b\x2e\x02\x39\x74\xb1\x40\x0b\xf4\xd0";

/// What a fixture leaves behind: the row to ask for, and the blob it points at.
struct Seeded {
    part: PartId,
    revision: RevisionId,
    hash: BlobHash,
    size_bytes: u64,
    stored_bytes: u64,
}

/// Writes `bytes` to the source store under `format`'s real ingest policy and records a
/// part for them, exactly as ingest does. Every field of the `blob` row comes off the
/// `StoredBlob` the write returned, so the row cannot describe bytes that were stored
/// some other way.
async fn seed(pool: &sqlx::PgPool, root: &Path, name: &str, format: &str, bytes: &[u8]) -> Seeded {
    let stored = legacy::put(root, bytes, legacy::Compression::for_source_format(format))
        .expect("stores the source file");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: None,
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library: library(),
            name,
            source_path: name,
            blob: &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format,
        })
        .await
        .expect("records");
    let revision = PgParts(pool.clone())
        .latest_revision(part)
        .await
        .expect("query")
        .expect("the ingested revision");
    Seeded {
        part,
        revision,
        hash: stored.hash,
        size_bytes: stored.size_bytes,
        stored_bytes: stored.stored_bytes,
    }
}

/// `seed`'s twin for the other layout: writes `bytes` at `rel` — a store-relative path
/// under the model's own directory — rather than at its hash, and records a `file` row
/// whose `storage_path` names it. This is what ingest has written since the previous task
/// (`handler.rs`, always `legacy::Compression::AsIs`), and it is the state every part ingested
/// from now on is in, where `seed`'s `storage_path: None` is the state every part ingested
/// before it is in. Both are live on the same corpus for as long as `migrate_storage`
/// takes to drain (migration `0009`), so the route has to answer both.
async fn seed_at_path(
    pool: &sqlx::PgPool,
    root: &Path,
    rel: &str,
    name: &str,
    format: &str,
    bytes: &[u8],
) -> Seeded {
    let stored = legacy::put_at(root, rel, bytes, legacy::Compression::AsIs)
        .expect("stores the source file at its path");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: Some(rel),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library: library(),
            name,
            source_path: name,
            blob: &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format,
        })
        .await
        .expect("records");
    let revision = PgParts(pool.clone())
        .latest_revision(part)
        .await
        .expect("query")
        .expect("the ingested revision");
    Seeded {
        part,
        revision,
        hash: stored.hash,
        size_bytes: stored.size_bytes,
        stored_bytes: stored.stored_bytes,
    }
}

/// Where `lapidary-storage` puts a blob. Duplicated here rather than exported, because a
/// test that reached for the store's own path helper could not corrupt a file behind the
/// store's back, which is exactly what one test below has to do.
fn blob_file(root: &Path, hash: &BlobHash) -> PathBuf {
    let hex = hash.to_hex();
    root.join("blobs")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(hex)
}

/// `blob.last_accessed_at` as epoch microseconds, `None` while the column is still NULL —
/// the same trick `tests/blob.rs` uses, and for the same reason: sqlx here carries neither
/// `chrono` nor `time`.
async fn last_read_us(
    touches: &lapidary_db::Touches,
    pool: &sqlx::PgPool,
    hash: &BlobHash,
) -> Option<i64> {
    // What the server's flush would have written by now (`DATA.md` §1.4).
    touches.flush(pool).await.expect("flushes the reads");
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM last_accessed_at) * 1000000)::bigint FROM blob WHERE blake3 = $1",
    )
    .bind(hash.to_hex())
    .fetch_one(pool)
    .await
    .expect("the blob row exists")
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body")
        .to_vec();
    (status, headers, body)
}

/// Like [`get`], but keeps a body that fails part-way instead of panicking on it.
///
/// Since the body streams, a blob whose bytes do not hash to their digest is reported by
/// closing the stream early rather than by a status code — so a test for that case has to
/// be able to hold a partial body and the error that ended it.
async fn get_streaming(
    app: axum::Router,
    uri: &str,
) -> (StatusCode, Vec<(String, String)>, Result<Vec<u8>, String>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string());
    (status, headers, body)
}

fn download_uri(revision: RevisionId, query: &str) -> String {
    format!("/api/revisions/{revision}/download{query}")
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn message(body: &[u8]) -> String {
    let json: serde_json::Value = serde_json::from_slice(body).expect("a JSON body");
    json["message"].as_str().expect("a message").to_owned()
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_compressed_source_comes_back_byte_identical(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let bytes = ascii_stl();
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &bytes).await;
    // The assertion that makes the rest of this test mean anything. If the fixture were
    // stored as-is, a reader that stopped decoding would return the same bytes and this
    // test would pass while proving nothing — the exact shape slice 4 shipped.
    assert!(
        seeded.stored_bytes < seeded.size_bytes,
        "the fixture must really be compressed on disk, got {} stored for {} ingested",
        seeded.stored_bytes,
        seeded.size_bytes
    );
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes, "byte-identical to what was ingested");
    assert_eq!(
        header(&headers, "content-type"),
        Some("application/octet-stream"),
        "never model/stl: a browser that renders it inline is a download that did not \
         download"
    );
    assert_eq!(
        header(&headers, "etag"),
        Some(format!("\"{}\"", seeded.hash.to_hex()).as_str()),
        "the digest DATA.md asks the UI to show beside the button"
    );
    assert_eq!(
        header(&headers, "cache-control"),
        Some("no-cache"),
        "revalidate before reuse: this URL names a revision, not a hash"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_3mf_stored_as_is_comes_back_byte_identical(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(
        &pool,
        root.path(),
        "Kılavuz burcu, LP-2207-01",
        "3mf",
        THREE_MF,
    )
    .await;
    assert_eq!(
        seeded.stored_bytes, seeded.size_bytes,
        "3MF is already a deflate ZIP, so ingest stores it as-is (DATA.md §1.2)"
    );
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, THREE_MF, "byte-identical to what was ingested");
    assert_eq!(
        header(&headers, "content-disposition"),
        Some(
            "attachment; filename=\"K_lavuz burcu, LP-2207-01.3mf\"; \
             filename*=UTF-8''K%C4%B1lavuz%20burcu%2C%20LP-2207-01.3mf"
        ),
        "the extension is the source file's format, synthesized rather than stored"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_whose_bytes_have_migrated_downloads_from_its_folder_path(pool: sqlx::PgPool) {
    // The other half of the layout the previous task introduced: `file.storage_path` is
    // `Some` and the bytes live under the model's own directory, not at `blobs/ab/cd/
    // <hash>`. Every part ingested from now on is in this state, and migration `0009`
    // states that both this and the null-path state are supported for as long as
    // `migrate_storage` takes to drain a real corpus — hours, not an edge case.
    let root = tempfile::tempdir().expect("temp dir");
    let bytes = ascii_stl();
    let rel = "libraries/default/Fasteners/LP-3120-05/LP-3120-05.stl";
    let seeded = seed_at_path(&pool, root.path(), rel, TURKISH_NAME, "stl", &bytes).await;
    assert_eq!(
        seeded.stored_bytes, seeded.size_bytes,
        "ingest writes the folder-tree copy as-is (handler.rs); a fixture stored some \
         other way would not be testing the branch this test exists for"
    );
    // The proof that the route actually took the `storage_path` branch rather than
    // falling through to the hash-addressed one and happening to find nothing: there is
    // nothing to find there. A route that ignored `storage_path` would 500 here, not
    // silently pass.
    assert!(
        !blob_file(root.path(), &seeded.hash).exists(),
        "this part's bytes exist only at the folder path, not also at the hash"
    );
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, bytes,
        "byte-identical to what was ingested, read back from the folder path"
    );
    assert_eq!(
        header(&headers, "etag"),
        Some(format!("\"{}\"", seeded.hash.to_hex()).as_str()),
        "the digest is still the content hash, whichever path the bytes were read from"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_nonzero_recorded_level_at_a_folder_path_still_decodes(pool: sqlx::PgPool) {
    // `a_compressed_source_comes_back_byte_identical`'s claim, crossed to the other layout,
    // and deliberately at a *negative* recorded level rather than the positive one
    // `put_at` returned. A positive level (3) cannot tell a route that decodes on
    // `zstd_level != 0` apart from one that decodes only on `zstd_level > 0`, because 3
    // satisfies both — and this fixture exists to catch the second, weaker check as much
    // as the first. `SourceReader::get`'s own doc names why: zstd's `--fast=N` levels are
    // spelled negative and still produce a real frame, and DATA.md §1.3's tiering job is
    // specced to recompress cold files in place at exactly these storage_path locations,
    // which is the future producer of a `Some(rel)` row at a level this route has to get
    // right today, not once that job ships.
    let root = tempfile::tempdir().expect("temp dir");
    let bytes = ascii_stl();
    let rel = "libraries/default/Fasteners/LP-3120-05/LP-3120-05.stl";
    let stored = legacy::put_at(root.path(), rel, &bytes, legacy::Compression::Zstd)
        .expect("stores the source file at its path, compressed");
    assert!(
        stored.stored_bytes < stored.size_bytes,
        "the fixture must really be compressed on disk, or the decode leg proves nothing"
    );
    // The row says the negative spelling of the level the bytes were actually compressed
    // at, not the positive one `put_at` returned: the route has to read past the sign, not
    // just past zero.
    let blob = StoredBlobRow {
        hash: stored.hash,
        size_bytes: stored.size_bytes,
        stored_bytes: stored.stored_bytes,
        zstd_level: -stored.zstd_level,
    };
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: Some(rel),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library: library(),
            name: TURKISH_NAME,
            source_path: TURKISH_NAME,
            blob: &blob,
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("records");
    let revision = PgParts(pool.clone())
        .latest_revision(part)
        .await
        .expect("query")
        .expect("the ingested revision");
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &download_uri(revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, bytes,
        "byte-identical after decoding a negative-level frame read from its folder path"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn bytes_at_a_folder_path_that_do_not_hash_truncate_the_download(pool: sqlx::PgPool) {
    // `bytes_that_do_not_hash_to_their_digest_truncate_the_download`'s claim, pinned again
    // for the other layout. The re-hash runs over whichever reader the branch opened, so
    // corruption at a folder path is caught exactly as corruption at a hash-addressed one
    // is -- and reported the same way, by ending the body rather than by a status code,
    // because both legs stream now and by the time the mismatch is known the status line
    // has been sent. It was a 500 with a body while this leg alone buffered; that
    // difference was an artefact of the two legs reading differently, which is the thing
    // the merge removed.
    let root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/Fasteners/LP-3120-05/LP-3120-05.stl";
    let seeded = seed_at_path(&pool, root.path(), rel, TURKISH_NAME, "stl", &ascii_stl()).await;
    std::fs::write(
        root.path().join(rel),
        b"solid LP-9911-00\nendsolid LP-9911-00\n",
    )
    .expect("overwrite the file with bytes that are not it");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get_streaming(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header(&headers, "content-length"),
        Some(ascii_stl().len().to_string()).as_deref(),
        "the declared length is the ingested file's, so a short body is detectable"
    );

    let error = body.expect_err(
        "a model file that does not match its digest must end the body in an error, never \
         complete successfully with the wrong bytes",
    );
    assert!(
        error.contains(&seeded.hash.to_hex()),
        "the stream's error names the digest the bytes were filed under: {error}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_turkish_name_is_percent_encoded_in_one_half_and_degraded_in_the_other(
    pool: sqlx::PgPool,
) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, _) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    // Both halves of one header, asserted whole. ş, ğ and ı are the characters DATA.md
    // §5.1 names as the entire reason this shape exists — a naive `filename=` mangles or
    // breaks the download — so the `filename*` half must carry them and the ASCII half
    // must degrade to something a browser can still write to disk.
    assert_eq!(
        header(&headers, "content-disposition"),
        Some(
            "attachment; filename=\"_aft yatak kapa__, LP-3120-05.stl\"; \
             filename*=UTF-8''%C5%9Eaft%20yatak%20kapa%C4%9F%C4%B1%2C%20LP-3120-05.stl"
        )
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unknown_variant_and_a_missing_one_are_refused_differently(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let state = AppState {
        db: pool,
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };

    let (converted, _, converted_body) = get(
        router(state.clone(), Role::Api),
        &download_uri(seeded.revision, "?variant=dxf"),
    )
    .await;
    let (absent, _, absent_body) =
        get(router(state, Role::Api), &download_uri(seeded.revision, "")).await;

    assert_eq!(converted, StatusCode::BAD_REQUEST);
    assert_eq!(absent, StatusCode::BAD_REQUEST);
    let converted_body = message(&converted_body);
    let absent_body = message(&absent_body);
    // Two different questions. Somebody who sent `variant=dxf` asked which downloads there
    // are; telling them to add a parameter they already sent answers a question they did
    // not ask. A single shared body would be the tidy version of exactly that.
    assert_ne!(converted_body, absent_body);
    assert!(
        converted_body.contains("`dxf` is not a download variant"),
        "the unknown-variant message names what was sent: {converted_body}"
    );
    assert!(
        absent_body.contains("This download needs a variant"),
        "the missing-variant message says one is required: {absent_body}"
    );
    // Both name the value that works — spec §2.2 forbids a silent fallback, which makes
    // saying what to send instead the whole job of these two messages.
    assert!(converted_body.contains("variant=original"));
    assert!(absent_body.contains("?variant=original"));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_variant_with_nothing_after_the_equals_reads_as_absent(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    // `?variant=` is the literal shape of a URL template whose value never got filled in,
    // so the useful answer is the one that says a variant is required — not `` `` is not
    // a download variant ``, which names nothing and helps nobody. Still a 400: reading it
    // as `original` would be the silent fallback spec §2.2 forbids.
    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        message(&body).contains("This download needs a variant"),
        "an empty value is a missing one, not an unknown one: {}",
        message(&body)
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_part_is_not_found_and_its_blob_stays_cold(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let state = |touches: &lapidary_db::Touches| AppState {
        db: pool.clone(),
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: touches.clone(),
    };

    // The same request before the delete serves the bytes. Without this leg the 404 below
    // would pass just as well against a route that never serves anything at all. Its reads
    // go to a `Touches` that is never flushed, so the column below starts empty.
    let (live, _, _) = get(
        router(state(&lapidary_db::Touches::default()), Role::Api),
        &download_uri(seeded.revision, "?variant=original"),
    )
    .await;
    assert_eq!(live, StatusCode::OK);

    assert!(
        lapidary_db::PgParts(pool.clone())
            .delete(seeded.part)
            .await
            .expect("delete")
    );

    // A download URL is held by whoever was last shown the grid: the link must stop serving
    // bytes rather than outlive the delete.
    let touches = lapidary_db::Touches::default();
    let (deleted, _, _) = get(
        router(state(&touches), Role::Api),
        &download_uri(seeded.revision, "?variant=original"),
    )
    .await;
    assert_eq!(deleted, StatusCode::NOT_FOUND);
    assert_eq!(
        last_read_us(&touches, &pool, &seeded.hash).await,
        None,
        "a request that served nothing is not somebody reading this blob — otherwise a \
         caller could warm any blob by asking for it"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_with_no_recorded_compression_level_is_refused_by_name(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    // Cleared directly, because this test is about the route's answer and not about how
    // the row got that way. Since migration `0013` no ingest path produces one — the level
    // is written on the `file` row from what `put_at` reported — so this is now a row from
    // outside, which is exactly the case spec §2.5.1's refusal is for. Reading it raw would
    // be a guess that happens to be wrong here, since the bytes on disk are a zstd frame.
    sqlx::query("UPDATE file SET zstd_level = NULL WHERE blake3 = $1 AND role = 'source'")
        .bind(seeded.hash.to_hex())
        .execute(&pool)
        .await
        .expect("clear the recorded level");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: touches.clone(),
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let body = message(&body);
    // Not merely "contains the hash": the hash-mismatch 500 below names a blob too, so a
    // route that collapsed these two branches would still satisfy that. The wording is
    // the part an operator can act on, so the wording is what is pinned.
    assert!(
        body.contains(&format!(
            "Blob {} has no recorded compression level",
            seeded.hash.to_hex()
        )),
        "the message must name the blob and say what is wrong with the row: {body}"
    );
    assert_eq!(
        last_read_us(&touches, &pool, &seeded.hash).await,
        None,
        "nothing was served, so nothing was read"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn bytes_that_do_not_hash_to_their_digest_truncate_the_download(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    // A different part's bytes, stored at the same level, then moved on top of this
    // blob's file behind the store's back. That is the shape spec §2.5 describes: a
    // `blob` row and a file on disk that disagree, which a crash between `write_blob`'s
    // rename and the transaction commit can leave behind. Decoding still succeeds — the
    // frame is valid — so only the re-hash can catch it.
    let other = legacy::put(
        root.path(),
        b"solid LP-9911-00\nendsolid LP-9911-00\n",
        legacy::Compression::Zstd,
    )
    .expect("stores other bytes");
    std::fs::copy(
        blob_file(root.path(), &other.hash),
        blob_file(root.path(), &seeded.hash),
    )
    .expect("overwrite the blob with bytes that are not it");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: touches.clone(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get_streaming(app, &download_uri(seeded.revision, "?variant=original")).await;

    // 200, and it is not a lie: at the moment the status line is written the route knows
    // the row is sound and the blob opened. What it cannot know yet is whether the bytes
    // still hash to their digest, because knowing that means reading all of them — which
    // is the buffering this route stopped doing. See `download.rs`'s header.
    assert_eq!(status, StatusCode::OK);

    // The promise the client is given, and the one the truncation breaks visibly.
    assert_eq!(
        header(&headers, "content-length"),
        Some(ascii_stl().len().to_string()).as_deref(),
        "the declared length is the ingested file's, so a short body is detectable"
    );

    let error = body.expect_err(
        "a blob that does not match its digest must end the body in an error, never \
         complete successfully with the wrong bytes",
    );
    assert!(
        error.contains(&seeded.hash.to_hex()) && error.contains(&other.hash.to_hex()),
        "the stream's error names both what was expected and what was found: {error}"
    );

    // The property that did NOT survive streaming, recorded rather than quietly dropped.
    // The touch used to sit after the hash check, so refused bytes stayed cold. It now
    // runs before the first chunk goes out, because there is no longer a point after the
    // check and before the response. A blob that was read is marked read, which is what
    // the column has always claimed to mean; slice 7's eviction inherits that.
    assert!(
        last_read_us(&touches, &pool, &seeded.hash).await.is_some(),
        "the blob was read, so it is warm — even though the read ended in a refusal"
    );
}

/// A file bigger than the whole channel can hold at once, so the reader must actually
/// block on a full channel and resume as the response drains it.
///
/// The body is 4 × 64 KiB in flight; this is ~1.5 MB, so it crosses that boundary about
/// twenty-four times. A chunking bug that dropped, duplicated or reordered a boundary
/// would survive every other test in this file, because all of them fit in one chunk.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_larger_than_the_stream_buffer_still_comes_back_byte_identical(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");

    // Real ASCII STL, repeated: compressible (so the zstd path is genuinely exercised)
    // but not uniform, so a chunk served twice would show up as a wrong digest rather
    // than being masked by identical bytes either side of the seam.
    let mut bytes = Vec::with_capacity(1_600_000);
    bytes.extend_from_slice(b"solid bracket-lp-1042-03\n");
    let mut facet = 0u32;
    while bytes.len() < 1_500_000 {
        bytes.extend_from_slice(
            format!(
                "facet normal 0 0 1\n  outer loop\n    vertex {facet}.0 0.0 0.0\n    \
                 vertex {facet}.5 1.0 0.0\n    vertex {facet}.0 1.0 0.5\n  endloop\nendfacet\n"
            )
            .as_bytes(),
        );
        facet += 1;
    }
    bytes.extend_from_slice(b"endsolid bracket-lp-1042-03\n");

    let seeded = seed(&pool, root.path(), "bracket-lp-1042-03", "stl", &bytes).await;
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Api,
    );

    let (status, headers, body) =
        get_streaming(app, &download_uri(seeded.revision, "?variant=original")).await;
    let body = body.expect("a sound blob streams to completion");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, bytes,
        "byte-identical across every chunk boundary, not merely the right length"
    );
    assert_eq!(
        header(&headers, "content-length"),
        Some(bytes.len().to_string()).as_deref(),
        "Content-Length is the uncompressed length, which is what was actually sent"
    );
    assert!(
        bytes.len() > 4 * 64 * 1024,
        "the fixture must exceed the channel's capacity or this test proves nothing \
         about resuming a blocked reader ({} bytes)",
        bytes.len()
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_downloads(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
        },
        Role::Worker,
    );

    // Resolvable, on disk, and still not served. Both images run this one binary, so a
    // route mounted unconditionally is a route the worker serves — and `deploy/web/
    // Caddyfile` proxies only to `api:8080`, so a download mounted there is unreachable
    // from the browser this slice exists to serve.
    let (status, _, _) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_repeated_variant_is_refused_rather_than_resolved(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let state = AppState {
        db: pool,
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };

    // The one shape that reaches the handler as a `QueryRejection`, and the reason that
    // arm is not dead code. A malformed percent-escape does not: `form_urlencoded`
    // decodes lossily, so `%ZZ` arrives as three literal characters and is answered as an
    // unknown variant. Both are pinned here because the route once carried a comment
    // claiming the opposite, and nothing tested it.
    let (repeated, _, repeated_body) = get(
        router(state.clone(), Role::Api),
        &download_uri(seeded.revision, "?variant=original&variant=3mf"),
    )
    .await;
    assert_eq!(repeated, StatusCode::BAD_REQUEST);
    let repeated_body = message(&repeated_body);
    assert!(
        repeated_body.contains("duplicate field") && repeated_body.contains("?variant=original"),
        "asking for two variants at once is refused, not resolved to one of them: \
         {repeated_body}"
    );

    let (escape, _, escape_body) = get(
        router(state, Role::Api),
        &download_uri(seeded.revision, "?variant=%ZZ"),
    )
    .await;
    assert_eq!(escape, StatusCode::BAD_REQUEST);
    assert!(
        message(&escape_body).contains("`%ZZ` is not a download variant"),
        "a broken escape survives decoding and is answered as the unknown variant it is"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_source_blob_missing_from_disk_is_its_own_500(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    // A volume that came back empty, which is the only way a referenced source blob goes
    // missing: nothing evicts one while a part points at it. Spec §2.5.2 makes this a
    // third 500 rather than the 404 `blob.rs` answers for the same shape, and the whole
    // difference is in what the caller is told to do next — a derivative regenerates, so
    // "reload the grid and try the part again" is true there and can never be true here.
    std::fs::remove_file(blob_file(root.path(), &seeded.hash)).expect("remove the blob file");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: touches.clone(),
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let body = message(&body);
    assert!(
        body.contains(&format!(
            "Blob {} could not be read from the blob store",
            seeded.hash.to_hex()
        )) && body.contains("Check that the blob volume is mounted"),
        "the message names the blob and the one thing an operator can go and look at: \
         {body}"
    );
    // Distinct from the other two 500s in wording, not only in cause. Collapsing any pair
    // of them leaves an operator holding a confident explanation of a problem they do not
    // have: nothing is wrong with this row, and nothing is wrong with these bytes.
    assert!(
        !body.contains("no recorded compression level")
            && !body.contains("are not the file that was ingested"),
        "neither the unrecorded-level 500 nor the hash-mismatch one: {body}"
    );
    // `StorageError` names a filesystem path. That is an operator's business and reaches
    // them through the log; a caller gets what to check, not where we keep it.
    assert!(
        !body.contains(root.path().to_str().expect("a utf-8 temp path")),
        "the blob store's path stays out of the response: {body}"
    );
    assert_eq!(
        last_read_us(&touches, &pool, &seeded.hash).await,
        None,
        "nothing was served, so nothing was read"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_path_addressed_source_file_missing_from_disk_says_where_to_look(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    // The folder-path twin of `a_source_blob_missing_from_disk_is_its_own_500`, and not a
    // duplicate of it: "the file for that hash" is the wrong thing to tell an operator
    // about a part whose bytes were never written to `blobs/ab/cd/<hash>` at all — nothing
    // put them there, so nothing there is what is missing. `unreadable` has to say a
    // different sentence for this layout, or it sends whoever reads it looking for a file
    // that was never going to exist.
    let root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/Fasteners/LP-3120-05/LP-3120-05.stl";
    let seeded = seed_at_path(&pool, root.path(), rel, TURKISH_NAME, "stl", &ascii_stl()).await;
    std::fs::remove_file(root.path().join(rel)).expect("remove the folder-path file");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: touches.clone(),
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let body = message(&body);
    assert!(
        body.contains(&format!(
            "Blob {} could not be read from the blob store",
            seeded.hash.to_hex()
        )),
        "still names the blob: {body}"
    );
    assert!(
        body.contains("model directory") && !body.contains("the file for that hash"),
        "the advice must point at the model directory, not the content-addressed path \
         nothing was ever written to: {body}"
    );
    assert!(
        !body.contains(root.path().to_str().expect("a utf-8 temp path")),
        "the store's own path stays out of the response, same as the other layout: {body}"
    );
    assert_eq!(
        last_read_us(&touches, &pool, &seeded.hash).await,
        None,
        "nothing was served, so nothing was read"
    );
}

/// **A stale bookmark gets a sentence, not a 500.** `?variant=l0|l1|l2` was the viewer's own
/// download while rungs existed, so the links are in people's history and in the e2e rig. Nothing
/// builds a rung now, so the three fall through to the unknown-variant refusal — and that sentence
/// must stop advertising them, or it answers "`l0` is not a download variant. This server serves
/// ... `l0`".
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_rung_variant_is_refused_in_words_that_no_longer_offer_a_rung(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let root = root.path();
    let seeded = seed(&pool, root, "Bracket, LP-1042-03", "stl", &ascii_stl()).await;
    let app = || {
        router(
            AppState {
                db: pool.clone(),
                blob_root: root.to_path_buf(),
                upload_dir: PathBuf::from("/nonexistent-upload-dir"),
                host_storage_root: None,
                ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
                inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
                host_ingest_root: None,
                host_inbox_root: None,
                touches: Default::default(),
            },
            Role::Api,
        )
    };

    for level in ["l0", "l1", "l2"] {
        let (status, _, body) = get(
            app(),
            &download_uri(seeded.revision, &format!("?variant={level}")),
        )
        .await;
        let body = message(&body);
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "`{level}` is not a variant this server has: {body}"
        );
        assert!(
            body.contains(&format!("`{level}` is not a download variant")),
            "and the refusal names what was asked for: {body}"
        );
        // **The whole point**, and it has to be asserted on what the sentence *offers* rather
        // than on the whole sentence: the refusal quotes `l0` back, legitimately, so a bare
        // `!contains("l0")` would be unsatisfiable. What must not name a rung is everything
        // after "This server serves".
        let (_, offers) = body
            .split_once("This server serves")
            .unwrap_or_else(|| panic!("the refusal says what is served: {body}"));
        for rung in ["l0", "l1", "l2", "tessellation"] {
            assert!(
                !offers.contains(rung),
                "the refusal still advertises {rung}: {offers}"
            );
        }
        assert!(
            offers.contains("variant=original"),
            "and it still says what does work: {offers}"
        );
    }
}

/// A format is a download of that one format (`DATA.md` §5.1): the part's own file when it is
/// already in it, else the mesh export Lapidary wrote — a 404 naming the route that builds one
/// not written yet — and a refusal, never a mesh, for a B-rep format.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_format_downloads_as_the_original_in_it_or_as_an_export_and_never_a_mesh_as_b_rep(
    pool: sqlx::PgPool,
) {
    let root = tempfile::tempdir().expect("temp dir");
    let root = root.path();
    let stl = ascii_stl();
    let seeded = seed(&pool, root, "Bracket, LP-1042-03", "stl", &stl).await;
    let app = || {
        router(
            AppState {
                db: pool.clone(),
                blob_root: root.to_path_buf(),
                upload_dir: PathBuf::from("/nonexistent-upload-dir"),
                host_storage_root: None,
                ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
                inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
                host_ingest_root: None,
                host_inbox_root: None,
                touches: Default::default(),
            },
            Role::Api,
        )
    };

    let (status, headers, body) = get(app(), &download_uri(seeded.revision, "?variant=stl")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, stl, "an STL asked for as an STL is its own bytes");
    assert!(
        header(&headers, "content-disposition").is_some_and(|value| value
            .contains("Bracket, LP-1042-03.stl")
            && !value.contains("lapidary")),
        "under its own name, since Lapidary built nothing: {headers:?}"
    );

    let (status, _, body) = get(app(), &download_uri(seeded.revision, "?variant=3mf")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        message(&body).contains("exports/3mf"),
        "the 404 names the route that builds the export"
    );

    let three_mf = b"PK\x03\x04 a 3MF Lapidary wrote from the bracket".to_vec();
    let stored = lapidary_storage::DerivativeStore::open(root)
        .put(&three_mf)
        .expect("stores the export");
    PgIngest(pool.clone())
        .upsert_derivative(
            seeded.revision,
            lapidary_core::DerivativeKind::Export3mf,
            &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            None,
            "mesh stl-1+cpu-1",
        )
        .await
        .expect("the export lands");
    let (status, headers, body) = get(app(), &download_uri(seeded.revision, "?variant=3mf")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, three_mf, "the export's own bytes");
    assert_eq!(
        header(&headers, "content-type"),
        Some("application/octet-stream")
    );
    assert!(
        header(&headers, "content-disposition")
            .is_some_and(|value| value.contains("Bracket, LP-1042-03.lapidary.3mf")),
        "named as something Lapidary built: {headers:?}"
    );

    let (status, _, body) = get(app(), &download_uri(seeded.revision, "?variant=step")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let refusal = message(&body);
    assert!(
        refusal.contains("B-rep") && refusal.contains("stl, 3mf"),
        "a mesh is never handed over as a STEP: {refusal}"
    );

    // OBJ is a mesh too, only not one Lapidary writes: refused for that, not for B-rep.
    let (status, _, body) = get(app(), &download_uri(seeded.revision, "?variant=obj")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let refusal = message(&body);
    assert!(
        refusal.contains("writes meshes only as 3mf and stl") && !refusal.contains("B-rep"),
        "{refusal}"
    );
}

// ---------------------------------------------------------------------------------------
// F2: `GET /api/parts/{id}/file` — one link per part, always its current file, with Range.
// ---------------------------------------------------------------------------------------

fn file_uri(part: PartId) -> String {
    format!("/api/parts/{part}/file")
}

async fn get_with(
    app: axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let mut request = Request::builder().uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body")
        .to_vec();
    (status, headers, body)
}

fn app_over(pool: &sqlx::PgPool, root: &Path) -> axum::Router {
    router(
        AppState {
            db: pool.clone(),
            blob_root: root.to_path_buf(),
            host_storage_root: None,
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
            upload_dir: PathBuf::from("/nonexistent-upload-dir"),
        },
        Role::Api,
    )
}

const COVER_PATH: &str = "libraries/default/cover-lp-3120-05/cover-lp-3120-05.stl";

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_parts_file_link_serves_its_bytes_whole_and_by_range(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let bytes = ascii_stl();
    let seeded = seed_at_path(
        &pool,
        root.path(),
        COVER_PATH,
        "cover-lp-3120-05",
        "stl",
        &bytes,
    )
    .await;
    let app = app_over(&pool, root.path());

    let (status, headers, body) = get(app.clone(), &file_uri(seeded.part)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes, "byte-identical to what was ingested");
    assert_eq!(
        BlobHash::from_bytes(*blake3::hash(&body).as_bytes()),
        seeded.hash
    );
    assert_eq!(header(&headers, "accept-ranges"), Some("bytes"));
    assert!(
        header(&headers, "content-disposition").is_some_and(|d| d.starts_with("attachment;")),
        "{headers:?}"
    );
    let etag = header(&headers, "etag").expect("an etag").to_owned();

    let (status, headers, body) = get_with(
        app.clone(),
        &file_uri(seeded.part),
        &[("range", "bytes=0-99")],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, bytes[..100]);
    assert_eq!(
        header(&headers, "content-range"),
        Some(format!("bytes 0-99/{}", bytes.len()).as_str())
    );
    assert_eq!(header(&headers, "content-length"), Some("100"));

    // The rest of it, resumed under the same ETag, puts the file back together.
    let (status, _, rest) = get_with(
        app.clone(),
        &file_uri(seeded.part),
        &[("range", "bytes=100-"), ("if-range", etag.as_str())],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!([&bytes[..100], rest.as_slice()].concat(), bytes);

    // A suffix range is the end of the file.
    let (_, _, tail) = get_with(
        app.clone(),
        &file_uri(seeded.part),
        &[("range", "bytes=-10")],
    )
    .await;
    assert_eq!(tail, bytes[bytes.len() - 10..]);

    // An If-Range naming another file gets the whole of this one, not a spliced half.
    let (status, _, body) = get_with(
        app.clone(),
        &file_uri(seeded.part),
        &[("range", "bytes=100-"), ("if-range", "\"not-this-file\"")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);

    // Past the end is 416, and says how long the file is.
    let past = format!("bytes={}-", bytes.len());
    let (status, headers, _) = get_with(
        app.clone(),
        &file_uri(seeded.part),
        &[("range", past.as_str())],
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        header(&headers, "content-range"),
        Some(format!("bytes */{}", bytes.len()).as_str())
    );

    // The revision's own download route takes ranges too: it is the same code.
    let (status, _, body) = get_with(
        app,
        &download_uri(seeded.revision, "?variant=original"),
        &[("range", "bytes=0-9")],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, bytes[..10]);
}

/// A file in the owner's folders, recorded where it is, as a scan records one: the path under the
/// ingest mount, and the size and modification time of what was read.
async fn seed_in_folders(pool: &sqlx::PgPool, folders: &Path, rel: &str, bytes: &[u8]) -> Seeded {
    let file = folders.join(rel);
    std::fs::create_dir_all(file.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&file, bytes).expect("the owner's file");
    let stat = lapidary_storage::LibraryReader::open(folders)
        .stat_at(rel)
        .expect("stats");
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: Some(rel),
            root: lapidary_core::FileRoot::Library,
            mtime_ns: Some(stat.mtime_ns),
            library: library(),
            name: "cover-lp-3120-05",
            source_path: rel,
            blob: &StoredBlobRow {
                hash,
                size_bytes: bytes.len() as u64,
                stored_bytes: bytes.len() as u64,
                zstd_level: 0,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("records");
    let revision = PgParts(pool.clone())
        .latest_revision(part)
        .await
        .expect("query")
        .expect("the ingested revision");
    Seeded {
        part,
        revision,
        hash,
        size_bytes: bytes.len() as u64,
        stored_bytes: bytes.len() as u64,
    }
}

fn app_over_folders(pool: &sqlx::PgPool, folders: &Path) -> axum::Router {
    router(
        AppState {
            db: pool.clone(),
            blob_root: PathBuf::from("/nonexistent-blob-root"),
            host_storage_root: None,
            ingest_dir: folders.to_path_buf(),
            inbox_dir: PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
            upload_dir: PathBuf::from("/nonexistent-upload-dir"),
        },
        Role::Api,
    )
}

const IN_FOLDERS: &str = "Covers/cover-lp-3120-05.stl";

/// A file in the owner's folders downloads from where it is, byte for byte.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_in_the_owners_folders_downloads_from_where_it_is(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let bytes = ascii_stl();
    let seeded = seed_in_folders(&pool, folders.path(), IN_FOLDERS, &bytes).await;
    let app = app_over_folders(&pool, folders.path());
    let (status, _, body) = get(app.clone(), &file_uri(seeded.part)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
}

/// (c) The owner edits the file in place after it was indexed: the download is refused before a
/// byte is sent, with the sentence that says what to do, on both links — and a bundle is refused
/// the same way.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_changed_in_its_folder_is_refused_with_the_sentence(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let bytes = ascii_stl();
    let seeded = seed_in_folders(&pool, folders.path(), IN_FOLDERS, &bytes).await;
    let mut edited = bytes.clone();
    edited.extend_from_slice(b"solid extra\nendsolid extra\n");
    std::fs::write(folders.path().join(IN_FOLDERS), &edited).expect("the owner edits it");

    let app = app_over_folders(&pool, folders.path());
    for uri in [
        file_uri(seeded.part),
        download_uri(seeded.revision, "?variant=original"),
    ] {
        let (status, _, body) = get(app.clone(), &uri).await;
        assert_eq!(status, StatusCode::CONFLICT, "{uri}");
        assert_eq!(
            message(&body),
            "This file changed in your folder since Lapidary last read it — scan again."
        );
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/libraries/{}/bundle/plan", library()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "parts": [seeded.part] }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// The same bytes with a new modification time are a change the pre-flight sees, and the end hash
/// is what would catch an edit that kept both: a file rewritten to the same length with its old
/// time put back is cut short rather than served whole.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_edit_that_keeps_size_and_time_is_cut_short_by_the_hash(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let bytes = ascii_stl();
    let seeded = seed_in_folders(&pool, folders.path(), IN_FOLDERS, &bytes).await;
    let path = folders.path().join(IN_FOLDERS);
    let modified = std::fs::metadata(&path)
        .expect("stats")
        .modified()
        .expect("an mtime");
    let mut same_length = bytes.clone();
    same_length[0] = b'S';
    std::fs::write(&path, &same_length).expect("edits in place");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("opens")
        .set_modified(modified)
        .expect("puts the time back");

    let response = app_over_folders(&pool, folders.path())
        .oneshot(
            Request::builder()
                .uri(file_uri(seeded.part))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the pre-flight cannot see this edit"
    );
    let read = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await;
    let error = read.expect_err("the body ends short instead of finishing");
    assert!(
        format!("{error:?}").contains("changed in your folder"),
        "{error:?}"
    );
}

/// A file moved, renamed or deleted in the folder since it was indexed says so, and what to do.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_gone_from_its_folder_says_so(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let seeded = seed_in_folders(&pool, folders.path(), IN_FOLDERS, &ascii_stl()).await;
    std::fs::remove_file(folders.path().join(IN_FOLDERS)).expect("the owner removes it");
    let (status, _, body) = get(
        app_over_folders(&pool, folders.path()),
        &file_uri(seeded.part),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message(&body).contains("scan again"), "{}", message(&body));
}

/// The owner overwrote the file, and the scan recorded it as revision 2 where it is: the part's link
/// gives the new bytes, and revision 1, whose bytes Lapidary never copied, answers 410.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_parts_file_link_follows_its_newest_revision(pool: sqlx::PgPool) {
    let folders = tempfile::tempdir().expect("the owner's folders");
    let first = ascii_stl();
    let seeded = seed_in_folders(&pool, folders.path(), IN_FOLDERS, &first).await;

    let mut second = first.clone();
    second.extend_from_slice(b"\nsolid revised\nendsolid revised\n");
    std::fs::write(folders.path().join(IN_FOLDERS), &second).expect("the owner saves over it");
    let stat = lapidary_storage::LibraryReader::open(folders.path())
        .stat_at(IN_FOLDERS)
        .expect("stats");
    let measured = measurements();
    lapidary_db::PgRevisions(pool.clone())
        .record_revision(lapidary_db::RevisionRequest {
            part: seeded.part,
            parent: seeded.revision,
            origin: lapidary_core::RevisionOrigin::Ingest,
            root: lapidary_core::FileRoot::Library,
            storage_path: IN_FOLDERS,
            mtime_ns: Some(stat.mtime_ns),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes(*blake3::hash(&second).as_bytes()),
                size_bytes: second.len() as u64,
                stored_bytes: second.len() as u64,
                zstd_level: 0,
            },
            measurements: &measured,
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("revision 2");

    let app = app_over_folders(&pool, folders.path());
    let (status, _, body) = get(app.clone(), &file_uri(seeded.part)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, second, "the same link now gives the new file");
    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::GONE);
    assert!(
        message(&body).contains("no longer on disk"),
        "{}",
        message(&body)
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_or_unknown_parts_file_link_is_a_404_that_says_what_to_do(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed_at_path(
        &pool,
        root.path(),
        COVER_PATH,
        "cover-lp-3120-05",
        "stl",
        &ascii_stl(),
    )
    .await;
    assert!(
        lapidary_db::PgParts(pool.clone())
            .delete(seeded.part)
            .await
            .expect("deletes")
    );
    let app = app_over(&pool, root.path());
    for part in [seeded.part, PartId::new()] {
        let (status, _, body) = get(app.clone(), &file_uri(part)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            message(&body).contains("copy its link again"),
            "{}",
            message(&body)
        );
    }
}
