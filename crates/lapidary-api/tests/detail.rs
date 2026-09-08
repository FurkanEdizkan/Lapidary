//! `GET /api/parts/{id}` — the page a card links to.
//!
//! Driven through this crate's router against a live, migrated Postgres, and asserted
//! against rows written the way ingest writes them. What this route promises is that
//! every figure it shows carries the provenance the database recorded for it — so the
//! interesting cases here are the ones where a figure is absent, or where the two figures
//! on one part disagree about where they came from.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{LibraryId, PartId, RevisionId};
use lapidary_db::{IngestRequest, PgIngest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // This route reads rows and never a blob: the thumbnail travels inline out of the
        // row the query already read, and nothing else here touches the store.
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
    }
}

async fn get(pool: sqlx::PgPool, part: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(
            Request::builder()
                .uri(format!("/api/parts/{part}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body is JSON")
    };
    (status, json)
}

/// One part with a full set of mesh measurements, written the way ingest writes them.
/// `volume_source` and `surface_area_source` are set independently so a test can make them
/// disagree, which is the Phase 2 shape this route exists to be correct about.
async fn seed(
    pool: &sqlx::PgPool,
    watertight: bool,
    volume: Option<f64>,
    volume_source: Option<&str>,
) -> PartId {
    let part = PartId::new();
    sqlx::query(
        "INSERT INTO part (id, library_id, name, source_path, created_at, updated_at) \
         VALUES ($1, $2, 'LP-1042-03', 'brackets/steel/LP-1042-03.stl', now(), now())",
    )
    .bind(part.as_uuid())
    .bind(
        SEEDED_LIBRARY
            .parse::<LibraryId>()
            .expect("seeded library id parses")
            .as_uuid(),
    )
    .execute(pool)
    .await
    .expect("part inserts");

    let revision = RevisionId::new();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin, triangle_count, is_watertight, \
                               bbox_x, bbox_y, bbox_z, volume, volume_source, \
                               surface_area, surface_area_source, created_at) \
         VALUES ($1, $2, '1', 'ingest', 48112, $3, 61.0, 42.0, 18.5, $4, $5, 9804.25, \
                 'tessellated', now())",
    )
    .bind(revision.as_uuid())
    .bind(part.as_uuid())
    .bind(watertight)
    .bind(volume)
    .bind(volume_source)
    .execute(pool)
    .await
    .expect("revision inserts");
    part
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_page_shows_every_figure_with_the_provenance_it_was_recorded_with(
    pool: sqlx::PgPool,
) {
    let part = seed(&pool, true, Some(21478.5), Some("tessellated")).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["name"], "LP-1042-03");
    // The identity slice 6a moved from the name to the path, which is the reason two
    // parts called `bracket` are two parts. A page that omitted it could not tell them
    // apart either.
    assert_eq!(json["sourcePath"], "brackets/steel/LP-1042-03.stl");
    assert_eq!(json["revLabel"], "1");
    assert_eq!(json["triangleCount"], 48112);
    assert_eq!(json["isWatertight"], true);

    // Value and flag in one object, not a number beside a page-level boolean. This is the
    // shape `CLAUDE.md`'s labelling rule needs in order to survive a part whose figures
    // disagree.
    assert_eq!(json["volumeMm3"]["value"], 21478.5);
    assert_eq!(
        json["volumeMm3"]["approximate"], true,
        "a tessellated figure must be labelled: {}",
        json["volumeMm3"]
    );
    assert_eq!(json["surfaceAreaMm2"]["value"], 9804.25);
    assert_eq!(
        json["bboxMm"]["value"],
        serde_json::json!([61.0, 42.0, 18.5])
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_analytic_figure_is_not_labelled_approximate(pool: sqlx::PgPool) {
    // The Phase 2 shape, arriving early: one revision whose volume was read from a B-rep
    // entity and whose surface area was derived from the mesh. A page-level boolean is
    // wrong about one of them whichever way it is set, and this is the assertion that
    // fails if these two figures ever start sharing a flag.
    let part = seed(&pool, true, Some(21478.5), Some("analytic")).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["volumeMm3"]["approximate"], false);
    assert_eq!(
        json["surfaceAreaMm2"]["approximate"], true,
        "the other figure on the same part is still tessellated"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_open_mesh_reports_no_volume_rather_than_a_meaningless_one(pool: sqlx::PgPool) {
    // "Measurement must not lie" includes declining to measure. Signed-volume integration
    // over a non-watertight mesh returns a plausible number that means nothing, so ingest
    // writes NULL and this page must show nothing rather than zero.
    let part = seed(&pool, false, None, None).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["isWatertight"], false);
    assert!(
        json["volumeMm3"].is_null(),
        "an open mesh has no volume to report, got: {}",
        json["volumeMm3"]
    );
    // And the figure that *is* meaningful on an open mesh is still there — refusing the
    // volume must not take the rest of the page down with it.
    assert_eq!(json["surfaceAreaMm2"]["value"], 9804.25);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_figure_with_no_recorded_provenance_is_dropped_rather_than_guessed(pool: sqlx::PgPool) {
    // A value whose `*_source` column is NULL. Neither default is safe — one hedges by
    // labelling an exact figure approximate, the other presents a mesh figure as exact —
    // so the pair is dropped and the page shows the figure it can vouch for.
    let part = seed(&pool, true, Some(21478.5), None).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        json["volumeMm3"].is_null(),
        "a number nobody can vouch for is not shown, got: {}",
        json["volumeMm3"]
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_provenance_this_build_does_not_know_is_refused_rather_than_defaulted(
    pool: sqlx::PgPool,
) {
    // Written by something other than Lapidary. Answering 500 is the point: the two
    // available defaults are "needlessly hedge" and "lie", and a measurement that lies is
    // the one failure CLAUDE.md rules out outright.
    let part = seed(&pool, true, Some(21478.5), Some("guessed")).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let message = json["message"].as_str().expect("a message");
    assert!(
        message.contains("analytic") && message.contains("tessellated"),
        "must say what a provenance may be, got: {message}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_part_and_a_part_that_never_existed_answer_alike(pool: sqlx::PgPool) {
    // Undistinguished on purpose: telling them apart confirms a part exists to someone
    // who cannot see it.
    let part = seed(&pool, true, Some(21478.5), Some("tessellated")).await;
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(part.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");

    let (deleted, _) = get(pool.clone(), &part.to_string()).await;
    let (absent, _) = get(pool, "01931b6e-0000-7000-8000-00000000dead").await;
    assert_eq!(deleted, StatusCode::NOT_FOUND);
    assert_eq!(absent, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_serves_no_detail_route(pool: sqlx::PgPool) {
    // One binary builds both routers, so a route mounted unconditionally is served by the
    // worker image too.
    let response = router(state(pool), Role::Worker)
        .oneshot(
            Request::builder()
                .uri("/api/parts/01931b6e-0000-7000-8000-000000000001")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The two fields the "show in folder" and "move to…" controls are built on.
///
/// Written after they shipped without a check: `s.storage_path` was selected from a
/// LATERAL that never produced the column, and the six tests that caught it are about
/// provenance and deletion — every one of them failed with a 500 that named nothing.
/// A page's own claim about where its bytes sit needs an assertion that says so.
///
/// Ingested rather than hand-inserted, because `directory` is derived from
/// `file.storage_path` and only the ingest path writes that column the way the query
/// reads it.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_page_names_the_file_on_disk_and_the_directory_holding_it(pool: sqlx::PgPool) {
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: Some("libraries/default/vee-block-lp-3072-02/vee-block-lp-3072-02.stl"),
            library: LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid")),
            name: "Vee block, LP-3072-02",
            source_path: "vee-block-lp-3072-02.stl",
            blob: &StoredBlobRow {
                hash: lapidary_core::BlobHash::from_bytes([0x5a; 32]),
                size_bytes: 82_144,
                stored_bytes: 82_144,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [60.0, 60.0, 40.0],
                triangle_count: 1_648,
                surface_area_mm2: 18_400.0,
                volume_mm3: Some(64_800.0),
                is_watertight: true,
            },
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("a part with a source file on disk");

    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["storagePath"],
        "libraries/default/vee-block-lp-3072-02/vee-block-lp-3072-02.stl"
    );
    // The parent, and not a path assembled from slugs on the client: a store that has not
    // been migrated to this layout yet has no directory to name, and only the server knows.
    assert_eq!(json["directory"], "libraries/default/vee-block-lp-3072-02");
}

/// A part ingested before the store had a layout still has a page. Both fields are absent
/// rather than guessed — the same rule the measurement labelling follows.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_whose_bytes_are_content_addressed_names_no_directory(pool: sqlx::PgPool) {
    let part = seed(&pool, true, Some(21478.5), Some("tessellated")).await;
    let (status, json) = get(pool, &part.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["storagePath"], serde_json::Value::Null);
    assert_eq!(json["directory"], serde_json::Value::Null);
}
