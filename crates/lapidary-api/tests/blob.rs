//! `GET /api/blob/{blake3}`. Two of these are a security pair rather than a coverage
//! pair: `CLAUDE.md` says content addressing is not authorization, so a caller holding a
//! hash must not be able to learn from this endpoint whether those bytes are stored.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow, TessellationRow};
use lapidary_storage::DerivativeStore;
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

/// Bytes that stand in for a rung. Not real glTF: nothing here parses them, and a fake
/// that looked like glTF would invite a later reader to think something did.
const RUNG: &[u8] = b"pretend-this-is-a-glb";

/// The source file's hash in [`seed_reachable_rung`]. It gets a `blob` row like any other
/// blob, but nothing in `derivative` names it, so asking this route for it is refused at
/// the reachability gate -- which makes it the one hash that can prove where the touch
/// sits relative to that gate.
fn source_hash() -> BlobHash {
    BlobHash::from_bytes([0xb1; 32])
}

/// `blob.last_accessed_at` as epoch microseconds, `None` while the column is still NULL.
/// Microseconds because sqlx here carries neither `chrono` nor `time`, and the same
/// `extract(epoch ...)` trick `PgParts::page` uses is cheaper than adding one.
async fn last_read_us(pool: &sqlx::PgPool, hash: &BlobHash) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM last_accessed_at) * 1000000)::bigint FROM blob WHERE blake3 = $1",
    )
    .bind(hash.to_hex())
    .fetch_one(pool)
    .await
    .expect("the blob row exists")
}

/// Stores `RUNG` in the derivative store and records a part whose L0 points at it.
async fn seed_reachable_rung(pool: &sqlx::PgPool, root: &std::path::Path) -> BlobHash {
    let stored = DerivativeStore::open(root)
        .put(RUNG)
        .expect("stores the rung");
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &StoredBlobRow {
                hash: source_hash(),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &measurements(),
            thumbnail_webp: Some(b"the-thumbnail"),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &[TessellationRow {
                kind: "tessellation_l0",
                blob: StoredBlobRow {
                    hash: stored.hash,
                    size_bytes: stored.size_bytes,
                    stored_bytes: stored.stored_bytes,
                    zstd_level: stored.zstd_level,
                },
                grid: Some(32),
            }],
        })
        .await
        .expect("records");
    stored.hash
}

