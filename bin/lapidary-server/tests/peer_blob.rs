//! The blob route (sharing S3): a shared file's bytes, whole or resumed, over real stored — and compressed — bytes.
//! Here for `peer_sync.rs`'s reason: `#[sqlx::test]`, and `deny.toml`'s list of who may take `sqlx`.

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use lapidary_core::{DeviceId, FolderId, LibraryId, MeshMeasurements, ShareId};
use lapidary_db::{IngestRequest, PgFolders, PgIngest, PgShares, PgSharing, StoredBlobRow};
use lapidary_peer::PeerDevice;
use lapidary_storage::{Compression, SourceWriter};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

/// An ASCII STL of a standing stone, big enough that a resumed request has real bytes to skip.
fn standing_stone(label: &str) -> Vec<u8> {
    let mut stl = format!("solid {label}\n").into_bytes();
    for facet in 0..1_800 {
        let z = f64::from(facet) * 0.05;
        stl.extend_from_slice(
            format!(
                "facet normal 0 0 1\n outer loop\n  vertex 0 0 {z:.3}\n  vertex 12.5 0 {z:.3}\n  vertex 0 18.25 {z:.3}\n endloop\nendfacet\n"
            )
            .as_bytes(),
        );
    }
    stl.extend_from_slice(format!("endsolid {label}\n").as_bytes());
    stl
}

/// Store `bytes` for real, compressed as an STL is, and file a part for them under `folder`.
async fn filed(
    pool: &sqlx::PgPool,
    root: &std::path::Path,
    folder: FolderId,
    name: &str,
    bytes: &[u8],
) -> String {
    let staged = root.join(format!("{name}.staged"));
    std::fs::write(&staged, bytes).expect("stages the file");
    let hash = lapidary_core::BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let stored = SourceWriter::open(root)
        .put_file(&staged, &hash, Compression::for_source_format("stl"))
        .expect("stores it");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: Some(folder),
            storage_path: None,
            library: library(),
            name,
            source_path: &format!("{name}.stl"),
            blob: &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [12.5, 18.25, 90.0],
                triangle_count: 1_800,
                surface_area_mm2: 20_531.25,
                volume_mm3: None,
                is_watertight: false,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    hash.to_hex()
}

struct Shared {
    root: tempfile::TempDir,
    share: ShareId,
    inside: (String, Vec<u8>),
    outside: String,
}

async fn shared(pool: &sqlx::PgPool) -> Shared {
    let root = tempfile::tempdir().expect("a store");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    let stone = standing_stone("standing-stone-lp-tr-0140");
    let inside = filed(
        pool,
        root.path(),
        terrain,
        "Standing stone, LP-TR-0140",
        &stone,
    )
    .await;
    let outside = filed(
        pool,
        root.path(),
        bases,
        "Round base 32 mm, LP-BS-0032",
        &standing_stone("round-base"),
    )
    .await;
    let share = PgShares(pool.clone())
        .create(library(), terrain)
        .await
        .expect("shares")
        .expect("live")
        .id;
    PgSharing(pool.clone())
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("pairs");
    Shared {
        root,
        share,
        inside: (inside, stone),
        outside,
    }
}

