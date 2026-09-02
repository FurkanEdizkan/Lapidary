use lapidary_core::{BlobHash, LibraryId, MeshMeasurements};
use lapidary_db::{IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts, StoredBlobRow};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn blob_row(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 204_800,
        stored_bytes: 91_204,
        zstd_level: 3,
    }
}

fn watertight() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

fn open_mesh() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [88.0, 34.0, 12.0],
        triangle_count: 12_940,
        surface_area_mm2: 15_320.5,
        volume_mm3: None,
        is_watertight: false,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn recording_an_ingest_creates_a_part_a_revision_a_file_and_a_thumbnail(pool: sqlx::PgPool) {
    let blob = blob_row(0xab);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bearing block, 608ZZ",
            blob: &blob,
            measurements: &watertight(),
            thumbnail_webp: b"webp bytes",
            kernel_version: "mesh stl-1+cpu-1",
        })
        .await
        .expect("records");

    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM part WHERE id = $1),
                (SELECT count(*) FROM revision r WHERE r.part_id = $1),
                (SELECT count(*) FROM file f JOIN revision r ON f.revision_id = r.id WHERE r.part_id = $1),
                (SELECT count(*) FROM derivative d JOIN revision r ON d.revision_id = r.id WHERE r.part_id = $1 AND d.kind = 'thumbnail')",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(counts, (1, 1, 1, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn every_measurement_is_written_as_tessellated(pool: sqlx::PgPool) {
    let blob = blob_row(0xcd);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (vs, bs): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT volume_source, bbox_source FROM revision WHERE part_id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(vs.as_deref(), Some("tessellated"));
    assert_eq!(bs.as_deref(), Some("tessellated"));
}

#[sqlx::test(migrations = "./migrations")]
async fn an_open_mesh_stores_a_null_volume_but_still_stores_its_bbox(pool: sqlx::PgPool) {
    let blob = blob_row(0xef);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Cable clip, LP-3300-01",
            blob: &blob,
            measurements: &open_mesh(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (volume, vs, bx, watertight): (Option<f64>, Option<String>, Option<f64>, Option<bool>) =
        sqlx::query_as(
            "SELECT volume, volume_source, bbox_x, is_watertight FROM revision WHERE part_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(volume, None, "an open mesh must store no volume");
    assert_eq!(vs, None, "no volume means no provenance for one");
    assert_eq!(
        bx,
        Some(88.0),
        "the bbox is still measurable and still stored"
    );
    assert_eq!(watertight, Some(false));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_known_hash_is_reported_as_existing(pool: sqlx::PgPool) {
    let blob = blob_row(0x11);
    let blobs = PgBlobs(pool.clone());
    assert!(!blobs.exists(&blob.hash).await.expect("query"));
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Spacer, LP-2001-00",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    assert!(blobs.exists(&blob.hash).await.expect("query"));
}

#[sqlx::test(migrations = "./migrations")]
async fn linking_an_existing_blob_adds_a_part_without_touching_ref_count_twice(pool: sqlx::PgPool) {
    let blob = blob_row(0x22);
    let ingest = PgIngest(pool.clone());
    // Bound once, outside the closure: `measurements: &watertight()` inside the closure
    // body borrows a temporary that does not outlive the returned `IngestRequest`
    // (E0515) — the brief's original listing does not compile.
    let measurements = watertight();
    let req = |name: &'static str| IngestRequest {
        library: library(),
        name,
        blob: &blob,
        measurements: &measurements,
        kernel_version: "mesh stl-1+cpu-1",
        thumbnail_webp: b"webp",
    };
    ingest
        .record(req("Bracket, LP-1042-03"))
        .await
        .expect("first");
    ingest
        .link_existing(req("Bracket copy, LP-1042-03"))
        .await
        .expect("second");

    let ref_count: i32 = sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(blob.hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(ref_count, 2, "each file referencing the blob counts once");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_page_returns_newest_first_with_a_thumbnail_hash(pool: sqlx::PgPool) {
    let ingest = PgIngest(pool.clone());
    for (i, name) in [
        "Bracket, LP-1042-03",
        "Spacer, LP-2001-00",
        "Cable clip, LP-3300-01",
    ]
    .iter()
    .enumerate()
    {
        ingest
            .record(IngestRequest {
                library: library(),
                name,
                blob: &blob_row(0x30 + i as u8),
                measurements: &watertight(),
                kernel_version: "mesh stl-1+cpu-1",
                thumbnail_webp: b"webp",
            })
            .await
            .expect("records");
    }

    let page = PgParts(pool.clone())
        .page(library(), None, 2)
        .await
        .expect("page");
    assert_eq!(page.len(), 2, "limit is honoured");
    assert_eq!(page[0].name, "Cable clip, LP-3300-01", "newest first");
    assert!(
        page[0].approximate,
        "every mesh-derived part is approximate"
    );
    assert_eq!(page[0].triangle_count, Some(48_112));

    let next = PgParts(pool.clone())
        .page(library(), Some(page[1].id), 2)
        .await
        .expect("second page");
    assert_eq!(
        next.len(),
        1,
        "keyset pagination continues after the last id"
    );
    assert_eq!(next[0].name, "Bracket, LP-1042-03");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_deleted_part_never_appears_in_the_grid(pool: sqlx::PgPool) {
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x40),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");

    let page = PgParts(pool).page(library(), None, 50).await.expect("page");
    assert!(
        page.is_empty(),
        "delete is soft, but soft-deleted parts are still hidden"
    );
}
