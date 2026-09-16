//! The hello round (sharing S1b): the peer role asks each installation it is paired with what it is,
//! records the answer where the api reads it, and brings its own roster up to date.
//!
//! A timer rather than a job kind, for the quarantine sweep's reason (`lapidary_ingest::reap`): there is
//! no row to lease and nothing to report beyond what it writes. And it runs in the peer role because
//! that role alone holds the identity key a hello is made with; a job for the worker would need the key
//! mounted into a second container.

use crate::shares::{CATALOGUE_MAX, CataloguePage, Share};
use crate::{Hello, PROTOCOL, PeerIdentity, Roster, client_config};
use lapidary_core::DeviceId;
use lapidary_db::{
    DbError, MirroredPartIn, ONLINE_WITHIN_SECS, OfferedRemote, PgListener, PgMirror, PgPool,
    PgSharing, SHARING_CHANNEL,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// A third of the online window, so somebody silent for three rounds stops showing online.
pub const ROUND: Duration = Duration::from_secs(ONLINE_WITHIN_SECS.unsigned_abs() / 3);

/// How long one hello may take before the round says nothing answered.
const PATIENCE: Duration = Duration::from_secs(5);

/// The longest name kept from a hello, as the table holds names.
const NAME_MAX: usize = 64;

/// How many hellos a round has out at once. A machine that accepts and never answers costs its own hello's
/// patience; with this many in flight, a round of that many silent machines costs one patience, not their sum.
const HELLOS_AT_ONCE: usize = 8;

/// How long one request of a mirror read may take: a catalogue page of 500 parts, or one thumbnail.
const MIRROR_PATIENCE: Duration = Duration::from_secs(30);

/// Hello rounds until `shutdown`, each followed by a mirror of what every installation that answered shares.
///
/// A round starts on the tick, or at once when the api pairs, removes, shares or stops sharing
/// ([`SHARING_CHANNEL`]); the tick stays the floor, so a lost notification costs a round's wait and nothing more.
/// A mirror runs beside the rounds rather than inside one: reading a large catalogue must not push the next round's
/// hellos past the online window.
pub async fn run(
    db: PgPool,
    identity: std::sync::Arc<PeerIdentity>,
    roster: Roster,
    shutdown: CancellationToken,
) {
    let mut listener = listen(&db).await;
    let mirroring: Arc<Mutex<HashSet<DeviceId>>> = Arc::default();
    loop {
        // A round first and a sleep after, as the quarantine sweep does: a restart must not leave
        // everybody paired refused for a whole round while the roster is still empty.
        match round(&db, &identity, &roster).await {
            Ok(answered) => {
                for (device, address) in answered {
                    start_mirror(&db, &identity, &mirroring, device, address);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "the hello round could not read or record who this installation shares with; the next round tries again");
            }
        }
        if !wait(&mut listener, &shutdown).await {
            return;
        }
    }
}

/// Listen for the api's notifications, or run on the tick alone when that is not possible.
pub(crate) async fn listen(db: &PgPool) -> Option<PgListener> {
    let connected = match PgListener::connect_with(db).await {
        Ok(mut listener) => listener.listen(SHARING_CHANNEL).await.map(|()| listener),
        Err(error) => Err(error),
    };
    match connected {
        Ok(listener) => Some(listener),
        Err(error) => {
            tracing::warn!(%error, "could not listen for sharing changes; hello rounds run on the tick alone");
            None
        }
    }
}

/// Until the next round is due: the tick, or a notification. `false` when shutting down.
pub(crate) async fn wait(listener: &mut Option<PgListener>, shutdown: &CancellationToken) -> bool {
    let lost = match listener {
        Some(listening) => tokio::select! {
            () = shutdown.cancelled() => return false,
            () = tokio::time::sleep(ROUND) => false,
            heard = listening.recv() => heard.is_err(),
        },
        None => tokio::select! {
            () = shutdown.cancelled() => return false,
            () = tokio::time::sleep(ROUND) => false,
        },
    };
    if lost {
        tracing::warn!("stopped hearing sharing changes; hello rounds run on the tick alone");
        *listener = None;
    }
    true
}

/// Start mirroring `device`, unless a mirror of it is still reading.
fn start_mirror(
    db: &PgPool,
    identity: &Arc<PeerIdentity>,
    mirroring: &Arc<Mutex<HashSet<DeviceId>>>,
    device: DeviceId,
    address: String,
) {
    if !mirroring
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(device)
    {
        return;
    }
    let (db, identity, mirroring) = (db.clone(), identity.clone(), mirroring.clone());
    tokio::spawn(async move {
        match mirror(&db, &identity, device, &address).await {
            Ok(report) if report.read > 0 => {
                tracing::info!(device_id = %device, ?report, "mirrored what an installation shares")
            }
            Ok(_) => {}
            Err(reason) => {
                tracing::warn!(device_id = %device, %reason, "could not mirror what an installation shares; the next round tries again")
            }
        }
        mirroring
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&device);
    });
}

