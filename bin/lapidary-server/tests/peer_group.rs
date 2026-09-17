//! Three installations, a folder with several holders, end to end (sharing S5–S10).
//!
//! Unlike this crate's other peer tests, the three do **not** share a database: each has its own, because the
//! whole subject here is what one installation knows about another's folder and when it stops knowing it. The
//! databases are made beside the test's own, from `DATABASE_URL`, and dropped at the end.
//!
//! The run is the plan's: Ayşe owns Terrain and shares it with Burak and Cem, who accept the introductions she
//! publishes. Burak pulls it whole. **Ayşe stops answering.** Cem still browses Terrain through Burak's copy,
//! marked as Burak read it; Cem opens a part he does not hold and it comes from Burak. Ayşe comes back and
//! takes Cem off the folder: Cem's copies stay, the folder leaves his list, and Burak refuses his next file.
#![cfg(feature = "mock-kernel")]

use lapidary_core::{
    BlobHash, DeviceId, LibraryId, MeshMeasurements, PartId, PeerShareId, ShareId,
};
use lapidary_db::{
    IngestRequest, PgFolders, PgIngest, PgJobs, PgMirror, PgParts, PgPulls, PgRevisions, PgShares,
    PgSharing, StoredBlobRow,
};
use lapidary_peer::{PeerIdentity, Roster, pull, router, serve, server_config, sync};
use lapidary_storage::{Compression, SourceWriter};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// An ASCII STL, big enough to be a real file and small enough to move in a test.
fn mesh(name: &str, facets: usize) -> Vec<u8> {
    let mut stl = format!("solid {name}\n").into_bytes();
    for step in 0..facets {
        let x = step as f64 * 0.25;
        stl.extend_from_slice(
            format!("facet normal 0 0 1\n outer loop\n  vertex {x:.2} 0 0\n  vertex {:.2} 0 0\n  vertex {x:.2} 18.5 4\n endloop\nendfacet\n", x + 0.25).as_bytes(),
        );
    }
    stl.extend_from_slice(format!("endsolid {name}\n").as_bytes());
    stl
}

/// One installation: its own database, store, staging, identity and peer service.
struct Installation {
    db: PgPool,
    name: &'static str,
    identity: Arc<PeerIdentity>,
    device: DeviceId,
    address: String,
    store: tempfile::TempDir,
    staging: tempfile::TempDir,
    _ingest: tempfile::TempDir,
    shutdown: tokio_util::sync::CancellationToken,
    worker: tokio::task::JoinHandle<Result<(), lapidary_jobs::JobsError>>,
    /// Who its peer service accepts. Replaced as it pairs, as the hello round replaces it in the real thing.
    roster: Roster,
}

/// A database of this installation's own, beside the test's, migrated and empty.
async fn own_database(suffix: &str) -> (PgPool, String) {
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL, as every database test needs");
    let (server, _) = base.rsplit_once('/').expect("a database in the url");
    // The whole uuid, not a prefix of it: two runs a moment apart share the first half of a v7.
    let name = format!("lapidary_group_{suffix}_{}", uuid::Uuid::now_v7().simple());
    let admin = PgPool::connect(&base).await.expect("connects");
    // A name this test made from a uuid, against a database this test owns: the lint is for user input.
    // `AssertSqlSafe` because `CREATE DATABASE` takes no bind parameter, and the name is this test's own:
    // a fixed prefix and a uuid it just made.
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("makes a database");
    admin.close().await;
    let url = format!("{server}/{name}");
    let db = PgPool::connect(&url).await.expect("connects to it");
    sqlx::migrate!("../../crates/lapidary-db/migrations")
        .run(&db)
        .await
        .expect("migrates it");
    (db, name)
}

