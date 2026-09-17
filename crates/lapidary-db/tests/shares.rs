//! Sharing S2a: a category offered to the people this installation is paired with, and its catalogue.

use lapidary_core::{BlobHash, DeviceId, FolderId, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{
    Grant, IngestRequest, NewPartSource, PgFolders, PgIngest, PgParts, PgShares, PgSharing,
    SHARING_CHANNEL, StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn watertight() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

/// A terrain piece filed under `folder`, its bytes named by `seed`, with a thumbnail or without.
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
                size_bytes: 204_800 + u64::from(seed),
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &watertight(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: thumbnail,
        })
        .await
        .expect("records")
}

async fn licensed(pool: &sqlx::PgPool, part: PartId, license: &str) {
    PgParts(pool.clone())
        .add_part_source(
            part,
            NewPartSource {
                url: Some(&format!(
                    "https://www.printables.com/model/{}",
                    part.as_uuid()
                )),
                license: Some(license),
                ..Default::default()
            },
        )
        .await
        .expect("records the source");
}

/// Terrain, holding Rocks, beside Bases — the category a share must never leak into.
struct Library {
    terrain: FolderId,
    rocks: FolderId,
    bases: FolderId,
    standing_stone: PartId,
    cliff: PartId,
    round_base: PartId,
}

