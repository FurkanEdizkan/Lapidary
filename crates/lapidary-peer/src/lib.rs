//! The peer protocol: what one installation answers to another it has been paired with.
//!
//! This crate is the only thing in Lapidary that speaks to another installation, and the only
//! process that serves it is `LAPIDARY_ROLE=peer`. It carries no route of the api's: someone the
//! owner paired with can ask this crate what it is (`/peer/v1/hello`) and, later, what is shared
//! and for its bytes — and nothing else. `lapidary-api` may never depend on it, which
//! `xtask/src/layers.rs` enforces by name rather than by review.
//!
//! **Identity is a keypair, and the id is its digest.** Both ends present a raw public key
//! (RFC 7250) rather than a certificate, so nothing here generates or parses X.509: the verifiers
//! compare `DeviceId::from_public_key` of what the other end presented against the ids their owner
//! paired with, and refuse during the handshake if it is not one of them. A connection that gets
//! past that is from a key its owner wrote down; a connection that does not reaches no route.

use lapidary_core::DeviceId;
use std::path::Path;
use std::sync::Arc;

pub mod blob;
pub mod pull;
pub mod shares;
pub mod sync;

/// What the identity key is called inside the peer directory.
const KEY_FILE: &str = "identity.pkcs8";

/// Write the key so only its owner can read it.
///
/// It is the whole of this installation's identity: anyone who can read it can answer as this
/// machine to everyone paired with it. The peer directory is a mounted volume that other things on
/// the host can see, so the mode is set as the file is made rather than after, leaving no moment
/// where the key sits there world-readable.
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

/// Every deployment that runs the peer role is a container on Linux (`docs/ARCHITECTURE.md`), so
/// the mode above is the real path. This keeps the crate building anywhere else, and says plainly
/// that the key is not protected there.
#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// What a refusal here says. Every one of them names what to do about it, because the person
/// reading it is setting up sharing between two machines and cannot see the other one.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error(
        "Could not make this installation's identity key: {detail}. Sharing needs one key per installation, kept in the peer directory."
    )]
    KeyGeneration { detail: String },

    #[error(
        "Could not read the identity key at {path}: {detail}. First check that the directory and the key belong to the user the peer role runs as: in the container image that is `lapidary` (uid 10001). Only if the key itself is gone does removing what is left there make a new one — and this installation's device id changes with it, so everyone sharing with it has to add the new id."
    )]
    KeyUnreadable { path: String, detail: String },

    #[error(
        "Could not write this installation's identity key into {dir}: {detail}. No key was there to lose. The directory has to be writable by the user the peer role runs as: in the container image that is `lapidary` (uid 10001), and a named volume keeps the owner it was created with, so one created by an image that did not make this directory is owned by root. Give it to that user, then start the peer role again."
    )]
    KeyUnwritable { dir: String, detail: String },

    #[error("Could not set up the peer connection: {detail}.")]
    Tls { detail: String },
}

/// This installation's keypair, and the id every other installation knows it by.
///
/// Kept on disk rather than in Postgres, for the reason `upload_dir` is: only the peer role holds
/// it, and a key in the database is a key the api's credentials can read.
pub struct PeerIdentity {
    /// The PKCS#8 document `ring` generated, which is the private key.
    pkcs8: Vec<u8>,
}

