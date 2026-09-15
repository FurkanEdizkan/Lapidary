//! `POST /api/libraries/{id}/bundle` and its plan (Phase 4 slice 2 spec §6): the selected parts as
//! one ZIP, every revision byte-identical, with the manifest that names them.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, RevisionOrigin};
use lapidary_db::{
    IngestRequest, NewPartSource, PgIngest, PgParts, PgRevisions, RevisionRequest, StoredBlobRow,
};
use std::io::Read;
use std::path::Path;
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const FLANGE: &str = "flanges/flange-dn40-lp-3310-02.stl";
const FLANGE_STORED: &str =
    "libraries/default/flanges/flange-dn40-lp-3310-02/flange-dn40-lp-3310-02.stl";
const VEE: &str = "vee-block-lp-3072-02.stl";
const VEE_STORED: &str = "libraries/default/vee-block-lp-3072-02/vee-block-lp-3072-02.stl";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn measurements() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [150.0, 150.0, 16.0],
        triangle_count: 784,
        surface_area_mm2: 41_210.5,
        volume_mm3: Some(243_100.0),
        is_watertight: true,
    }
}

fn row(bytes: &[u8]) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes(*blake3::hash(bytes).as_bytes()),
        size_bytes: bytes.len() as u64,
        stored_bytes: bytes.len() as u64,
        zstd_level: 0,
    }
}

/// A part whose one file is filed in its model directory, as ingest files one.
async fn filed(
    pool: &sqlx::PgPool,
    root: &Path,
    name: &str,
    source_path: &str,
    stored: &str,
    bytes: &[u8],
) -> PartId {
    let file = root.join(stored);
    std::fs::create_dir_all(file.parent().expect("a model directory")).expect("the directory");
    std::fs::write(&file, bytes).expect("the file");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path,
            folder: None,
            storage_path: Some(stored),
            blob: &row(bytes),
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("records")
}

/// The flange saved again through the agent, its files moved as the revision path moves them.
async fn revise(pool: &sqlx::PgPool, root: &Path, part: PartId, bytes: &[u8]) {
    let revisions = PgRevisions(pool.clone());
    let current = revisions
        .current(library(), FLANGE)
        .await
        .expect("reads")
        .expect("the part");
    revisions
        .record_revision(
            RevisionRequest {
                part,
                parent: current.revision,
                origin: RevisionOrigin::Agent,
                lock: None,
                blob: &row(bytes),
                measurements: &measurements(),
                provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
                thumbnail_webp: None,
                kernel_version: "mesh stl-1+glb-1+cpu-1",
                format: "stl",
                tessellations: &[],
            },
            |current, aside| {
                let aside = root.join(aside);
                std::fs::create_dir_all(aside.parent().expect("a folder"))
                    .map_err(|e| e.to_string())?;
                std::fs::rename(root.join(current), &aside).map_err(|e| e.to_string())?;
                std::fs::write(root.join(current), bytes).map_err(|e| e.to_string())
            },
        )
        .await
        .expect("revision 2");
}

async fn post(
    pool: sqlx::PgPool,
    root: &Path,
    uri: &str,
    content_type: &str,
    body: String,
) -> axum::response::Response {
    router(
        AppState {
            db: pool,
            blob_root: root.to_path_buf(),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            touches: Default::default(),
        },
        Role::Api,
    )
    .oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .expect("request builds"),
    )
    .await
    .expect("router responds")
}

async fn plan(
    pool: sqlx::PgPool,
    root: &Path,
    parts: serde_json::Value,
) -> axum::response::Response {
    post(
        pool,
        root,
        &format!("/api/libraries/{SEEDED_LIBRARY}/bundle/plan"),
        "application/json",
        serde_json::json!({ "parts": parts }).to_string(),
    )
    .await
}

