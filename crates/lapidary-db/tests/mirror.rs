//! Sharing S2b: other installations' shares, mirrored here so a shared library browses while they are asleep.

use lapidary_core::{DeviceId, PartId, ShareId};
use lapidary_db::{MirroredPartIn, OfferedRemote, PgMirror, PgSharing, RemoteMember};

/// Ayşe's workshop PC, on the same LAN.
fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

fn terrain() -> ShareId {
    ShareId::from_uuid(
        "01a07c41-5d22-7b03-9014-7e2f6dab0001"
            .parse()
            .expect("uuid"),
    )
}

fn bases() -> ShareId {
    ShareId::from_uuid(
        "01a07c41-5d22-7b03-9014-7e2f6dab0002"
            .parse()
            .expect("uuid"),
    )
}

fn offer<'a>(
    remote: ShareId,
    name: &'a str,
    part_count: i64,
    digest: &'a str,
) -> OfferedRemote<'a> {
    OfferedRemote {
        remote,
        name,
        part_count,
        digest,
    }
}

fn part<'a>(
    source_path: &'a str,
    name: &'a str,
    thumbnail: Option<&'a [u8]>,
) -> MirroredPartIn<'a> {
    MirroredPartIn {
        source_path,
        remote_part: PartId::new(),
        name,
        part_number: None,
        tags: &[],
        licences: &[],
        blake3: Some("5c0f8d3e9a1b2c4d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5"),
        size_bytes: Some(204_800),
        format: Some("stl"),
        thumbnail,
    }
}

async fn paired(pool: &sqlx::PgPool) -> PgMirror {
    PgSharing(pool.clone())
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("pairs");
    PgMirror(pool.clone())
}