impl PeerIdentity {
    /// A fresh keypair. The caller writes it down; this does not touch the disk.
    pub fn generate() -> Result<Self, PeerError> {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| {
            PeerError::KeyGeneration {
                detail: "the system's random number generator would not answer".to_owned(),
            }
        })?;
        Ok(Self {
            pkcs8: pkcs8.as_ref().to_vec(),
        })
    }

    /// The identity in `dir`, made and written on the first start and read back on every one
    /// after. The device id must not change between runs: it is what other people added by hand.
    pub fn load_or_generate(dir: &Path) -> Result<Self, PeerError> {
        let path = dir.join(KEY_FILE);
        let unreadable = |detail: String| PeerError::KeyUnreadable {
            path: path.display().to_string(),
            detail,
        };
        match std::fs::read(&path) {
            Ok(pkcs8) => {
                let identity = Self { pkcs8 };
                // Read it back through ring before trusting it: a truncated file would otherwise
                // become a device id nobody paired with, discovered only at the first handshake.
                identity.public_key()?;
                Ok(identity)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate()?;
                let unwritable = |err: std::io::Error| PeerError::KeyUnwritable {
                    dir: dir.display().to_string(),
                    detail: err.to_string(),
                };
                std::fs::create_dir_all(dir).map_err(unwritable)?;
                write_private(&path, &identity.pkcs8).map_err(unwritable)?;
                Ok(identity)
            }
            Err(err) => Err(unreadable(err.to_string())),
        }
    }

    /// The public half, in exactly the form a peer connection presents it: an RFC 7250
    /// `SubjectPublicKeyInfo`, which is the raw Ed25519 key inside a short DER wrapper.
    ///
    /// The wrapper is hashed along with the key, deliberately. Both ends hash the bytes rustls put
    /// on the wire, so neither has to unwrap anything to know what it was shown — which is what
    /// keeps an X.509 parser out of the trust path entirely.
    pub fn public_key(&self) -> Result<Vec<u8>, PeerError> {
        Ok(self.signing_key()?.1)
    }

    /// The key rustls signs with, and the public key it presents for it.
    fn signing_key(&self) -> Result<(Arc<dyn rustls::sign::SigningKey>, Vec<u8>), PeerError> {
        let pkcs8 = rustls::pki_types::PrivatePkcs8KeyDer::from(self.pkcs8.clone());
        let signing = rustls::crypto::ring::sign::any_eddsa_type(&pkcs8).map_err(|err| {
            PeerError::KeyGeneration {
                detail: format!(
                    "the identity key is not an Ed25519 key this build can read: {err}"
                ),
            }
        })?;
        let presented = signing
            .public_key()
            .ok_or_else(|| PeerError::Tls {
                detail: "this installation's identity key does not offer a public key to present"
                    .to_owned(),
            })?
            .to_vec();
        Ok((signing, presented))
    }

    /// What everyone else calls this installation.
    pub fn device_id(&self) -> Result<DeviceId, PeerError> {
        Ok(DeviceId::from_public_key(&self.public_key()?))
    }
}

/// The identity as rustls presents it: the public key itself, where a certificate would go.
///
/// RFC 7250 sends a bare `SubjectPublicKeyInfo` in the slot the certificate chain occupies, which
/// is why this reads as a one-entry chain. Nothing here parses X.509, and nothing signs one.
fn certified_key(identity: &PeerIdentity) -> Result<Arc<rustls::sign::CertifiedKey>, PeerError> {
    let (signing, presented) = identity.signing_key()?;
    Ok(Arc::new(rustls::sign::CertifiedKey::new(
        vec![rustls::pki_types::CertificateDer::from(presented)],
        signing,
    )))
}

/// The digest of what the other end presented, or a refusal if it presented nothing usable.
fn presented(spki: &rustls::pki_types::CertificateDer<'_>) -> Result<DeviceId, rustls::Error> {
    Ok(DeviceId::from_public_key(spki.as_ref()))
}

/// Ed25519 and nothing else: both ends are Lapidary, and the key each presents is one this crate
/// generated.
fn schemes() -> Vec<rustls::SignatureScheme> {
    vec![rustls::SignatureScheme::ED25519]
}

/// What the accepting end says: somebody its owner has not paired with tried to connect.
const REFUSED_UNPAIRED: &str =
    "refused a connection: this installation is not paired with that device id";

/// What the connecting end says: something answered at that address, but not the machine expected.
const REFUSED_UNEXPECTED: &str =
    "refused: the installation answering at that address is not the one expected";

/// Who this installation is paired with and what it calls itself, as the peer role last read them.
///
/// Refreshed from the database by each hello round ([`sync::round`]) and read on every connection, so
/// somebody paired or removed through the api is accepted or refused within a round, without a restart.
/// The handshake reads it once, and a handshake is not a path that needs anything faster than a lock.
#[derive(Clone, Debug, Default)]
pub struct Roster(Arc<std::sync::RwLock<Entries>>);

#[derive(Debug, Default)]
struct Entries {
    paired: Vec<DeviceId>,
    name: Option<String>,
}

impl Roster {
    pub fn new(paired: Vec<DeviceId>, name: Option<String>) -> Self {
        Self(Arc::new(std::sync::RwLock::new(Entries { paired, name })))
    }

    /// Take up what the database says now.
    pub fn replace(&self, paired: Vec<DeviceId>, name: Option<String>) {
        // A plain list with no invariant a panicking writer could have broken halfway, so a poisoned
        // lock still holds a usable one.
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Entries { paired, name };
    }

