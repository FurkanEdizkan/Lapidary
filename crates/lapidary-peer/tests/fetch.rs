//! A pull's fetch (sharing S3): resumed from what is staged, and checked when whole. Against a plain loopback server
//! that answers Range as the blob route does, so the test sees exactly what the puller asked for.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::BlobHash;
use lapidary_peer::pull::{FetchError, fetch};
use std::sync::{Arc, Mutex};

/// An STL of a cliff face, long enough to stop part way through.
fn cliff_face() -> Vec<u8> {
    let mut stl = b"solid cliff-face-LP-TR-0112\n".to_vec();
    for facet in 0..900 {
        let z = f64::from(facet) * 0.1;
        stl.extend_from_slice(
            format!("facet normal 0 0 1\n outer loop\n  vertex 0 0 {z:.2}\n  vertex 40 0 {z:.2}\n  vertex 0 25 {z:.2}\n endloop\nendfacet\n").as_bytes(),
        );
    }
    stl.extend_from_slice(b"endsolid cliff-face-LP-TR-0112\n");
    stl
}

#[derive(Clone)]
struct Sharer {
    bytes: Arc<Vec<u8>>,
    asked: Arc<Mutex<Vec<Option<String>>>>,
}

async fn blob(State(sharer): State<Sharer>, headers: HeaderMap) -> Response {
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    sharer
        .asked
        .lock()
        .expect("not poisoned")
        .push(range.clone());
    let size = sharer.bytes.len();
    match range.and_then(|range| {
        range
            .strip_prefix("bytes=")?
            .strip_suffix('-')?
            .parse::<usize>()
            .ok()
    }) {
        Some(start) => (
            StatusCode::PARTIAL_CONTENT,
            [(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{size}", size - 1),
            )],
            sharer.bytes[start..].to_vec(),
        )
            .into_response(),
        None => (StatusCode::OK, sharer.bytes.to_vec()).into_response(),
    }
}

async fn serve(bytes: Vec<u8>) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let sharer = Sharer {
        bytes: Arc::new(bytes),
        asked: Arc::default(),
    };
    let asked = sharer.asked.clone();
    let app = axum::Router::new()
        .route("/blob", axum::routing::get(blob))
        .with_state(sharer);
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port on the loopback");
    let url = format!("http://{}/blob", tcp.local_addr().expect("its port"));
    tokio::spawn(async move { axum::serve(tcp, app).await });
    (url, asked)
}

fn hash(bytes: &[u8]) -> BlobHash {
    BlobHash::from_bytes(*blake3::hash(bytes).as_bytes())
}

#[tokio::test]
async fn a_staged_start_is_resumed_from_its_length() {
    let bytes = cliff_face();
    let (url, asked) = serve(bytes.clone()).await;
    let staging = tempfile::tempdir().expect("a staging directory");
    let expect = hash(&bytes);
    std::fs::write(
        staging.path().join(format!("{}.part", expect.to_hex())),
        &bytes[..20_000],
    )
    .expect("stages the start");

    let fetched = fetch(
        &reqwest::Client::new(),
        &url,
        staging.path(),
        &expect,
        bytes.len() as u64,
    )
    .await
    .expect("fetches the rest");

    assert_eq!(
        asked.lock().expect("not poisoned").as_slice(),
        [Some("bytes=20000-".to_owned())]
    );
    assert_eq!(fetched.sent, bytes.len() as u64 - 20_000);
    assert_eq!(std::fs::read(&fetched.path).expect("staged whole"), bytes);
    assert!(
        !staging
            .path()
            .join(format!("{}.part", expect.to_hex()))
            .exists()
    );

    // Whole already: nothing is asked for again.
    let again = fetch(
        &reqwest::Client::new(),
        &url,
        staging.path(),
        &expect,
        bytes.len() as u64,
    )
    .await
    .expect("already whole");
    assert_eq!(again.sent, 0);
    assert_eq!(asked.lock().expect("not poisoned").len(), 1);
}

#[tokio::test]
async fn bytes_that_are_not_the_catalogues_file_are_refused_and_dropped() {
    let bytes = cliff_face();
    let mut tampered = bytes.clone();
    tampered[4_096] ^= 0x20;
    let (url, _) = serve(tampered).await;
    let staging = tempfile::tempdir().expect("a staging directory");
    let expect = hash(&bytes);

    let refused = fetch(
        &reqwest::Client::new(),
        &url,
        staging.path(),
        &expect,
        bytes.len() as u64,
    )
    .await
    .expect_err("refuses the wrong bytes");

    assert!(
        matches!(refused, FetchError::Refused(ref why) if why.contains("BLAKE3")),
        "{refused:?}"
    );
    assert_eq!(
        std::fs::read_dir(staging.path())
            .expect("lists staging")
            .count(),
        0,
        "nothing staged is kept"
    );
}
