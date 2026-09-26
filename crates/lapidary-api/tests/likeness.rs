//! The likeness routes: which parts look alike, and what somebody decided about a pair.
//!
//! Every profile here is synthetic and goes in through `PgShapes::record`, the repository the worker
//! itself uses — the descriptors are made by hand rather than tessellated, because what is under test
//! is the ranking, the clustering and the decisions, and a real mesh would make each case a statement
//! about `lapidary-cad` instead.
//!
//! The one thing to keep straight while reading: a descriptor here is a flat vector with one bin moved,
//! so the distance between two of them is exactly the amount that bin moved. `NEAR_DUPLICATE_DISTANCE`
//! is 0.04, so 0.01 apart is alike and 0.06 apart is not, and the size band is 2%, so 20.0 mm and
//! 40.0 mm are the same shape at a size nobody would call one part.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{
    BlobHash, DESCRIPTOR_LEN, LibraryId, MeasurementProvenance, MeshMeasurements, PartId,
    RevisionOrigin, ShapeProfile,
};
use lapidary_db::{
    IngestRequest, PgIngest, PgParts, PgRevisions, PgShapes, RevisionRequest, StoredBlobRow,
};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // Nothing here reads a byte off disk: a likeness is metadata and derivatives, and a path that
        // does not exist is what makes that a real assertion rather than a coincidence.
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        touches: Default::default(),
    }
}

async fn send(pool: sqlx::PgPool, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("body reads");
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body is JSON")
    };
    (status, json)
}

async fn call(pool: sqlx::PgPool, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request builds");
    send(pool, request).await
}

async fn call_json(
    pool: sqlx::PgPool,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    send(pool, request).await
}

/// A part with a real ingest record behind it: a `blob` row, a revision and a source file, so the
/// "same bytes" read has something a repository actually maintains to look at.
async fn seed(pool: &sqlx::PgPool, name: &str, path: &str, blob: u8) -> PartId {
    seed_in(pool, library(), "default", name, path, blob).await
}

