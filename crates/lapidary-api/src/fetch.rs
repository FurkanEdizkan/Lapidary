//! Fetching an image from a URL somebody pasted, which is a request to make this server
//! send traffic wherever a stranger points it.
//!
//! `docs/DATA.md` §4.1 opens with the sentence this module exists to satisfy — *"Fetching a
//! user-supplied URL is SSRF"* — and lists the controls. `images.rs` owns the ones about the
//! bytes; these are the ones about the network.
//!
//! # What an attacker is reaching for
//!
//! An application that fetches a URL for you is an application that will make requests from
//! inside your network. The prize is usually the cloud metadata endpoint at
//! **`169.254.169.254`**, which hands out instance credentials to anything that asks from the
//! right place — and this server is in the right place. After that: `127.0.0.1` and whatever
//! is listening on it, `10.0.0.0/8` and the rest of the private ranges, and the Postgres this
//! process can already reach.
//!
//! # The controls, and why each one is not enough alone
//!
//! - **Resolve first, then check the address, then connect to the address we checked.**
//!   Checking the *hostname* is worthless — `localtest.me` resolves to `127.0.0.1` and so can
//!   anything an attacker owns. Checking the resolved address and then letting the client
//!   resolve again is worth almost as little: that is DNS rebinding, and the gap between the
//!   two lookups is the whole attack. [`Client::resolve`] pins the connection to the address
//!   this module validated, so there is no second lookup to poison.
//! - **Every address a name resolves to must pass, not just the one we use.** A name that
//!   answers with one public and one private address is a name that gets a different one on
//!   the next lookup, and the round-robin is the attacker's to arrange.
//! - **Redirects are followed by hand, one hop at a time.** A client that follows redirects
//!   for you is a client that resolves and connects to hosts you never checked — the check
//!   above applies to the first URL only, and `302 Location: http://169.254.169.254/` walks
//!   straight past it. So the policy is [`redirect::Policy::none`] and the loop below is the
//!   redirect handling, with every control re-applied to every hop.
//! - **A budget for the whole fetch, not per request.** Four hops at ten seconds each is
//!   forty seconds of a request handler held open by someone else's server.
//! - **The byte cap is enforced while reading.** `Content-Length` is a claim by the same
//!   stranger; a cap applied after the body arrives is a cap that has already cost what it
//!   was meant to save.
//!
//! # What this does not defend against, stated rather than left out
//!
//! A host that is *legitimately* public and serves something enormous or slow is bounded by
//! the byte cap and the time budget and nothing else — which is the right answer, because it
//! is an ordinary fetch of an ordinary URL. And an operator who runs this server somewhere
//! that routes private space *into* the public ranges has a network this module cannot
//! reason about.

use crate::images::MAX_INPUT_BYTES;
use reqwest::{Client, redirect};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use url::Url;

/// `DATA.md` §4.1: max 3 redirects. Four requests, counting the first.
const MAX_REDIRECTS: usize = 3;

/// The whole fetch, not one request of it. §4.1's ten seconds, spent across every hop.
const BUDGET: Duration = Duration::from_secs(10);

/// Why a URL was not fetched. Every variant is a sentence for the person who pasted it —
/// and deliberately vague about *which* private range was hit, because the useful answer to
/// "is 10.0.0.5 up?" is the same one for every address in there.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error(
        "That is not a URL Lapidary can fetch. Paste the address of an image, starting with http:// or https://."
    )]
    NotFetchable,

    #[error(
        "That address is not one this server will fetch from. Lapidary only fetches from the public internet — an address inside a private network, or one that resolves into one, is refused so that a pasted link cannot be used to reach machines behind the server."
    )]
    NotPublic,

    #[error(
        "That host could not be found. Check the address, and that it is reachable from this server."
    )]
    Unresolvable,

    #[error(
        "That address redirected more than {MAX_REDIRECTS} times, so Lapidary stopped following it. Paste the address of the image itself rather than a link that leads to it."
    )]
    TooManyRedirects,

    #[error(
        "That address redirected somewhere Lapidary could not read. The `Location` it sent back is not a URL."
    )]
    BadRedirect,

    #[error("That address took longer than {} seconds to answer, so Lapidary stopped waiting.", BUDGET.as_secs())]
    TimedOut,

    #[error("That address answered {status}, so there was nothing to fetch.")]
    NotOk { status: u16 },

    #[error(
        "That address did not answer with an image — it says it is `{content_type}`. Paste the address of the image itself; a page that shows an image is not the image."
    )]
    NotAnImage { content_type: String },

    #[error(
        "That image is larger than the {} MB limit, so Lapidary stopped downloading it.",
        MAX_INPUT_BYTES / (1024 * 1024)
    )]
    TooLarge,

    #[error("That address could not be reached: {detail}")]
    Unreachable { detail: String },
}