async fn get(
    pool: &sqlx::PgPool,
    root: &std::path::Path,
    device: DeviceId,
    uri: &str,
    range: Option<&str>,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut request = Request::builder().uri(uri);
    if let Some(range) = range {
        request = request.header(header::RANGE, range);
    }
    let response = lapidary_peer::blob::blob_router(pool.clone(), root.to_path_buf())
        .layer(MockConnectInfo(PeerDevice(Some(device))))
        .oneshot(request.body(Body::empty()).expect("request builds"))
        .await
        .expect("router responds");
    let status = response.status();
    let content_range = response
        .headers()
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .expect("body reads");
    (status, content_range, body.to_vec())
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_shared_file_is_served_whole_and_resumed_from_an_offset(pool: sqlx::PgPool) {
    let shared = shared(&pool).await;
    let (hash, bytes) = &shared.inside;
    let uri = format!("/peer/v1/shares/{}/blob/{hash}", shared.share.as_uuid());

    let (status, _, whole) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(whole.len(), bytes.len());
    assert!(whole == *bytes, "exactly the stored bytes, decompressed");

    let (status, content_range, tail) = get(
        &pool,
        shared.root.path(),
        ayse(),
        &uri,
        Some("bytes=10000-"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        content_range,
        Some(format!("bytes 10000-{}/{}", bytes.len() - 1, bytes.len()))
    );
    assert!(
        tail == bytes[10_000..],
        "the rest, from where the staged file stopped"
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_file_one_category_outside_the_share_is_refused(pool: sqlx::PgPool) {
    let shared = shared(&pool).await;
    let uri = format!(
        "/peer/v1/shares/{}/blob/{}",
        shared.share.as_uuid(),
        shared.outside
    );
    let (status, _, body) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let refusal: serde_json::Value = serde_json::from_slice(&body).expect("a refusal");
    assert_eq!(
        refusal["reason"], "notInShare",
        "a real stored file, and not one this share offers"
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_stranger_gets_no_file(pool: sqlx::PgPool) {
    let shared = shared(&pool).await;
    let uri = format!(
        "/peer/v1/shares/{}/blob/{}",
        shared.share.as_uuid(),
        shared.inside.0
    );
    let stranger = DeviceId::from_public_key(b"a key nobody here paired with");
    let (status, _, body) = get(&pool, shared.root.path(), stranger, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let refusal: serde_json::Value = serde_json::from_slice(&body).expect("a refusal");
    assert_eq!(refusal["reason"], "notShared");
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_range_past_the_end_is_refused(pool: sqlx::PgPool) {
    let shared = shared(&pool).await;
    let (hash, bytes) = &shared.inside;
    let uri = format!("/peer/v1/shares/{}/blob/{hash}", shared.share.as_uuid());
    let past = format!("bytes={}-", bytes.len() + 1);
    let (status, _, _) = get(&pool, shared.root.path(), ayse(), &uri, Some(&past)).await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
}

async fn ask(
    pool: &sqlx::PgPool,
    device: DeviceId,
    share: ShareId,
) -> (StatusCode, serde_json::Value) {
    let response = lapidary_peer::shares::shares_router(pool.clone())
        .layer(MockConnectInfo(PeerDevice(Some(device))))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/peer/v1/shares/{}/request", share.as_uuid()))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

fn reason(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body).expect("a refusal")["reason"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_share_that_asks_first_sends_files_only_once_its_owner_grants_them(pool: sqlx::PgPool) {
    let shared = shared(&pool).await;
    let shares = PgShares(pool.clone());
    shares
        .set_asks_first(shared.share, true)
        .await
        .expect("asks first");
    let uri = format!(
        "/peer/v1/shares/{}/blob/{}",
        shared.share.as_uuid(),
        shared.inside.0
    );

    let (status, _, body) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::FORBIDDEN, "askFirst".to_owned())
    );

    let (status, answer) = ask(&pool, ayse(), shared.share).await;
    assert_eq!(
        (status, &answer["grant"]),
        (StatusCode::ACCEPTED, &serde_json::json!("asked"))
    );
    let (status, _, body) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::FORBIDDEN, "askFirst".to_owned())
    );

    shares
        .decide(shared.share, ayse(), true)
        .await
        .expect("grants");
    let (status, _, body) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body == shared.inside.1);
    let (_, answer) = ask(&pool, ayse(), shared.share).await;
    assert_eq!(answer["grant"], "granted");

    shares
        .decide(shared.share, ayse(), false)
        .await
        .expect("denies");
    let (status, _, body) = get(&pool, shared.root.path(), ayse(), &uri, None).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::FORBIDDEN, "denied".to_owned())
    );

    let stranger = DeviceId::from_public_key(b"a key nobody here paired with");
    let (status, answer) = ask(&pool, stranger, shared.share).await;
    assert_eq!(
        (status, &answer["reason"]),
        (StatusCode::NOT_FOUND, &serde_json::json!("notShared"))
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn past_two_files_to_one_installation_or_eight_in_all_the_next_is_told_to_try_again(
    pool: sqlx::PgPool,
) {
    let shared = shared(&pool).await;
    let uri = format!(
        "/peer/v1/shares/{}/blob/{}",
        shared.share.as_uuid(),
        shared.inside.0
    );
    let streams = lapidary_peer::blob::Streams::default();
    let send = |streams: lapidary_peer::blob::Streams| {
        let (pool, root, uri) = (pool.clone(), shared.root.path().to_path_buf(), uri.clone());
        async move {
            let response = lapidary_peer::blob::blob_router_with(pool, root, streams)
                .layer(MockConnectInfo(PeerDevice(Some(ayse()))))
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request builds"),
                )
                .await
                .expect("router responds");
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
                .await
                .expect("body reads");
            (status, body.to_vec())
        }
    };

    let held: Vec<_> = (0..2)
        .map(|_| streams.take(ayse()).expect("two to one installation"))
        .collect();
    let (status, body) = send(streams.clone()).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::TOO_MANY_REQUESTS, "busy".to_owned())
    );

    drop(held);
    let (status, _) = send(streams.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // Its stream ended with its body, so both are free again.
    let again: Vec<_> = (0..2)
        .map(|_| {
            streams
                .take(ayse())
                .expect("released when the file was sent")
        })
        .collect();
    drop(again);

    // A file still being sent keeps its count: its body is not read yet, so the sending task is still at work.
    let unread = lapidary_peer::blob::blob_router_with(
        pool.clone(),
        shared.root.path().to_path_buf(),
        streams.clone(),
    )
    .layer(MockConnectInfo(PeerDevice(Some(ayse()))))
    .oneshot(
        Request::builder()
            .uri(uri.clone())
            .body(Body::empty())
            .expect("request builds"),
    )
    .await
    .expect("router responds");
    assert_eq!(unread.status(), StatusCode::OK);
    let one = streams.take(ayse()).expect("one of two is free");
    assert!(
        streams.take(ayse()).is_none(),
        "the file being sent holds the other"
    );
    drop((one, unread));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        // Both at once: one alone was free all along.
        while (streams.take(ayse()), streams.take(ayse())).1.is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a dropped body gives its count back");

    let others: Vec<_> = (0u8..8)
        .map(|n| {
            streams
                .take(DeviceId::from_bytes([n; 32]))
                .expect("eight in all")
        })
        .collect();
    let (status, body) = send(streams.clone()).await;
    assert_eq!(
        (status, reason(&body)),
        (StatusCode::TOO_MANY_REQUESTS, "busy".to_owned())
    );
    drop(others);
}
