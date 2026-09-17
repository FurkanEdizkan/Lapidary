//! The hello round's mirror (sharing S2b): what a paired installation shares, read into this one's mirror over a
//! real pinned connection. Both installations use one database, as `peer_sync.rs`'s tests do: the sharer's
//! tables and the puller's mirror are different tables, and each side's device is paired with the other.

use lapidary_core::{BlobHash, DeviceId, FolderId, LibraryId, MeshMeasurements, PartId, ShareId};
use lapidary_db::{
    IngestRequest, PgFolders, PgIngest, PgMirror, PgShares, PgSharing, StoredBlobRow,
};
use lapidary_peer::{PeerIdentity, Roster, router, serve, server_config, sync};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
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

/// The sharer, listening on the loopback with its share routes, and this installation paired with it both ways.
struct Pair {
    here: Arc<PeerIdentity>,
    sharer: DeviceId,
    address: String,
    share: ShareId,
    cliff: PartId,
    here_id: DeviceId,
    /// How often the sharer has been asked for a folder's roster.
    rosters_asked: Arc<AtomicUsize>,
}

async fn sharing_terrain(pool: &sqlx::PgPool) -> Pair {
    let here = Arc::new(PeerIdentity::generate().expect("this installation's identity"));
    let here_id = here.device_id().expect("its id");
    let sharer_identity = PeerIdentity::generate().expect("the sharer's identity");
    let sharer = sharer_identity.device_id().expect("the sharer's id");

    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    record(pool, terrain, "Standing stone, LP-TR-0140", 1, None).await;
    let cliff = record(
        pool,
        rocks,
        "Cliff face, LP-TR-0112",
        2,
        Some(b"RIFF\x24\0\0\0WEBPVP8 cliff"),
    )
    .await;
    let share = PgShares(pool.clone())
        .create(library(), terrain)
        .await
        .expect("shares")
        .expect("live")
        .id;

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port on the loopback");
    let address = tcp.local_addr().expect("the port it took").to_string();
    let roster = Roster::new(vec![here_id], Some("Ayşe's workshop".to_owned()));
    let config = server_config(&sharer_identity, &roster).expect("the sharer's side");
    let rosters_asked = Arc::new(AtomicUsize::new(0));
    let counter = rosters_asked.clone();
    let routes = router(sharer, roster)
        .merge(lapidary_peer::shares::shares_router(pool.clone()))
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let counter = counter.clone();
                async move {
                    if request.uri().path().ends_with("/members") {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    next.run(request).await
                }
            },
        ));
    tokio::spawn(serve(tcp, config, routes));

    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(sharer, &address)
        .await
        .expect("this installation pairs with the sharer");
    sharing
        .add_peer(here_id, "127.0.0.1:9")
        .await
        .expect("and the sharer with this installation");
    Pair {
        here,
        sharer,
        address,
        share,
        cliff,
        here_id,
        rosters_asked,
    }
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn what_a_paired_installation_shares_is_mirrored_with_its_thumbnails(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;

    let report = sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("mirrors");
    assert_eq!(
        (report.shares, report.read, report.parts),
        (1, 1, 2),
        "{report:?}"
    );
    assert_eq!(report.thumbnail_bytes, b"RIFF\x24\0\0\0WEBPVP8 cliff".len());

    let mirror = PgMirror(pool.clone());
    let shares = mirror.shares_of(pair.sharer).await.expect("lists");
    assert_eq!(
        (shares[0].name.as_str(), shares[0].part_count),
        ("Terrain", 2)
    );
    let parts = mirror.parts(shares[0].id, None, 50).await.expect("reads");
    let cliff = parts
        .iter()
        .find(|part| part.remote_part == pair.cliff)
        .expect("the cliff face");
    assert!(cliff.thumbnail);
    assert_eq!(
        mirror
            .thumbnail(shares[0].id, &cliff.source_path)
            .await
            .expect("reads")
            .as_deref(),
        Some(&b"RIFF\x24\0\0\0WEBPVP8 cliff"[..])
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn an_unchanged_share_is_not_read_again_and_a_changed_one_is(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;
    sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("first read");

    let again = sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("second read");
    assert_eq!(
        (again.shares, again.read, again.pages),
        (1, 0, 0),
        "nothing moved: {again:?}"
    );

    sqlx::query("UPDATE part SET deleted_at = now(), updated_at = now() WHERE id = $1")
        .bind(pair.cliff.as_uuid())
        .execute(&pool)
        .await
        .expect("the sharer removes the cliff face");
    let changed = sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("third read");
    assert_eq!((changed.read, changed.parts), (1, 1), "{changed:?}");
    let mirror = PgMirror(pool.clone());
    let share = mirror
        .shares_of(pair.sharer)
        .await
        .expect("lists")
        .remove(0);
    let parts = mirror.parts(share.id, None, 50).await.expect("reads");
    assert!(
        parts.iter().all(|part| part.remote_part != pair.cliff),
        "the sharer stopped offering it"
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_share_its_sharer_stops_offering_leaves_the_mirror(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;
    sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("first read");

    PgShares(pool.clone())
        .remove(pair.share)
        .await
        .expect("the sharer stops sharing");
    let report = sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("reads the list again");
    assert_eq!(report.shares, 0);
    assert!(
        PgMirror(pool.clone())
            .shares_of(pair.sharer)
            .await
            .expect("lists")
            .is_empty()
    );
}

/// A machine that accepts the connection and never answers costs its own hello's patience, not everybody's: three
/// of them beside one that answers still leave a round well short of three patiences.
#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn hellos_to_silent_machines_do_not_hold_up_one_that_answers(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;
    let sharing = PgSharing(pool.clone());
    let mut held = Vec::new();
    for key in [b"silent one".as_slice(), b"silent two", b"silent three"] {
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port");
        let address = silent.local_addr().expect("its port").to_string();
        held.push(tokio::spawn(async move {
            let mut open = Vec::new();
            while let Ok((stream, _)) = silent.accept().await {
                open.push(stream);
            }
        }));
        sharing
            .add_peer(DeviceId::from_public_key(key), &address)
            .await
            .expect("pairs a silent machine");
    }

    let began = std::time::Instant::now();
    let answered = sync::round(&pool, &pair.here, &Roster::default())
        .await
        .expect("a round");
    let took = began.elapsed();
    assert_eq!(
        answered
            .iter()
            .map(|(device, _, _)| *device)
            .collect::<Vec<_>>(),
        [pair.sharer]
    );
    assert!(
        took < std::time::Duration::from_secs(10),
        "one round took {took:?}"
    );
    for task in held {
        task.abort();
    }
}

/// S1b left an open connection outliving a removal: the listener refuses at the handshake, and a connection already past
/// it kept going. Every share route asks the database who may read at every request, so the next request on that same
/// connection is refused. The roster here is never refreshed, so a new connection would still be accepted: the refusal
/// below can only come from the per-request check.
#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_connection_opened_before_a_removal_is_refused_at_its_next_request(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;
    let client = reqwest::Client::builder()
        .use_preconfigured_tls(
            lapidary_peer::client_config(&pair.here, pair.sharer).expect("pinned"),
        )
        .build()
        .expect("a client");
    let url = format!("https://{}/peer/v1/shares", pair.address);
    let first = client.get(&url).send().await.expect("answers");
    assert_eq!(first.status(), reqwest::StatusCode::OK);
    first.bytes().await.expect("reads the list");

    PgSharing(pool.clone())
        .remove_peer(pair.here.device_id().expect("its id"))
        .await
        .expect("the sharer removes this installation");
    let next = client.get(&url).send().await.expect("answers");
    assert_eq!(next.status(), reqwest::StatusCode::FORBIDDEN);
    let refusal: serde_json::Value =
        serde_json::from_slice(&next.bytes().await.expect("reads")).expect("a refusal");
    assert_eq!(refusal["reason"], "notPaired");
}

/// A folder's roster, mirrored beside its catalogue (sharing S6): the people it goes to are who this
/// installation may reach for its files when its owner is away.
///
/// Both installations share one database here, so everybody on the sharer's roster is already a row in it;
/// what this test shows is that the roster crossed the wire and landed whole. Whom it then offers to meet is
/// `lapidary-db`'s `mirror.rs` test, where a roster can name somebody this installation has no row for.
#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_mirrored_folders_roster_is_read_over_the_wire(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;
    let mira = DeviceId::from_public_key(b"ed25519 public key of mira's studio pc");
    PgSharing(pool.clone())
        .add_peer(mira, "192.168.1.31:8082")
        .await
        .expect("the sharer pairs with Mira too");
    // A folder publishes a roster once its owner has picked who it goes to, and not before.
    PgShares(pool.clone())
        .set_members(pair.share, &[pair.here_id, mira])
        .await
        .expect("the sharer says who Terrain goes to");

    let report = sync::mirror(
        &pool,
        &pair.here,
        pair.sharer,
        &pair.address,
        &[lapidary_peer::ROSTERS.to_owned()],
    )
    .await
    .expect("mirrors");
    assert_eq!(report.rosters, 1, "{report:?}");
    assert_eq!(pair.rosters_asked.load(Ordering::SeqCst), 1);

    let roster: Vec<(Vec<u8>, String, bool)> = sqlx::query_as(
        "SELECT m.device_id, m.address, m.may_fetch FROM peer_share_member m \
         JOIN peer_share ps ON ps.id = m.peer_share_id WHERE ps.device_id = $1 ORDER BY m.address",
    )
    .bind(pair.sharer.as_bytes().as_slice())
    .fetch_all(&pool)
    .await
    .expect("reads the mirrored roster");
    // One database for both sides, so the sharer's `peer` table also holds a row for itself; what matters is
    // that everybody Terrain reaches came over, with where to reach them and the ask-first answer.
    let landed: Vec<(&[u8], &str, bool)> = roster
        .iter()
        .map(|(device, address, may_fetch)| (device.as_slice(), address.as_str(), *may_fetch))
        .collect();
    assert!(
        landed.contains(&(mira.as_bytes().as_slice(), "192.168.1.31:8082", true)),
        "{landed:?}"
    );
    assert!(
        landed.contains(&(pair.here_id.as_bytes().as_slice(), "127.0.0.1:9", true)),
        "{landed:?}"
    );
}

/// An installation from before rosters existed lists none, and is never asked for one — not once, and not once
/// a round for ever. The protocol number cannot say this: it stays 1 so older installations keep talking.
#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn an_installation_from_before_rosters_is_never_asked_for_one(pool: sqlx::PgPool) {
    let pair = sharing_terrain(&pool).await;

    let report = sync::mirror(&pool, &pair.here, pair.sharer, &pair.address, &[])
        .await
        .expect("mirrors");
    assert_eq!(report.read, 1, "the catalogue still comes over");
    assert_eq!(report.rosters, 0);
    assert_eq!(pair.rosters_asked.load(Ordering::SeqCst), 0);
    assert!(
        PgMirror(pool.clone())
            .introductions()
            .await
            .expect("lists")
            .is_empty()
    );
}
