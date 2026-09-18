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
        owner: None,
        as_of: None,
    }
}

/// The same folder as another of its people passes it on: its owner's id for it, and when they read it (S7).
fn relayed<'a>(
    owner: DeviceId,
    remote: ShareId,
    name: &'a str,
    part_count: i64,
    digest: &'a str,
    as_of: jiff::Timestamp,
) -> OfferedRemote<'a> {
    OfferedRemote {
        remote,
        name,
        part_count,
        digest,
        owner: Some(owner),
        as_of: Some(as_of),
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

/// Sharing S7: Terrain is Ayşe's, and Mira holds it too. When Ayşe is away, Mira's list is how it stays
/// browsable — and a copy passed on must never be able to say a folder is gone, or to undo a fresher reading.
#[sqlx::test(migrations = "./migrations")]
async fn a_relayed_catalogue_only_ever_adds_and_only_when_it_is_newer(pool: sqlx::PgPool) {
    // Relative to the clock, never written as dates: the direct read below is stamped `now()` by the
    // database, so a relay's reading is behind it only if it is earlier than today, and ahead only if
    // it is later. Fixed dates passed the day they were written and failed the day after.
    let now = jiff::Timestamp::now()
        .round(jiff::Unit::Second)
        .expect("rounds");
    let behind = now
        .checked_sub(jiff::SignedDuration::from_hours(24))
        .expect("a day earlier");
    let ahead = now
        .checked_add(jiff::SignedDuration::from_hours(1))
        .expect("an hour later");
    let mirror = paired(&pool).await;
    PgSharing(pool.clone())
        .add_peer(mira(), "192.168.1.31:8082")
        .await
        .expect("pairs with Mira as well");

    // Read from Ayşe herself, as every round has until now.
    let stale = mirror
        .take_offer(ayse(), &[offer(terrain(), "Terrain", 2, "2-100")])
        .await
        .expect("takes the offer");
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].owner, ayse(), "hers, and read from her");
    mirror
        .replace_catalogue(
            stale[0].id,
            "2-100",
            &[
                part("rocks/cliff-face.stl", "Cliff face, LP-TR-0112", None),
                part("standing-stone.stl", "Standing stone, LP-TR-0140", None),
            ],
        )
        .await
        .expect("reads it");

    // Mira offers her own folder and passes Terrain on, as she read it — before this installation did.
    let stale = mirror
        .take_offer(
            mira(),
            &[
                offer(bases(), "Bases", 1, "1-1"),
                relayed(ayse(), terrain(), "Terrain", 2, "2-100", behind),
            ],
        )
        .await
        .expect("takes Mira's list");
    assert_eq!(
        stale.iter().map(|share| share.owner).collect::<Vec<_>>(),
        vec![mira()],
        "only Mira's own folder is new; the Terrain she passes on is the one already held, and older"
    );
    let held = mirror.shares_of(ayse()).await.expect("lists");
    assert_eq!(
        held.len(),
        1,
        "Terrain is still Ayşe's folder, mirrored once"
    );
    assert_eq!(held[0].read_from, None, "read from her, not from Mira");
    assert_eq!(
        mirror
            .parts(held[0].id, None, 10)
            .await
            .expect("reads")
            .len(),
        2,
        "and a list from Mira never empties a folder of Ayşe's"
    );

    // Ayşe has changed Terrain since, and Mira has read it. That copy is newer, so it is taken.
    let stale = mirror
        .take_offer(
            mira(),
            &[
                offer(bases(), "Bases", 1, "1-1"),
                relayed(ayse(), terrain(), "Terrain", 3, "3-200", ahead),
            ],
        )
        .await
        .expect("takes Mira's list again");
    let through_mira = stale
        .iter()
        .find(|share| share.owner == ayse())
        .expect("Ayşe's folder is to be read again, through Mira");
    assert_eq!(through_mira.digest, "3-200");
    mirror
        .relay_catalogue(
            through_mira.id,
            "3-200",
            &[
                part("rocks/cliff-face.stl", "Cliff face, LP-TR-0112", None),
                part("rocks/scree.stl", "Scree slope, LP-TR-0118", None),
                part("standing-stone.stl", "Standing stone, LP-TR-0140", None),
            ],
            mira(),
            ahead,
        )
        .await
        .expect("takes what Mira read");
    let held = mirror.shares_of(ayse()).await.expect("lists");
    assert_eq!(held[0].read_from, Some(mira()));
    assert_eq!(
        held[0].as_of.map(|at| at.to_string()),
        Some(ahead.to_string())
    );
    assert_eq!(
        mirror
            .parts(held[0].id, None, 10)
            .await
            .expect("reads")
            .len(),
        3
    );

    // Mira falls behind — an older reading of the same folder is not taken, however often she offers it.
    let stale = mirror
        .take_offer(
            mira(),
            &[relayed(ayse(), terrain(), "Terrain", 2, "2-100", behind)],
        )
        .await
        .expect("takes Mira's list");
    assert!(
        stale.is_empty(),
        "a reading older than the one held is nothing to read"
    );
    assert_eq!(
        mirror
            .parts(held[0].id, None, 10)
            .await
            .expect("reads")
            .len(),
        3,
        "and it leaves what is held alone"
    );

    // Bases was Mira's own, and she stops offering it: that one she is entitled to withdraw.
    mirror
        .take_offer(
            mira(),
            &[relayed(ayse(), terrain(), "Terrain", 3, "3-200", ahead)],
        )
        .await
        .expect("takes Mira's list");
    assert!(mirror.shares_of(mira()).await.expect("lists").is_empty());
    assert_eq!(mirror.shares_of(ayse()).await.expect("lists").len(), 1);
}