/// The round reads a catalogue only when it has never been read or its digest moved, so an unchanged share costs
/// one list request and nothing more.
#[sqlx::test(migrations = "./migrations")]
async fn only_a_share_never_read_or_changed_since_needs_reading(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;

    let stale = mirror
        .take_offer(
            ayse(),
            &[
                offer(terrain(), "Terrain", 2, "2-100"),
                offer(bases(), "Bases", 1, "1-100"),
            ],
        )
        .await
        .expect("takes the offer");
    assert_eq!(stale.len(), 2, "neither was ever read");
    let terrain_id = stale
        .iter()
        .find(|s| s.remote == terrain())
        .expect("Terrain")
        .id;
    mirror
        .replace_catalogue(
            terrain_id,
            "2-100",
            &[part("cliff-face.stl", "Cliff face", None)],
        )
        .await
        .expect("reads");

    let stale = mirror
        .take_offer(
            ayse(),
            &[
                offer(terrain(), "Terrain", 2, "2-100"),
                offer(bases(), "Bases", 1, "1-100"),
            ],
        )
        .await
        .expect("takes the offer");
    assert_eq!(
        stale.iter().map(|s| s.remote).collect::<Vec<_>>(),
        [bases()],
        "Terrain is as it was read"
    );

    let stale = mirror
        .take_offer(
            ayse(),
            &[
                offer(terrain(), "Terrain", 3, "3-200"),
                offer(bases(), "Bases", 1, "1-100"),
            ],
        )
        .await
        .expect("takes the offer");
    let terrain_stale = stale
        .iter()
        .find(|s| s.remote == terrain())
        .expect("Terrain moved");
    assert_eq!(
        terrain_stale.digest, "3-200",
        "and is to be recorded under the digest it moved to"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_share_no_longer_offered_leaves_the_mirror_with_its_parts(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;
    let stale = mirror
        .take_offer(
            ayse(),
            &[
                offer(terrain(), "Terrain", 1, "1-100"),
                offer(bases(), "Bases", 1, "1-100"),
            ],
        )
        .await
        .expect("takes the offer");
    let terrain_id = stale
        .iter()
        .find(|s| s.remote == terrain())
        .expect("Terrain")
        .id;
    mirror
        .replace_catalogue(
            terrain_id,
            "1-100",
            &[part("cliff-face.stl", "Cliff face", None)],
        )
        .await
        .expect("reads");

    mirror
        .take_offer(ayse(), &[offer(bases(), "Bases", 1, "1-100")])
        .await
        .expect("takes the smaller offer");
    let listed: Vec<_> = mirror
        .shares_of(ayse())
        .await
        .expect("lists")
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(listed, ["Bases"]);
    assert!(
        mirror
            .parts(terrain_id, None, 50)
            .await
            .expect("reads")
            .is_empty(),
        "its parts went with it"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_catalogue_read_again_replaces_what_was_mirrored(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;
    let id = mirror
        .take_offer(ayse(), &[offer(terrain(), "Terrain", 2, "2-100")])
        .await
        .expect("offer")[0]
        .id;
    mirror
        .replace_catalogue(
            id,
            "2-100",
            &[
                part("cliff-face.stl", "Cliff face", None),
                part("standing-stone.stl", "Standing stone", None),
            ],
        )
        .await
        .expect("first read");
    mirror
        .replace_catalogue(
            id,
            "2-200",
            &[
                part("standing-stone.stl", "Standing stone", None),
                part(
                    "dolmen.stl",
                    "Dolmen",
                    Some(b"RIFF\x24\0\0\0WEBPVP8 dolmen"),
                ),
            ],
        )
        .await
        .expect("second read");

    let first = mirror.parts(id, None, 1).await.expect("page one");
    let second = mirror
        .parts(id, Some(&first[0].source_path), 50)
        .await
        .expect("page two");
    let paths: Vec<_> = first
        .iter()
        .chain(&second)
        .map(|p| p.source_path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["dolmen.stl", "standing-stone.stl"],
        "the cliff face is no longer offered"
    );
    assert!(first[0].thumbnail);
    assert_eq!(
        mirror
            .thumbnail(id, "dolmen.stl")
            .await
            .expect("reads")
            .as_deref(),
        Some(&b"RIFF\x24\0\0\0WEBPVP8 dolmen"[..])
    );
    let share = mirror.share(id).await.expect("reads").expect("mirrored");
    assert!(share.synced_at.is_some(), "a whole catalogue has been read");
}

#[sqlx::test(migrations = "./migrations")]
async fn nothing_mirrored_from_somebody_removed_is_shown(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;
    let id = mirror
        .take_offer(ayse(), &[offer(terrain(), "Terrain", 1, "1-100")])
        .await
        .expect("offer")[0]
        .id;
    mirror
        .replace_catalogue(
            id,
            "1-100",
            &[part("cliff-face.stl", "Cliff face", Some(b"webp"))],
        )
        .await
        .expect("reads");

    PgSharing(pool.clone())
        .remove_peer(ayse())
        .await
        .expect("removes");
    assert!(mirror.shares_of(ayse()).await.expect("lists").is_empty());
    assert_eq!(mirror.share(id).await.expect("reads"), None);
    assert!(mirror.parts(id, None, 50).await.expect("reads").is_empty());
    assert_eq!(
        mirror.thumbnail(id, "cliff-face.stl").await.expect("reads"),
        None
    );
}

/// Mira's studio PC, whom Ayşe knows and this installation does not — yet.
fn mira() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of mira's studio pc")
}

/// Sharing S6: the roster of a mirrored folder, and the introductions it offers.
///
/// An introduction is offered once a person a folder goes to is somebody this installation has no row for.
/// Answering it is what stops it being offered: accepted, they are paired, which is the same row pairing by
/// hand makes; declined, the answer is kept, so the next round does not ask again.
#[sqlx::test(migrations = "./migrations")]
async fn a_mirrored_folders_roster_offers_the_people_in_it_once_each(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;
    let sharing = PgSharing(pool.clone());
    let here = DeviceId::from_public_key(b"ed25519 public key of this installation");
    sharing.claim_identity(here).await.expect("an identity");
    mirror
        .take_offer(ayse(), &[offer(terrain(), "Terrain", 402, "402-1")])
        .await
        .expect("takes the offer");

    let roster = [
        RemoteMember {
            device: mira(),
            name: Some("Mira’s studio"),
            address: "192.168.1.31:8082",
            may_fetch: true,
        },
        RemoteMember {
            device: here,
            name: None,
            address: "192.168.1.12:8082",
            may_fetch: true,
        },
    ];
    assert!(
        mirror
            .take_roster(ayse(), terrain(), &roster)
            .await
            .expect("takes the roster")
    );

    let offered = mirror.introductions().await.expect("lists");
    assert_eq!(
        offered
            .iter()
            .map(|row| (row.device, row.name.as_deref(), row.share_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(mira(), Some("Mira’s studio"), "Terrain")],
        "this installation is on the roster and is never somebody to meet; Ayşe is already paired with"
    );
    assert_eq!(offered[0].introducer, ayse());

    // Accepting is the ordinary pairing, with who introduced them kept.
    let (address, introducer) = mirror
        .introduction(offered[0].share, mira())
        .await
        .expect("reads")
        .expect("one to answer");
    assert_eq!(address, "192.168.1.31:8082");
    let row = sharing
        .accept_introduction(mira(), &address, introducer)
        .await
        .expect("pairs");
    assert_eq!(row.address, "192.168.1.31:8082");
    assert!(
        mirror.introductions().await.expect("lists").is_empty(),
        "somebody paired with is not somebody to meet"
    );

    // Removed again, they are offered again — and declining keeps them from being offered a third time.
    assert!(sharing.remove_peer(mira()).await.expect("removes"));
    assert_eq!(mirror.introductions().await.expect("lists").len(), 1);
    assert!(
        mirror
            .decline(offered[0].share, mira())
            .await
            .expect("declines")
    );
    assert!(mirror.introductions().await.expect("lists").is_empty());

    // Taken off the folder, the row goes with the roster: there is nobody to introduce any more.
    mirror
        .take_roster(ayse(), terrain(), &roster[1..])
        .await
        .expect("takes the roster again");
    assert!(
        !mirror
            .decline(offered[0].share, mira())
            .await
            .expect("answers"),
        "the roster no longer names them"
    );
}