/// Fetch the bytes at `url`, or say why not.
///
/// The bytes are untrusted and are **not** an image yet — `images::normalize` is what decides
/// that, from the file's own header. This function's job ends at "these bytes came from a
/// place we were willing to ask".
pub async fn fetch_image(url: &str) -> Result<Vec<u8>, FetchError> {
    tokio::time::timeout(BUDGET, follow(url, Policy::PublicOnly))
        .await
        .map_err(|_| FetchError::TimedOut)?
}

/// Which addresses this module will connect to.
///
/// `PublicOnly` is the only variant that exists in a built binary: the other is
/// `#[cfg(test)]`, and `#[cfg(test)]` items are invisible even to this crate's own
/// integration tests. There is no flag, no environment variable and no builder that turns
/// the guard off in something that ships.
///
/// **`Exempt` names one address, not a range**, and that is the point. A test needs to reach
/// a server on loopback; blanket-allowing private space would also allow the redirect target
/// in the test that matters most, and the guard would go unexercised in exactly the case it
/// exists for. With one address exempt, `169.254.169.254` is still refused — so the redirect
/// test proves the check runs on every hop rather than proving the policy was disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    PublicOnly,
    #[cfg(test)]
    Exempt(IpAddr),
}

async fn follow(url: &str, policy: Policy) -> Result<Vec<u8>, FetchError> {
    let mut next = Url::parse(url).map_err(|_| FetchError::NotFetchable)?;

    for _ in 0..=MAX_REDIRECTS {
        // Re-applied to every hop, which is the entire reason redirects are followed here
        // rather than by the client.
        let response = one_hop(&next, policy).await?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(FetchError::BadRedirect)?;
            // Joined against the current URL, because `Location` is allowed to be relative.
            next = next.join(location).map_err(|_| FetchError::BadRedirect)?;
            continue;
        }

        if !response.status().is_success() {
            return Err(FetchError::NotOk {
                status: response.status().as_u16(),
            });
        }

        // Checked, and then not trusted: `images::normalize` reads the file's own header,
        // because this string is written by the same stranger as the bytes. What it buys is
        // a better sentence for the common mistake of pasting the page rather than the image.
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if !content_type.is_empty() && !content_type.starts_with("image/") {
            return Err(FetchError::NotAnImage { content_type });
        }

        return read_capped(response).await;
    }

    Err(FetchError::TooManyRedirects)
}

