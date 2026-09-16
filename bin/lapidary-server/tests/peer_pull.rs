//! A pull (sharing S3), end to end: the sharer's routes over a real pinned connection, the mirror, the puller's fetch
//! and bundles, and the worker importing them. Both installations use one database, as `peer_mirror.rs`'s tests do,
//! each with its own store. Here for `peer_sync.rs`'s reason: `#[sqlx::test]`, and `deny.toml`'s list of who may take
//! `sqlx`. Needs the worker, so only with `mock-kernel`.
#![cfg(feature = "mock-kernel")]

use lapidary_core::{BlobHash, FolderId, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{
    IngestRequest, NewPartSource, PgFolders, PgIngest, PgJobs, PgMirror, PgParts, PgPulls,
    PgRevisions, PgShares, PgSharing, StoredBlobRow,
};
use lapidary_peer::{PeerIdentity, Roster, pull, router, serve, server_config, sync};
use lapidary_storage::{Compression, SourceWriter};
use std::sync::Arc;
use std::time::Duration;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn sharers_library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// An ASCII STL of a cliff face, a real mesh for the worker to import.
fn cliff_face() -> Vec<u8> {
    let mut stl = b"solid cliff-face-lp-tr-0112\n".to_vec();
    for step in 0..400 {
        let x = f64::from(step) * 0.25;
        stl.extend_from_slice(
            format!("facet normal 0 0 1\n outer loop\n  vertex {x:.2} 0 0\n  vertex {:.2} 0 0\n  vertex {x:.2} 18.5 4\n endloop\nendfacet\n", x + 0.25).as_bytes(),
        );
    }
    stl.extend_from_slice(b"endsolid cliff-face-lp-tr-0112\n");
    stl
}

/// Store `bytes` in the sharer's store and file a part for them at `source_path`, under `folder`.
async fn filed(
    pool: &sqlx::PgPool,
    root: &std::path::Path,
    folder: FolderId,
    source_path: &str,
    bytes: &[u8],
) -> PartId {
    let staged = root.join("staged.stl");
    std::fs::write(&staged, bytes).expect("stages the file");
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let stored = SourceWriter::open(root)
        .put_file(&staged, &hash, Compression::for_source_format("stl"))
        .expect("stores it");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: Some(folder),
            storage_path: None,
            library: sharers_library(),
            name: "Cliff face, LP-TR-0112",
            source_path,
            blob: &StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [100.0, 18.5, 4.0],
                triangle_count: 400,
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

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_pulled_share_lands_under_its_sharer_and_a_second_pull_moves_nothing(pool: sqlx::PgPool) {
    let sharer_store = tempfile::tempdir().expect("the sharer's store");
    let our_store = tempfile::tempdir().expect("this installation's store");
    let staging = tempfile::tempdir().expect("the staging volume");
    let ingest = tempfile::tempdir().expect("an ingest mount nobody scans");

    // The sharer: Terrain/Rocks holding a cliff face with a licence and tags, and Terrain shared.
    let here = Arc::new(PeerIdentity::generate().expect("this installation's identity"));
    let here_id = here.device_id().expect("its id");
    let sharer_identity = PeerIdentity::generate().expect("the sharer's identity");
    let sharer = sharer_identity.device_id().expect("the sharer's id");
    let folders = PgFolders(pool.clone());
    let terrain = folders
        .get_or_create(sharers_library(), None, "Terrain", "Terrain")
        .await
        .expect("Terrain");
    let rocks = folders
        .get_or_create(sharers_library(), Some(terrain), "Rocks", "Rocks")
        .await
        .expect("Rocks");
    let cliff = filed(
        &pool,
        sharer_store.path(),
        rocks,
        "Terrain/Rocks/cliff-face-lp-tr-0112.stl",
        &cliff_face(),
    )
    .await;
    let parts = PgParts(pool.clone());
    parts
        .add_part_source(
            cliff,
            NewPartSource {
                license: Some("CC BY-NC 4.0"),
                ..Default::default()
            },
        )
        .await
        .expect("licensed");
    parts
        .set_tags(cliff, &["terrain".to_owned(), "28mm".to_owned()])
        .await
        .expect("tagged");
    PgShares(pool.clone())
        .create(sharers_library(), terrain)
        .await
        .expect("shares")
        .expect("live");

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port on the loopback");
    let address = tcp.local_addr().expect("the port it took").to_string();
    let roster = Roster::new(vec![here_id], Some("Ayşe/Workshop".to_owned()));
    let config = server_config(&sharer_identity, &roster).expect("the sharer's side");
    let routes = router(sharer, roster)
        .merge(lapidary_peer::shares::shares_router(pool.clone()))
        .merge(lapidary_peer::blob::blob_router(
            pool.clone(),
            sharer_store.path().to_path_buf(),
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

    // This installation: learns the sharer's name, mirrors, and pulls into a library of its own.
    sync::round(&pool, &here, &Roster::default())
        .await
        .expect("says hello");
    sync::mirror(&pool, &here, sharer, &address)
        .await
        .expect("mirrors");
    let share = PgMirror(pool.clone())
        .shares_of(sharer)
        .await
        .expect("lists")
        .first()
        .expect("one share")
        .id;
    let ours = parts
        .create_library("Pulled terrain", "hobby")
        .await
        .expect("a library to pull into");

    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = tokio::spawn(lapidary_jobs::run(
        PgJobs(pool.clone()),
        Arc::new(lapidary_ingest::WorkerHandler {
            db: pool.clone(),
            ingest_dir: ingest.path().to_path_buf(),
            blob_root: our_store.path().to_path_buf(),
            cad: None,
        }),
        lapidary_jobs::WorkerConfig {
            poll_interval: Duration::from_millis(100),
            ..Default::default()
        },
        shutdown.clone(),
    ));

    let pulls = PgPulls(pool.clone());
    let first = pulls
        .start(share, ours)
        .await
        .expect("records")
        .expect("the share is mirrored");
    let working = pulls.next().await.expect("reads").expect("waiting");
    assert_eq!(working.id, first);
    tokio::time::timeout(
        Duration::from_secs(60),
        pull::work(&pool, &here, staging.path(), our_store.path(), &working),
    )
    .await
    .expect("settles within a minute")
    .expect("finishes");

    let done = pulls.latest(share).await.expect("reads").expect("the pull");
    assert_eq!(
        (done.state.as_str(), done.files_total, done.error.as_deref()),
        ("done", 1, None),
        "{done:?}"
    );
    let group = &sharer.to_string()[..5];
    let place = format!("Shared/Ayşe-Workshop ({group})/Terrain/Rocks/cliff-face-lp-tr-0112.stl");
    let landed = PgRevisions(pool.clone())
        .current(ours, &place)
        .await
        .expect("reads")
        .unwrap_or_else(|| panic!("a part at {place}"));
    let licences: Vec<Option<String>> = parts
        .part_sources(landed.part)
        .await
        .expect("reads")
        .into_iter()
        .map(|source| source.license)
        .collect();
    assert_eq!(licences, [Some("CC BY-NC 4.0".to_owned())]);
    assert_eq!(
        pulls.provenance(landed.part).await.expect("reads"),
        Some((sharer, Some("Ayşe/Workshop".to_owned())))
    );
    let category: Option<String> = sqlx::query_scalar(
        "SELECT f.name FROM part p JOIN folder f ON f.id = p.folder_id WHERE p.id = $1",
    )
    .bind(landed.part.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("reads");
    assert_eq!(category.as_deref(), Some("Rocks"));
    assert_eq!(
        std::fs::read_dir(staging.path())
            .expect("lists staging")
            .count(),
        0,
        "staging is emptied once the import is queued"
    );

    // Again: the library holds that file at that place, so nothing is fetched and no part is added.
    pulls
        .start(share, ours)
        .await
        .expect("records")
        .expect("still mirrored");
    let again = pulls.next().await.expect("reads").expect("waiting");
    tokio::time::timeout(
        Duration::from_secs(60),
        pull::work(&pool, &here, staging.path(), our_store.path(), &again),
    )
    .await
    .expect("settles")
    .expect("finishes");
    let done = pulls.latest(share).await.expect("reads").expect("the pull");
    assert_eq!(
        (done.state.as_str(), done.files_total),
        ("done", 0),
        "{done:?}"
    );
    let in_ours: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(ours.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(in_ours, 1);

    shutdown.cancel();
    let _ = worker.await;
}