async fn install(name: &'static str, suffix: &str) -> (Installation, String) {
    let (db, database) = own_database(suffix).await;
    let store = tempfile::tempdir().expect("a store");
    let staging = tempfile::tempdir().expect("a staging volume");
    let ingest = tempfile::tempdir().expect("an ingest mount nobody scans");
    let identity = Arc::new(PeerIdentity::generate().expect("an identity"));
    let device = identity.device_id().expect("its id");
    PgSharing(db.clone())
        .claim_identity(device)
        .await
        .expect("claims its id");
    PgSharing(db.clone())
        .set_name(Some(name))
        .await
        .expect("names itself");

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port on the loopback");
    let address = tcp.local_addr().expect("the port it took").to_string();
    let roster = Roster::new(Vec::new(), Some(name.to_owned()));
    let config = server_config(&identity, &roster).expect("its side of the handshake");
    let routes = router(device, roster.clone())
        .merge(lapidary_peer::shares::shares_router(db.clone()))
        .merge(lapidary_peer::blob::blob_router(
            db.clone(),
            store.path().to_path_buf(),
        ));
    tokio::spawn(serve(tcp, config, routes));

    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = tokio::spawn(lapidary_jobs::run(
        PgJobs(db.clone()),
        Arc::new(lapidary_ingest::WorkerHandler {
            db: db.clone(),
            ingest_dir: ingest.path().to_path_buf(),
            blob_root: store.path().to_path_buf(),
            cad: None,
        }),
        lapidary_jobs::WorkerConfig {
            poll_interval: Duration::from_millis(100),
            ..Default::default()
        },
        shutdown.clone(),
    ));
    (
        Installation {
            db,
            name,
            identity,
            device,
            address,
            store,
            staging,
            _ingest: ingest,
            shutdown,
            worker,
            roster,
        },
        database,
    )
}

impl Installation {
    /// Pair with `other`, both ways as two people do, and let each accept the other's handshake.
    async fn pairs_with(&self, other: &Installation) {
        PgSharing(self.db.clone())
            .add_peer(other.device, &other.address)
            .await
            .expect("pairs");
        self.refresh_roster().await;
    }

    /// The peer service accepts whoever this installation is paired with, as its hello round keeps it.
    async fn refresh_roster(&self) {
        let paired = PgSharing(self.db.clone())
            .paired()
            .await
            .expect("reads who it is paired with");
        self.roster.replace(
            paired.into_iter().map(|(device, _)| device).collect(),
            Some(self.name.to_owned()),
        );
    }

    /// A hello round and a mirror of everybody who answered, as the peer role runs them.
    async fn catch_up(&self) {
        let answered = sync::round(&self.db, &self.identity, &self.roster)
            .await
            .expect("says hello");
        for (device, address, features) in answered {
            sync::mirror(&self.db, &self.identity, device, &address, &features)
                .await
                .expect("mirrors");
        }
    }

    /// Work the next pull to a finish, or answer why it stopped.
    async fn pull_once(&self) -> Result<(), String> {
        let Some(next) = PgPulls(self.db.clone()).next().await.expect("reads") else {
            return Ok(());
        };
        tokio::time::timeout(
            Duration::from_secs(60),
            pull::work(
                &self.db,
                &self.identity,
                self.staging.path(),
                self.store.path(),
                &next,
            ),
        )
        .await
        .expect("settles within a minute")
    }