/// A folder is passed on to the people its owner said it goes to, and to nobody else: holding the bytes is
/// not what entitles anybody to them.
#[sqlx::test(migrations = "./migrations")]
async fn a_folder_is_passed_on_only_to_the_people_its_owner_named(pool: sqlx::PgPool) {
    let mirror = paired(&pool).await;
    let nazli = DeviceId::from_public_key(b"ed25519 public key of nazli's laptop");
    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(mira(), "192.168.1.31:8082")
        .await
        .expect("pairs");
    sharing
        .add_peer(nazli, "192.168.1.44:8082")
        .await
        .expect("pairs");
    let stale = mirror
        .take_offer(ayse(), &[offer(terrain(), "Terrain", 1, "1-1")])
        .await
        .expect("takes the offer");
    mirror
        .replace_catalogue(
            stale[0].id,
            "1-1",
            &[part(
                "standing-stone.stl",
                "Standing stone, LP-TR-0140",
                None,
            )],
        )
        .await
        .expect("reads it");
    mirror
        .take_roster(
            ayse(),
            terrain(),
            &[RemoteMember {
                device: mira(),
                name: Some("Mira’s studio"),
                address: "192.168.1.31:8082",
                may_fetch: true,
            }],
        )
        .await
        .expect("takes the roster");

    let passed = mirror.relayable_to(mira()).await.expect("lists");
    assert_eq!(
        passed
            .iter()
            .map(|share| (
                share.owner,
                share.remote,
                share.name.as_str(),
                share.digest.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![(ayse(), terrain(), "Terrain", "1-1")],
        "Mira is on Terrain's roster, so she may have it from here while Ayşe is away"
    );
    assert!(
        mirror.relayable_to(nazli).await.expect("lists").is_empty(),
        "Nazlı is paired with this installation and is not in that folder"
    );
    assert_eq!(
        mirror
            .relayed_to(ayse(), terrain(), mira())
            .await
            .expect("answers"),
        Some(stale[0].id)
    );
    assert_eq!(
        mirror
            .relayed_to(ayse(), terrain(), nazli)
            .await
            .expect("answers"),
        None,
        "the refusal a stranger gets"
    );
    assert_eq!(
        mirror
            .relayed_to(ayse(), bases(), mira())
            .await
            .expect("answers"),
        None,
        "a folder this installation does not mirror"
    );
}