async fn get(app: axum::Router, hash: &str) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/blob/{hash}"))
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

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_referenced_blob_is_served_with_immutable_caching_and_an_etag(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, root.path()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
        },
        Role::Api,
    );

    let (status, headers, body) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, RUNG, "the bytes come back exactly as stored");
    assert_eq!(
        header(&headers, "cache-control"),
        Some("public, max-age=31536000, immutable"),
        "the URL contains the hash of the content, so the bytes at it cannot change"
    );
    assert_eq!(
        header(&headers, "etag"),
        Some(format!("\"{}\"", hash.to_hex()).as_str()),
        "quoted per RFC 9110, and strong because these are exact bytes"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_on_disk_that_no_derivative_references_is_not_found(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    // On disk, and nothing points at it -- the shape a stale or orphaned derivative
    // leaves behind. Knowing its hash must not be enough to read it.
    let stored = DerivativeStore::open(root.path())
        .put(b"bytes-nothing-references")
        .expect("stores");
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
        },
        Role::Api,
    );

    let (status, _, body) = get(app, &stored.hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        String::from_utf8_lossy(&body),
        r#"{"message":"No such file."}"#
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unknown_hash_is_not_found_with_the_same_body(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let present = DerivativeStore::open(root.path())
        .put(b"bytes-nothing-references")
        .expect("stores");
    let absent = BlobHash::from_bytes([0xee; 32]);
    let state = AppState {
        db: pool,
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    };

    let (unreferenced_status, _, unreferenced_body) =
        get(router(state.clone(), Role::Api), &present.hash.to_hex()).await;
    let (unknown_status, _, unknown_body) = get(router(state, Role::Api), &absent.to_hex()).await;

    // The point of the pair: a caller must not be able to tell "these bytes exist but you
    // may not have them" from "these bytes do not exist". Any difference -- status, body,
    // or a header -- is a confirmation oracle for arbitrary content.
    assert_eq!(unreferenced_status, unknown_status);
    assert_eq!(unreferenced_body, unknown_body);
    assert_eq!(unknown_status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_blobs(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, root.path()).await;
    let app = router(
        AppState {
            db: pool,
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
        },
        Role::Worker,
    );

    // Reachable, on disk, and still not served: the worker has no business handing bytes
    // to anyone. Both images run this one binary, so a route mounted unconditionally is
    // a route the worker serves.
    let (status, _, _) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn serving_a_blob_records_when_it_was_last_read(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, root.path()).await;
    let app = router(
        AppState {
            db: pool.clone(),
            blob_root: root.path().to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
        },
        Role::Api,
    );

    assert_eq!(
        last_read_us(&pool, &hash).await,
        None,
        "a blob nobody has read yet has never been touched"
    );

    let (status, _, _) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::OK);

    let read_at = last_read_us(&pool, &hash).await;
    assert!(
        read_at.is_some(),
        "handing the bytes to somebody is what this column records, got {read_at:?}"
    );
    // The other blob in this database, and it was never served. Asserted because the
    // whole value of the column is that it discriminates: an UPDATE that lost its WHERE
    // would mark every blob recently used and still pass the assertion above.
    assert_eq!(
        last_read_us(&pool, &source_hash()).await,
        None,
        "the source file was not the blob that was read"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn reading_a_blob_twice_moves_the_timestamp_forward(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, root.path()).await;
    let state = AppState {
        db: pool.clone(),
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    };

    let (first_status, _, _) = get(router(state.clone(), Role::Api), &hash.to_hex()).await;
    let first = last_read_us(&pool, &hash)
        .await
        .expect("the first read recorded a timestamp");
    let (second_status, _, _) = get(router(state, Role::Api), &hash.to_hex()).await;
    let second = last_read_us(&pool, &hash)
        .await
        .expect("the second read recorded a timestamp");

    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    // Strictly forward, and it cannot flake: `now()` is transaction-start time, each
    // touch is its own implicit transaction, and a request costs at least two round trips
    // to Postgres -- hundreds of microseconds against the column's one-microsecond
    // resolution. Folding the touch into the reachability transaction would make these
    // two equal, which is the regression this comparison exists to catch.
    assert!(
        second > first,
        "the second read must record a later instant than the first, got {second} after {first}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_that_is_not_served_is_not_recorded_as_read(pool: sqlx::PgPool) {
    // The bytes live somewhere the server is not looking, so the rung is reachable in the
    // database and absent from the store it serves -- an evicted derivative, exactly.
    let elsewhere = tempfile::tempdir().expect("temp dir");
    let served = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, elsewhere.path()).await;
    let state = AppState {
        db: pool.clone(),
        blob_root: served.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    };

    // Refused after the reachability check, when the bytes turn out not to be there.
    let (missing, _, _) = get(router(state.clone(), Role::Api), &hash.to_hex()).await;
    // Refused at the reachability check itself: a real blob row, and no derivative names
    // it. This is the request that pins the touch *after* the gate rather than before --
    // if it moved, guessing any stored hash would be enough to keep bytes looking warm.
    let (unreachable, _, _) = get(router(state, Role::Api), &source_hash().to_hex()).await;

    assert_eq!(missing, StatusCode::NOT_FOUND);
    assert_eq!(unreachable, StatusCode::NOT_FOUND);
    assert_eq!(
        last_read_us(&pool, &hash).await,
        None,
        "bytes that were never handed over were never read"
    );
    assert_eq!(
        last_read_us(&pool, &source_hash()).await,
        None,
        "a hash this route refuses to serve must not be touchable by asking for it"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_head_request_warms_last_accessed_at_the_way_a_get_does(pool: sqlx::PgPool) {
    // Carried from slice 6a as an open item, and slice 7 is where it lands because
    // `last_accessed_at` is a lifecycle column: Phase 4's tiering job reads it to decide
    // what to move to cold storage, and a client that checks a blob is present is a client
    // using that blob. Tiering out bytes in active use because the only requests for them
    // were HEADs is the failure this prevents, before there is a job to prevent it for.
    //
    // Nothing had to be written to make it true. `axum::routing::get` answers HEAD with the
    // same handler and strips the body, so the touch was always on this path — this test
    // exists because that is a property of a routing helper, invisible at the call site,
    // and swapping `get` for an explicit `on(MethodFilter::GET, ...)` would silently take
    // it away.
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_rung(&pool, root.path()).await;
    let state = AppState {
        db: pool.clone(),
        blob_root: root.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    };
    assert_eq!(last_read_us(&pool, &hash).await, None);

    let response = router(state, Role::Api)
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri(format!("/api/blob/{}", hash.to_hex()))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::ETAG)
            .and_then(|v| v.to_str().ok()),
        Some(format!("\"{}\"", hash.to_hex()).as_str()),
        "a HEAD answers with the headers a GET would, which is what it is for"
    );
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body reads");
    assert!(body.is_empty(), "HEAD carries no body");
    assert!(
        last_read_us(&pool, &hash).await.is_some(),
        "a HEAD is somebody using these bytes, and the column that decides what gets \
         tiered out has to know it"
    );
}
