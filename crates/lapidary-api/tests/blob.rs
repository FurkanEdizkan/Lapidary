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

/// Stores `RUNG` in the derivative store and records a part whose L0 points at it.
async fn seed_reachable_rung(pool: &sqlx::PgPool, root: &std::path::Path) -> BlobHash {
    let stored = DerivativeStore::open(root)
        .put(RUNG)
        .expect("stores the rung");
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0xb1; 32]),
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
        },
        Role::Worker,
    );

    // Reachable, on disk, and still not served: the worker has no business handing bytes
    // to anyone. Both images run this one binary, so a route mounted unconditionally is
    // a route the worker serves.
    let (status, _, _) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
