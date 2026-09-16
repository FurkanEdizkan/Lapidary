//! The hello round (sharing S1b): the peer role asks each installation it is paired with what it is,
//! records the answer where the api reads it, and brings its own roster up to date.
//!
//! A timer rather than a job kind, for the quarantine sweep's reason (`lapidary_ingest::reap`): there is
//! no row to lease and nothing to report beyond what it writes. And it runs in the peer role because
//! that role alone holds the identity key a hello is made with; a job for the worker would need the key
//! mounted into a second container.

use crate::{Hello, PROTOCOL, PeerIdentity, Roster, client_config};
use lapidary_core::DeviceId;
use lapidary_db::{DbError, ONLINE_WITHIN_SECS, PgPool, PgSharing};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// A third of the online window, so somebody silent for three rounds stops showing online.
pub const ROUND: Duration = Duration::from_secs(ONLINE_WITHIN_SECS.unsigned_abs() / 3);

/// How long one hello may take before the round says nothing answered.
const PATIENCE: Duration = Duration::from_secs(5);

/// The longest name kept from a hello, as the table holds names.
const NAME_MAX: usize = 64;

/// Hello rounds until `shutdown`.
pub async fn run(
    db: PgPool,
    identity: std::sync::Arc<PeerIdentity>,
    roster: Roster,
    shutdown: CancellationToken,
) {
    loop {
        // A round first and a sleep after, as the quarantine sweep does: a restart must not leave
        // everybody paired refused for a whole round while the roster is still empty.
        if let Err(error) = round(&db, &identity, &roster).await {
            tracing::warn!(%error, "the hello round could not read or record who this installation shares with; the next round tries again");
        }
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = tokio::time::sleep(ROUND) => {}
        }
    }
}

/// One round: the roster from the database, then a hello to everybody on it.
pub async fn round(db: &PgPool, identity: &PeerIdentity, roster: &Roster) -> Result<(), DbError> {
    let sharing = PgSharing(db.clone());
    let name = sharing.identity().await?.and_then(|identity| identity.name);
    let paired = sharing.paired().await?;
    // The roster before any hello, so the listener takes up a pairing or a removal even when every
    // hello below waits out its patience.
    roster.replace(paired.iter().map(|(device, _)| *device).collect(), name);
    // ponytail: one hello at a time, each allowed PATIENCE. Right for the handful of machines a person
    // pairs with; past about six that do not answer, a round outlasts the online window and the hellos
    // want a JoinSet.
    for (device, address) in paired {
        match hello(identity, device, &address).await {
            Ok(answer) => {
                sharing
                    .seen(device, given_name(answer.name).as_deref())
                    .await?
            }
            Err(reason) => sharing.unreachable(device, &reason).await?,
        }
    }
    Ok(())
}

/// Ask one installation what it is, over a connection pinned to the id its owner pasted.
async fn hello(identity: &PeerIdentity, device: DeviceId, address: &str) -> Result<Hello, String> {
    let tls = client_config(identity, device).map_err(|err| err.to_string())?;
    // A client per hello, never a pooled one: a connection kept open from an earlier round would skip
    // the handshake, and the handshake is where a removal on either side takes effect.
    let client = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .timeout(PATIENCE)
        .build()
        .map_err(|err| format!("Could not set up a connection to {address}: {err}."))?;
    let response = client
        .get(format!("https://{address}/peer/v1/hello"))
        .send()
        .await
        .map_err(|err| why(address, &err))?;
    if !response.status().is_success() {
        return Err(format!(
            "The installation at {address} answered the hello with {}. Check that both installations run the same Lapidary version.",
            response.status()
        ));
    }
    let body = response.bytes().await.map_err(|err| why(address, &err))?;
    let answer: Hello = serde_json::from_slice(&body).map_err(|_| {
        format!(
            "Something at {address} answered, but not with a Lapidary hello. Check the address."
        )
    })?;
    if answer.protocol != PROTOCOL {
        return Err(format!(
            "The installation at {address} speaks sharing protocol {}, and this one speaks {PROTOCOL}. Update whichever Lapidary is older.",
            answer.protocol
        ));
    }
    Ok(answer)
}

/// Why a hello failed, in words for the person reading the list.
fn why(address: &str, error: &reqwest::Error) -> String {
    match tls_error(error) {
        Some(rustls::Error::InvalidCertificate(_)) => format!(
            "Something answered at {address}, but it is not the installation with this device id. Check the address, and the id that was pasted."
        ),
        Some(rustls::Error::AlertReceived(_)) => format!(
            "The installation at {address} turned this one away: it has not added this installation's device id, or has removed it."
        ),
        Some(other) => {
            format!("The connection to {address} failed while it was being set up: {other}.")
        }
        None if error.is_timeout() => format!(
            "Nothing answered at {address} within {} seconds. Check that the sharing service there is running and that this address reaches it.",
            PATIENCE.as_secs()
        ),
        None if error.is_connect() => format!(
            "Nothing answered at {address}. Check that the sharing service there is running and that this address reaches it."
        ),
        None => format!("The hello to {address} failed: {error}."),
    }
}

/// The TLS layer's own error, wherever in reqwest's chain it was wrapped.
///
/// An `io::Error`'s `source` skips over the error it carries, and hyper wraps a refusal from this end's
/// own verifier in two of them (`Other` around `InvalidData` around the `rustls::Error`), so each one is
/// opened rather than walked past. Walking `source` alone finds an alert the other end sent, which is
/// wrapped once, and misses this end's own refusal — reporting the wrong machine at an address as
/// nothing answering there.
fn tls_error(error: &reqwest::Error) -> Option<&rustls::Error> {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = cause {
        if let Some(tls) = current.downcast_ref::<rustls::Error>() {
            return Some(tls);
        }
        cause = match current.downcast_ref::<std::io::Error>() {
            Some(io) => io
                .get_ref()
                .map(|inner| inner as &(dyn std::error::Error + 'static)),
            None => current.source(),
        };
    }
    None
}

/// A name as the other installation gave it, kept to what the table holds: trimmed, at most
/// [`NAME_MAX`] characters, and none at all when nothing is left.
fn given_name(name: Option<String>) -> Option<String> {
    let kept: String = name?.trim().chars().take(NAME_MAX).collect();
    let kept = kept.trim_end();
    (!kept.is_empty()).then(|| kept.to_owned())
}

#[cfg(test)]
mod tests {
    use super::given_name;

    /// Another installation names itself however it likes; what is stored must fit the table's check,
    /// or one odd name would fail the whole round's write.
    #[test]
    fn a_given_name_is_kept_to_what_the_table_holds() {
        assert_eq!(
            given_name(Some("  Ayşe's workshop ".to_owned())).as_deref(),
            Some("Ayşe's workshop")
        );
        assert_eq!(given_name(Some("   ".to_owned())), None);
        assert_eq!(given_name(None), None);
        // Counted in characters, not bytes, and trimmed again where the cut leaves a space.
        let long = format!("{} workshop", "ş".repeat(63));
        assert_eq!(
            given_name(Some(long)).map(|name| name.chars().count()),
            Some(63)
        );
    }
}
