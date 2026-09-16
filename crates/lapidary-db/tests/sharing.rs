//! Sharing (S1b): this installation's identity, and the list of people it shares with.

use lapidary_core::DeviceId;
use lapidary_db::PgSharing;

/// Ayşe's workshop PC, on the same LAN.
fn ayse() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the workshop pc in Ayse's garage")
}

/// The makerspace's machine, reached over the Tailscale network its members share.
fn makerspace() -> DeviceId {
    DeviceId::from_public_key(b"ed25519 public key of the Kadikoy makerspace print server")
}

/// Every row in `peer`, removed or not, as (removed?, address).
async fn every_row(pool: &sqlx::PgPool) -> Vec<(bool, String)> {
    sqlx::query_as("SELECT removed_at IS NOT NULL, address FROM peer ORDER BY added_at")
        .fetch_all(pool)
        .await
        .expect("reads the table directly")
}

#[sqlx::test(migrations = "./migrations")]
async fn an_installation_whose_peer_role_never_ran_has_no_identity_to_name(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool);
    assert_eq!(sharing.identity().await.expect("reads"), None);
    assert!(
        !sharing
            .set_name(Some("Furkan's workbench"))
            .await
            .expect("answers"),
        "nothing to name until the peer role has claimed an id"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_new_key_changes_the_id_and_keeps_the_name(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool);
    let first = DeviceId::from_public_key(b"the key the peer role made on its first start");
    sharing.claim_identity(first).await.expect("claims");
    assert!(
        sharing
            .set_name(Some("Furkan's workbench"))
            .await
            .expect("names")
    );

    let again = DeviceId::from_public_key(b"a new key, after the peer volume was lost");
    sharing.claim_identity(again).await.expect("claims again");
    let identity = sharing.identity().await.expect("reads").expect("claimed");
    assert_eq!(identity.device_id, again, "the id follows the key");
    assert_eq!(
        identity.name.as_deref(),
        Some("Furkan's workbench"),
        "the name stays"
    );

    assert!(sharing.set_name(None).await.expect("clears"));
    assert_eq!(
        sharing
            .identity()
            .await
            .expect("reads")
            .expect("claimed")
            .name,
        None
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn somebody_just_added_is_listed_and_not_yet_online(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool);
    let added = sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("adds");
    assert_eq!(added.device_id, ayse());
    assert_eq!(added.address, "192.168.1.24:8082");
    assert_eq!(
        (added.name, added.last_seen_at, added.online),
        (None, None, false)
    );

    let listed = sharing.peers().await.expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].device_id, ayse());
}

/// The latest round decides: an answer makes somebody online and clears the last failure, and a failure
/// after that takes them offline at once while keeping when they were last seen.
#[sqlx::test(migrations = "./migrations")]
async fn an_answered_hello_is_online_and_a_failed_one_says_why(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool);
    sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("adds");

    sharing
        .unreachable(ayse(), "nothing answered at 192.168.1.24:8082")
        .await
        .expect("records");
    let refused = &sharing.peers().await.expect("lists")[0];
    assert!(!refused.online);
    assert_eq!(
        refused.last_error.as_deref(),
        Some("nothing answered at 192.168.1.24:8082")
    );

    sharing
        .seen(ayse(), Some("Ayşe's workshop"))
        .await
        .expect("records");
    let seen = &sharing.peers().await.expect("lists")[0];
    assert!(seen.online, "answered just now");
    assert_eq!(seen.name.as_deref(), Some("Ayşe's workshop"));
    assert_eq!(seen.last_error, None, "an answer clears the last failure");
    let when = seen.last_seen_at.expect("seen");

    sharing
        .unreachable(ayse(), "refused: not the installation expected")
        .await
        .expect("records");
    let after = &sharing.peers().await.expect("lists")[0];
    assert!(
        !after.online,
        "the failure is the latest word, however recent the answer before it"
    );
    assert_eq!(
        after.last_seen_at,
        Some(when),
        "and when they were last seen is kept"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn somebody_silent_for_three_rounds_is_no_longer_online(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("adds");
    sharing.seen(ayse(), None).await.expect("records");
    sqlx::query("UPDATE peer SET last_seen_at = now() - make_interval(secs => $1::float8 + 1)")
        .bind(lapidary_db::ONLINE_WITHIN_SECS)
        .execute(&pool)
        .await
        .expect("ages the answer");
    assert!(!sharing.peers().await.expect("lists")[0].online);
}

#[sqlx::test(migrations = "./migrations")]
async fn removing_somebody_hides_them_and_deletes_nothing(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool.clone());
    sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("adds");
    sharing
        .add_peer(makerspace(), "100.101.12.7:8082")
        .await
        .expect("adds");

    assert!(sharing.remove_peer(ayse()).await.expect("removes"));
    let listed: Vec<_> = sharing
        .peers()
        .await
        .expect("lists")
        .into_iter()
        .map(|p| p.device_id)
        .collect();
    assert_eq!(listed, [makerspace()]);
    let paired: Vec<_> = sharing
        .paired()
        .await
        .expect("reads")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        paired,
        [makerspace()],
        "and the peer role stops accepting them"
    );

    assert_eq!(
        every_row(&pool).await,
        [
            (true, "192.168.1.24:8082".to_owned()),
            (false, "100.101.12.7:8082".to_owned())
        ],
        "the row is still there, marked removed"
    );
    assert!(
        !sharing.remove_peer(ayse()).await.expect("answers"),
        "removed already"
    );
    assert!(
        !sharing
            .remove_peer(DeviceId::from_public_key(b"never paired"))
            .await
            .expect("answers"),
        "never paired"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn adding_somebody_removed_brings_their_row_back(pool: sqlx::PgPool) {
    let sharing = PgSharing(pool.clone());
    let first = sharing
        .add_peer(ayse(), "192.168.1.24:8082")
        .await
        .expect("adds");
    sharing.remove_peer(ayse()).await.expect("removes");

    let back = sharing
        .add_peer(ayse(), "192.168.1.31:8082")
        .await
        .expect("adds again");
    assert_eq!(back.added_at, first.added_at, "the same row, not a new one");
    assert_eq!(
        back.address, "192.168.1.31:8082",
        "at the address given now"
    );
    assert_eq!(
        every_row(&pool).await,
        [(false, "192.168.1.31:8082".to_owned())]
    );
    assert_eq!(
        sharing.paired().await.expect("reads"),
        [(ayse(), "192.168.1.31:8082".to_owned())]
    );
}