/// [`seed`], into a library the test named itself. `slug` is that library's directory, so two parts
/// with one file name do not claim one storage path.
async fn seed_in(
    pool: &sqlx::PgPool,
    library: LibraryId,
    slug: &str,
    name: &str,
    path: &str,
    blob: u8,
) -> PartId {
    let stored = format!("libraries/{slug}/{path}");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library,
            name,
            source_path: path,
            folder: None,
            storage_path: Some(&stored),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([blob; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}

/// A descriptor whose first bin is `off` away from the baseline, so the distance between two of these
/// is the difference of their `off`s.
fn profile(size_mm: f64, off: f32) -> ShapeProfile {
    let mut descriptor = [0.12_f32; DESCRIPTOR_LEN];
    descriptor[0] += off;
    ShapeProfile {
        size_mm,
        descriptor,
    }
}

/// Record a profile against the part's current revision, exactly as G2's job will.
async fn record(pool: &sqlx::PgPool, part: PartId, path: &str, profile: &ShapeProfile) {
    record_in(pool, library(), part, path, profile).await;
}

async fn record_in(
    pool: &sqlx::PgPool,
    library: LibraryId,
    part: PartId,
    path: &str,
    profile: &ShapeProfile,
) {
    let revision = PgRevisions(pool.clone())
        .current(library, path)
        .await
        .expect("reads the current revision")
        .expect("the part has one")
        .revision;
    assert!(
        PgShapes(pool.clone())
            .record(part, revision, BlobHash::from_bytes([0x71; 32]), profile)
            .await
            .expect("records the profile")
    );
}

/// A part, its file name, and a profile for it in one step.
async fn profiled(
    pool: &sqlx::PgPool,
    name: &str,
    path: &str,
    blob: u8,
    shape: ShapeProfile,
) -> PartId {
    let part = seed(pool, name, path, blob).await;
    record(pool, part, path, &shape).await;
    part
}

fn ids(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array of cards")
        .iter()
        .map(|card| card["id"].as_str().expect("a card id").to_owned())
        .collect()
}

async fn likeness(pool: &sqlx::PgPool, part: PartId) -> serde_json::Value {
    let (status, json) = call(pool.clone(), "GET", &format!("/api/parts/{part}/likeness")).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

async fn duplicates(pool: &sqlx::PgPool, query: &str) -> serde_json::Value {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/duplicates{query}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

/// Every cluster as a set of ids, so a test says which parts grouped without pinning the order of the
/// groups it does not care about.
fn cluster_sets(json: &serde_json::Value) -> Vec<Vec<String>> {
    json["clusters"]
        .as_array()
        .expect("clusters")
        .iter()
        .map(|cluster| {
            let mut parts = ids(&cluster["parts"]);
            parts.sort();
            parts
        })
        .collect()
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_sharing_its_bytes_is_identical_and_needs_no_profile(pool: sqlx::PgPool) {
    let bracket = seed(&pool, "Bracket, LP-1042-03", "bracket-lp-1042-03.stl", 0xa1).await;
    let copy = seed(
        &pool,
        "Bracket, LP-1042-03 (copy)",
        "archive/bracket-lp-1042-03.stl",
        0xa1,
    )
    .await;
    // Different bytes, so not identical however alike the shape.
    seed(&pool, "Spacer, 20 mm", "spacer-20.stl", 0xa2).await;

    let json = likeness(&pool, bracket).await;
    assert_eq!(json["profiled"], false, "nothing has been profiled yet");
    assert_eq!(ids(&json["identical"]), vec![copy.to_string()]);
    assert!(ids(&json["nearDuplicates"]).is_empty());
    assert!(ids(&json["similar"]).is_empty());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scaled_copy_is_similar_but_never_a_near_duplicate(pool: sqlx::PgPool) {
    let spacer = profiled(
        &pool,
        "Spacer, 20 mm",
        "spacer-20.stl",
        0xa1,
        profile(20.0, 0.0),
    )
    .await;
    let twice = profiled(
        &pool,
        "Spacer, 40 mm",
        "spacer-40.stl",
        0xa2,
        profile(40.0, 0.0),
    )
    .await;
    let alike = profiled(
        &pool,
        "Spacer, 20.1 mm",
        "spacer-20-1.stl",
        0xa3,
        profile(20.1, 0.01),
    )
    .await;

    let json = likeness(&pool, spacer).await;
    assert_eq!(json["profiled"], true);
    assert_eq!(
        ids(&json["nearDuplicates"]),
        vec![alike.to_string()],
        "the same shape at twice the size is not a duplicate of it"
    );
    assert_eq!(ids(&json["similar"]), vec![twice.to_string()]);

    // And the same fact from the queue's side: no cluster at all.
    let json = duplicates(&pool, "").await;
    assert_eq!(
        cluster_sets(&json),
        vec![sorted(vec![spacer, alike])],
        "the scaled copy is in no cluster"
    );
}

fn sorted(parts: Vec<PartId>) -> Vec<String> {
    let mut ids: Vec<String> = parts.iter().map(|p| p.to_string()).collect();
    ids.sort();
    ids
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn distinct_takes_a_pair_out_of_the_queue_and_a_variant_shows_on_both_parts(
    pool: sqlx::PgPool,
) {
    let left = profiled(
        &pool,
        "Bracket, LP-1042-03 L",
        "bracket-l.stl",
        0xa1,
        profile(38.4, 0.0),
    )
    .await;
    let right = profiled(
        &pool,
        "Bracket, LP-1042-03 R",
        "bracket-r.stl",
        0xa2,
        profile(38.4, 0.01),
    )
    .await;
    assert_eq!(cluster_sets(&duplicates(&pool, "").await).len(), 1);

    let (status, json) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{left}/links/{right}"),
        serde_json::json!({ "kind": "variant" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    assert!(
        cluster_sets(&duplicates(&pool, "").await).is_empty(),
        "a decided pair never comes back to the queue"
    );

    let from_left = likeness(&pool, left).await;
    assert_eq!(ids(&from_left["variants"]), vec![right.to_string()]);
    assert!(ids(&from_left["nearDuplicates"]).is_empty());
    let from_right = likeness(&pool, right).await;
    assert_eq!(
        ids(&from_right["variants"]),
        vec![left.to_string()],
        "a variant is shown on both parts"
    );

    // `distinct` replaces it, and the pair still stays out of the queue.
    let (status, _) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{right}/links/{left}"),
        serde_json::json!({ "kind": "distinct" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cluster_sets(&duplicates(&pool, "").await).is_empty());
    let from_left = likeness(&pool, left).await;
    assert!(ids(&from_left["variants"]).is_empty());
    assert!(ids(&from_left["nearDuplicates"]).is_empty());
    assert_eq!(
        ids(&from_left["similar"]),
        vec![right.to_string()],
        "not the same is still more like this"
    );

    // Undoing the decision proposes the pair again.
    let (status, _) = call(
        pool.clone(),
        "DELETE",
        &format!("/api/parts/{left}/links/{right}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(cluster_sets(&duplicates(&pool, "").await).len(), 1);
    let (status, _) = call(
        pool.clone(),
        "DELETE",
        &format!("/api/parts/{left}/links/{right}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing left to undo");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_link_refuses_folding_itself_and_another_library(pool: sqlx::PgPool) {
    let bracket = seed(&pool, "Bracket, LP-1042-03", "bracket.stl", 0xa1).await;
    let plate = seed(&pool, "Plate, 120 x 80", "plate.stl", 0xa2).await;

    let (status, json) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{bracket}/links/{plate}"),
        serde_json::json!({ "kind": "foldedInto" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");

    let (status, _) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{bracket}/links/{bracket}"),
        serde_json::json!({ "kind": "variant" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let elsewhere = PartId::new();
    let (status, _) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{bracket}/links/{elsewhere}"),
        serde_json::json!({ "kind": "variant" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn pairs_a_b_and_b_c_are_one_cluster(pool: sqlx::PgPool) {
    // Each neighbour 0.03 apart, the ends 0.06 apart — a pair on its own, but not the outer pair.
    let a = profiled(
        &pool,
        "Knob, 24 mm A",
        "knob-a.stl",
        0xa1,
        profile(24.0, 0.0),
    )
    .await;
    let b = profiled(
        &pool,
        "Knob, 24 mm B",
        "knob-b.stl",
        0xa2,
        profile(24.0, 0.03),
    )
    .await;
    let c = profiled(
        &pool,
        "Knob, 24 mm C",
        "knob-c.stl",
        0xa3,
        profile(24.0, 0.06),
    )
    .await;

    let json = duplicates(&pool, "").await;
    assert_eq!(
        cluster_sets(&json),
        vec![sorted(vec![a, b, c])],
        "one group of three, not two overlapping pairs"
    );
    assert_eq!(json["clusters"][0]["identical"], false);
    assert_eq!(json["unprofiled"], 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn identical_parts_cluster_and_say_so_and_unprofiled_parts_are_counted(pool: sqlx::PgPool) {
    let one = seed(&pool, "Gear, M1 Z20", "gear-m1-z20.stl", 0xa1).await;
    let copy = seed(&pool, "Gear, M1 Z20 (copy)", "backup/gear.stl", 0xa1).await;
    seed(&pool, "Plate, 120 x 80", "plate.stl", 0xa2).await;

    let json = duplicates(&pool, "").await;
    assert_eq!(cluster_sets(&json), vec![sorted(vec![one, copy])]);
    assert_eq!(
        json["clusters"][0]["identical"], true,
        "the same bytes, so no shape judgement went into it"
    );
    assert_eq!(json["unprofiled"], 3, "nothing has been profiled");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_profile_of_an_older_revision_is_not_a_profile(pool: sqlx::PgPool) {
    let path = "cam-lobe-a.stl";
    let cam = profiled(&pool, "Cam lobe, 32 mm", path, 0xa1, profile(32.0, 0.0)).await;
    let twin = profiled(
        &pool,
        "Cam lobe, 32 mm (dup)",
        "cam-lobe-b.stl",
        0xa2,
        profile(32.0, 0.01),
    )
    .await;
    let json = duplicates(&pool, "").await;
    assert_eq!(cluster_sets(&json), vec![sorted(vec![cam, twin])]);
    assert_eq!(json["unprofiled"], 0);

    // A new revision of one of them: the stored profile now describes bytes nobody has any more, and
    // the worker has not caught up. Until it does, that part counts as uncompared.
    revise(&pool, cam, path).await;
    let json = likeness(&pool, cam).await;
    assert_eq!(
        json["profiled"], false,
        "the profile is of an older revision"
    );
    assert!(ids(&json["nearDuplicates"]).is_empty());
    let json = duplicates(&pool, "").await;
    assert!(cluster_sets(&json).is_empty());
    assert_eq!(json["unprofiled"], 1);
}

/// A second revision of a part, as an upload records one, so its stored profile falls behind.
async fn revise(pool: &sqlx::PgPool, part: PartId, path: &str) {
    let parent = PgRevisions(pool.clone())
        .current(library(), path)
        .await
        .expect("reads the current revision")
        .expect("the part has one")
        .revision;
    let blob = StoredBlobRow {
        hash: BlobHash::from_bytes([0xb1; 32]),
        size_bytes: 211_000,
        stored_bytes: 94_310,
        zstd_level: 3,
    };
    let measurements = MeshMeasurements {
        bbox_mm: [61.0, 42.0, 19.0],
        triangle_count: 49_006,
        surface_area_mm2: 9_902.5,
        volume_mm3: Some(21_610.0),
        is_watertight: true,
    };
    PgRevisions(pool.clone())
        .record_revision(
            RevisionRequest {
                part,
                parent,
                origin: RevisionOrigin::Upload,
                lock: None,
                blob: &blob,
                measurements: &measurements,
                provenance: MeasurementProvenance::TESSELLATED,
                thumbnail_webp: None,
                kernel_version: "mesh stl-1+cpu-1",
                format: "stl",
                tessellations: &[],
            },
            |_, _| Ok(()),
        )
        .await
        .expect("records the second revision");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_removed_part_is_in_no_likeness_and_no_cluster(pool: sqlx::PgPool) {
    let keep = profiled(
        &pool,
        "Washer, M6",
        "washer-m6.stl",
        0xa1,
        profile(12.0, 0.0),
    )
    .await;
    let gone = profiled(
        &pool,
        "Washer, M6 (dup)",
        "dup/washer-m6.stl",
        0xa1,
        profile(12.0, 0.01),
    )
    .await;
    assert_eq!(cluster_sets(&duplicates(&pool, "").await).len(), 1);

    let (status, _) = call(pool.clone(), "DELETE", &format!("/api/parts/{gone}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let json = likeness(&pool, keep).await;
    assert!(ids(&json["identical"]).is_empty());
    assert!(ids(&json["nearDuplicates"]).is_empty());
    assert!(ids(&json["similar"]).is_empty());
    assert!(cluster_sets(&duplicates(&pool, "").await).is_empty());
    let (status, _) = call(pool.clone(), "GET", &format!("/api/parts/{gone}/likeness")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn since_keeps_only_the_clusters_a_new_part_is_in(pool: sqlx::PgPool) {
    let old_a = profiled(&pool, "Pin, 3 x 12", "pin-a.stl", 0xa1, profile(9.0, 0.0)).await;
    let old_b = profiled(
        &pool,
        "Pin, 3 x 12 (dup)",
        "pin-b.stl",
        0xa2,
        profile(9.0, 0.01),
    )
    .await;
    let new_a = profiled(&pool, "Hub, 40 mm", "hub-a.stl", 0xa3, profile(40.0, 0.0)).await;
    let new_b = profiled(
        &pool,
        "Hub, 40 mm (dup)",
        "hub-b.stl",
        0xa4,
        profile(40.0, 0.01),
    )
    .await;

    let all = cluster_sets(&duplicates(&pool, "").await);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.contains(&sorted(vec![old_a, old_b])));

    // The cut is a stamp the database wrote, not this process's clock: `part.created_at` is Postgres's
    // `now()`, and comparing it against a time read here would make the test depend on the two agreeing.
    let cut = created_at(&pool, new_a).await;
    let json = duplicates(&pool, &format!("?since={}", urlencoding(&cut))).await;
    assert_eq!(cluster_sets(&json), vec![sorted(vec![new_a, new_b])]);

    // An empty `since` is no filter, not a 400 — `?since=` is the natural shape of the URL.
    assert_eq!(cluster_sets(&duplicates(&pool, "?since=").await).len(), 2);
    let (status, _) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/duplicates?since=yesterday"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// When the library says this part arrived, as the card carries it.
async fn created_at(pool: &sqlx::PgPool, part: PartId) -> String {
    let (status, grid) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?limit=50"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    grid["parts"]
        .as_array()
        .expect("cards")
        .iter()
        .find(|card| card["id"] == part.to_string())
        .expect("the part is in the grid")["createdAt"]
        .as_str()
        .expect("a timestamp")
        .to_owned()
}

/// Just enough escaping for a timestamp in a query string.
fn urlencoding(raw: &str) -> String {
    raw.replace(':', "%3A").replace('+', "%2B")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_fold_removes_the_duplicate_and_restore_brings_the_pair_back(pool: sqlx::PgPool) {
    let keep = profiled(
        &pool,
        "Flange, DN50 PN16",
        "flange-dn50.stl",
        0xa1,
        profile(165.0, 0.0),
    )
    .await;
    let dup = profiled(
        &pool,
        "Flange, DN50 PN16 (import)",
        "import/flange-dn50.stl",
        0xa2,
        profile(165.0, 0.01),
    )
    .await;
    assert_eq!(cluster_sets(&duplicates(&pool, "").await).len(), 1);

    let (status, json) = call_json(
        pool.clone(),
        "POST",
        &format!("/api/parts/{dup}/fold"),
        serde_json::json!({ "into": keep.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");

    // Removed by the same rule as any removal: gone from the grid, and nothing else touched.
    let (status, grid) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?limit=50"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&grid["parts"]), vec![keep.to_string()]);
    assert!(cluster_sets(&duplicates(&pool, "").await).is_empty());

    // And the removed page can say where it went.
    let (status, folds) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/folds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folds[0]["part"], dup.to_string());
    assert_eq!(folds[0]["into"]["id"], keep.to_string());

    let (status, _) = call(pool.clone(), "POST", &format!("/api/parts/{dup}/restore")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        cluster_sets(&duplicates(&pool, "").await).len(),
        1,
        "Restore brings the pair back, fold recorded no decision about it"
    );

    // Removed the ordinary way afterwards, it is an ordinary removed part: the fold no longer
    // describes why it is gone.
    let (status, _) = call(pool.clone(), "DELETE", &format!("/api/parts/{dup}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, folds) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/folds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folds, serde_json::json!([]), "no longer a fold");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_fold_refuses_itself_another_library_and_a_removed_target(pool: sqlx::PgPool) {
    let keep = seed(&pool, "Flange, DN50 PN16", "flange-dn50.stl", 0xa1).await;
    let dup = seed(
        &pool,
        "Flange, DN50 PN16 (import)",
        "import/flange.stl",
        0xa2,
    )
    .await;

    let (status, json) = call_json(
        pool.clone(),
        "POST",
        &format!("/api/parts/{dup}/fold"),
        serde_json::json!({ "into": dup.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");

    let elsewhere = PartId::new();
    let (status, _) = call_json(
        pool.clone(),
        "POST",
        &format!("/api/parts/{dup}/fold"),
        serde_json::json!({ "into": elsewhere.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another library is a 404");

    assert!(
        PgParts(pool.clone())
            .soft_delete(keep)
            .await
            .expect("removes the target")
    );
    let (status, _) = call_json(
        pool.clone(),
        "POST",
        &format!("/api/parts/{dup}/fold"),
        serde_json::json!({ "into": keep.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a removed target is a 404");
    let (status, folds) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/folds"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folds, serde_json::json!([]), "and it folded nothing");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_libraries_never_see_each_other(pool: sqlx::PgPool) {
    // The same file, the same bytes and the same shape, in two libraries. Everything that could make
    // them one part is true except the one thing that matters.
    let path = "bushing-ptfe-8x12.stl";
    let here = profiled(
        &pool,
        "Bushing, PTFE 8 x 12",
        path,
        0xa1,
        profile(11.5, 0.0),
    )
    .await;
    let jigs = PgParts(pool.clone())
        .create_library("Workshop jigs", "hobby")
        .await
        .expect("a second library");
    let there = seed_in(
        &pool,
        jigs,
        "workshop-jigs",
        "Bushing, PTFE 8 x 12",
        path,
        0xa1,
    )
    .await;
    record_in(&pool, jigs, there, path, &profile(11.5, 0.0)).await;

    let (status, json) = call_json(
        pool.clone(),
        "POST",
        &format!("/api/parts/{here}/fold"),
        serde_json::json!({ "into": there.to_string() }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a part of another library cannot be folded into: {json}"
    );
    let (status, json) = call_json(
        pool.clone(),
        "PUT",
        &format!("/api/parts/{here}/links/{there}"),
        serde_json::json!({ "kind": "variant" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "nor decided about across libraries: {json}"
    );

    // And neither library's reads name the other's part.
    let json = likeness(&pool, here).await;
    for list in ["identical", "nearDuplicates", "similar", "variants"] {
        assert!(
            ids(&json[list]).is_empty(),
            "{list} named a part of another library"
        );
    }
    assert!(cluster_sets(&duplicates(&pool, "").await).is_empty());
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{jigs}/duplicates"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cluster_sets(&json).is_empty());
    assert_eq!(json["unprofiled"], 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unknown_library_and_an_unknown_part_are_not_found(pool: sqlx::PgPool) {
    let nowhere = LibraryId::new();
    for uri in [
        format!("/api/libraries/{nowhere}/duplicates"),
        format!("/api/libraries/{nowhere}/folds"),
        format!("/api/parts/{}/likeness", PartId::new()),
    ] {
        let (status, _) = call(pool.clone(), "GET", &uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
}

/// 10,000 synthetic profiles through `PgShapes::record`, then both routes timed warm. `#[ignore]`
/// because it is a measurement and not a rule — the gate should not pay a minute of seeding to learn a
/// number that belongs in the goal's Record. Run it with:
///
/// ```text
/// cargo xtask heavy -- cargo test --release -p lapidary-api --test likeness -- --ignored --nocapture
/// ```
#[sqlx::test(migrations = "../lapidary-db/migrations")]
#[ignore = "a measurement; see the goal's Record for the numbers"]
async fn ten_thousand_profiles_answer_both_routes_inside_their_budgets(pool: sqlx::PgPool) {
    const PARTS: usize = 10_000;
    let seeded = std::time::Instant::now();
    let mut parts = Vec::with_capacity(PARTS);
    // In chunks, not all at once: 10,000 tasks queueing on a five-connection pool would pass sqlx's
    // acquire timeout before they ever ran.
    for chunk in (0..PARTS).collect::<Vec<usize>>().chunks(32) {
        let mut joined = tokio::task::JoinSet::new();
        for n in chunk.iter().copied() {
            let pool = pool.clone();
            joined.spawn(async move {
                let path = format!("bay-{:02}/part-{n:05}.stl", n % 64);
                // A few percent share their bytes with a neighbour, and sizes spread over three
                // decades so the size band admits a handful of candidates rather than the library.
                let blob = if n % 40 == 0 && n > 0 { n - 1 } else { n };
                let part =
                    seed_measured(&pool, &path, (blob % 251) as u8, (blob / 251) as u8).await;
                let size_mm = 5.0 + (n % 997) as f64 * 0.5;
                let off = (n % 23) as f32 * 0.01;
                record(&pool, part, &path, &profile(size_mm, off)).await;
                part
            });
        }
        while let Some(part) = joined.join_next().await {
            parts.push(part.expect("the seed task finishes"));
        }
    }
    println!("seeded {PARTS} profiled parts in {:.1?}", seeded.elapsed());

    let subject = parts[PARTS / 2];
    let timings = |label: &'static str, uri: String| {
        let pool = pool.clone();
        async move {
            // Warm first: the first call pays for the connection and the plan.
            let (status, _) = call(pool.clone(), "GET", &uri).await;
            assert_eq!(status, StatusCode::OK);
            let mut runs = Vec::new();
            for _ in 0..7 {
                let at = std::time::Instant::now();
                let (status, _) = call(pool.clone(), "GET", &uri).await;
                assert_eq!(status, StatusCode::OK);
                runs.push(at.elapsed());
            }
            runs.sort();
            let median = runs[runs.len() / 2];
            println!(
                "{label}: median {median:.1?} over {} runs ({runs:.1?})",
                runs.len()
            );
            median
        }
    };
    let likeness = timings("/likeness", format!("/api/parts/{subject}/likeness")).await;
    let duplicates = timings(
        "/duplicates",
        format!("/api/libraries/{SEEDED_LIBRARY}/duplicates"),
    )
    .await;
    // What that 300 ms was actually spent on: a cluster's every part is a card with an inline
    // thumbnail, so the shape of the answer says more than the number on its own.
    let answer = duplicates_body(&pool).await;
    let clusters = answer["clusters"].as_array().expect("clusters");
    let clustered: usize = clusters
        .iter()
        .map(|c| c["parts"].as_array().expect("parts").len())
        .sum();
    println!(
        "/duplicates answered {} cluster(s) holding {clustered} card(s), {} unprofiled",
        clusters.len(),
        answer["unprofiled"]
    );
    assert!(
        likeness < std::time::Duration::from_millis(50),
        "/likeness took {likeness:.1?}"
    );
    assert!(
        duplicates < std::time::Duration::from_millis(300),
        "/duplicates took {duplicates:.1?}"
    );
}

async fn duplicates_body(pool: &sqlx::PgPool) -> serde_json::Value {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/duplicates"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json
}

/// The measurement's seeder: like [`seed`], with the blob hash spread over two bytes so 10,000 parts
/// can each have their own and a chosen few can share.
async fn seed_measured(pool: &sqlx::PgPool, path: &str, low: u8, high: u8) -> PartId {
    let mut hash = [0u8; 32];
    hash[0] = low;
    hash[1] = high;
    let stored = format!("libraries/default/{path}");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: library(),
            name: path,
            source_path: path,
            folder: None,
            storage_path: Some(&stored),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes(hash),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}
