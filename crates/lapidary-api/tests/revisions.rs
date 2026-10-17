//! A part's history, and the switch that turns a library's history on.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use lapidary_core::{Approximate, BlobHash, LibraryId, MeshMeasurements, PartId, RevisionOrigin};
use lapidary_db::{IngestRequest, PgIngest, PgParts, PgRevisions, RevisionRequest, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const PATH: &str = "flange-dn40-lp-3310-02.stl";
/// Where the file is in the owner's folders; both revisions name it, and only the second is it now.
const STORED: &str = "Flanges/flange-dn40-lp-3310-02.stl";

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
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: std::path::PathBuf::from("/nonexistent-inbox"),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
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
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Flange DN40, LP-3310-02",
            source_path: PATH,
            folder: None,
            storage_path: Some(STORED),
            root: lapidary_core::FileRoot::Library,
            mtime_ns: Some(1_760_000_000_000_000_000),
            blob: &blob_row(0x41),
            measurements: &flange(214_780.0, 150.0),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
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
        .record_revision(RevisionRequest {
            part,
            parent: first,
            origin: RevisionOrigin::Agent,
            root: lapidary_core::FileRoot::Library,
            storage_path: STORED,
            mtime_ns: Some(1_760_000_100_000_000_000),
            blob: &blob_row(0x42),
            measurements: &flange(236_258.0, 165.0),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
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
    // The second replaced the first in the owner's folder (M4): the first keeps its figures and
    // its hash, and says it is no longer on disk.
    assert_eq!(revisions[0]["onDisk"], true);
    assert_eq!(revisions[1]["onDisk"], false);
    assert_eq!(revisions[1]["sourceHash"], "41".repeat(32));
}

/// (d) An old revision whose file the newer one replaced answers 410, with a sentence, before the
/// disk is looked at — this state's folders do not even exist.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_old_revision_no_longer_on_disk_answers_gone(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    let history = PgRevisions(pool.clone())
        .history(part)
        .await
        .expect("history");
    let (status, json) = send(
        pool.clone(),
        request(
            "GET",
            &format!("/api/revisions/{}/download?variant=original", history[1].id),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "{json}");
    assert_eq!(json["reason"], "noLongerOnDisk");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("no longer on disk")),
        "{json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_part_and_one_that_never_existed_have_no_history(pool: sqlx::PgPool) {
    let part = two_revisions(&pool).await;
    assert!(PgParts(pool.clone()).delete(part).await.expect("delete"));
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

fn put(uri: String, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

/// The newest revision's mass, as the history serves it.
async fn mass_of(pool: &sqlx::PgPool, part: PartId) -> serde_json::Value {
    let (status, history) = send(
        pool.clone(),
        request("GET", &format!("/api/parts/{part}/revisions")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    history[0]["massG"].clone()
}

/// Mass is worked out when read: a revision's volume times the density of the part's one material as
/// it is today, and always approximate. Two materials, none, or a material with no density give none,
/// and a change between two revisions is worked out from the same density on both sides.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn mass_is_volume_times_the_density_of_the_parts_one_material(pool: sqlx::PgPool) {
    let bracket = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            folder: None,
            storage_path: Some("libraries/default/bracket-lp-1042-03/bracket-lp-1042-03.stl"),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            blob: &blob_row(0x43),
            measurements: &flange(21_478.5, 60.0),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("the bracket");
    assert!(
        mass_of(&pool, bracket).await.is_null(),
        "no material, no mass"
    );

    let steel = "S235JR steel";
    let (status, _) = send(
        pool.clone(),
        put(
            format!("/api/libraries/{SEEDED_LIBRARY}/densities/S235JR%20steel"),
            serde_json::json!({ "densityKgM3": 7850 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let given = |part: PartId, materials: serde_json::Value| {
        put(
            format!("/api/parts/{part}/materials"),
            serde_json::json!({ "materials": materials }),
        )
    };

    send(pool.clone(), given(bracket, serde_json::json!([steel]))).await;
    let mass = mass_of(&pool, bracket).await;
    let grams = mass["value"].as_f64().expect("a mass");
    assert!(
        (grams - 168.6).abs() < 0.05,
        "21,478.5 mm³ of a 7,850 kg/m³ steel is about 168.6 g: {grams}"
    );
    assert_eq!(
        mass["approximate"], true,
        "a typed density is not a measurement"
    );

    send(
        pool.clone(),
        given(bracket, serde_json::json!([steel, "EN AW-6082 T6"])),
    )
    .await;
    assert!(
        mass_of(&pool, bracket).await.is_null(),
        "two materials, no mass"
    );
    send(
        pool.clone(),
        given(bracket, serde_json::json!(["EN AW-6082 T6"])),
    )
    .await;
    assert!(
        mass_of(&pool, bracket).await.is_null(),
        "a material with no density, no mass"
    );

    let widened = two_revisions(&pool).await;
    send(pool.clone(), given(widened, serde_json::json!([steel]))).await;
    let (_, history) = send(
        pool.clone(),
        request("GET", &format!("/api/parts/{widened}/revisions")),
    )
    .await;
    let (newer, older) = (&history[0]["massG"], &history[1]["massG"]);
    let more = newer["value"].as_f64().expect("a mass") - older["value"].as_f64().expect("a mass");
    assert!(
        (more - 168.602_3).abs() < 0.001,
        "21,478 mm³ more of the same steel: {newer} against {older}"
    );
    assert_eq!(newer["approximate"], true);
}

/// A revision's centre of mass comes back with where it came from, and a revision recorded before
/// centres were has none.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn each_revision_keeps_its_own_centre_of_mass_and_an_older_one_may_have_none(
    pool: sqlx::PgPool,
) {
    let part = two_revisions(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let recorded = revisions.history(part).await.expect("history");
    let (newer, older) = (recorded[0].id, recorded[1].id);
    let history = || {
        let pool = pool.clone();
        async move {
            send(
                pool,
                request("GET", &format!("/api/parts/{part}/revisions")),
            )
            .await
            .1
        }
    };

    revisions
        .set_centre_of_mass(
            newer,
            [82.5, 75.0, 9.0],
            lapidary_core::Provenance::Tessellated,
        )
        .await
        .expect("writes");
    let json = history().await;
    assert_eq!(
        json[0]["centreMm"],
        serde_json::to_value(Approximate::tessellated([82.5_f64, 75.0, 9.0])).expect("serialises")
    );
    assert!(
        json[1]["centreMm"].is_null(),
        "revision 1 was recorded before centres were"
    );

    revisions
        .set_centre_of_mass(
            older,
            [75.0, 75.0, 9.0],
            lapidary_core::Provenance::Tessellated,
        )
        .await
        .expect("writes");
    let json = history().await;
    assert_eq!(
        json[1]["centreMm"],
        serde_json::to_value(Approximate::tessellated([75.0_f64, 75.0, 9.0])).expect("serialises")
    );
}