    pub fn includes(&self, device: &DeviceId) -> bool {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .paired
            .contains(device)
    }

    pub fn name(&self) -> Option<String> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .name
            .clone()
    }
}

/// Refuses anyone whose key its owner did not write down.
#[derive(Debug)]
struct Pinned {
    /// Empty means nobody is paired yet, which refuses everyone rather than admitting everyone.
    allowed: Roster,
    /// What a refusal here means. The two ends turn a connection down for different reasons, and
    /// whoever reads a log can see only their own end of it.
    refusal: &'static str,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl Pinned {
    /// The accepting end, which admits the installations its owner paired with and nobody else.
    fn accepting(roster: &Roster, provider: Arc<rustls::crypto::CryptoProvider>) -> Self {
        Self {
            allowed: roster.clone(),
            refusal: REFUSED_UNPAIRED,
            provider,
        }
    }

    /// The connecting end, which expects exactly one installation to be answering over there.
    fn connecting(expect: DeviceId, provider: Arc<rustls::crypto::CryptoProvider>) -> Self {
        Self {
            allowed: Roster::new(vec![expect], None),
            refusal: REFUSED_UNEXPECTED,
            provider,
        }
    }

    fn allows(&self, spki: &rustls::pki_types::CertificateDer<'_>) -> Result<(), rustls::Error> {
        let presented = presented(spki)?;
        if self.allowed.includes(&presented) {
            Ok(())
        } else {
            // The handshake ends here, so no route of ours ever sees this connection — which is
            // also why this is the only place a refusal can be named. Somebody setting sharing up
            // has two machines and one mistyped id between them, and without this line both ends
            // are silent about it.
            tracing::info!(device_id = %presented, "{}", self.refusal);
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    /// The signature over the handshake, checked against the raw public key the other end
    /// presented rather than against a certificate.
    ///
    /// `rustls::crypto::verify_tls13_signature` reads its `cert` argument as X.509 to find the key
    /// in it, which is exactly wrong here: what was presented is a bare `SubjectPublicKeyInfo`, so
    /// that helper fails as a DER parse rather than as a bad signature. This is the raw-key
    /// variant, and the difference is invisible in a passing test — it shows only as
    /// `BadEncoding` on every connection.
    fn verify_raw_key(
        &self,
        message: &[u8],
        presented: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature_with_raw_key(
            message,
            &rustls::pki_types::SubjectPublicKeyInfoDer::from(presented.as_ref()),
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    /// rustls has no raw-public-key signature check for TLS 1.2, and both peer configs offer
    /// TLS 1.3 alone, so this cannot be reached. It refuses rather than falling back to the
    /// certificate path: that fallback is what made every connection fail as a DER parse, and a
    /// version downgrade must not quietly turn the pinning into something else.
    fn refuse_tls12(
        &self,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General(
            "a peer connection speaks TLS 1.3 only: raw public keys have no TLS 1.2 signature check"
                .to_owned(),
        ))
    }
}

impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        self.allows(end_entity)?;
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.refuse_tls12()
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.verify_raw_key(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        schemes()
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

impl rustls::server::danger::ClientCertVerifier for Pinned {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        self.allows(end_entity)?;
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.refuse_tls12()
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.verify_raw_key(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        schemes()
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

/// The provider this crate signs and verifies with: `ring`, the same one reqwest already pulls in,
/// rather than a second implementation of the same primitives.
fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accept connections from the installations on `roster`, as it stands at each handshake, and no others.
pub fn server_config(
    identity: &PeerIdentity,
    roster: &Roster,
) -> Result<rustls::ServerConfig, PeerError> {
    let provider = provider();
    let verifier = Arc::new(Pinned::accepting(roster, provider.clone()));
    Ok(rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|err| PeerError::Tls {
            detail: err.to_string(),
        })?
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(Arc::new(
            rustls::server::AlwaysResolvesServerRawPublicKeys::new(certified_key(identity)?),
        )))
}

/// Connect to exactly this installation, and refuse any other answering at that address.
pub fn client_config(
    identity: &PeerIdentity,
    expect: DeviceId,
) -> Result<rustls::ClientConfig, PeerError> {
    let provider = provider();
    let verifier = Arc::new(Pinned::connecting(expect, provider.clone()));
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|err| PeerError::Tls {
            detail: err.to_string(),
        })?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_cert_resolver(Arc::new(
            rustls::client::AlwaysResolvesClientRawPublicKeys::new(certified_key(identity)?),
        )))
}

/// What this installation says when another asks what it is.
///
/// `protocol` is the peer protocol's own version, not the build's: two installations on different
/// Lapidary versions still share these routes, and what they must agree on is the shape of them.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub device_id: String,
    pub protocol: u16,
    /// What this installation calls itself, when its owner has named it.
    pub name: Option<String>,
    /// What it answers beyond the routes protocol 1 shipped with, as [`FEATURES`] lists them.
    ///
    /// This is how the protocol grows without its number moving. An installation from before a route existed
    /// sends no list at all, and `serde` reads that as none, so nothing here ever asks it for that route; and
    /// it reads this hello with a field it does not know and ignores it. Bumping `PROTOCOL` instead would make
    /// every older installation mark this one permanently unreachable, which is the opposite of an upgrade.
    #[serde(default)]
    pub features: Vec<String>,
}

/// The peer protocol's version, answered at hello and carried in every route's path.
pub const PROTOCOL: u16 = 1;

/// What this installation answers beyond protocol 1's routes.
///
/// [`ROSTERS`]: `GET /peer/v1/shares/{share}/members`, the roster of a folder, which is how the people a folder
/// goes to learn about each other (sharing S6).
///
/// [`RELAY`]: folders held here but owned by somebody else, listed to the people that folder goes to and served
/// under `?owner=`, so a folder stays browsable while its owner is away (sharing S7).
pub const FEATURES: &[&str] = &[ROSTERS, RELAY];

/// The feature string for a folder's roster: answered by the installation that serves it, and asked for only of
/// an installation whose hello listed it.
pub const ROSTERS: &str = "members";

/// The feature string for a folder passed on by one of its people rather than by its owner. An installation
/// that does not list it is never sent one: it would record the folder as this installation's own and then ask
/// this installation for files it does not share.
pub const RELAY: &str = "relay";

/// The hello route. `shares::shares_router` and `blob::blob_router` hold the rest of what this installation answers.
pub fn router(identity: DeviceId, roster: Roster) -> axum::Router {
    axum::Router::new().route(
        "/peer/v1/hello",
        axum::routing::get(move || {
            let name = roster.name();
            async move {
                axum::Json(Hello {
                    device_id: identity.to_string(),
                    protocol: PROTOCOL,
                    name,
                    features: FEATURES
                        .iter()
                        .map(|feature| (*feature).to_owned())
                        .collect(),
                })
            }
        }),
    )
}

/// Serve `router` on `listener`, to the installations `config` was built to accept.
///
/// The binary's `peer` role calls this, and so does the handshake test: one function, so what runs
/// in a container is what the test exercised.
pub async fn serve(
    listener: tokio::net::TcpListener,
    config: rustls::ServerConfig,
    router: axum::Router,
) -> std::io::Result<()> {
    serve_with_shutdown(listener, config, router, std::future::pending::<()>()).await
}

/// The same, stopping when `shutdown` completes.
///
/// `bin/lapidary-server` serves every role through one `axum::serve` with one shutdown token, and
/// the peer role must not be the exception that ignores it: a container that will not stop on
/// `SIGTERM` is one an operator learns to kill.
pub async fn serve_with_shutdown(
    listener: tokio::net::TcpListener,
    config: rustls::ServerConfig,
    router: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_for(listener, config, router, shutdown, HANDSHAKE_TIMEOUT).await
}

/// How long a connection may take over its TLS handshake before it is closed.
///
/// A handshake between two installations on a LAN or a VPN takes milliseconds; ten seconds is room for a
/// slow link, and short enough that somebody opening the port and saying nothing costs a task for a few
/// seconds rather than a connection held forever.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The first wait after the port fails to accept a connection. It doubles for each failure in a row, up
/// to [`MOST_BACKOFF`], and goes back to this after a connection is accepted.
const FIRST_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);

/// The longest wait between attempts while accepting keeps failing — out of file descriptors, say.
const MOST_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);

