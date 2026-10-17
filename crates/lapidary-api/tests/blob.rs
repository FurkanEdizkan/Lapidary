//! `GET /api/blob/{blake3}`. Two of these are a security pair rather than a coverage
//! pair: `CLAUDE.md` says content addressing is not authorization, so a caller holding a
//! hash must not be able to learn from this endpoint whether those bytes are stored.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
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

/// Bytes that stand in for a mesh export. Not real STL: nothing here parses them, and a fake
/// that looked like one would invite a later reader to think something did.
const RUNG: &[u8] = b"pretend-this-is-a-glb";

/// The source file's hash in [`seed_reachable_export`]. It gets a `blob` row like any other
/// blob, but nothing in `derivative` names it, so asking this route for it is refused at
/// the reachability gate -- which makes it the one hash that can prove where the touch
/// sits relative to that gate.
fn source_hash() -> BlobHash {
    BlobHash::from_bytes([0xb1; 32])
}

/// `blob.last_accessed_at` as epoch microseconds, `None` while the column is still NULL.
/// Microseconds because sqlx here carries neither `chrono` nor `time`, and the same
/// `extract(epoch ...)` trick `PgParts::page` uses is cheaper than adding one.
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

/// Stores `RUNG` in the derivative store and records a part whose STL export points at it.
///
/// Two steps since R3: ingest writes no derivative at all, so the part is recorded and the
/// derivative is upserted after — which is what a `derive` job does.
async fn seed_reachable_export(pool: &sqlx::PgPool, root: &std::path::Path) -> BlobHash {
    let stored = DerivativeStore::open(root)
        .put(RUNG)
        .expect("stores the export");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: None,
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
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
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("records");
    file_export(pool, part, &stored).await;
    stored.hash
}

