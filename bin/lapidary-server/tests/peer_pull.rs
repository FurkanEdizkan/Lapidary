//! A pull (sharing S3), end to end: the sharer's routes over a real pinned connection, the mirror, the puller's fetch
//! and bundles, and the worker importing them. Both installations use one database, as `peer_mirror.rs`'s tests do,
//! each with its own store. Here for `peer_sync.rs`'s reason: `#[sqlx::test]`, and `deny.toml`'s list of who may take
//! `sqlx`. Needs the worker, so only with `mock-kernel`.
#![cfg(feature = "mock-kernel")]

use lapidary_core::{BlobHash, FolderId, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{
    Grant, IngestRequest, NewPartSource, PgFolders, PgIngest, PgJobs, PgMirror, PgParts, PgPulls,
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

/// Everything a pull needs, on one database: the sharer's Terrain/Rocks with a licensed, tagged cliff face, Terrain
/// shared and served over a real pinned connection, this installation paired and mirrored, a library to pull into, and a
/// worker importing.
struct Terrain {
    here: Arc<PeerIdentity>,
    sharer: lapidary_core::DeviceId,
    /// The share as this installation mirrors it.
    share: lapidary_core::PeerShareId,
    /// The same share on the sharer's side.
    sharer_share: lapidary_core::ShareId,
    ours: LibraryId,
    staging: tempfile::TempDir,
    our_store: tempfile::TempDir,
    _sharer_store: tempfile::TempDir,
    _ingest: tempfile::TempDir,
    shutdown: tokio_util::sync::CancellationToken,
    worker: tokio::task::JoinHandle<Result<(), lapidary_jobs::JobsError>>,
}

async fn terrain(pool: &sqlx::PgPool) -> Terrain {
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
        pool,
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
    let sharer_share = PgShares(pool.clone())
        .create(sharers_library(), terrain)
        .await
        .expect("shares")
        .expect("live")
        .id;

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
    sync::round(pool, &here, &Roster::default())
        .await
        .expect("says hello");
    sync::mirror(pool, &here, sharer, &address)
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

    Terrain {
        here,
        sharer,
        share,
        sharer_share,
        ours,
        staging,
        our_store,
        _sharer_store: sharer_store,
        _ingest: ingest,
        shutdown,
        worker,
    }
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_pulled_share_lands_under_its_sharer_and_a_second_pull_moves_nothing(pool: sqlx::PgPool) {
    let Terrain {
        here,
        sharer,
        share,
        ours,
        staging,
        our_store,
        shutdown,
        worker,
        // Held, not left to `..`: an unbound field is dropped at once, and with it the sharer's store.
        _sharer_store,
        _ingest,
        ..
    } = terrain(&pool).await;
    let parts = PgParts(pool.clone());
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

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_pull_waits_for_a_grant_pauses_and_resumes_and_a_share_stopped_fails_it_by_name(
    pool: sqlx::PgPool,
) {
    let terrain = terrain(&pool).await;
    let here_id = terrain.here.device_id().expect("its id");
    let shares = PgShares(pool.clone());
    let pulls = PgPulls(pool.clone());
    let work = |row| {
        let (pool, here) = (pool.clone(), terrain.here.clone());
        let (staging, store) = (
            terrain.staging.path().to_path_buf(),
            terrain.our_store.path().to_path_buf(),
        );
        async move {
            tokio::time::timeout(
                Duration::from_secs(60),
                pull::work(&pool, &here, &staging, &store, &row),
            )
            .await
            .expect("settles within a minute")
        }
    };
    shares
        .set_asks_first(terrain.sharer_share, true)
        .await
        .expect("the sharer asks first");

    let first = pulls
        .start(terrain.share, terrain.ours)
        .await
        .expect("records")
        .expect("mirrored");
    let why = work(pulls.next().await.expect("reads").expect("queued"))
        .await
        .expect_err("waits for the sharer");
    assert!(
        why.contains("Ayşe/Workshop") && why.contains("Terrain"),
        "{why}"
    );
    let waiting = pulls
        .latest(terrain.share)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!((waiting.state.as_str(), waiting.files_done), ("waiting", 0));
    assert_eq!(
        shares
            .grant(here_id, terrain.sharer_share)
            .await
            .expect("reads"),
        Grant::Asked,
        "the pull asked"
    );

    // Paused, the peer role passes it over; resumed, it is picked up again.
    assert!(pulls.pause(first).await.expect("pauses"));
    assert_eq!(pulls.next().await.expect("reads"), None);
    assert!(pulls.resume(first).await.expect("resumes"));
    let resumed = pulls.next().await.expect("reads").expect("picked up again");
    assert_eq!(resumed.id, first);
    // Paused again after the peer role read the row: its work starts nothing.
    assert!(pulls.pause(first).await.expect("pauses"));
    shares
        .decide(terrain.sharer_share, here_id, true)
        .await
        .expect("the sharer grants it");
    work(resumed.clone()).await.expect("stops at once");
    let still = pulls
        .latest(terrain.share)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!(
        (still.state.as_str(), still.files_done),
        ("paused", 0),
        "{still:?}"
    );
    assert!(pulls.resume(first).await.expect("resumes"));
    let resumed = pulls.next().await.expect("reads").expect("picked up again");

    work(resumed).await.expect("finishes");
    let done = pulls
        .latest(terrain.share)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!(
        (done.state.as_str(), done.files_total),
        ("done", 1),
        "{done:?}"
    );

    // Into a second library, after the sharer stopped sharing: refused, naming the share, and the parts pulled stay.
    let second = PgParts(pool.clone())
        .create_library("Second copy", "hobby")
        .await
        .expect("a second library");
    pulls
        .start(terrain.share, second)
        .await
        .expect("records")
        .expect("still mirrored here");
    shares
        .remove(terrain.sharer_share)
        .await
        .expect("stops sharing");
    work(pulls.next().await.expect("reads").expect("queued"))
        .await
        .expect("finishes, refused");
    let stopped = pulls
        .latest(terrain.share)
        .await
        .expect("reads")
        .expect("the pull");
    let error = stopped.error.clone().unwrap_or_default();
    assert_eq!(stopped.state, "failed");
    assert!(
        error.contains("Ayşe/Workshop") && error.contains("Terrain"),
        "{error}"
    );
    let in_ours: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(terrain.ours.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(in_ours, 1, "what was pulled stays");

    terrain.shutdown.cancel();
    let _ = terrain.worker.await;
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_denied_request_and_a_sharer_removed_here_each_fail_the_pull_saying_so(
    pool: sqlx::PgPool,
) {
    let terrain = terrain(&pool).await;
    let here_id = terrain.here.device_id().expect("its id");
    let shares = PgShares(pool.clone());
    let pulls = PgPulls(pool.clone());
    shares
        .set_asks_first(terrain.sharer_share, true)
        .await
        .expect("asks first");
    shares
        .ask(here_id, terrain.sharer_share)
        .await
        .expect("asked");
    shares
        .decide(terrain.sharer_share, here_id, false)
        .await
        .expect("the sharer denies it");

    pulls
        .start(terrain.share, terrain.ours)
        .await
        .expect("records")
        .expect("mirrored");
    let row = pulls.next().await.expect("reads").expect("queued");
    pull::work(
        &pool,
        &terrain.here,
        terrain.staging.path(),
        terrain.our_store.path(),
        &row,
    )
    .await
    .expect("finishes, denied");
    let denied = pulls
        .latest(terrain.share)
        .await
        .expect("reads")
        .expect("the pull");
    assert_eq!(denied.state, "failed");
    assert!(
        denied
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("declined"),
        "{denied:?}"
    );

    pulls
        .start(terrain.share, terrain.ours)
        .await
        .expect("records")
        .expect("mirrored");
    PgSharing(pool.clone())
        .remove_peer(terrain.sharer)
        .await
        .expect("this installation removes the sharer");
    let row = pulls
        .next()
        .await
        .expect("reads")
        .expect("still picked up, to be told");
    pull::work(
        &pool,
        &terrain.here,
        terrain.staging.path(),
        terrain.our_store.path(),
        &row,
    )
    .await
    .expect("finishes");
    let removed = pulls.get(row.id).await.expect("reads").expect("the pull");
    assert_eq!(removed.state, "failed");
    assert!(
        removed
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("removed"),
        "{removed:?}"
    );

    terrain.shutdown.cancel();
    let _ = terrain.worker.await;
}