/// One request, to an address this function has resolved and checked itself.
async fn one_hop(url: &Url, policy: Policy) -> Result<reqwest::Response, FetchError> {
    // `http` and `https` only. Everything else — `file:`, `gopher:`, `ftp:`, and whatever a
    // client library happens to support — is refused before anything else happens.
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::NotFetchable);
    }
    let port = url
        .port_or_known_default()
        .ok_or(FetchError::NotFetchable)?;
    // Matched on `Host` rather than taken as a string, and the difference is not cosmetic:
    // `host_str()` hands back `[::1]` **with the brackets** for a literal IPv6 URL, which
    // `lookup_host` cannot parse. That answered `Unresolvable` — still a refusal, but for
    // the wrong reason, and it meant the address guard was never reached for a literal IPv6
    // address at all. A literal is also not a name, so there is nothing to look up.
    let (host, addresses): (String, Vec<SocketAddr>) = match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.to_owned();
            let resolved: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), port))
                .await
                .map_err(|_| FetchError::Unresolvable)?
                .collect();
            (name, resolved)
        }
        Some(url::Host::Ipv4(ip)) => (ip.to_string(), vec![SocketAddr::from((ip, port))]),
        Some(url::Host::Ipv6(ip)) => (ip.to_string(), vec![SocketAddr::from((ip, port))]),
        None => return Err(FetchError::NotFetchable),
    };
    if addresses.is_empty() {
        return Err(FetchError::Unresolvable);
    }
    // **Every** address, not the one we are about to use. A name answering with one public
    // and one private address is a name whose next lookup is the attacker's to choose.
    if !addresses.iter().all(|a| allowed(a.ip(), policy)) {
        return Err(FetchError::NotPublic);
    }
    let pinned = addresses[0];

    Client::builder()
        // The pin. Without it the client resolves the host again when it connects, and the
        // window between this module's lookup and that one is DNS rebinding.
        .resolve(&host, pinned)
        // The loop above is the redirect handling. A client that followed them would connect
        // to hosts nothing here has checked.
        .redirect(redirect::Policy::none())
        .timeout(BUDGET)
        .build()
        .map_err(|e| FetchError::Unreachable {
            detail: e.to_string(),
        })?
        .get(url.clone())
        .send()
        .await
        .map_err(|e| FetchError::Unreachable {
            detail: e.to_string(),
        })
}