    /// A part of this installation's own, stored and recorded.
    async fn files(
        &self,
        folder: lapidary_core::FolderId,
        name: &str,
        path: &str,
        facets: usize,
    ) -> PartId {
        let bytes = mesh(path, facets);
        let staged = self.store.path().join("staged.stl");
        std::fs::write(&staged, &bytes).expect("stages it");
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        let stored = SourceWriter::open(self.store.path())
            .put_file(&staged, &hash, Compression::for_source_format("stl"))
            .expect("stores it");
        PgIngest(self.db.clone())
            .record(IngestRequest {
                origin: lapidary_core::RevisionOrigin::Ingest,
                folder: Some(folder),
                storage_path: None,
                library: library(),
                name,
                source_path: path,
                blob: &StoredBlobRow {
                    hash: stored.hash,
                    size_bytes: stored.size_bytes,
                    stored_bytes: stored.stored_bytes,
                    zstd_level: stored.zstd_level,
                },
                measurements: &MeshMeasurements {
                    bbox_mm: [100.0, 18.5, 4.0],
                    triangle_count: u32::try_from(facets).unwrap_or(u32::MAX),
                    surface_area_mm2: 3_730.0,
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
            .expect("records")
    }

    /// The folder `owner` owns, as this installation mirrors it.
    async fn mirrored(&self, owner: DeviceId) -> Option<PeerShareId> {
        PgMirror(self.db.clone())
            .shares_of(owner)
            .await
            .expect("lists")
            .first()
            .map(|share| share.id)
    }

    /// Point this installation at an address of `other`'s that answers nothing, and find that out: what an
    /// installation being away looks like from here, without taking anybody's service down.
    async fn cannot_reach(&self, other: &Installation) {
        PgSharing(self.db.clone())
            .add_peer(other.device, "127.0.0.1:9")
            .await
            .expect("keeps the pairing, at an address that answers nothing");
        self.catch_up().await;
    }

    /// And back again.
    async fn can_reach_again(&self, other: &Installation) {
        PgSharing(self.db.clone())
            .add_peer(other.device, &other.address)
            .await
            .expect("at their real address again");
        self.catch_up().await;
    }

    async fn stop(self) {
        self.shutdown.cancel();
        let _ = self.worker.await;
        self.db.close().await;
    }
}

/// Drop a database this test made.
async fn drop_database(name: &str) {
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let admin = PgPool::connect(&base).await.expect("connects");
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await;
    admin.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_folder_with_three_holders_outlives_its_owner_and_is_taken_back() {
    if std::env::var("DATABASE_URL").is_err() {
        return;
    }
    let (ayse, ayse_db) = install("Ayşe’s workshop", "a").await;
    let (burak, burak_db) = install("Burak’s bench", "b").await;
    let (cem, cem_db) = install("Cem’s studio", "c").await;

    // Ayşe's Terrain: two parts, shared with Burak and Cem and nobody else.
    let folders = PgFolders(ayse.db.clone());
    let terrain = folders
        .get_or_create(library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    ayse.files(
        terrain,
        "Cliff face, LP-TR-0112",
        "Terrain/cliff-face.stl",
        400,
    )
    .await;
    ayse.files(
        terrain,
        "Standing stone, LP-TR-0140",
        "Terrain/standing-stone.stl",
        200,
    )
    .await;
    let shares = PgShares(ayse.db.clone());
    let terrain_share: ShareId = shares
        .create(library(), terrain)
        .await
        .expect("shares")
        .expect("live")
        .id;
    ayse.pairs_with(&burak).await;
    ayse.pairs_with(&cem).await;
    burak.pairs_with(&ayse).await;
    cem.pairs_with(&ayse).await;
    shares
        .set_members(terrain_share, &[burak.device, cem.device])
        .await
        .expect("says who Terrain goes to");

    // Both read it, and each is introduced to the other on its roster.
    burak.catch_up().await;
    cem.catch_up().await;
    let offered = PgMirror(cem.db.clone())
        .introductions()
        .await
        .expect("lists");
    assert_eq!(
        offered
            .iter()
            .map(|row| (row.device, row.share_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(burak.device, "Terrain")],
        "Cem is introduced to Burak, and to nobody else"
    );
    let (address, introducer) = PgMirror(cem.db.clone())
        .introduction(offered[0].share, burak.device)
        .await
        .expect("reads")
        .expect("one to answer");
    assert_eq!(introducer, ayse.device, "introduced by the folder's owner");
    PgSharing(cem.db.clone())
        .accept_introduction(burak.device, &address, introducer)
        .await
        .expect("Cem accepts");
    cem.refresh_roster().await;
    let offered = PgMirror(burak.db.clone())
        .introductions()
        .await
        .expect("lists");
    let (address, introducer) = PgMirror(burak.db.clone())
        .introduction(offered[0].share, cem.device)
        .await
        .expect("reads")
        .expect("one to answer");
    PgSharing(burak.db.clone())
        .accept_introduction(cem.device, &address, introducer)
        .await
        .expect("Burak accepts");
    burak.refresh_roster().await;

    // Burak pulls Terrain whole.
    let burak_terrain = burak.mirrored(ayse.device).await.expect("mirrored");
    let burak_library = PgParts(burak.db.clone())
        .create_library("Pulled terrain", "hobby")
        .await
        .expect("a library to pull into");
    PgPulls(burak.db.clone())
        .start(burak_terrain, burak_library, None)
        .await
        .expect("records")
        .expect("mirrored");
    burak.pull_once().await.expect("pulls Terrain");
    let pulled = PgPulls(burak.db.clone())
        .latest(burak_terrain)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!(
        (pulled.state.as_str(), pulled.files_total),
        ("done", 2),
        "{pulled:?}"
    );

    // Ayşe adds a part, and Burak reads it. Then she stops answering Cem — who learns of it all the same,
    // through Burak, which is the whole point of a folder several people hold.
    ayse.files(terrain, "Dolmen, LP-TR-0155", "Terrain/dolmen.stl", 120)
        .await;
    burak.catch_up().await;
    cem.cannot_reach(&ayse).await;
    let cems_terrain = PgMirror(cem.db.clone())
        .share(cem.mirrored(ayse.device).await.expect("still mirrored"))
        .await
        .expect("reads")
        .expect("a folder");
    assert_eq!(cems_terrain.name, "Terrain");
    assert_eq!(
        cems_terrain.read_from,
        Some(burak.device),
        "read through Burak while Ayşe is away"
    );
    assert!(cems_terrain.as_of.is_some(), "and says when he read it");
    assert_eq!(
        PgMirror(cem.db.clone())
            .parts(cems_terrain.id, None, 50)
            .await
            .expect("lists")
            .len(),
        3,
        "including the part Ayşe added while he could not reach her"
    );

    // Cem opens one part he does not hold. It can only come from Burak.
    let cem_library = PgParts(cem.db.clone())
        .create_library("Terrain from the group", "hobby")
        .await
        .expect("a library to pull into");
    PgPulls(cem.db.clone())
        .start(cems_terrain.id, cem_library, Some("Terrain/cliff-face.stl"))
        .await
        .expect("records")
        .expect("mirrored");
    cem.pull_once().await.expect("pulls one part from Burak");
    let pulled = PgPulls(cem.db.clone())
        .latest(cems_terrain.id)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!(
        (
            pulled.state.as_str(),
            pulled.files_total,
            pulled.error.as_deref()
        ),
        ("done", 1, None),
        "{pulled:?}"
    );
    let group = &ayse.device.to_string()[..5];
    let place = format!("Shared/Ayşe’s workshop ({group})/Terrain/cliff-face.stl");
    assert!(
        PgRevisions(cem.db.clone())
            .current(cem_library, &place)
            .await
            .expect("reads")
            .is_some(),
        "the part landed under the folder's owner, though Burak sent it"
    );

    // Ayşe comes back and takes Cem off Terrain. Burak reads her roster first, as the folder's holder; then
    // Cem reads her list, which no longer carries Terrain for him.
    PgShares(ayse.db.clone())
        .set_members(terrain_share, &[burak.device])
        .await
        .expect("takes Cem off Terrain");
    burak.can_reach_again(&ayse).await;
    cem.can_reach_again(&ayse).await;

    assert!(
        cem.mirrored(ayse.device).await.is_none(),
        "the folder has left Cem's list: a cache catching up, never something of his removed"
    );
    assert!(
        PgRevisions(cem.db.clone())
            .current(cem_library, &place)
            .await
            .expect("reads")
            .is_some(),
        "and what he pulled is his: it stays, with its provenance"
    );
    assert_eq!(
        PgMirror(burak.db.clone())
            .serves(ayse.device, terrain_share, cem.device)
            .await
            .expect("answers"),
        lapidary_db::Serving::NotShared,
        "Burak refuses Cem's next file: the roster its owner published is what decides, not who holds the bytes"
    );
    assert_eq!(
        PgSharing(cem.db.clone())
            .peers()
            .await
            .expect("lists")
            .into_iter()
            .find(|peer| peer.device_id == burak.device)
            .expect("Burak is still on Cem's list")
            .folders_in_common,
        0,
        "and Burak is still paired with Cem, with nothing between them, until somebody removes him"
    );

    ayse.stop().await;
    burak.stop().await;
    cem.stop().await;
    drop_database(&ayse_db).await;
    drop_database(&burak_db).await;
    drop_database(&cem_db).await;
}