/// One round: the roster from the database, then a hello to everybody on it. Answers who answered.
pub async fn round(
    db: &PgPool,
    identity: &std::sync::Arc<PeerIdentity>,
    roster: &Roster,
) -> Result<Vec<(DeviceId, String)>, DbError> {
    let sharing = PgSharing(db.clone());
    let name = sharing.identity().await?.and_then(|identity| identity.name);
    let paired = sharing.paired().await?;
    // The roster before any hello, so the listener takes up a pairing or a removal even when every
    // hello below waits out its patience.
    roster.replace(paired.iter().map(|(device, _)| *device).collect(), name);
    let mut hellos = tokio::task::JoinSet::new();
    let mut outcomes = Vec::with_capacity(paired.len());
    for (device, address) in paired {
        if hellos.len() >= HELLOS_AT_ONCE
            && let Some(done) = hellos.join_next().await
        {
            outcomes.extend(done.ok());
        }
        let identity = identity.clone();
        hellos.spawn(async move {
            let outcome = hello(&identity, device, &address).await;
            (device, address, outcome)
        });
    }
    // A hello that panicked records nothing, and the next round asks again.
    outcomes.extend(hellos.join_all().await);
    let mut answered = Vec::new();
    for (device, address, outcome) in outcomes {
        match outcome {
            Ok(answer) => {
                sharing
                    .seen(device, given_name(answer.name).as_deref())
                    .await?;
                answered.push((device, address));
            }
            Err(reason) => sharing.unreachable(device, &reason).await?,
        }
    }
    Ok(answered)
}

/// What one mirror read, for the log and for the measurement.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MirrorReport {
    /// Shares the other installation offers.
    pub shares: usize,
    /// Shares whose catalogue was read, because it was new or its digest moved.
    pub read: usize,
    pub pages: usize,
    /// The catalogue pages' bodies, as they came over the wire.
    pub catalogue_bytes: usize,
    pub parts: usize,
    pub thumbnail_bytes: usize,
    /// Thumbnails larger than [`THUMBNAIL_MAX`], left out of the mirror.
    pub thumbnails_skipped: usize,
}

/// The largest thumbnail mirrored. A thumbnail is stored inline only below 64 KB (`DATA.md` §1.5), so nothing a
/// sharer made is larger; anything that is, is not a thumbnail this installation should keep.
pub const THUMBNAIL_MAX: usize = 64 * 1024;

