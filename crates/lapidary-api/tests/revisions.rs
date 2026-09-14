//! A part's history, and the switch that turns a library's history on.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{Approximate, BlobHash, LibraryId, MeshMeasurements, PartId, RevisionOrigin};
use lapidary_db::{IngestRequest, PgIngest, PgParts, PgRevisions, RevisionRequest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const PATH: &str = "flange-dn40-lp-3310-02.stl";
const STORED: &str = "libraries/default/flange-dn40-lp-3310-02/flange-dn40-lp-3310-02.stl";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // Rows only: neither route here reads the store.
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request builds")
}

fn blob_row(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 184_342,
        stored_bytes: 184_342,
        zstd_level: 0,
    }
}

fn flange(volume: f64, x: f64) -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [x, 150.0, 18.0],
        triangle_count: 36_868,
        surface_area_mm2: 41_210.5,
        volume_mm3: Some(volume),
        is_watertight: true,
    }
}

/// A flange, then the same flange 10% wider saved back through the agent — written by the
/// writers ingest and the revision path use, not by hand.
async fn two_revisions(pool: &sqlx::PgPool) -> PartId {
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Flange DN40, LP-3310-02",
            source_path: PATH,
            folder: None,
            storage_path: Some(STORED),
            blob: &blob_row(0x41),
            measurements: &flange(214_780.0, 150.0),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: Some(b"webp-first"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("revision 1");
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the part")
        .revision;
    revisions
        .record_revision(
            RevisionRequest {
                part,
                parent: first,
                origin: RevisionOrigin::Agent,
                lock: None,
                blob: &blob_row(0x42),
                measurements: &flange(236_258.0, 165.0),
                provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
                thumbnail_webp: Some(b"webp-second"),
                kernel_version: "mesh stl-1+cpu-1",
                format: "stl",
                tessellations: &[],
            },
            |_, _| Ok(()),
        )
        .await
        .expect("revision 2");
    part
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_parts_history_lists_every_revision_newest_first_with_its_provenance(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    let (status, json) = send(
        pool,
        request("GET", &format!("/api/parts/{part}/revisions")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let revisions = json.as_array().expect("an array");
    assert_eq!(revisions.len(), 2);

    assert_eq!(revisions[0]["revLabel"], "2");
    assert_eq!(revisions[0]["origin"], "agent");
    assert_eq!(revisions[0]["parent"], revisions[1]["id"]);
    assert_eq!(revisions[1]["revLabel"], "1");
    assert_eq!(revisions[1]["origin"], "ingest");
    assert!(
        revisions[1]["parent"].is_null(),
        "a first revision has none"
    );

    // Mesh-derived, and saying so on every figure — the history is not where the ≈ drops.
    assert_eq!(
        revisions[0]["volumeMm3"],
        serde_json::to_value(Approximate::tessellated(236_258.0_f64)).expect("serialises")
    );
    assert_eq!(
        revisions[1]["bboxMm"],
        serde_json::to_value(Approximate::tessellated([150.0_f64, 150.0, 18.0]))
            .expect("serialises")
    );
    assert_eq!(revisions[0]["sourceHash"], "42".repeat(32));
    assert_eq!(revisions[0]["sourceBytes"], 184_342);
    assert_eq!(
        revisions[0]["thumbnail"], "data:image/webp;base64,d2VicC1zZWNvbmQ=",
        "each revision's own preview, inline"
    );
    assert_eq!(
        revisions[1]["thumbnail"],
        "data:image/webp;base64,d2VicC1maXJzdA=="
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_part_and_one_that_never_existed_have_no_history(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    assert!(
        PgParts(pool.clone())
            .soft_delete(part)
            .await
            .expect("soft delete")
    );
    let (deleted, _) = send(
        pool.clone(),
        request("GET", &format!("/api/parts/{part}/revisions")),
    )
    .await;
    assert_eq!(deleted, StatusCode::NOT_FOUND);

    let (unknown, json) = send(
        pool,
        request("GET", &format!("/api/parts/{}/revisions", PartId::new())),
    )
    .await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("deleted")),
        "the page's own answer: {json}"
    );
}

/// One-way and idempotent: switched once, switched again, and the library list says so.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_switched_to_controlled_stays_controlled(pool: sqlx::PgPool) {
    let uri = format!("/api/libraries/{}/controlled", library());
    let (first, _) = send(pool.clone(), request("POST", &uri)).await;
    assert_eq!(first, StatusCode::NO_CONTENT);
    let (again, _) = send(pool.clone(), request("POST", &uri)).await;
    assert_eq!(
        again,
        StatusCode::NO_CONTENT,
        "switching again changes nothing"
    );

    let (_, libraries) = send(pool, request("GET", "/api/libraries")).await;
    assert_eq!(libraries[0]["mode"], "controlled");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn switching_a_library_that_does_not_exist_is_a_404(pool: sqlx::PgPool) {
    let (status, json) = send(
        pool,
        request(
            "POST",
            &format!("/api/libraries/{}/controlled", LibraryId::new()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
}

/// Each revision says what changed from its parent, and the change keeps the mesh mark.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn each_revision_says_what_changed_from_its_parent(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    let (status, json) = send(
        pool,
        request("GET", &format!("/api/parts/{part}/revisions")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let delta = &json[0]["deltaFromParent"];
    assert_eq!(delta["volumeMm3"]["change"], 21_478.0);
    assert_eq!(delta["volumeMm3"]["approximate"], true);
    let percent = delta["volumeMm3"]["percent"]
        .as_f64()
        .expect("a percentage of a non-zero base");
    assert!((percent - 10.0).abs() < 0.01, "about a tenth: {percent}");
    assert_eq!(delta["bboxMm"][0]["change"], 15.0);
    assert!(
        json[1]["deltaFromParent"].is_null(),
        "a first revision has nothing to change from"
    );
}

/// Any two revisions of one part compare, either way round. A revision that is not this part's
/// is refused rather than compared, and a request missing one says what to send.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn any_two_revisions_of_a_part_compare_and_no_others_do(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    let (_, history) = send(
        pool.clone(),
        request("GET", &format!("/api/parts/{part}/revisions")),
    )
    .await;
    let newer = history[0]["id"].as_str().expect("an id").to_owned();
    let older = history[1]["id"].as_str().expect("an id").to_owned();
    let compare = |from: &str, to: &str| {
        request(
            "GET",
            &format!("/api/parts/{part}/diff?from={from}&to={to}"),
        )
    };

    let (status, forward) = send(pool.clone(), compare(&older, &newer)).await;
    assert_eq!(status, StatusCode::OK, "{forward}");
    assert_eq!(forward["volumeMm3"]["change"], 21_478.0);
    let (_, backward) = send(pool.clone(), compare(&newer, &older)).await;
    assert_eq!(backward["volumeMm3"]["change"], -21_478.0);

    let stranger = lapidary_core::RevisionId::new().to_string();
    let (status, json) = send(pool.clone(), compare(&older, &stranger)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");

    let (status, json) = send(
        pool,
        request("GET", &format!("/api/parts/{part}/diff?from={older}")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}