async fn terrain_library(pool: &sqlx::PgPool) -> Library {
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
    let standing_stone = record(pool, terrain, "Standing stone, LP-TR-0140", 1, None).await;
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
    Library {
        terrain,
        rocks,
        bases,
        standing_stone,
        cliff,
        round_base,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn sharing_a_category_offers_it_and_sharing_it_again_is_the_same_share(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());

    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("a live category");
    let again = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("still live");
    assert_eq!(again.id, shared.id, "one live share per category");
    assert_eq!(shared.name, "Terrain");

    let listed = shares.list(library()).await.expect("lists");
    assert_eq!(listed.iter().map(|s| s.id).collect::<Vec<_>>(), [shared.id]);
    let offered = shares.offered().await.expect("offers");
    assert_eq!(offered.len(), 1);
    assert_eq!(
        (
            offered[0].id,
            offered[0].name.as_str(),
            offered[0].part_count
        ),
        (shared.id, "Terrain", 2)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_category_of_another_library_or_a_deleted_one_cannot_be_shared(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let other = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, 'Shop floor', 'shop floor')")
        .bind(other.as_uuid())
        .execute(&pool)
        .await
        .expect("a second library");
    assert_eq!(
        shares.create(other, lib.terrain).await.expect("answers"),
        None,
        "not that library's category"
    );
    assert!(
        shares.list(library()).await.expect("lists").is_empty(),
        "and nothing was shared in the library the category does belong to"
    );

    sqlx::query("UPDATE folder SET deleted_at = now() WHERE id = $1")
        .bind(lib.bases.as_uuid())
        .execute(&pool)
        .await
        .expect("deletes Bases");
    assert_eq!(
        shares.create(library(), lib.bases).await.expect("answers"),
        None,
        "a deleted category"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_catalogue_is_the_category_and_everything_under_it_and_nothing_else(
    pool: sqlx::PgPool,
) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");

    let catalogue = shares.catalogue(shared.id, None, 50).await.expect("reads");
    let paths: Vec<_> = catalogue
        .iter()
        .map(|row| row.source_path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "cliff-face--lp-tr-0112.stl",
            "standing-stone--lp-tr-0140.stl"
        ],
        "Terrain and Rocks under it, by source path, and not Bases beside it"
    );
    let cliff = &catalogue[0];
    assert_eq!(cliff.part, lib.cliff);
    assert_eq!(cliff.format.as_deref(), Some("stl"));
    assert_eq!(cliff.size_bytes, Some(204_802));
    assert_eq!(cliff.blake3, Some(BlobHash::from_bytes([2; 32]).to_hex()));
    assert!(cliff.thumbnail, "the cliff face has a preview");
    assert!(!catalogue[1].thumbnail, "the standing stone has none");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_catalogue_pages_by_source_path(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");

    let first = shares
        .catalogue(shared.id, None, 1)
        .await
        .expect("page one");
    assert_eq!(first.len(), 1);
    let second = shares
        .catalogue(shared.id, Some(&first[0].source_path), 1)
        .await
        .expect("page two");
    assert_eq!(
        second.iter().map(|r| r.part).collect::<Vec<_>>(),
        [lib.standing_stone]
    );
    let past = shares
        .catalogue(shared.id, Some(&second[0].source_path), 1)
        .await
        .expect("page three");
    assert!(past.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_filed_under_a_shared_category_later_is_offered_and_moves_the_digest(
    pool: sqlx::PgPool,
) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");
    let before = shares.offered().await.expect("offers").remove(0);

    let dolmen = record(&pool, lib.rocks, "Dolmen, LP-TR-0151", 4, None).await;
    let after = shares.offered().await.expect("offers").remove(0);
    assert_eq!(after.part_count, 3);
    assert_ne!(
        after.digest, before.digest,
        "a puller must see that something changed"
    );
    let catalogue = shares.catalogue(shared.id, None, 50).await.expect("reads");
    assert!(catalogue.iter().any(|row| row.part == dolmen));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_removed_share_a_deleted_category_or_a_deleted_part_is_not_offered(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");

    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(lib.cliff.as_uuid())
        .execute(&pool)
        .await
        .expect("removes the cliff face");
    let catalogue = shares.catalogue(shared.id, None, 50).await.expect("reads");
    assert_eq!(
        catalogue.iter().map(|r| r.part).collect::<Vec<_>>(),
        [lib.standing_stone]
    );

    sqlx::query("UPDATE folder SET deleted_at = now() WHERE id = $1")
        .bind(lib.terrain.as_uuid())
        .execute(&pool)
        .await
        .expect("deletes Terrain");
    assert!(
        shares.offered().await.expect("offers").is_empty(),
        "a deleted category is not offered"
    );
    assert!(
        shares
            .catalogue(shared.id, None, 50)
            .await
            .expect("reads")
            .is_empty()
    );

    sqlx::query("UPDATE folder SET deleted_at = NULL WHERE id = $1")
        .bind(lib.terrain.as_uuid())
        .execute(&pool)
        .await
        .expect("restores Terrain");
    assert!(shares.remove(shared.id).await.expect("removes"));
    assert!(
        !shares.remove(shared.id).await.expect("answers"),
        "removed already"
    );
    assert!(shares.offered().await.expect("offers").is_empty());
    assert!(shares.list(library()).await.expect("lists").is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn only_somebody_paired_reaches_a_share(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");
    let ayse = DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage");
    let stranger = DeviceId::from_public_key(b"a key nobody here paired with");
    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(ayse, "192.168.1.24:8082")
        .await
        .expect("pairs");

    assert!(shares.access(ayse, shared.id).await.expect("answers"));
    assert!(
        !shares.access(stranger, shared.id).await.expect("answers"),
        "never paired"
    );
    sharing.remove_peer(ayse).await.expect("removes");
    assert!(
        !shares.access(ayse, shared.id).await.expect("answers"),
        "removed"
    );
    sharing
        .add_peer(ayse, "192.168.1.24:8082")
        .await
        .expect("pairs again");
    shares.remove(shared.id).await.expect("stops sharing");
    assert!(
        !shares.access(ayse, shared.id).await.expect("answers"),
        "the share is gone"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_thumbnail_is_given_only_for_a_part_inside_the_share(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");

    assert_eq!(
        shares
            .thumbnail(shared.id, lib.cliff)
            .await
            .expect("reads")
            .as_deref(),
        Some(&b"RIFF\x24\0\0\0WEBPVP8 cliff"[..])
    );
    assert_eq!(
        shares
            .thumbnail(shared.id, lib.round_base)
            .await
            .expect("reads"),
        None,
        "the round base has a preview, and is not in Terrain"
    );
}

/// The warning is counted from real licence strings, since a scanned corpus carries none: `NC` counts as a
/// token, never inside a word.
#[sqlx::test(migrations = "./migrations")]
async fn the_licence_warning_counts_unrecorded_and_non_commercial_parts(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    licensed(&pool, lib.cliff, "CC BY-NC 4.0").await;
    licensed(&pool, lib.standing_stone, "CC BY 4.0").await;
    let dolmen = record(&pool, lib.terrain, "Dolmen, LP-TR-0151", 4, None).await;
    licensed(&pool, dolmen, "CC-BY-NC-SA 4.0").await;
    let menhir = record(&pool, lib.rocks, "Menhir, LP-TR-0163", 5, None).await;
    licensed(&pool, menhir, "Non-Commercial, attribution required").await;
    let cairn = record(&pool, lib.terrain, "Cairn, LP-TR-0170", 6, None).await;
    licensed(
        &pool,
        cairn,
        "Licenced for incidental use in commercial prints",
    )
    .await;
    record(&pool, lib.rocks, "Tor, LP-TR-0188", 7, None).await;

    let counts = PgShares(pool.clone())
        .licences(lib.terrain)
        .await
        .expect("counts");
    assert_eq!(counts.parts, 6);
    assert_eq!(counts.unrecorded, 1, "the tor");
    assert_eq!(
        counts.non_commercial, 3,
        "the cliff face, the dolmen and the menhir"
    );
}

/// A puller re-reads a catalogue only when its digest moves, so a part revised under the share must move it —
/// even though a new revision leaves `part.updated_at` alone.
#[sqlx::test(migrations = "./migrations")]
async fn a_newer_revision_under_the_share_moves_the_digest(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");
    let before = shares.offered().await.expect("offers").remove(0);

    sqlx::query("UPDATE revision SET created_at = now() + interval '1 minute' WHERE part_id = $1")
        .bind(lib.cliff.as_uuid())
        .execute(&pool)
        .await
        .expect("stands in for a revision recorded later");
    let after = shares.offered().await.expect("offers").remove(0);
    assert_eq!(after.part_count, before.part_count, "no part was added");
    assert_ne!(after.digest, before.digest, "and the digest still moved");
}

/// Pairing, removing, sharing and stopping each tell the peer role at once, so its round starts then rather
/// than at the next tick.
#[sqlx::test(migrations = "./migrations")]
async fn pairing_and_sharing_tell_the_peer_role_at_once(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .expect("listens");
    listener
        .listen(SHARING_CHANNEL)
        .await
        .expect("on the sharing channel");

    let ayse = DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage");
    PgSharing(pool.clone())
        .add_peer(ayse, "192.168.1.24:8082")
        .await
        .expect("pairs");
    assert!(heard(&mut listener).await, "pairing");
    let shared = PgShares(pool.clone())
        .create(library(), lib.terrain)
        .await
        .expect("shares")
        .expect("live");
    assert!(heard(&mut listener).await, "sharing");
    PgShares(pool.clone())
        .remove(shared.id)
        .await
        .expect("stops");
    assert!(heard(&mut listener).await, "stopping");
    PgSharing(pool.clone())
        .remove_peer(ayse)
        .await
        .expect("removes");
    assert!(heard(&mut listener).await, "removing");
}

/// Whether a notification arrives on the listener within two seconds.
async fn heard(listener: &mut sqlx::postgres::PgListener) -> bool {
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), listener.recv()).await,
        Ok(Ok(_))
    )
}

/// A file is reachable through a share exactly when its part is offered: inside the category, live, and not merely
/// sharing a shape with a part one category over.
#[sqlx::test(migrations = "./migrations")]
async fn a_file_is_found_only_when_its_part_is_offered(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let shares = PgShares(pool.clone());
    let shared = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");
    let cliff = BlobHash::from_bytes([2; 32]).to_hex();
    let round_base = BlobHash::from_bytes([3; 32]).to_hex();

    let found = shares
        .blob(shared.id, &cliff)
        .await
        .expect("reads")
        .expect("the cliff face is offered");
    assert_eq!(found.size_bytes, 204_802);
    assert_eq!(found.zstd_level, Some(3));
    assert_eq!(
        shares.blob(shared.id, &round_base).await.expect("reads"),
        None,
        "Bases is not shared"
    );

    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(lib.cliff.as_uuid())
        .execute(&pool)
        .await
        .expect("removes the cliff face");
    assert_eq!(
        shares.blob(shared.id, &cliff).await.expect("reads"),
        None,
        "a removed part's file is not offered"
    );
}

fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

fn mira() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of mira's laptop at the makerspace")
}

#[sqlx::test(migrations = "./migrations")]
async fn a_share_that_asks_first_gives_files_only_to_whom_its_owner_granted(pool: sqlx::PgPool) {
    let lib = terrain_library(&pool).await;
    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("pairs");
    sharing
        .add_peer(mira(), "192.168.1.31:8082")
        .await
        .expect("pairs");
    let shares = PgShares(pool.clone());
    let terrain = shares
        .create(library(), lib.terrain)
        .await
        .expect("creates")
        .expect("live");
    assert!(
        !terrain.asks_first,
        "a share is open unless asked otherwise"
    );

    // Open: nobody needs to ask, and asking records nothing.
    assert_eq!(
        shares.grant(ayse(), terrain.id).await.expect("reads"),
        Grant::Open
    );
    assert_eq!(
        shares.ask(ayse(), terrain.id).await.expect("asks"),
        Some(Grant::Open)
    );
    assert!(shares.requests().await.expect("lists").is_empty());

    assert!(
        shares
            .set_asks_first(terrain.id, true)
            .await
            .expect("switches")
    );
    assert!(shares.list(library()).await.expect("lists")[0].asks_first);
    assert_eq!(
        shares.grant(ayse(), terrain.id).await.expect("reads"),
        Grant::NotAsked
    );

    // Asking twice is one request.
    assert_eq!(
        shares.ask(ayse(), terrain.id).await.expect("asks"),
        Some(Grant::Asked)
    );
    assert_eq!(
        shares.ask(ayse(), terrain.id).await.expect("asks"),
        Some(Grant::Asked)
    );
    assert_eq!(
        shares.ask(mira(), terrain.id).await.expect("asks"),
        Some(Grant::Asked)
    );
    let requests = shares.requests().await.expect("lists");
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.state == Grant::Asked && request.share_name == "Terrain")
    );

    assert!(
        shares
            .decide(terrain.id, ayse(), true)
            .await
            .expect("grants")
    );
    assert!(
        shares
            .decide(terrain.id, mira(), false)
            .await
            .expect("denies")
    );
    assert_eq!(
        shares.grant(ayse(), terrain.id).await.expect("reads"),
        Grant::Granted
    );
    assert_eq!(
        shares.grant(mira(), terrain.id).await.expect("reads"),
        Grant::Denied
    );
    // A denial is not undone by asking again; its owner can still grant it.
    assert_eq!(
        shares.ask(mira(), terrain.id).await.expect("asks"),
        Some(Grant::Denied)
    );

    // Somebody removed, or a share stopped, is asked nothing and granted nothing.
    sharing.remove_peer(ayse()).await.expect("removes");
    assert_eq!(
        shares.grant(ayse(), terrain.id).await.expect("reads"),
        Grant::NotShared
    );
    assert_eq!(shares.ask(ayse(), terrain.id).await.expect("asks"), None);
    shares.remove(terrain.id).await.expect("stops");
    assert_eq!(
        shares.grant(mira(), terrain.id).await.expect("reads"),
        Grant::NotShared
    );
    assert!(
        !shares
            .decide(terrain.id, mira(), true)
            .await
            .expect("decides nothing")
    );
}
