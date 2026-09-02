//! Task 10: the grid endpoint. Exercises `GET /api/libraries/{id}/parts?after=&limit=`
//! end to end — seeding rows through `PgIngest` (the same path `lapidary-ingest`'s scan
//! handler uses) against a live, migrated Postgres (via `sqlx::test`), then reading them
//! back through this crate's router.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql` — nothing in slice 1
/// creates a library through the API, so every test either uses this one or inserts a
/// second directly, same as `crates/lapidary-db/tests/repo.rs`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

/// Records one part through `PgIngest`, the same repository the scan handler uses —
/// this endpoint only reads, so there is no API of its own to create fixture data with.
/// `seed` distinguishes the blob hash between calls; a real hash would do just as well,
/// but nothing here inspects the bytes' content, only their round trip through the
/// thumbnail column.
async fn seed_part(
    pool: &sqlx::PgPool,
    library: LibraryId,
    seed: u8,
    name: &str,
    thumbnail_webp: &[u8],
) {
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 2_048,
        stored_bytes: 1_024,
        zstd_level: 3,
    };
    PgIngest(pool.clone())
        .record(IngestRequest {
            library,
            name,
            blob: &blob,
            measurements: &measurements(),
            thumbnail_webp,
            kernel_version: "mesh stl-1+cpu-1",
        })
        .await
        .expect("seed part");
}

/// GETs `/api/libraries/{library}/parts`, appending `query` (already
/// `key=value&key=value` shaped) when non-empty, and returns the decoded JSON body
/// alongside the status.
async fn get_page(
    pool: sqlx::PgPool,
    library: &str,
    query: &str,
) -> (StatusCode, serde_json::Value) {
    let app = router(AppState { db: pool }, Role::Api);
    let uri = if query.is_empty() {
        format!("/api/libraries/{library}/parts")
    } else {
        format!("/api/libraries/{library}/parts?{query}")
    };
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    (status, json)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_empty_library_returns_no_parts_and_no_next(pool: sqlx::PgPool) {
    let (status, json) = get_page(pool, SEEDED_LIBRARY, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["parts"], serde_json::json!([]));
    assert!(json["next"].is_null());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn three_parts_come_back_newest_first(pool: sqlx::PgPool) {
    seed_part(&pool, library(), 0x01, "Bracket, LP-1042-03", b"webp-1").await;
    seed_part(&pool, library(), 0x02, "Spacer, LP-2001-00", b"webp-2").await;
    seed_part(&pool, library(), 0x03, "Cable clip, LP-3300-01", b"webp-3").await;

    let (status, json) = get_page(pool, SEEDED_LIBRARY, "").await;
    assert_eq!(status, StatusCode::OK);
    let parts = json["parts"].as_array().expect("parts is an array");
    assert_eq!(parts.len(), 3);
    assert_eq!(
        parts[0]["name"], "Cable clip, LP-3300-01",
        "ingested last, listed first"
    );
    assert_eq!(parts[1]["name"], "Spacer, LP-2001-00");
    assert_eq!(
        parts[2]["name"], "Bracket, LP-1042-03",
        "ingested first, listed last"
    );
    assert!(
        json["next"].is_null(),
        "the whole library fit in one page, so there is nothing to page to"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn limit_is_honoured_and_next_is_the_last_id_only_while_a_further_page_exists(
    pool: sqlx::PgPool,
) {
    seed_part(&pool, library(), 0x11, "Bracket, LP-1042-03", b"webp-1").await;
    seed_part(&pool, library(), 0x12, "Spacer, LP-2001-00", b"webp-2").await;
    seed_part(&pool, library(), 0x13, "Cable clip, LP-3300-01", b"webp-3").await;

    let (status, first) = get_page(pool.clone(), SEEDED_LIBRARY, "limit=2").await;
    assert_eq!(status, StatusCode::OK);
    let first_parts = first["parts"].as_array().expect("parts is an array");
    assert_eq!(first_parts.len(), 2, "limit caps the page at 2");
    // A mutant that hardcodes `next: null` would fail here: a further page genuinely
    // exists (one row remains), so `next` must be set, not absent.
    let next = first["next"]
        .as_str()
        .expect("a further page exists, so next must be set")
        .to_owned();
    assert_eq!(
        next, first_parts[1]["id"],
        "next is the last id ON this page, not the next one to come"
    );

    let (status2, second) = get_page(pool, SEEDED_LIBRARY, &format!("limit=2&after={next}")).await;
    assert_eq!(status2, StatusCode::OK);
    let second_parts = second["parts"].as_array().expect("parts is an array");
    assert_eq!(second_parts.len(), 1, "the one remaining part");
    assert_eq!(second_parts[0]["name"], "Bracket, LP-1042-03");
    // A mutant that always echoes back the last row's id as `next` (dropping the
    // "was this page full" check) would fail here: this page was short, so there is no
    // further page and `next` must be null.
    assert!(
        second["next"].is_null(),
        "a short page proves there is no further page"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_thumbnail_is_a_data_url_that_decodes_to_the_ingested_bytes(pool: sqlx::PgPool) {
    let original = b"a small deterministic stand-in for a real WebP payload";
    seed_part(&pool, library(), 0x21, "Bracket, LP-1042-03", original).await;

    let (status, json) = get_page(pool, SEEDED_LIBRARY, "").await;
    assert_eq!(status, StatusCode::OK);
    let thumbnail = json["parts"][0]["thumbnail"]
        .as_str()
        .expect("thumbnail is a string");

    let prefix = "data:image/webp;base64,";
    assert!(thumbnail.starts_with(prefix), "got {thumbnail}");
    // Decode and compare the exact bytes, not just the prefix — a truncated or
    // wrongly-encoded payload would still pass a prefix-only check.
    let decoded = BASE64
        .decode(&thumbnail[prefix.len()..])
        .expect("the payload after the prefix is valid base64");
    assert_eq!(
        decoded, original,
        "the decoded bytes must be exactly what was ingested"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_in_another_library_never_appears(pool: sqlx::PgPool) {
    // Nothing in slice 1 creates a library through the API (same note as
    // crates/lapidary-db/tests/repo.rs), so a second library is inserted directly.
    let other_library: LibraryId = "01931b6e-0000-7000-8000-000000000002"
        .parse()
        .expect("valid uuid");
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, 'Fixture library, other tenant')")
        .bind(other_library.as_uuid())
        .execute(&pool)
        .await
        .expect("insert second library");

    seed_part(&pool, library(), 0x31, "Bracket, LP-1042-03", b"webp-a").await;
    seed_part(&pool, other_library, 0x32, "Widget, LP-9999-00", b"webp-b").await;

    let (status, json) = get_page(pool, SEEDED_LIBRARY, "").await;
    assert_eq!(status, StatusCode::OK);
    let parts = json["parts"].as_array().expect("parts is an array");
    // If the repository's `WHERE p.library_id = $1` clause were dropped, this would
    // read 2, not 1 — both parts exist, in two different libraries.
    assert_eq!(
        parts.len(),
        1,
        "only the requested library's part comes back"
    );
    assert_eq!(parts[0]["name"], "Bracket, LP-1042-03");
}