async fn export(pool: sqlx::PgPool, root: &Path, parts: &str) -> axum::response::Response {
    post(
        pool,
        root,
        &format!("/api/libraries/{SEEDED_LIBRARY}/bundle"),
        "application/x-www-form-urlencoded",
        format!("parts={parts}"),
    )
    .await
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_holds_every_revision_byte_identical_with_a_manifest_that_names_them(
    pool: sqlx::PgPool,
) {
    let store = tempfile::tempdir().expect("a store");
    let root = store.path();
    let first = b"solid flange-dn40-lp-3310-02, revision 1\n".repeat(300);
    let second = b"solid flange-dn40-lp-3310-02, revision 2, 10% wider\n".repeat(300);
    let vee = b"solid vee-block-lp-3072-02\n".repeat(200);
    let flange = filed(
        &pool,
        root,
        "Flange DN40, LP-3310-02",
        FLANGE,
        FLANGE_STORED,
        &first,
    )
    .await;
    PgParts(pool.clone())
        .add_part_source(
            flange,
            NewPartSource {
                url: Some("https://www.printables.com/model/381-flange-dn40"),
                license: Some("CC-BY-4.0"),
                ..Default::default()
            },
        )
        .await
        .expect("a source");
    revise(&pool, root, flange, &second).await;
    let vee_block = filed(&pool, root, "Vee block, LP-3072-02", VEE, VEE_STORED, &vee).await;

    let planned = plan(pool.clone(), root, serde_json::json!([flange, vee_block])).await;
    assert_eq!(planned.status(), StatusCode::OK);
    let planned: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(planned.into_body(), 64 * 1024)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(planned["parts"], 2);
    assert_eq!(planned["revisions"], 3);

    let response = export(pool, root, &format!("{flange}%2C{vee_block}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("ascii")
            .contains("Default-bundle.lapidary.zip")
    );
    let length: u64 = response.headers()[header::CONTENT_LENGTH]
        .to_str()
        .expect("ascii")
        .parse()
        .expect("a number");
    assert_eq!(serde_json::json!(length), planned["bytes"]);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the whole bundle");
    assert_eq!(body.len() as u64, length);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).expect("a ZIP");
    let names: Vec<String> = (0..archive.len())
        .map(|index| archive.by_index(index).expect("entry").name().to_owned())
        .collect();
    let earlier = "flanges/revisions/1/flange-dn40-lp-3310-02.stl";
    assert_eq!(names, [earlier, FLANGE, VEE, "manifest.json"]);
    for (name, bytes) in [(earlier, &first), (FLANGE, &second), (VEE, &vee)] {
        let mut entry = archive.by_name(name).expect("entry");
        assert_eq!(
            entry.compression(),
            zip::CompressionMethod::Stored,
            "{name}"
        );
        let mut back = Vec::new();
        entry.read_to_end(&mut back).expect("reads");
        assert_eq!(&back, bytes, "{name} is byte-identical");
    }

    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .expect("the manifest")
        .read_to_string(&mut manifest)
        .expect("reads");
    let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("json");
    assert_eq!(manifest["format"], "lapidary-bundle");
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["library"]["mode"], "hobby");
    let part = &manifest["parts"][0];
    assert_eq!(part["sourcePath"], FLANGE);
    assert_eq!(part["sources"][0]["license"], "CC-BY-4.0");
    assert_eq!(part["revisions"][0]["revLabel"], "1");
    assert_eq!(part["revisions"][0]["path"], earlier);
    assert!(part["revisions"][0]["parentLabel"].is_null());
    assert_eq!(part["revisions"][1]["parentLabel"], "1");
    assert_eq!(part["revisions"][1]["origin"], "agent");
    assert_eq!(
        part["revisions"][1]["blake3"],
        blake3::hash(&second).to_hex().to_string()
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_bundle_that_cannot_be_made_whole_is_refused_before_a_byte_is_sent(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let root = store.path();
    let flange = filed(
        &pool,
        root,
        "Flange DN40, LP-3310-02",
        FLANGE,
        FLANGE_STORED,
        b"solid one\n",
    )
    .await;
    revise(&pool, root, flange, b"solid two\n").await;

    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([]))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let many: Vec<String> = (0..501).map(|_| PartId::new().to_string()).collect();
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!(many))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!(vec![flange; 501]))
            .await
            .status(),
        StatusCode::BAD_REQUEST,
        "counted before de-duplicating"
    );
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([PartId::new()]))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "a part in no library"
    );
    // A part filed at the path the flange's first revision takes inside a bundle.
    let squatter = filed(
        &pool,
        root,
        "Flange DN40, spare copy",
        "flanges/revisions/1/flange-dn40-lp-3310-02.stl",
        "libraries/default/flanges/spare/flange-dn40-lp-3310-02.stl",
        b"solid spare\n",
    )
    .await;
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([flange, squatter]))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        export(pool, root, "not-a-part").await.status(),
        StatusCode::BAD_REQUEST
    );
}

