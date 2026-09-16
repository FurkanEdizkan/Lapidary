//! The share routes (sharing S2a), as a paired installation reads them. Here rather than in
//! `crates/lapidary-peer/tests` for `peer_sync.rs`'s reason: `#[sqlx::test]`, and `deny.toml`'s list of who may
//! take `sqlx`. Each request says which installation is asking through `MockConnectInfo`, which is where the
//! TLS layer puts the key its handshake proved (`lapidary_peer::PeerDevice`).

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use lapidary_core::{BlobHash, DeviceId, FolderId, LibraryId, MeshMeasurements, PartId, ShareId};
use lapidary_db::{IngestRequest, PgFolders, PgIngest, PgShares, PgSharing, StoredBlobRow};
use lapidary_peer::PeerDevice;
use serde_json::Value;
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

async fn record(
    pool: &sqlx::PgPool,
    folder: FolderId,
    name: &str,
    seed: u8,
    thumbnail: Option<&[u8]>,
) -> PartId {
    let source_path = format!("{}.stl", name.to_lowercase().replace([' ', ','], "-"));
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: Some(folder),
            storage_path: None,
            library: library(),
            name,
            source_path: &source_path,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: thumbnail,
        })
        .await
        .expect("records")
}

struct Shared {
    share: ShareId,
    cliff: PartId,
    round_base: PartId,
}

/// Terrain, holding Rocks, shared, and paired with Ayşe; Bases beside it, not shared.
async fn shared_terrain(pool: &sqlx::PgPool) -> Shared {
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    let bases = folders
        .get_or_create(library(), None, "Bases", "Bases")
        .await
        .expect("Bases");
    record(pool, terrain, "Standing stone, LP-TR-0140", 1, None).await;
    let cliff = record(
        pool,
        rocks,
        "Cliff face, LP-TR-0112",
        2,
        Some(b"RIFF\x24\0\0\0WEBPVP8 cliff"),
    )
    .await;
    let round_base = record(
        pool,
        bases,
        "Round base 32 mm, LP-BS-0032",
        3,
        Some(b"RIFF\x24\0\0\0WEBPVP8 base"),
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
        share,
        cliff,
        round_base,
    }
}

/// A request from `device`, as the listener would pass it on.
async fn get(
    pool: &sqlx::PgPool,
    device: Option<DeviceId>,
    uri: &str,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = lapidary_peer::shares::shares_router(pool.clone())
        .layer(MockConnectInfo(PeerDevice(device)))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    (status, content_type, bytes.to_vec())
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn somebody_paired_reads_the_shares_and_nobody_else_does(pool: sqlx::PgPool) {
    let shared = shared_terrain(&pool).await;

    let (status, _, body) = get(&pool, Some(ayse()), "/peer/v1/shares").await;
    assert_eq!(status, StatusCode::OK);
    let shares = json(&body);
    assert_eq!(shares.as_array().map(Vec::len), Some(1), "{shares}");
    assert_eq!(shares[0]["id"], shared.share.as_uuid().to_string());
    assert_eq!(shares[0]["name"], "Terrain");
    assert_eq!(shares[0]["partCount"], 2);

    let stranger = DeviceId::from_public_key(b"a key nobody here paired with");
    for device in [Some(stranger), None] {
        let (status, _, body) = get(&pool, device, "/peer/v1/shares").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{device:?}");
        assert_eq!(json(&body)["reason"], "notPaired");
    }
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn the_catalogue_pages_and_never_carries_what_is_not_shared(pool: sqlx::PgPool) {
    let shared = shared_terrain(&pool).await;
    let mut paths = Vec::new();
    let mut after = String::new();
    loop {
        let uri = format!(
            "/peer/v1/shares/{}/catalogue?limit=1&after={after}",
            shared.share.as_uuid()
        );
        let (status, _, body) = get(&pool, Some(ayse()), &uri).await;
        assert_eq!(status, StatusCode::OK);
        let page = json(&body);
        for part in page["parts"].as_array().expect("parts") {
            paths.push(part["sourcePath"].as_str().expect("a path").to_owned());
        }
        match page["next"].as_str() {
            Some(next) => after = next.to_owned(),
            None => break,
        }
    }
    assert_eq!(
        paths,
        [
            "cliff-face--lp-tr-0112.stl",
            "standing-stone--lp-tr-0140.stl"
        ]
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_stranger_or_a_withdrawn_share_gets_no_catalogue(pool: sqlx::PgPool) {
    let shared = shared_terrain(&pool).await;
    let uri = format!("/peer/v1/shares/{}/catalogue", shared.share.as_uuid());

    let stranger = DeviceId::from_public_key(b"a key nobody here paired with");
    let (status, _, body) = get(&pool, Some(stranger), &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&body)["reason"], "notShared");

    PgShares(pool.clone())
        .remove(shared.share)
        .await
        .expect("stops sharing");
    let (status, _, body) = get(&pool, Some(ayse()), &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let refusal = json(&body);
    assert_eq!(refusal["reason"], "notShared");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|message| message.contains("not shared with you")),
        "{refusal}"
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_thumbnail_comes_only_from_inside_the_share(pool: sqlx::PgPool) {
    let shared = shared_terrain(&pool).await;

    let (status, content_type, body) = get(
        &pool,
        Some(ayse()),
        &format!(
            "/peer/v1/shares/{}/thumbnail?part={}",
            shared.share.as_uuid(),
            shared.cliff.as_uuid()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("image/webp"));
    assert_eq!(body, b"RIFF\x24\0\0\0WEBPVP8 cliff");

    let (status, _, _) = get(
        &pool,
        Some(ayse()),
        &format!(
            "/peer/v1/shares/{}/thumbnail?part={}",
            shared.share.as_uuid(),
            shared.round_base.as_uuid()
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the round base has a preview, and is not in Terrain"
    );
}