/// Read the body, stopping at the cap rather than after it.
async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
    // A fast refusal for an honest server, and never the actual guard: the header is written
    // by whoever wrote the body.
    if response
        .content_length()
        .is_some_and(|n| n > MAX_INPUT_BYTES as u64)
    {
        return Err(FetchError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| FetchError::Unreachable {
            detail: e.to_string(),
        })?
    {
        if body.len() + chunk.len() > MAX_INPUT_BYTES {
            return Err(FetchError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Whether this server will open a connection to `ip`.
///
/// **A deny-list of ranges, checked explicitly.** `std`'s `is_global` would be the obvious
/// call and is still unstable, so every range is spelled out here — which is worth doing
/// anyway for something whose failure mode is handing out cloud credentials. Each arm names
/// its RFC so the list can be checked against one.
fn allowed(ip: IpAddr, policy: Policy) -> bool {
    #[cfg(test)]
    if policy == Policy::Exempt(ip) {
        return true;
    }
    let _ = policy;
    match ip {
        IpAddr::V4(v4) => v4_allowed(v4),
        IpAddr::V6(v6) => v6_allowed(v6),
    }
}

fn v4_allowed(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0                       // 0.0.0.0/8 "this network", RFC 1122 — and NOT
                                   // `is_unspecified`, which is 0.0.0.0 and nothing else.
                                   // `0.0.0.1` reaches localhost on Linux.
        || ip.is_loopback()        // 127.0.0.0/8
        || ip.is_private()         // 10/8, 172.16/12, 192.168/16 — RFC 1918
        || ip.is_link_local()      // 169.254/16, RFC 3927 — the metadata endpoint lives here
        || ip.is_broadcast()
        || ip.is_documentation()   // 192.0.2/24, 198.51.100/24, 203.0.113/24 — RFC 5737
        || ip.is_multicast()       // 224/4
        || (a == 100 && (64..128).contains(&b))          // 100.64/10 CGNAT, RFC 6598
        || (a == 198 && (18..20).contains(&b))           // 198.18/15 benchmarking, RFC 2544
        || (a == 192 && b == 0 && c == 0)                // 192.0.0.0/**24**, IETF protocol
                                   // assignments. A /16 here would refuse 192.0.1.0 too,
                                   // which is ordinary public space — a deny-list that
                                   // over-reaches is a fetch that fails for no reason.
        || a >= 240) // 240/4 reserved, RFC 1112 — includes 255.255.255.255
}

fn v6_allowed(ip: Ipv6Addr) -> bool {
    // An IPv6 address that carries a v4 one is judged on the v4 address it carries.
    // `::ffff:169.254.169.254` reaches the metadata endpoint exactly as the v4 form does,
    // and reads as a perfectly ordinary IPv6 address to anything that does not look.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_allowed(v4);
    }
    let segments = ip.segments();
    // The NAT64 well-known prefix, RFC 6052: the last 32 bits are a v4 address, and on a
    // network with NAT64 they are reachable. Cheap to check, and the alternative is a
    // documented hole.
    if segments[0] == 0x0064 && segments[1] == 0xff9b {
        let [.., a, b, c, d] = ip.octets();
        return v4_allowed(Ipv4Addr::new(a, b, c, d));
    }
    // `::a.b.c.d`, deprecated by RFC 4291 and still parsed by things.
    if segments[..6].iter().all(|s| *s == 0) && !ip.is_loopback() && !ip.is_unspecified() {
        let [.., a, b, c, d] = ip.octets();
        return v4_allowed(Ipv4Addr::new(a, b, c, d));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()                         // ::1
        || ip.is_multicast()                        // ff00::/8
        || (segments[0] & 0xfe00) == 0xfc00         // fc00::/7 unique local, RFC 4193
        || (segments[0] & 0xffc0) == 0xfe80) // fe80::/10 link-local, RFC 4291
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(addr: &str) -> bool {
        allowed(addr.parse().expect("an address"), Policy::PublicOnly)
    }

    /// **The address this whole module exists for.**
    ///
    /// `169.254.169.254` is the cloud metadata endpoint. It answers to anything that asks
    /// from inside the instance, and this server is inside the instance — so a URL that
    /// resolves there is a URL that hands somebody the machine's credentials.
    #[test]
    fn the_metadata_endpoint_is_refused_in_every_spelling_of_it() {
        assert!(!public("169.254.169.254"), "the address itself");
        assert!(
            !public("::ffff:169.254.169.254"),
            "as an IPv4-mapped IPv6 address"
        );
        assert!(
            !public("::ffff:a9fe:a9fe"),
            "the same mapping, written in hex"
        );
        assert!(
            !public("64:ff9b::169.254.169.254"),
            "through the NAT64 well-known prefix"
        );
        assert!(
            !public("::169.254.169.254"),
            "as a deprecated IPv4-compatible address"
        );
    }

    /// Everything RFC 1918 and its neighbours. Table-driven so a range that stops being
    /// checked fails by name.
    #[test]
    fn private_and_reserved_ranges_are_refused() {
        for (addr, what) in [
            ("0.0.0.0", "unspecified"),
            (
                "0.1.2.3",
                "this network, RFC 1122 — reaches localhost on Linux",
            ),
            ("239.255.255.255", "the top of multicast"),
            ("127.0.0.1", "loopback"),
            ("127.255.255.254", "the far end of loopback"),
            ("10.0.0.1", "RFC 1918 10/8"),
            ("172.16.0.1", "RFC 1918 172.16/12"),
            ("172.31.255.254", "the far end of 172.16/12"),
            ("192.168.1.1", "RFC 1918 192.168/16"),
            ("169.254.0.1", "link-local"),
            ("100.64.0.1", "CGNAT, RFC 6598"),
            ("100.127.255.254", "the far end of CGNAT"),
            ("198.18.0.1", "benchmarking, RFC 2544"),
            ("192.0.0.1", "IETF protocol assignments"),
            ("192.0.2.1", "documentation, RFC 5737"),
            ("203.0.113.1", "documentation"),
            ("224.0.0.1", "multicast"),
            ("240.0.0.1", "reserved, RFC 1112"),
            ("255.255.255.255", "broadcast"),
            ("::", "IPv6 unspecified"),
            ("::1", "IPv6 loopback"),
            ("fc00::1", "IPv6 unique local"),
            ("fd12:3456::1", "IPv6 unique local, the fd half"),
            ("fe80::1", "IPv6 link-local"),
            ("ff02::1", "IPv6 multicast"),
            ("::ffff:127.0.0.1", "loopback, mapped"),
            ("::ffff:10.0.0.1", "RFC 1918, mapped"),
        ] {
            assert!(!public(addr), "{addr} ({what}) must be refused");
        }
    }

    /// And the other half, so the guard is not simply "no": ordinary public addresses pass.
    /// A deny-list that denied everything would satisfy every test above.
    #[test]
    fn ordinary_public_addresses_are_allowed() {
        for addr in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.255.255", // just below RFC 1918's 172.16/12
            "172.32.0.1",     // just above it
            "100.63.255.255", // just below CGNAT
            "100.128.0.1",    // just above it
            "198.17.255.255", // just below benchmarking
            "198.20.0.1",     // just above it
            "192.0.1.1",      // just outside 192.0.0.0/24 — a /16 check refuses this
            "2606:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(
                public(addr),
                "{addr} must be allowed — the boundaries are what a range check gets wrong, \
                 and a deny-list that over-reaches is a fetch that fails for no reason"
            );
        }
    }

    /// A scheme other than http or https is refused before a name is even looked up.
    #[tokio::test]
    async fn only_http_and_https_are_fetchable() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.invalid/image.png",
            "gopher://example.invalid/",
            "not a url at all",
        ] {
            assert!(
                matches!(
                    fetch_image(url).await,
                    Err(FetchError::NotFetchable | FetchError::NotPublic)
                ),
                "{url} must not be fetched"
            );
        }
    }

    /// A hostname that resolves to loopback is refused — which is the point. Checking the
    /// *name* would let this through; checking the resolved address does not.
    #[tokio::test]
    async fn a_public_name_that_resolves_into_private_space_is_refused() {
        // `localhost` is the case every reviewer thinks of, and the one a name check misses.
        assert!(matches!(
            fetch_image("http://localhost:9/image.png").await,
            Err(FetchError::NotPublic)
        ));
        assert!(matches!(
            fetch_image("http://127.0.0.1:9/image.png").await,
            Err(FetchError::NotPublic)
        ));
        assert!(matches!(
            fetch_image("http://[::1]:9/image.png").await,
            Err(FetchError::NotPublic)
        ));
    }
}