/// The wait after the one just taken, when accepting has failed again.
fn next_backoff(wait: std::time::Duration) -> std::time::Duration {
    (wait * 2).min(MOST_BACKOFF)
}

/// [`serve_with_shutdown`], with the handshake's time allowed given rather than fixed, so a test can
/// watch a silent connection closed without waiting the full ten seconds.
async fn serve_for(
    listener: tokio::net::TcpListener,
    config: rustls::ServerConfig,
    router: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    handshake: std::time::Duration,
) -> std::io::Result<()> {
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    axum::serve(
        PeerListener::start(listener, acceptor, handshake)?,
        router.into_make_service_with_connect_info::<PeerDevice>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

/// The installation on the other end of a connection, as its handshake proved it.
///
/// Read off the key the TLS session verified, never off anything the request says, so a route that asks
/// "who is this" gets the answer the pinning already checked. `None` only for a connection that presented no
/// key, which the verifier refuses before any route runs; a route treats it as nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerDevice(pub Option<DeviceId>);

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, PeerListener>>
    for PeerDevice
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, PeerListener>) -> Self {
        let (_, session) = stream.io().get_ref();
        // The same bytes the verifier hashed, so the id a route sees is the id the pinning checked.
        PeerDevice(
            session
                .peer_certificates()
                .and_then(|chain| chain.first())
                .map(|presented| DeviceId::from_public_key(presented.as_ref())),
        )
    }
}

