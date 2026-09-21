//! `PgShapes`: a part's shape profile follows its newest revision, and goes with the part when it is purged.

use lapidary_core::{
    BlobHash, DESCRIPTOR_LEN, LibraryId, MeasurementProvenance, MeshMeasurements, PartId,
    RevisionId, RevisionOrigin, SHAPE_VERSION, ShapeProfile,
};
use lapidary_db::{
    IngestRequest, PgIngest, PgParts, PgRevisions, PgShapes, Purged, RevisionRequest, Shows,
    StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn blob_row(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 204_800,
        stored_bytes: 204_800,
        zstd_level: 0,
    }
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

fn profile(size_mm: f64, fill: f32) -> ShapeProfile {
    ShapeProfile {
        size_mm,
        descriptor: [fill; DESCRIPTOR_LEN],
    }
}

async fn seed(pool: &sqlx::PgPool, name: &str, path: &str, blob: u8) -> PartId {
    // A revision is recorded against a stored file, so the part has one.
    let stored = format!("libraries/default/{}/{path}", path.trim_end_matches(".stl"));
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: path,
            folder: None,
            storage_path: Some(&stored),
            blob: &blob_row(blob),
            measurements: &measurements(),
            provenance: MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}

async fn current(pool: &sqlx::PgPool, path: &str) -> RevisionId {
    PgRevisions(pool.clone())
        .current(library(), path)
        .await
        .expect("reads")
        .expect("the part")
        .revision
}

/// Records a second revision of the part at `path` and returns the first and the second.
async fn revise(pool: &sqlx::PgPool, part: PartId, path: &str) -> (RevisionId, RevisionId) {
    let first = current(pool, path).await;
    let blob = blob_row(0x62);
    let m = measurements();
    let second = PgRevisions(pool.clone())
        .record_revision(
            RevisionRequest {
                part,
                parent: first,
                origin: RevisionOrigin::Upload,
                lock: None,
                blob: &blob,
                measurements: &m,
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
    (first, second)
}

#[sqlx::test(migrations = "./migrations")]
async fn a_profile_is_recorded_and_read_back_with_what_it_came_from(pool: sqlx::PgPool) {
    let path = "bracket-lp-1042-03.stl";
    let part = seed(&pool, "Bracket, LP-1042-03", path, 0x61).await;
    let revision = current(&pool, path).await;
    let l0 = BlobHash::from_bytes([0x71; 32]);
    let shapes = PgShapes(pool.clone());

    assert_eq!(shapes.of_part(part).await.expect("reads"), None);
    assert!(
        shapes
            .record(part, revision, l0, &profile(38.4, 0.17))
            .await
            .expect("records")
    );

    let stored = shapes.of_part(part).await.expect("reads").expect("stored");
    assert_eq!(stored.revision, revision);
    assert_eq!(stored.l0, l0);
    assert_eq!(stored.version, SHAPE_VERSION);
    assert_eq!(stored.profile, profile(38.4, 0.17));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_newer_revision_replaces_the_profile_and_an_older_one_does_not(pool: sqlx::PgPool) {
    let path = "bracket-lp-1042-03.stl";
    let part = seed(&pool, "Bracket, LP-1042-03", path, 0x61).await;
    let (first, second) = revise(&pool, part, path).await;
    let shapes = PgShapes(pool.clone());

    assert!(
        shapes
            .record(
                part,
                first,
                BlobHash::from_bytes([0x71; 32]),
                &profile(38.4, 0.17)
            )
            .await
            .expect("records")
    );
    assert!(
        shapes
            .record(
                part,
                second,
                BlobHash::from_bytes([0x72; 32]),
                &profile(40.1, 0.19)
            )
            .await
            .expect("records"),
        "the newer revision replaces it"
    );
    assert!(
        !shapes
            .record(
                part,
                first,
                BlobHash::from_bytes([0x71; 32]),
                &profile(38.4, 0.17)
            )
            .await
            .expect("answers"),
        "a late backfill of the older revision writes nothing"
    );
    assert!(
        shapes
            .record(
                part,
                second,
                BlobHash::from_bytes([0x73; 32]),
                &profile(40.1, 0.21)
            )
            .await
            .expect("records"),
        "the same revision again is a rebuilt L0 or a new version, and replaces it"
    );

    let stored = shapes.of_part(part).await.expect("reads").expect("stored");
    assert_eq!(stored.revision, second);
    assert_eq!(stored.l0, BlobHash::from_bytes([0x73; 32]));
    assert_eq!(stored.profile, profile(40.1, 0.21));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_that_is_gone_records_nothing(pool: sqlx::PgPool) {
    let shapes = PgShapes(pool.clone());
    let path = "bracket-lp-1042-03.stl";
    seed(&pool, "Bracket, LP-1042-03", path, 0x61).await;
    let revision = current(&pool, path).await;
    let nobody = PartId::from_uuid(
        "01931b6e-0000-7000-8000-0000000000ff"
            .parse()
            .expect("uuid"),
    );
    assert!(
        !shapes
            .record(
                nobody,
                revision,
                BlobHash::from_bytes([0x71; 32]),
                &profile(38.4, 0.17)
            )
            .await
            .expect("answers")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn purge_takes_the_parts_profile_and_its_links_from_either_side(pool: sqlx::PgPool) {
    let bracket_path = "bracket-lp-1042-03.stl";
    let bracket = seed(&pool, "Bracket, LP-1042-03", bracket_path, 0x61).await;
    let mirrored = seed(
        &pool,
        "Bracket, LP-1042-04 (mirrored)",
        "bracket-lp-1042-04.stl",
        0x63,
    )
    .await;
    let spacer = seed(&pool, "Spacer, LP-2210-20", "spacer-lp-2210-20.stl", 0x64).await;
    PgShapes(pool.clone())
        .record(
            bracket,
            current(&pool, bracket_path).await,
            BlobHash::from_bytes([0x71; 32]),
            &profile(38.4, 0.17),
        )
        .await
        .expect("records");
    let (low, high) = if bracket.as_uuid() < mirrored.as_uuid() {
        (bracket, mirrored)
    } else {
        (mirrored, bracket)
    };
    for (part, other, kind) in [
        (low.as_uuid(), high.as_uuid(), "variant"),
        (spacer.as_uuid(), bracket.as_uuid(), "folded_into"),
    ] {
        sqlx::query(
            "INSERT INTO part_link (part_id, other_id, library_id, kind) VALUES ($1, $2, $3, $4)",
        )
        .bind(part)
        .bind(other)
        .bind(library().as_uuid())
        .bind(kind)
        .execute(&pool)
        .await
        .expect("links");
    }

    let parts = PgParts(pool.clone());
    assert!(parts.soft_delete(bracket).await.expect("removes"));
    assert!(matches!(
        parts.purge(bracket).await.expect("purges"),
        Purged::Done(_)
    ));

    let left: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM part_shape WHERE part_id = $1), \
                (SELECT count(*) FROM part_link WHERE part_id = $1 OR other_id = $1)",
    )
    .bind(bracket.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(left, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn rows_come_back_in_the_order_asked_and_only_from_the_side_asked(pool: sqlx::PgPool) {
    let bracket = seed(&pool, "Bracket, LP-1042-03", "bracket-lp-1042-03.stl", 0x61).await;
    let clamp = seed(
        &pool,
        "Clamp jaw, LP-3310-01",
        "clamp-jaw-lp-3310-01.stl",
        0x63,
    )
    .await;
    let spacer = seed(&pool, "Spacer, LP-2210-20", "spacer-lp-2210-20.stl", 0x64).await;
    let parts = PgParts(pool.clone());
    assert!(parts.soft_delete(clamp).await.expect("removes"));

    let ids = |rows: Vec<lapidary_db::PartRow>| -> Vec<PartId> {
        rows.into_iter().map(|row| row.summary.id).collect()
    };
    let asked = [spacer, clamp, bracket];
    assert_eq!(
        ids(parts
            .rows_by_id(library(), &asked, Shows::Live)
            .await
            .expect("reads")),
        [spacer, bracket],
        "the caller's order, removed parts left out"
    );
    assert_eq!(
        ids(parts
            .rows_by_id(library(), &asked, Shows::Removed)
            .await
            .expect("reads")),
        [clamp]
    );
    let elsewhere = LibraryId::from_uuid(
        "01931b6e-0000-7000-8000-0000000000a2"
            .parse()
            .expect("uuid"),
    );
    assert!(
        parts
            .rows_by_id(elsewhere, &asked, Shows::Live)
            .await
            .expect("reads")
            .is_empty(),
        "another library's id names nothing"
    );
}
