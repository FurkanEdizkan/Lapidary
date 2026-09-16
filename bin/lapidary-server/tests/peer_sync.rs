//! The hello round (sharing S1b): each paired installation asked what it is, its answer recorded where
//! the page reads it, and the listener's roster brought up to date.
//!
//! Here rather than in `crates/lapidary-peer/tests`, because these need `#[sqlx::test]` and `deny.toml`
//! lets only a fixed list of crates take `sqlx` at all — a list whose own comment says that growing it means
//! SQL has leaked. `lapidary-peer` holds no SQL; this binary, which wires the peer role, is already on the
//! list for exactly these tests' reason.

use lapidary_core::DeviceId;
use lapidary_db::PgSharing;
use lapidary_peer::{PeerIdentity, Roster, router, serve, server_config, sync};

/// Another installation listening on the loopback, paired with `paired` and going by `name`.
async fn listening(paired: Vec<DeviceId>, name: Option<&str>) -> (DeviceId, String) {
    let other = PeerIdentity::generate().expect("its identity");
    let id = other.device_id().expect("its id");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port on the loopback");
    let address = tcp.local_addr().expect("the port it took").to_string();
    let roster = Roster::new(paired, name.map(str::to_owned));
    let config = server_config(&other, &roster).expect("its side of the connection");
    tokio::spawn(serve(tcp, config, router(id, roster)));
    (id, address)
}

/// This installation, its identity claimed in `pool` as the peer role claims it on starting.
async fn this_installation(pool: &sqlx::PgPool) -> (PeerIdentity, DeviceId) {
    let identity = PeerIdentity::generate().expect("this installation's identity");
    let id = identity.device_id().expect("its id");
    PgSharing(pool.clone())
        .claim_identity(id)
        .await
        .expect("claims its id");
    (identity, id)
}

/// A loopback address nothing listens on: a port taken and let go again.
fn nothing_listening() -> String {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a port on the loopback")
        .local_addr()
        .expect("the port it took")
        .to_string()
}

/// Why the round says the one person paired is not online.
async fn reason(pool: &sqlx::PgPool) -> String {
    let peer = PgSharing(pool.clone())
        .peers()
        .await
        .expect("lists")
        .remove(0);
    assert!(!peer.online, "not online");
    peer.last_error.expect("a reason")
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn an_installation_that_answers_is_online_under_the_name_it_gives(pool: sqlx::PgPool) {
    let (here, here_id) = this_installation(&pool).await;
    let (ayse, address) = listening(vec![here_id], Some("Ayşe's workshop")).await;
    let sharing = PgSharing(pool.clone());
    sharing.add_peer(ayse, &address).await.expect("pairs");

    sync::round(&pool, &here, &Roster::default())
        .await
        .expect("a round");

    let peer = sharing.peers().await.expect("lists").remove(0);
    assert!(peer.online, "answered: {:?}", peer.last_error);
    assert_eq!(peer.name.as_deref(), Some("Ayşe's workshop"));
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn an_installation_that_has_not_added_this_one_says_it_turned_this_one_away(
    pool: sqlx::PgPool,
) {
    let (here, _) = this_installation(&pool).await;
    let (ayse, address) = listening(Vec::new(), None).await;
    PgSharing(pool.clone())
        .add_peer(ayse, &address)
        .await
        .expect("pairs");

    sync::round(&pool, &here, &Roster::default())
        .await
        .expect("a round");

    let reason = reason(&pool).await;
    assert!(reason.contains("turned this one away"), "{reason}");
    assert!(reason.contains(&address), "and names where: {reason}");
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_different_installation_at_the_address_is_not_taken_for_the_one_paired(
    pool: sqlx::PgPool,
) {
    let (here, here_id) = this_installation(&pool).await;
    // It pairs with this installation, so the only refusal left is this end's own.
    let (_somebody_else, address) = listening(vec![here_id], None).await;
    let ayse =
        DeviceId::from_public_key(b"the key of the workshop pc that should be at that address");
    PgSharing(pool.clone())
        .add_peer(ayse, &address)
        .await
        .expect("pairs");

    sync::round(&pool, &here, &Roster::default())
        .await
        .expect("a round");

    let reason = reason(&pool).await;
    assert!(
        reason.contains("not the installation with this device id"),
        "{reason}"
    );
}

#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn nothing_listening_at_the_address_says_nothing_answered(pool: sqlx::PgPool) {
    let (here, _) = this_installation(&pool).await;
    let address = nothing_listening();
    let makerspace = DeviceId::from_public_key(b"the key of the Kadikoy makerspace print server");
    PgSharing(pool.clone())
        .add_peer(makerspace, &address)
        .await
        .expect("pairs");

    sync::round(&pool, &here, &Roster::default())
        .await
        .expect("a round");

    let reason = reason(&pool).await;
    assert!(reason.contains("Nothing answered at"), "{reason}");
}

/// Pairing and removal happen through the api, in the database; the listener learns of both from a
/// round, along with the name this installation's owner gave it.
#[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
async fn a_round_takes_up_who_is_paired_and_the_name_this_installation_goes_by(pool: sqlx::PgPool) {
    let (here, _) = this_installation(&pool).await;
    let sharing = PgSharing(pool.clone());
    assert!(
        sharing
            .set_name(Some("Furkan's workbench"))
            .await
            .expect("names")
    );
    let makerspace = DeviceId::from_public_key(b"the key of the Kadikoy makerspace print server");
    sharing
        .add_peer(makerspace, &nothing_listening())
        .await
        .expect("pairs");
    let roster = Roster::default();

    sync::round(&pool, &here, &roster).await.expect("a round");
    assert!(
        roster.includes(&makerspace),
        "paired, so the listener accepts it"
    );
    assert_eq!(roster.name().as_deref(), Some("Furkan's workbench"));

    sharing.remove_peer(makerspace).await.expect("removes");
    sync::round(&pool, &here, &roster)
        .await
        .expect("another round");
    assert!(
        !roster.includes(&makerspace),
        "removed, so the listener refuses it"
    );
}