/// A connection through its handshake, and who it came from.
type Handshaken = (
    tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    std::net::SocketAddr,
);

/// A TCP listener that hands back connections already through the handshake.
///
/// Axum's own `Listener` is the seam for this, so the peer role serves through the same
/// `axum::serve` the api does — the difference between them is this type, and nothing else.
///
/// The handshakes happen in a task of their own, each in a task of its own, and only finished ones
/// reach `accept`. Awaiting each one inline, as the first version did, let one connection that opened
/// the port and said nothing hold up every connection after it, for as long as it stayed open.
pub struct PeerListener {
    handshaken: tokio::sync::mpsc::Receiver<Handshaken>,
    local: std::net::SocketAddr,
}

impl PeerListener {
    fn start(
        tcp: tokio::net::TcpListener,
        acceptor: tokio_rustls::TlsAcceptor,
        handshake: std::time::Duration,
    ) -> std::io::Result<Self> {
        let local = tcp.local_addr()?;
        let (done, handshaken) = tokio::sync::mpsc::channel(64);
        tokio::spawn(accepting(tcp, acceptor, handshake, done));
        Ok(Self { handshaken, local })
    }
}

/// Accept connections and start each one's handshake, until the listener is dropped.
async fn accepting(
    tcp: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    handshake: std::time::Duration,
    done: tokio::sync::mpsc::Sender<Handshaken>,
) {
    let mut wait = FIRST_BACKOFF;
    loop {
        let accepted = tokio::select! {
            () = done.closed() => return,
            accepted = tcp.accept() => accepted,
        };
        let (stream, address) = match accepted {
            Ok(accepted) => {
                wait = FIRST_BACKOFF;
                accepted
            }
            Err(error) => {
                // Axum's trait has nowhere to report this, and returning would end the listener. A
                // failure that persists — out of file descriptors — would otherwise spin a core.
                tracing::warn!(%error, wait_ms = wait.as_millis(), "could not accept a connection on the peer port; waiting before trying again");
                tokio::time::sleep(wait).await;
                wait = next_backoff(wait);
                continue;
            }
        };
        let (acceptor, done) = (acceptor.clone(), done.clone());
        let handshaking = async move {
            // A refused handshake is where a machine nobody paired with stops: it never becomes a
            // connection, so no route of ours is reached. For a *pinning* refusal the verifier has
            // already named the device id in the log, and that line is the whole record of it. A
            // handshake that fails or runs out of time before any key is presented — plain HTTP on this
            // port, a port scan — leaves no record at all.
            if let Ok(Ok(tls)) = tokio::time::timeout(handshake, acceptor.accept(stream)).await {
                let _ = done.send((tls, address)).await;
            }
        };
        tokio::spawn(handshaking);
    }
}