/// The HTTP half: redirects, caps and content types, against a server on loopback.
///
/// Loopback is the first thing this module refuses, so these run under `Policy::Exempt` —
/// which exempts **that one address** and nothing else. Every other refusal still applies,
/// which is what lets the redirect tests below mean something.
#[cfg(test)]
mod http_tests {
    use super::*;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use std::net::Ipv4Addr;

    /// Serve `app` on a loopback port the OS picks, and hand back its base URL.
    ///
    /// The task is detached and dies with the test; a `JoinHandle` nobody awaits is exactly
    /// what a fixture server wants.
    async fn serve(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("a port");
        let port = listener.local_addr().expect("its address").port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://127.0.0.1:{port}")
    }

    async fn fetch(url: &str) -> Result<Vec<u8>, FetchError> {
        tokio::time::timeout(
            BUDGET,
            follow(url, Policy::Exempt(Ipv4Addr::LOCALHOST.into())),
        )
        .await
        .expect("the test server answers well inside the budget")
    }

    /// A WebP header, so the bytes are the shape the caller will hand to `images::normalize`.
    const IMAGE: &[u8] = b"RIFF\x24\x00\x00\x00WEBPVP8 fixture bytes";

    #[tokio::test]
    async fn an_image_at_a_reachable_address_comes_back() {
        let base = serve(axum::Router::new().route(
            "/cliff.webp",
            get(|| async { ([(axum::http::header::CONTENT_TYPE, "image/webp")], IMAGE) }),
        ))
        .await;

        assert_eq!(
            fetch(&format!("{base}/cliff.webp")).await.expect("fetches"),
            IMAGE
        );
    }

    /// **The attack this module is written against.**
    ///
    /// Verified by mutation, 2026-09-07: make `one_hop` check the address only on the first
    /// hop — a single up-front check, which is what a client that follows redirects for you
    /// gives you — and this test and the one below both fail, each after burning the whole
    /// 10 s budget trying to reach an address nothing answers on. The check is load-bearing,
    /// not decorative.
    ///
    /// A perfectly ordinary first hop, and a `302` into the cloud metadata endpoint. A client
    /// that follows redirects itself checks the first URL and connects to the second, which
    /// is the whole bypass — so this must be refused, and refused as `NotPublic` rather than
    /// as a network error that happened to save us.
    #[tokio::test]
    async fn a_redirect_into_the_metadata_endpoint_is_refused() {
        let base = serve(axum::Router::new().route(
            "/innocent.png",
            get(|| async {
                axum::response::Redirect::temporary("http://169.254.169.254/latest/meta-data/")
                    .into_response()
            }),
        ))
        .await;

        assert!(
            matches!(
                fetch(&format!("{base}/innocent.png")).await,
                Err(FetchError::NotPublic)
            ),
            "the check has to run on the hop as well as on the first URL"
        );
    }