/// One export onto a part's latest revision, as a `derive` job files one.
async fn file_export(
    pool: &sqlx::PgPool,
    part: lapidary_core::PartId,
    stored: &lapidary_storage::StoredBlob,
) {
    let revision: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM revision WHERE part_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(part.as_uuid())
            .fetch_one(pool)
            .await
            .expect("the revision");
    PgIngest(pool.clone())
        .upsert_derivative(
            lapidary_core::RevisionId::from_uuid(revision),
            lapidary_core::DerivativeKind::ExportStl,
            &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            None,
            "mesh stl-1+glb-2+cpu-1",
        )
        .await
        .expect("the export lands");
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

/// An earlier revision's export stays served once a newer revision is current, because
/// reachability asks whether any revision of a live part points at the bytes, not whether the
/// newest one does.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_earlier_revisions_export_is_still_served_once_a_newer_one_is_current(
    pool: sqlx::PgPool,
) {
    let root = tempfile::tempdir().expect("temp dir");
    let earlier = DerivativeStore::open(root.path())
        .put(RUNG)
        .expect("stores the export");
    // Filed in its model directory, as every file ingest writes now: a revision is recorded
    // only on top of a file with a `storage_path` to set aside.
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: Some("libraries/default/bracket-lp-1042-03/bracket-lp-1042-03.stl"),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &StoredBlobRow {
                hash: source_hash(),
                size_bytes: 204_800,
                stored_bytes: 204_800,
                zstd_level: 0,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("revision 1");
    file_export(&pool, part, &earlier).await;
    let earlier = earlier.hash;
    let revisions = lapidary_db::PgRevisions(pool.clone());
    let current = revisions
        .current(library(), "bracket-lp-1042-03.stl")
        .await
        .expect("reads")
        .expect("the part");
    let newer = DerivativeStore::open(root.path())
        .put(b"pretend-this-is-the-wider-bracket")
        .expect("stores the newer export");
    revisions
        .record_revision(lapidary_db::RevisionRequest {
            part: current.part,
            parent: current.revision,
            origin: lapidary_core::RevisionOrigin::Agent,
            root: lapidary_core::FileRoot::Store,
            storage_path: "libraries/default/bracket-lp-1042-03/bracket-lp-1042-03.stl",
            mtime_ns: None,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([0xb2; 32]),
                size_bytes: 214_016,
                stored_bytes: 214_016,
                zstd_level: 0,
            },
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("revision 2");
    file_export(&pool, part, &newer).await;
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

    let (status, _, body) = get(app, &earlier.to_hex()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, RUNG);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_referenced_blob_is_served_with_immutable_caching_and_an_etag(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_export(&pool, root.path()).await;
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
            ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
            inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
            host_ingest_root: None,
            host_inbox_root: None,
            touches: Default::default(),
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
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
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
    let hash = seed_reachable_export(&pool, root.path()).await;
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

    // Reachable, on disk, and still not served: the worker has no business handing bytes
    // to anyone. Both images run this one binary, so a route mounted unconditionally is
    // a route the worker serves.
    let (status, _, _) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn serving_a_blob_records_when_it_was_last_read(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_export(&pool, root.path()).await;
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

    assert_eq!(
        last_read_us(&touches, &pool, &hash).await,
        None,
        "a blob nobody has read yet has never been touched"
    );

    let (status, _, _) = get(app, &hash.to_hex()).await;
    assert_eq!(status, StatusCode::OK);

    let read_at = last_read_us(&touches, &pool, &hash).await;
    assert!(
        read_at.is_some(),
        "handing the bytes to somebody is what this column records, got {read_at:?}"
    );
    // The other blob in this database, and it was never served. Asserted because the
    // whole value of the column is that it discriminates: an UPDATE that lost its WHERE
    // would mark every blob recently used and still pass the assertion above.
    assert_eq!(
        last_read_us(&touches, &pool, &source_hash()).await,
        None,
        "the source file was not the blob that was read"
    );
}

/// Day precision, `DATA.md` §1.4: a blob read again the same day is not written again, which is
/// what keeps a busy afternoon from being a write per read.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn reading_a_blob_again_the_same_day_does_not_write_it_again(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    let root = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_export(&pool, root.path()).await;
    let state = AppState {
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

    let (first_status, _, _) = get(router(state.clone(), Role::Api), &hash.to_hex()).await;
    let first = last_read_us(&touches, &pool, &hash)
        .await
        .expect("the first read recorded a timestamp");
    let (second_status, _, _) = get(router(state, Role::Api), &hash.to_hex()).await;
    let second = last_read_us(&touches, &pool, &hash)
        .await
        .expect("still recorded");

    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(second, first, "the same day's second read writes nothing");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_that_is_not_served_is_not_recorded_as_read(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
    // The bytes live somewhere the server is not looking, so the rung is reachable in the
    // database and absent from the store it serves -- an evicted derivative, exactly.
    let elsewhere = tempfile::tempdir().expect("temp dir");
    let served = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_export(&pool, elsewhere.path()).await;
    let state = AppState {
        db: pool.clone(),
        blob_root: served.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: touches.clone(),
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
        last_read_us(&touches, &pool, &hash).await,
        None,
        "bytes that were never handed over were never read"
    );
    assert_eq!(
        last_read_us(&touches, &pool, &source_hash()).await,
        None,
        "a hash this route refuses to serve must not be touchable by asking for it"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_head_request_warms_last_accessed_at_the_way_a_get_does(pool: sqlx::PgPool) {
    let touches = lapidary_db::Touches::default();
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
    let hash = seed_reachable_export(&pool, root.path()).await;
    let state = AppState {
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
    assert_eq!(last_read_us(&touches, &pool, &hash).await, None);

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
        last_read_us(&touches, &pool, &hash).await.is_some(),
        "a HEAD is somebody using these bytes, and the column that decides what gets \
         tiered out has to know it"
    );
}

/// An export the database points at and the store has lost is rebuilt from its source, and the
/// answer says so. Asking twice builds it once.
///
/// **The promise has to be keepable.** `rebuild` asked `rungs_for_blob` before R3, so after it a
/// lost rung would have been answered "being rebuilt" by a pipeline that no longer exists. It
/// asks `exports_for_blob` now, and a rung's bytes get the other sentence.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_missing_export_is_queued_for_rebuilding_and_says_so(pool: sqlx::PgPool) {
    let elsewhere = tempfile::tempdir().expect("temp dir");
    let served = tempfile::tempdir().expect("temp dir");
    let hash = seed_reachable_export(&pool, elsewhere.path()).await;
    let state = AppState {
        db: pool.clone(),
        blob_root: served.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };

    for _ in 0..2 {
        let (status, _, body) = get(router(state.clone(), Role::Api), &hash.to_hex()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let json: serde_json::Value = serde_json::from_slice(&body).expect("a JSON body");
        assert!(
            json["message"]
                .as_str()
                .is_some_and(|m| m.contains("being rebuilt")),
            "{json}"
        );
    }

    let revision: sqlx::types::Uuid = sqlx::query_scalar("SELECT id FROM revision")
        .fetch_one(&pool)
        .await
        .expect("one revision");
    let payloads: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM job WHERE kind = 'derive'")
            .fetch_all(&pool)
            .await
            .expect("derive jobs");
    assert_eq!(
        payloads,
        vec![serde_json::json!({ "revision": revision.to_string(), "produce": "export_stl" })],
        "one rebuild of the lost export"
    );
}

/// **A rung's bytes are not rebuildable, and the route must not promise they are.**
///
/// `rebuild` asked `rungs_for_blob` before R3. Keep that and a lost rung is answered "that file
/// is being rebuilt" by a pipeline that no longer exists — the viewer waits for a job that
/// cannot run, and `enqueue_derive_if_absent` leaves a `derive` row naming a kind nothing builds.
/// So the other sentence, and no job at all.
///
/// The row is written as raw SQL: `tessellation_l0` is not a kind this build can name, and it is
/// what a library that pre-dates the pivot is full of until migration `0055` (R4) deletes them.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_missing_rung_is_not_promised_a_rebuild_and_queues_nothing(pool: sqlx::PgPool) {
    let elsewhere = tempfile::tempdir().expect("temp dir");
    let served = tempfile::tempdir().expect("temp dir");
    // A part with a real revision, and a rung row pointing at bytes only `elsewhere` holds.
    let hash = seed_reachable_export(&pool, elsewhere.path()).await;
    let rung = DerivativeStore::open(elsewhere.path())
        .put(b"glTF\x02\x00\x00\x00 a rung an older pipeline wrote")
        .expect("stores the rung");
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
         VALUES ($1, $2, $2, NULL, 1) ON CONFLICT (blake3) DO NOTHING",
    )
    .bind(rung.hash.to_hex())
    .bind(rung.size_bytes as i64)
    .execute(&pool)
    .await
    .expect("the rung's blob row");
    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, blake3, kernel_version, params_json) \
         SELECT uuidv7(), id, 'tessellation_l0', $1, 'mesh stl-1+glb-1+cpu-1', '{}'::jsonb \
           FROM revision ORDER BY id DESC LIMIT 1",
    )
    .bind(rung.hash.to_hex())
    .execute(&pool)
    .await
    .expect("the rung's row");

    let state = AppState {
        db: pool.clone(),
        blob_root: served.path().to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };

    let (status, _, body) = get(router(state.clone(), Role::Api), &rung.hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let json: serde_json::Value = serde_json::from_slice(&body).expect("a JSON body");
    let message = json["message"].as_str().expect("a sentence");
    assert!(
        message.contains("cannot be rebuilt on its own"),
        "a rung is not rebuildable and the sentence has to say so: {message}"
    );
    assert!(
        !message.contains("being rebuilt"),
        "and must not promise a rebuild nothing will do: {message}"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("counts the queue");
    assert_eq!(
        queued, 0,
        "and queues nothing, because nothing could run it"
    );

    // The export beside it, same part, is rebuildable — so this is a discrimination assertion
    // and not merely the absence of a feature.
    let (status, _, body) = get(router(state, Role::Api), &hash.to_hex()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        message_of(&body).contains("being rebuilt"),
        "{}",
        message_of(&body)
    );
}

fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json["message"].as_str().map(str::to_owned))
        .unwrap_or_default()
}
