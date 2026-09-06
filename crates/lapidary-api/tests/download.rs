//! `GET /api/revisions/{id}/download?variant=original`.
//!
//! Every fixture here is written by `SourceStore::put` and recorded from the `StoredBlob`
//! it returns — hash, sizes and level all four. Typing a level literal instead is how a
//! test comes to describe bytes that were never written that way: the `blob` row would
//! say 3 over a raw ZIP, the reader would try to decode it, and the failure would have
//! nothing to do with the route. The db crate's own fixtures can afford that shorthand
//! because nothing there reads bytes; this file cannot.
//!
//! The compressed leg and the `AsIs` leg are not a coverage pair. They take genuinely
//! different paths through spec §2.5 — one decodes, one does not — and only the first can
//! detect a reader that stopped decoding, which is the drift the byte-identity claim
//! exists to catch.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, RevisionId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};
use lapidary_storage::{Compression, SourceStore, WorkerRole};
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
/// reason: `Compression::for_source_format("stl")` compresses, so these bytes reach disk
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
    let stored = SourceStore::open(root, &WorkerRole::assume())
        .put(bytes, Compression::for_source_format(format))
        .expect("stores the source file");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
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
            thumbnail_webp: Some(b"the-thumbnail"),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format,
            tessellations: &[],
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
async fn last_read_us(pool: &sqlx::PgPool, hash: &BlobHash) -> Option<i64> {
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
async fn a_turkish_name_is_percent_encoded_in_one_half_and_degraded_in_the_other(
    pool: sqlx::PgPool,
) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
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
    };

    let (converted, _, converted_body) = get(
        router(state.clone(), Role::Api),
        &download_uri(seeded.revision, "?variant=3mf"),
    )
    .await;
    let (absent, _, absent_body) =
        get(router(state, Role::Api), &download_uri(seeded.revision, "")).await;

    assert_eq!(converted, StatusCode::BAD_REQUEST);
    assert_eq!(absent, StatusCode::BAD_REQUEST);
    let converted_body = message(&converted_body);
    let absent_body = message(&absent_body);
    // Two different questions. Somebody who sent `variant=3mf` asked where converted
    // downloads are; telling them to add a parameter they already sent answers a question
    // they did not ask. A single shared body would be the tidy version of exactly that.
    assert_ne!(converted_body, absent_body);
    assert!(
        converted_body.contains("`3mf` is not a download variant"),
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
async fn a_soft_deleted_part_is_not_found_and_its_blob_stays_cold(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(seeded.part.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");
    let state = AppState {
        db: pool.clone(),
        blob_root: root.path().to_path_buf(),
    };

    // Delete is soft, and a download URL is held by whoever was last shown the grid: the
    // link must stop serving bytes rather than outlive the delete.
    let (deleted, _, _) = get(
        router(state.clone(), Role::Api),
        &download_uri(seeded.revision, "?variant=original"),
    )
    .await;
    assert_eq!(deleted, StatusCode::NOT_FOUND);
    assert_eq!(
        last_read_us(&pool, &seeded.hash).await,
        None,
        "a request that served nothing is not somebody reading this blob — otherwise a \
         caller could warm any blob by asking for it"
    );

    // The same request, once the part is back. Without this leg the 404 above would pass
    // just as well against a route that never serves anything at all.
    sqlx::query("UPDATE part SET deleted_at = NULL WHERE id = $1")
        .bind(seeded.part.as_uuid())
        .execute(&pool)
        .await
        .expect("restore");
    let (restored, _, _) = get(
        router(state, Role::Api),
        &download_uri(seeded.revision, "?variant=original"),
    )
    .await;
    assert_eq!(restored, StatusCode::OK);
    assert!(
        last_read_us(&pool, &seeded.hash).await.is_some(),
        "handing the bytes to somebody is what this column records"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_with_no_recorded_compression_level_is_refused_by_name(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    // Cleared directly, because this test is about the route's answer and not about how
    // the row got that way — lapidary-db's
    // `a_source_blob_whose_level_nobody_recorded_reads_as_uncompressed` covers the ingest
    // path that produces one. Spec §2.5.1. Reading it raw would be a guess that happens
    // to be wrong here, since the bytes on disk are a zstd frame.
    sqlx::query("UPDATE blob SET zstd_level = NULL WHERE blake3 = $1")
        .bind(seeded.hash.to_hex())
        .execute(&pool)
        .await
        .expect("clear the recorded level");
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
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
        last_read_us(&pool, &seeded.hash).await,
        None,
        "nothing was served, so nothing was read"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn bytes_that_do_not_hash_to_their_digest_are_refused_as_bytes(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let seeded = seed(&pool, root.path(), TURKISH_NAME, "stl", &ascii_stl()).await;
    // A different part's bytes, stored at the same level, then moved on top of this
    // blob's file behind the store's back. That is the shape spec §2.5 describes: a
    // `blob` row and a file on disk that disagree, which a crash between `write_blob`'s
    // rename and the transaction commit can leave behind. Decoding still succeeds — the
    // frame is valid — so only the re-hash can catch it.
    let other = SourceStore::open(root.path(), &WorkerRole::assume())
        .put(
            b"solid LP-9911-00\nendsolid LP-9911-00\n",
            Compression::Zstd,
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
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &download_uri(seeded.revision, "?variant=original")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let body = message(&body);
    assert!(
        body.contains(&format!(
            "The bytes stored for blob {}",
            seeded.hash.to_hex()
        )) && body.contains(&other.hash.to_hex()),
        "the message names both what was expected and what was found: {body}"
    );
    assert!(
        !body.contains("no recorded compression level"),
        "this is the other 500 — the row is fine and the bytes are not: {body}"
    );
    assert_eq!(
        last_read_us(&pool, &seeded.hash).await,
        None,
        "the touch sits after the hash check, so bytes that were refused are not warm"
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
        last_read_us(&pool, &seeded.hash).await,
        None,
        "nothing was served, so nothing was read"
    );
}