/// A stored file that no longer hashes to its digest ends the bundle short of its promised length:
/// a download that does not finish, never a finished bundle holding the wrong file.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_file_that_no_longer_matches_its_hash_ends_the_bundle_short(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let root = store.path();
    let vee = b"solid vee-block-lp-3072-02\n".repeat(50);
    let vee_block = filed(&pool, root, "Vee block, LP-3072-02", VEE, VEE_STORED, &vee).await;
    std::fs::write(root.join(VEE_STORED), vec![b'x'; vee.len()]).expect("somebody edits the file");

    let response = export(pool, root, &vee_block.to_string()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .is_err(),
        "the body ends in an error, short of its Content-Length"
    );
}

/// Counts the statements sqlx runs, from its `sqlx::query` events, on the thread it is the default for.
struct Statements(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl tracing::Subscriber for Statements {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target() == "sqlx::query"
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Planning reads the selection in one query, not several per part and one per revision: forty parts, one of
/// them revised, cost the library's row and the selection's, however long their histories are.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn forty_parts_are_planned_in_two_queries(pool: sqlx::PgPool) {
    let root = tempfile::tempdir().expect("a blob root");
    let flange = filed(
        &pool,
        root.path(),
        "Flange DN40, LP-3310-02",
        FLANGE,
        FLANGE_STORED,
        b"solid flange-dn40 v1",
    )
    .await;
    revise(&pool, root.path(), flange, b"solid flange-dn40 v2").await;
    let mut parts = vec![flange.to_string()];
    for n in 1..40 {
        let file = format!("spacer-m8x{n:02}-lp-2001-{n:02}");
        let part = filed(
            &pool,
            root.path(),
            &format!("Spacer M8 x {n} mm, LP-2001-{n:02}"),
            &format!("spacers/{file}.stl"),
            &format!("libraries/default/spacers/{file}/{file}.stl"),
            format!("solid {file}").as_bytes(),
        )
        .await;
        parts.push(part.to_string());
    }

    let statements = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let response = {
        let _counting = tracing::subscriber::set_default(Statements(statements.clone()));
        plan(pool.clone(), root.path(), serde_json::json!(parts)).await
    };
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        statements.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the library's row, then the selection's"
    );
}

/// A removed part, and a part of another library, are refused as parts this library cannot bundle: the
/// selection's one query holds them back, as the reads it replaced did one part at a time.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_removed_part_or_one_of_another_library_is_not_bundled(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("a store");
    let root = store.path();
    let vee = filed(
        &pool,
        root,
        "Vee block, LP-3072-02",
        VEE,
        VEE_STORED,
        b"solid vee-block\n",
    )
    .await;
    let flange = filed(
        &pool,
        root,
        "Flange DN40, LP-3310-02",
        FLANGE,
        FLANGE_STORED,
        b"solid flange-dn40\n",
    )
    .await;
    assert!(
        PgParts(pool.clone())
            .soft_delete(flange)
            .await
            .expect("removes")
    );
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([vee, flange]))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "a removed part"
    );

    let jigs = PgParts(pool.clone())
        .create_library("Workshop jigs", "hobby")
        .await
        .expect("a second library");
    let clamp = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: jigs,
            name: "Toggle clamp, LP-4120-01",
            source_path: "clamps/toggle-clamp-lp-4120-01.stl",
            folder: None,
            storage_path: None,
            blob: &row(b"solid toggle-clamp-lp-4120-01\n"),
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("records");
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([vee, clamp]))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "a part of another library"
    );
    let gasket = PartId::new();
    sqlx::query(
        "INSERT INTO part (id, library_id, name, source_path) \
         VALUES ($1, $2, 'Gasket, LP-3312-01', 'gaskets/gasket-lp-3312-01.stl')",
    )
    .bind(gasket.as_uuid())
    .bind(library().as_uuid())
    .execute(&pool)
    .await
    .expect("a part row with no revision");
    assert_eq!(
        plan(pool.clone(), root, serde_json::json!([vee, gasket]))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "a part with no revision"
    );
    assert_eq!(
        plan(pool, root, serde_json::json!([vee])).await.status(),
        StatusCode::OK
    );
}
