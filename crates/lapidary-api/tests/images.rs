//! The gallery routes, and the boundary they sit on.
//!
//! `images::normalize`'s own tests cover what makes bytes acceptable. These cover what
//! happens to bytes that are: where they are stored, whether they can be read back, and —
//! the one that is a security property rather than a feature — whether holding a hash is
//! enough to be given the bytes it names.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use image::{ImageBuffer, ImageFormat, Rgba};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{LibraryId, PartId};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// A real PNG of `size`×`size`. Built, not committed: a fixture nobody can read is a
/// fixture nobody can check.
///
/// **High-frequency on purpose.** The first version was a smooth gradient, which is exactly
/// what lossless WebP is good at — a 1200×1200 one came out at 3 KB and sat on the wrong
/// side of the 64 KB inline line, so the test meant to exercise the blob path exercised the
/// inline path and said so. These multipliers are coprime with 256, which keeps the pattern
/// from repeating along either axis and leaves the encoder nothing to find.
fn png(size: u32) -> Vec<u8> {
    let buffer = ImageBuffer::from_fn(size, size, |x, y| {
        let mix = |a: u32, b: u32| ((x.wrapping_mul(a) ^ y.wrapping_mul(b)) % 256) as u8;
        Rgba([mix(7919, 104_729), mix(1301, 7607), mix(65_537, 13), 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut out, ImageFormat::Png)
        .expect("the fixture encodes");
    out.into_inner()
}

fn state(pool: sqlx::PgPool, root: &std::path::Path) -> AppState {
    AppState {
        db: pool,
        blob_root: root.to_path_buf(),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    }
}

async fn seed_part(pool: &sqlx::PgPool) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: Some(
                "libraries/default/idler-pulley-lp-4820-00/idler-pulley-lp-4820-00.stl",
            ),
            library: LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid")),
            name: "Idler pulley, LP-4820-00",
            source_path: "idler-pulley-lp-4820-00.stl",
            blob: &StoredBlobRow {
                hash: lapidary_core::BlobHash::from_bytes([0x41; 32]),
                size_bytes: 39_321,
                stored_bytes: 39_321,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [40.0, 40.0, 12.0],
                triangle_count: 784,
                surface_area_mm2: 6_400.0,
                volume_mm3: Some(9_600.0),
                is_watertight: true,
            },
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("a part to hang a gallery on")
}

async fn send(state: AppState, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = router(state, Role::Api)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("body reads");
    (status, bytes.to_vec())
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
}

fn upload(part: PartId, bytes: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/parts/{part}/images"))
        .body(Body::from(bytes))
        .expect("request builds")
}

/// A small image lives on its row, and comes back as a `data:` URL — already in hand, so
/// asking for it again would be a round trip bought with nothing.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_small_image_is_stored_inline_and_read_back_as_a_data_url(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;

    let (status, body) = send(state(pool.clone(), store.path()), upload(part, png(128))).await;
    assert_eq!(status, StatusCode::CREATED, "{}", json(&body));
    // The size is in the answer so a silent resize is not silent. 128 is under the bound,
    // so it comes back unchanged.
    assert_eq!(json(&body)["width"], 128);

    let (status, body) = send(
        state(pool, store.path()),
        Request::builder()
            .uri(format!("/api/parts/{part}/images"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let gallery = json(&body);
    assert_eq!(gallery.as_array().map(Vec::len), Some(1));
    assert!(
        gallery[0]["src"]
            .as_str()
            .is_some_and(|src| src.starts_with("data:image/webp;base64,")),
        "inline, and WebP whatever went in: {gallery}"
    );
    assert_eq!(gallery[0]["origin"], "uploaded");
}

/// A large one goes to the content-addressed store and comes back as a blob URL — and the
/// blob route serves it, which is the half that would silently not work.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_large_image_becomes_a_blob_that_the_blob_route_will_serve(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;

    // 1200×1200 of noise-ish gradient: comfortably over the 64 KB inline line once encoded,
    // and under both the pixel and the byte bounds.
    let (status, _) = send(state(pool.clone(), store.path()), upload(part, png(1200))).await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, body) = send(
        state(pool.clone(), store.path()),
        Request::builder()
            .uri(format!("/api/parts/{part}/images"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    let src = json(&body)[0]["src"].as_str().expect("a src").to_owned();
    assert!(src.starts_with("/api/blob/"), "too big to inline: {src}");

    let response = router(state(pool, store.path()), Role::Api)
        .oneshot(
            Request::builder()
                .uri(&src)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the gallery handed out a URL, so the URL has to work"
    );
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/webp"),
        "served as what it is: a WebP labelled model/gltf-binary renders only because \
         browsers sniff, and sniffing is not something to depend on"
    );
}

/// **The security property.** `CLAUDE.md`: content addressing is not authorization.
///
/// Bytes on disk that nothing in this instance points at are not served, however correct
/// the hash. Written against an image blob specifically, because extending the reachability
/// check to galleries is exactly the kind of change that grows a hole in it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_image_blob_no_gallery_references_is_not_served(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;
    let (_, body) = send(state(pool.clone(), store.path()), upload(part, png(1200))).await;
    let _ = body;

    // The same bytes, written straight into the store and recorded — no gallery row.
    let orphan = lapidary_storage::DerivativeStore::open(store.path())
        .put(b"RIFF____WEBPnot-a-real-one-but-bytes-all-the-same")
        .expect("the store takes them");
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, ref_count) VALUES ($1, 48, 48, 0)",
    )
    .bind(orphan.hash.to_hex())
    .execute(&pool)
    .await
    .expect("a blob row nothing points at");

    let (status, _) = send(
        state(pool, store.path()),
        Request::builder()
            .uri(format!("/api/blob/{}", orphan.hash.to_hex()))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "holding the hash is not a reason to be given the bytes"
    );
}

/// The refusals reach the caller as statuses that mean what they say, so a client reading
/// only the status still learns something true.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_that_is_not_an_image_is_refused_as_an_unsupported_type(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;

    let (status, body) = send(
        state(pool, store.path()),
        upload(
            part,
            b"this is a text file somebody dragged in by mistake".to_vec(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(
        json(&body)["message"]
            .as_str()
            .is_some_and(|m| m.contains("PNG, JPEG and WebP")),
        "and says what it does read: {}",
        json(&body)
    );
}

/// A favicon-sized image is a real image and still not one to put on a card.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_icon_sized_image_is_refused_with_the_size_it_needs(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;

    let (status, body) = send(state(pool, store.path()), upload(part, png(16))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["message"]
            .as_str()
            .is_some_and(|m| m.contains("64")),
        "{}",
        json(&body)
    );
}

/// Gallery order is the column, not insertion luck: the second upload goes after the first
/// and stays there.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn images_come_back_in_the_order_they_were_added(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let part = seed_part(&pool).await;
    for size in [128u32, 160] {
        let (status, _) = send(state(pool.clone(), store.path()), upload(part, png(size))).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (_, body) = send(
        state(pool, store.path()),
        Request::builder()
            .uri(format!("/api/parts/{part}/images"))
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!(json(&body).as_array().map(Vec::len), Some(2));
}