impl axum::serve::Listener for PeerListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.handshaken.recv().await {
            Some(connection) => connection,
            // Only if the accepting task is gone, which it never is while this listener lives: it
            // returns only once this receiver has been dropped.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory the peer role cannot write, such as a named volume that came up owned by root, is not a key that went
    /// missing: nothing was there to lose, and the answer is the directory's ownership, never removing anything.
    #[test]
    fn a_directory_it_cannot_write_is_named_as_such_and_not_as_a_lost_key() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("read-only");
        if std::fs::write(dir.path().join("probe"), b"").is_ok() {
            // Running as root, which ignores the mode: nothing to test here.
            return;
        }
        let refused = PeerIdentity::load_or_generate(dir.path())
            .err()
            .expect("refused");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("writable again");
        assert!(
            matches!(refused, PeerError::KeyUnwritable { .. }),
            "{refused}"
        );
        let message = refused.to_string();
        assert!(
            message.contains("writable") && !message.contains("removing"),
            "{message}"
        );
    }

    #[test]
    fn an_identity_is_named_by_the_digest_of_what_it_presents() {
        use ring::signature::KeyPair as _;
        let identity = PeerIdentity::generate().expect("a keypair");
        let presented = identity.public_key().expect("what it puts on the wire");
        let raw = ring::signature::Ed25519KeyPair::from_pkcs8(&identity.pkcs8)
            .expect("the key it just made")
            .public_key()
            .as_ref()
            .to_vec();

        assert_eq!(raw.len(), 32, "an Ed25519 public key is 32 bytes");
        assert!(
            presented.len() > raw.len() && presented.ends_with(&raw),
            "presented as a SubjectPublicKeyInfo wrapped around that key"
        );
        assert_eq!(
            identity.device_id().expect("its id"),
            DeviceId::from_public_key(&presented),
            "the id is the digest of exactly the bytes the other end is shown"
        );
    }

    #[test]
    fn two_installations_are_named_differently() {
        let one = PeerIdentity::generate().expect("a keypair");
        let other = PeerIdentity::generate().expect("another keypair");
        assert_ne!(
            one.device_id().expect("one id"),
            other.device_id().expect("the other id")
        );
    }

    /// The other end of a peer connection, set up to expect exactly one installation.
    fn client(identity: &PeerIdentity, expect: DeviceId) -> reqwest::Client {
        let tls = client_config(identity, expect).expect("the client's side of the connection");
        reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .build()
            .expect("a client over that connection")
    }

    /// The handshake with no HTTP client in the way, and each end's answer read separately: a
    /// refusal here is rustls's, and one only in the test below is the client library's.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_handshake_alone_succeeds_between_paired_installations() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let server =
            server_config(&host, &Roster::new(vec![guest_id], None)).expect("the host's side");
        let accepting = tokio::spawn(async move {
            let (stream, _) = tcp.accept().await.expect("a connection to accept");
            tokio_rustls::TlsAcceptor::from(Arc::new(server))
                .accept(stream)
                .await
                .map(|_| ())
        });

        let connector = tokio_rustls::TlsConnector::from(Arc::new(
            client_config(&guest, host_id).expect("the guest's side"),
        ));
        let stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("the port answers");
        // The name is not what either end trusts — the key is — but rustls needs one to ask for.
        let name = rustls::pki_types::ServerName::try_from("peer.invalid").expect("a name to ask");
        let connected = connector.connect(name, stream).await;
        let accepted = accepting.await.expect("the accepting task finishes");

        assert!(connected.is_ok(), "the guest's end: {:?}", connected.err());
        assert!(accepted.is_ok(), "the host's end: {:?}", accepted.err());
    }

    /// What S1a is for: an installation its owner paired with gets an answer, and one nobody
    /// paired with is refused while the connection is still being made — before any route runs, so
    /// a stranger on the network learns nothing about what is here.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_paired_installation_is_answered_and_a_stranger_never_reaches_a_route() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let stranger = PeerIdentity::generate().expect("somebody else's identity");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let config = server_config(&host, &Roster::new(vec![guest_id], None))
            .expect("the host's side, pairing the guest");
        tokio::spawn(serve(tcp, config, router(host_id, Roster::default())));

        let answer = client(&guest, host_id)
            .get(format!("https://{address}/peer/v1/hello"))
            .send()
            .await
            .expect("the host answers somebody it paired with")
            .text()
            .await
            .expect("an answer to read");
        assert!(
            answer.contains(&host_id.to_string()),
            "the host names itself: {answer}"
        );
        assert!(
            answer.contains("\"protocol\":1"),
            "and says which protocol it speaks: {answer}"
        );

        let refused = client(&stranger, host_id)
            .get(format!("https://{address}/peer/v1/hello"))
            .send()
            .await
            .expect_err("a machine nobody paired with gets no answer");
        assert!(
            !refused.is_status(),
            "refused while connecting rather than by a route: {refused}"
        );
    }

    /// Everything this crate logged on this thread, one string per event with its fields in it, so
    /// a test can read a refusal the way the person setting sharing up reads it.
    struct Logged(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl tracing::Subscriber for Logged {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.target() == "lapidary_peer"
        }
        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::TRACE)
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut line = String::new();
            event.record(&mut Fields(&mut line));
            self.0.lock().expect("the log").push(line);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The message and the device id both reach the string, since both are fields of the event.
    struct Fields<'a>(&'a mut String);

    impl tracing::field::Visit for Fields<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!("{}={value:?} ", field.name()));
        }
    }

    /// A refusal that says nothing is the one failure its owner cannot work out: two machines, one
    /// mistyped id, and silence at both ends. Each end names the id it turned away, once, and says
    /// which end it is — the accepting side was never paired with it, the connecting side expected
    /// somebody else at that address.
    #[test]
    fn a_refusal_names_the_device_id_it_turned_away() {
        let stranger = PeerIdentity::generate().expect("somebody else's identity");
        let presented = rustls::pki_types::CertificateDer::from(
            stranger.public_key().expect("what it would present"),
        );
        let stranger_id = stranger.device_id().expect("the id it is refused under");

        // Somebody else entirely, so the connecting end's expectation is genuinely not met.
        let expected = PeerIdentity::generate()
            .expect("the installation that should be there")
            .device_id()
            .expect("its id");

        let logged = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        {
            let _capturing = tracing::subscriber::set_default(Logged(logged.clone()));
            let ends = [
                Pinned::accepting(&Roster::default(), provider()),
                Pinned::connecting(expected, provider()),
            ];
            for end in ends {
                assert!(end.allows(&presented).is_err(), "neither end wanted this");
            }
        }

        let lines = logged.lock().expect("the log").clone();
        assert_eq!(
            lines.len(),
            2,
            "one line per refusal and no more: {lines:?}"
        );
        for (line, refusal) in lines.iter().zip([REFUSED_UNPAIRED, REFUSED_UNEXPECTED]) {
            assert!(
                line.contains(&stranger_id.to_string()),
                "the id it turned away is in the line: {line}"
            );
            assert!(
                line.contains(refusal),
                "and so is which end turned it down: {line}"
            );
        }
    }

    /// The other direction, and the one a wrong address actually produces: the device id is one the
    /// guest paired with, but something else is answering there. The connecting end refuses it
    /// rather than talking to it — the pinning is not only about who may come in.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_machine_that_is_not_the_one_expected_is_refused() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let elsewhere = PeerIdentity::generate().expect("the installation the guest meant");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        // The host would have this guest: the refusal below is the guest's own, not the host's.
        let config = server_config(&host, &Roster::new(vec![guest_id], None))
            .expect("the host's side, pairing the guest");
        tokio::spawn(serve(tcp, config, router(host_id, Roster::default())));

        let expected = elsewhere
            .device_id()
            .expect("the id the guest expects there");
        let refused = client(&guest, expected)
            .get(format!("https://{address}/peer/v1/hello"))
            .send()
            .await
            .expect_err("what answers there is not what the guest paired with");
        assert!(
            !refused.is_status(),
            "refused while connecting rather than by a route: {refused}"
        );
    }

    /// Pairing is a row somebody adds through the api while the listener is already running. The roster
    /// the listener checks is the one each hello round refreshes, so somebody added is answered and
    /// somebody removed is refused again, with no restart in between.
    #[tokio::test(flavor = "multi_thread")]
    async fn somebody_paired_or_removed_while_the_listener_runs_is_answered_or_refused() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let roster = Roster::default();
        let config = server_config(&host, &roster).expect("the host's side, nobody paired yet");
        tokio::spawn(serve(tcp, config, router(host_id, roster.clone())));
        // A new client each time: a pooled connection would skip the very handshake this is about.
        let hello = || {
            let request = client(&guest, host_id).get(format!("https://{address}/peer/v1/hello"));
            async move { request.send().await }
        };

        assert!(hello().await.is_err(), "refused while nobody is paired");

        roster.replace(vec![guest_id], Some("Ayşe's workshop".to_owned()));
        let answer = hello()
            .await
            .expect("answered once paired")
            .text()
            .await
            .expect("an answer to read");
        assert!(
            answer.contains("Ayşe's workshop"),
            "and says the name it goes by: {answer}"
        );

        roster.replace(Vec::new(), None);
        assert!(hello().await.is_err(), "refused again once removed");
    }

    /// Every share route asks which installation is asking, and the answer is the key its handshake proved.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_route_learns_the_device_its_handshake_proved() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let config = server_config(&host, &Roster::new(vec![guest_id], None))
            .expect("the host's side, pairing the guest");
        let whoami = axum::Router::new().route(
            "/peer/v1/whoami",
            axum::routing::get(
                |axum::extract::ConnectInfo(PeerDevice(device)): axum::extract::ConnectInfo<
                    PeerDevice,
                >| async move { device.map(|id| id.to_string()).unwrap_or_default() },
            ),
        );
        tokio::spawn(serve(tcp, config, whoami));

        let answer = client(&guest, host_id)
            .get(format!("https://{address}/peer/v1/whoami"))
            .send()
            .await
            .expect("the host answers the guest")
            .text()
            .await
            .expect("an answer to read");
        assert_eq!(
            answer,
            guest_id.to_string(),
            "the guest, as its key proved it"
        );
    }

    /// Somebody opens the peer port and says nothing: a port scan, or a client that never starts its
    /// handshake. That must cost a paired installation nothing — its hello is answered while the silent
    /// connection is still open.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_connection_that_never_handshakes_does_not_hold_up_a_paired_one() {
        let host = PeerIdentity::generate().expect("the host's identity");
        let guest = PeerIdentity::generate().expect("the guest's identity");
        let host_id = host.device_id().expect("the host's id");
        let guest_id = guest.device_id().expect("the guest's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let config = server_config(&host, &Roster::new(vec![guest_id], None))
            .expect("the host's side, pairing the guest");
        tokio::spawn(serve(tcp, config, router(host_id, Roster::default())));

        let _silent = tokio::net::TcpStream::connect(address)
            .await
            .expect("the port answers the silent one");
        // Its connection is accepted first, so a listener that waits on it would wait on it alone.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let answered = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client(&guest, host_id)
                .get(format!("https://{address}/peer/v1/hello"))
                .send(),
        )
        .await;
        assert!(
            matches!(answered, Ok(Ok(_))),
            "the paired installation is answered while the silent connection waits: {answered:?}"
        );
    }

    /// A silent connection is not held open forever: once its handshake's time is up, the listener hangs
    /// up on it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_connection_that_never_handshakes_is_closed_once_its_time_is_up() {
        use tokio::io::AsyncReadExt as _;
        let host = PeerIdentity::generate().expect("the host's identity");
        let host_id = host.device_id().expect("the host's id");

        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a port on the loopback");
        let address = tcp.local_addr().expect("the port it took");
        let config = server_config(&host, &Roster::default()).expect("the host's side");
        tokio::spawn(serve_for(
            tcp,
            config,
            router(host_id, Roster::default()),
            std::future::pending::<()>(),
            std::time::Duration::from_millis(300),
        ));

        let mut silent = tokio::net::TcpStream::connect(address)
            .await
            .expect("the port answers");
        let mut byte = [0u8; 1];
        let hung_up =
            tokio::time::timeout(std::time::Duration::from_secs(3), silent.read(&mut byte)).await;
        assert!(
            matches!(hung_up, Ok(Ok(0)) | Ok(Err(_))),
            "the listener closes it after 300 ms rather than waiting on it: {hung_up:?}"
        );
    }

    /// While accepting keeps failing — out of file descriptors, say — the listener waits longer each time,
    /// up to a second, rather than spinning a core.
    #[test]
    fn a_failing_accept_waits_longer_each_time_up_to_a_second() {
        let mut wait = FIRST_BACKOFF;
        let waits: Vec<u128> = (0..6)
            .map(|_| {
                let this = wait.as_millis();
                wait = next_backoff(wait);
                this
            })
            .collect();
        assert_eq!(waits, [100, 200, 400, 800, 1000, 1000]);
    }

    /// The id is what other people wrote down, so a restart must not change it.
    #[test]
    fn an_installation_keeps_its_id_across_restarts() {
        let dir = tempfile::tempdir().expect("a peer directory");
        let first = PeerIdentity::load_or_generate(dir.path()).expect("made on the first start");
        let again = PeerIdentity::load_or_generate(dir.path()).expect("read back on the next");
        assert_eq!(
            first.device_id().expect("the id it made"),
            again.device_id().expect("the id it read back"),
            "a restart must not change what everyone else added by hand"
        );
        assert!(
            dir.path().join("identity.pkcs8").exists(),
            "the key is written where the peer directory is mounted"
        );
    }
}