/// Mirror what `device` shares: its list, and the catalogue and thumbnails of every share that is new or whose
/// digest moved.
pub async fn mirror(
    db: &PgPool,
    identity: &PeerIdentity,
    device: DeviceId,
    address: &str,
) -> Result<MirrorReport, String> {
    // One client for the whole read, so a thousand thumbnails ride a few connections. The sharer checks access
    // on every request, so a removal on its side still stops the next one.
    let tls = client_config(identity, device).map_err(|err| err.to_string())?;
    let client = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .timeout(MIRROR_PATIENCE)
        .build()
        .map_err(|err| format!("Could not set up a connection to {address}: {err}."))?;
    let base = format!("https://{address}/peer/v1/shares");

    let (offered, _): (Vec<Share>, usize) = read_json(client.get(&base), address).await?;
    let offers: Vec<OfferedRemote<'_>> = offered
        .iter()
        .map(|share| OfferedRemote {
            remote: share.id,
            name: &share.name,
            part_count: share.part_count,
            digest: &share.digest,
        })
        .collect();
    let mirror = PgMirror(db.clone());
    let stale = mirror
        .take_offer(device, &offers)
        .await
        .map_err(|err| err.to_string())?;
    let mut report = MirrorReport {
        shares: offered.len(),
        ..MirrorReport::default()
    };

    for share in stale {
        let remote = share.remote.as_uuid();
        let mut parts = Vec::new();
        let mut after = String::new();
        loop {
            let request = client.get(format!("{base}/{remote}/catalogue")).query(&[
                ("after", after.as_str()),
                ("limit", &CATALOGUE_MAX.to_string()),
            ]);
            let (page, page_bytes): (CataloguePage, usize) = read_json(request, address).await?;
            report.pages += 1;
            report.catalogue_bytes += page_bytes;
            parts.extend(page.parts);
            match page.next {
                Some(next) => after = next,
                None => break,
            }
        }
        let mut thumbnails = Vec::with_capacity(parts.len());
        for part in &parts {
            let bytes = if part.thumbnail {
                let request = client
                    .get(format!("{base}/{remote}/thumbnail"))
                    .query(&[("part", part.part.as_uuid().to_string())]);
                read_thumbnail(request, address).await?
            } else {
                None
            };
            match bytes {
                Some(bytes) if bytes.len() > THUMBNAIL_MAX => {
                    report.thumbnails_skipped += 1;
                    thumbnails.push(None);
                }
                Some(bytes) => {
                    report.thumbnail_bytes += bytes.len();
                    thumbnails.push(Some(bytes));
                }
                None => thumbnails.push(None),
            }
        }
        let rows: Vec<MirroredPartIn<'_>> = parts
            .iter()
            .zip(&thumbnails)
            .map(|(part, thumbnail)| MirroredPartIn {
                source_path: &part.source_path,
                remote_part: part.part,
                name: &part.name,
                part_number: part.part_number.as_deref(),
                tags: &part.tags,
                licences: &part.licences,
                blake3: part.blake3.as_deref(),
                size_bytes: part.size_bytes,
                format: part.format.as_deref(),
                thumbnail: thumbnail.as_deref(),
            })
            .collect();
        // Under the digest the list gave before the pages were read: a change made while they were being read moves
        // the digest again, and the next round reads the catalogue once more.
        mirror
            .replace_catalogue(share.id, &share.digest, &rows)
            .await
            .map_err(|err| err.to_string())?;
        report.read += 1;
        report.parts += parts.len();
    }
    Ok(report)
}

/// A JSON answer from the sharer and how many bytes it took, or why there was none, in words.
async fn read_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    address: &str,
) -> Result<(T, usize), String> {
    let response = request.send().await.map_err(|err| why(address, &err))?;
    if !response.status().is_success() {
        return Err(format!(
            "The installation at {address} would not give what it shares ({}). It may have stopped sharing, or removed this installation.",
            response.status()
        ));
    }
    let body = response.bytes().await.map_err(|err| why(address, &err))?;
    let bytes = body.len();
    serde_json::from_slice(&body).map(|answer| (answer, bytes)).map_err(|_| {
        format!("The installation at {address} answered with a list this installation cannot read. Check that both run the same Lapidary version.")
    })
}

/// A thumbnail, `None` when the part has none any more — it may have been removed while the catalogue was read.
async fn read_thumbnail(
    request: reqwest::RequestBuilder,
    address: &str,
) -> Result<Option<Vec<u8>>, String> {
    let response = request.send().await.map_err(|err| why(address, &err))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!(
            "The installation at {address} would not give a thumbnail ({}).",
            response.status()
        ));
    }
    let body = response.bytes().await.map_err(|err| why(address, &err))?;
    Ok(Some(body.to_vec()))
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
