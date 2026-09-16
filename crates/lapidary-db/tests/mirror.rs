//! Sharing S2b: other installations' shares, mirrored here so a shared library browses while they are asleep.

use lapidary_core::{DeviceId, PartId, ShareId};
use lapidary_db::{MirroredPartIn, OfferedRemote, PgMirror, PgSharing};

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