    /// The same, with a private range rather than the metadata endpoint — and through a
    /// *relative* `Location`, which is legal and which a naive join gets wrong.
    #[tokio::test]
    async fn a_redirect_into_private_space_is_refused_and_a_relative_one_is_resolved() {
        let base = serve(
            axum::Router::new()
                .route(
                    "/to-private",
                    get(|| async {
                        axum::response::Redirect::temporary("http://10.0.0.1/secret")
                            .into_response()
                    }),
                )
                .route(
                    "/relative",
                    get(|| async {
                        axum::response::Redirect::temporary("/cliff.webp").into_response()
                    }),
                )
                .route(
                    "/cliff.webp",
                    get(|| async { ([(axum::http::header::CONTENT_TYPE, "image/webp")], IMAGE) }),
                ),
        )
        .await;

        assert!(matches!(
            fetch(&format!("{base}/to-private")).await,
            Err(FetchError::NotPublic)
        ));
        // And a relative Location resolves against the current URL rather than being treated
        // as a URL in its own right, which would fail to parse.
        assert_eq!(
            fetch(&format!("{base}/relative")).await.expect("follows"),
            IMAGE
        );
    }

    /// Four hops is the limit, and a chain that never lands is stopped rather than followed.
    #[tokio::test]
    async fn a_redirect_loop_stops_at_the_limit() {
        let base = serve(axum::Router::new().route(
            "/loop",
            get(|| async { axum::response::Redirect::temporary("/loop").into_response() }),
        ))
        .await;

        assert!(matches!(
            fetch(&format!("{base}/loop")).await,
            Err(FetchError::TooManyRedirects)
        ));
    }

    /// **The cap is enforced while reading, not after.**
    ///
    /// This server lies in both directions: it declares no length and then sends more than
    /// the limit. A cap that trusted `Content-Length`, or that applied after the body
    /// arrived, has already spent what it was meant to save.
    #[tokio::test]
    async fn a_body_larger_than_the_cap_is_stopped_while_it_arrives() {
        let base = serve(axum::Router::new().route(
            "/enormous.webp",
            get(|| async {
                // A megabyte at a time, past the limit — and streamed, so the response
                // carries no `Content-Length` for the fast path to catch it by.
                let body = axum::body::Body::from_stream(tokio_stream::iter(
                    (0..(MAX_INPUT_BYTES / (1024 * 1024)) + 2)
                        .map(|_| Ok::<_, std::io::Error>(vec![0u8; 1024 * 1024])),
                ));
                ([(axum::http::header::CONTENT_TYPE, "image/webp")], body)
            }),
        ))
        .await;

        assert!(matches!(
            fetch(&format!("{base}/enormous.webp")).await,
            Err(FetchError::TooLarge)
        ));
    }

    /// The common mistake — pasting the page rather than the image — gets a sentence about
    /// that rather than a decode failure three layers down.
    #[tokio::test]
    async fn a_page_rather_than_an_image_says_so() {
        let base = serve(axum::Router::new().route(
            "/product",
            get(|| async { ([(axum::http::header::CONTENT_TYPE, "text/html")], "<html>") }),
        ))
        .await;

        let err = fetch(&format!("{base}/product"))
            .await
            .expect_err("refused");
        assert!(matches!(err, FetchError::NotAnImage { .. }));
        assert!(err.to_string().contains("text/html"), "{err}");
    }

    #[tokio::test]
    async fn a_status_that_is_not_success_is_reported_with_its_status() {
        let base = serve(
            axum::Router::new().route("/gone", get(|| async { axum::http::StatusCode::NOT_FOUND })),
        )
        .await;

        assert!(matches!(
            fetch(&format!("{base}/gone")).await,
            Err(FetchError::NotOk { status: 404 })
        ));
    }
}
