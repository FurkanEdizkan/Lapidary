//! The routes that make a second library possible.
//!
//! `0002_parts.sql` seeded one and said so: *"Whichever slice adds a second library replaces
//! this seed rather than building beside it."* The seed stays — it is the library an
//! existing deployment has been using — but it is no longer the only one there can be.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use tower::ServiceExt;

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    }
}

async fn send(pool: sqlx::PgPool, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn create(name: &str, mode: Option<&str>) -> Request<Body> {
    let body = match mode {
        Some(mode) => serde_json::json!({ "name": name, "mode": mode }),
        None => serde_json::json!({ "name": name }),
    };
    Request::builder()
        .method("POST")
        .uri("/api/libraries")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

fn list() -> Request<Body> {
    Request::builder()
        .uri("/api/libraries")
        .body(Body::empty())
        .expect("request builds")
}

/// The seeded library is there, and a new one joins it — oldest first, so the library an
/// existing deployment has been using stays where it was.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_created_library_joins_the_seeded_one_rather_than_replacing_it(pool: sqlx::PgPool) {
    let (status, before) = send(pool.clone(), list()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before.as_array().map(Vec::len), Some(1));
    assert_eq!(before[0]["name"], "Default");

    let (status, made) = send(pool.clone(), create("Tabletop terrain", None)).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(made["partCount"], 0, "brand new, so empty");
    // `hobby` unless somebody says otherwise — `CLAUDE.md`: governance is opt-in.
    assert_eq!(made["mode"], "hobby");

    let (_, after) = send(pool, list()).await;
    assert_eq!(after.as_array().map(Vec::len), Some(2));
    assert_eq!(after[0]["name"], "Default", "the seed is still first");
    assert_eq!(after[1]["name"], "Tabletop terrain");
}

/// `controlled` is a choice made at creation, because later means asking about a library
/// somebody has already filled.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_can_be_created_controlled(pool: sqlx::PgPool) {
    let (status, made) = send(pool, create("Production parts", Some("controlled"))).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(made["mode"], "controlled");
}

/// **Two libraries with one name is a switcher nobody can use** — no way to tell which is
/// which, and no way to find out but opening both. Enforced by a unique index rather than a
/// prior `SELECT`, which a concurrent insert could invalidate between statements.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_name_another_library_already_has_is_refused(pool: sqlx::PgPool) {
    let (status, _) = send(pool.clone(), create("Tabletop terrain", None)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, refusal) = send(pool.clone(), create("Tabletop terrain", None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["reason"], "nameTaken");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|m| m.contains("Tabletop terrain")),
        "the refusal names the library in the way: {refusal}"
    );

    // **And the pair that looks different on screen and is not on disk.** This assertion
    // used to say `CREATED`, which is what `0017`'s name index allowed and what running the
    // stack then showed to be wrong: `library_slug` is `slugify(name).to_lowercase()`, so
    // both of these want `libraries/tabletop terrain/`, and two libraries would have been
    // interleaved in one folder the owner is invited to open and read.
    //
    // `0018` gives a library its own `slug` column with its own unique index, which is
    // `folder.slug`'s design one level up and for the same reason.
    let (status, refusal) = send(pool, create("tabletop terrain", None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refusal["reason"], "slugTaken",
        "and not `nameTaken` — the name is genuinely free, the folder is not: {refusal}"
    );
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|m| m.contains("tabletop terrain")),
        "the refusal names the folder both wanted: {refusal}"
    );
}

/// Names that differ only in characters a filesystem cannot store are the other half of the
/// same pair — the case `folder_slug_unique_per_parent` has always caught for categories.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_library_names_that_slug_alike_cannot_both_exist(pool: sqlx::PgPool) {
    let (status, _) = send(pool.clone(), create("Rocks?", None)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, refusal) = send(pool, create("Rocks*", None)).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "distinct names, one folder: {refusal}"
    );
    assert_eq!(refusal["reason"], "slugTaken");
}

/// The seeded library keeps the folder its models are already in.
///
/// `0018` backfills `slug` from the name, and `Default` has been `libraries/default/` since
/// the folder tree. A backfill that produced anything else would point an existing
/// deployment at an empty directory while its bytes sat in the old one — the grid would look
/// healthy and every download would 500.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_backfilled_slug_matches_the_directory_the_seeded_library_already_uses(
    pool: sqlx::PgPool,
) {
    let slug: String = sqlx::query_scalar("SELECT slug FROM library WHERE name = 'Default'")
        .fetch_one(&pool)
        .await
        .expect("the seeded library has a slug");
    assert_eq!(
        slug, "default",
        "the directory an existing store's models are already in"
    );
}

/// A name of nothing but spaces is not a name.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_needs_a_name(pool: sqlx::PgPool) {
    let (status, refusal) = send(pool, create("   ", None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refusal["reason"], "emptyName");
}

/// Parts belong to the library they were ingested into, and the count says so — which is
/// what makes a switcher able to show where anything is.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_count_belongs_to_the_library_that_holds_the_parts(pool: sqlx::PgPool) {
    let (_, made) = send(pool.clone(), create("Tabletop terrain", None)).await;
    let new_id = made["id"].as_str().expect("an id").to_owned();

    lapidary_db::PgIngest(pool.clone())
        .record(lapidary_db::IngestRequest {
            folder: None,
            storage_path: Some("libraries/default/vee-block/vee-block.stl"),
            library: lapidary_core::LibraryId::from_uuid(
                "01931b6e-0000-7000-8000-000000000001"
                    .parse()
                    .expect("seeded id"),
            ),
            name: "vee-block-lp-3072-02",
            source_path: "vee-block-lp-3072-02.stl",
            blob: &lapidary_db::StoredBlobRow {
                hash: lapidary_core::BlobHash::from_bytes([0x5a; 32]),
                size_bytes: 9_684,
                stored_bytes: 9_684,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [60.0, 50.0, 40.0],
                triangle_count: 192,
                surface_area_mm2: 16_918.0,
                volume_mm3: Some(106_830.0),
                is_watertight: true,
            },
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("a part in the seeded library");

    let (_, after) = send(pool, list()).await;
    let counts: Vec<(String, i64)> = after
        .as_array()
        .expect("array")
        .iter()
        .map(|row| {
            (
                row["id"].as_str().expect("id").to_owned(),
                row["partCount"].as_i64().expect("count"),
            )
        })
        .collect();
    assert!(
        counts.contains(&("01931b6e-0000-7000-8000-000000000001".to_owned(), 1)),
        "the seeded library holds the part: {counts:?}"
    );
    assert!(
        counts.contains(&(new_id.clone(), 0)),
        "and the new one holds nothing: {counts:?}"
    );
}
