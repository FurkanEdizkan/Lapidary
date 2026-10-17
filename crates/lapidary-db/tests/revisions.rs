//! `PgRevisions`: the next revision is recorded against the current one, or not at all — and
//! since M4 it moves no file: the revision it replaces in place is marked no longer on disk.

use lapidary_core::{BlobHash, FileRoot, LibraryId, MeshMeasurements, PartId, RevisionOrigin};
use lapidary_db::{
    DbError, GridQuery, IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts, PgRevisions,
    RevisionRequest, Sort, StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const PATH: &str = "vee-block-lp-3072-02.stl";
/// Where the file is in the owner's folders: the library's root (`''`) joined with `PATH`.
const STORED: &str = PATH;

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

async fn seed(pool: &sqlx::PgPool) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Vee block, LP-3072-02",
            source_path: PATH,
            folder: None,
            storage_path: Some(STORED),
            root: FileRoot::Library,
            mtime_ns: Some(1_760_000_000_000_000_000),
            blob: &blob_row(0x31),
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seeds the part")
}

fn request<'a>(
    part: PartId,
    parent: lapidary_core::RevisionId,
    blob: &'a StoredBlobRow,
    measurements: &'a MeshMeasurements,
) -> RevisionRequest<'a> {
    RevisionRequest {
        part,
        parent,
        origin: RevisionOrigin::Upload,
        root: FileRoot::Library,
        storage_path: STORED,
        mtime_ns: Some(1_760_000_100_000_000_000),
        blob,
        measurements,
        provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
        format: "stl",
    }
}

/// Every part's name in volume order, a page of one at a time.
async fn by_volume(parts: &PgParts) -> Vec<String> {
    let mut names = Vec::new();
    let mut after = None;
    loop {
        let page = parts
            .page(
                &GridQuery {
                    after,
                    ..GridQuery::new(library(), 1)
                },
                Sort::Volume,
            )
            .await
            .expect("sorted page");
        let Some(last) = page.last() else {
            return names;
        };
        after = Some(last.summary.id);
        names.extend(page.iter().map(|row| row.summary.name.clone()));
    }
}

/// The grid's order reads the part row's copy of its latest revision's figures (`0035`): a revision that
/// changes them moves its part, the copy holds the new revision's, and paging still lists each part once.
#[sqlx::test(migrations = "./migrations")]
async fn a_revision_moves_its_part_in_the_grids_order(pool: sqlx::PgPool) {
    let vee = seed(&pool).await;
    for (name, path, blob, volume_mm3) in [
        (
            "Linear rail, LP-1101-02",
            "linear-rail-lp-1101-02.stl",
            0x41,
            90_000.0,
        ),
        ("Spacer, LP-1050-01", "spacer-lp-1050-01.stl", 0x42, 500.0),
    ] {
        PgIngest(pool.clone())
            .record(IngestRequest {
                origin: RevisionOrigin::Ingest,
                library: library(),
                name,
                source_path: path,
                folder: None,
                storage_path: None,
                root: lapidary_core::FileRoot::Store,
                mtime_ns: None,
                blob: &blob_row(blob),
                measurements: &MeshMeasurements {
                    volume_mm3: Some(volume_mm3),
                    ..measurements()
                },
                provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
                format: "stl",
            })
            .await
            .expect("seeds the part");
    }
    let parts = PgParts(pool.clone());
    assert_eq!(
        by_volume(&parts).await,
        [
            "Linear rail, LP-1101-02",
            "Vee block, LP-3072-02",
            "Spacer, LP-1050-01"
        ]
    );

    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part");
    let widened = MeshMeasurements {
        bbox_mm: [140.0, 42.0, 18.5],
        triangle_count: 61_204,
        surface_area_mm2: 14_220.5,
        volume_mm3: Some(120_000.0),
        is_watertight: true,
    };
    revisions
        .record_revision(request(vee, first.revision, &blob_row(0x32), &widened))
        .await
        .expect("records the widened revision");

    assert_eq!(
        by_volume(&parts).await,
        [
            "Vee block, LP-3072-02",
            "Linear rail, LP-1101-02",
            "Spacer, LP-1050-01"
        ]
    );
    let copy: (Option<f64>, Option<f64>, Option<f64>, Option<i32>) = sqlx::query_as(
        "SELECT latest_volume, latest_surface_area, latest_longest_side, latest_triangle_count \
         FROM part WHERE id = $1",
    )
    .bind(vee.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("reads the part row");
    assert_eq!(
        copy,
        (Some(120_000.0), Some(14_220.5), Some(140.0), Some(61_204)),
        "the widened revision's figures, the longest side its largest extent"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_revision_goes_on_top_and_the_file_it_replaced_is_no_longer_on_disk(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part");
    let blob = blob_row(0x32);
    let m = measurements();

    let second = revisions
        .record_revision(request(part, first.revision, &blob, &m))
        .await
        .expect("records the second revision");

    let history = revisions.history(part).await.expect("history");
    let labels: Vec<&str> = history.iter().map(|r| r.rev_label.as_str()).collect();
    assert_eq!(labels, ["2", "1"], "newest first");
    assert_eq!(history[0].id, second);
    assert_eq!(history[0].parent, Some(first.revision));
    assert_eq!(history[0].origin, RevisionOrigin::Upload);
    assert_eq!(history[1].origin, RevisionOrigin::Ingest);
    // Both name the one file in the owner's folder; only the newer still describes it.
    assert_eq!(history[0].storage_path.as_deref(), Some(STORED));
    assert_eq!(history[1].storage_path.as_deref(), Some(STORED));
    assert!(history[0].on_disk, "the new revision is the file there now");
    assert!(
        !history[1].on_disk,
        "the file it replaced is gone, and Lapidary kept no copy"
    );

    let now = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the part");
    assert_eq!(now.revision, second);
    assert_eq!(now.source_hash, Some(blob.hash));

    // The revert rule's other half: bytes only an older revision holds are a change.
    let blobs = PgBlobs(pool.clone());
    assert!(
        !blobs
            .library_holds(library(), PATH, &blob_row(0x31).hash)
            .await
            .expect("reads"),
        "revision 1's bytes, arriving again, are a revert and not a re-scan"
    );
    assert!(
        blobs
            .library_holds(library(), PATH, &blob.hash)
            .await
            .expect("reads")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_parent_that_is_no_longer_current_is_a_conflict(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part")
        .revision;
    let m = measurements();
    revisions
        .record_revision(request(part, first, &blob_row(0x32), &m))
        .await
        .expect("the second revision");

    let error = revisions
        .record_revision(request(part, first, &blob_row(0x33), &m))
        .await
        .expect_err("revision 1 is no longer current");
    assert!(
        matches!(error, DbError::RevisionConflict { .. }),
        "got {error:?}"
    );
    assert_eq!(revisions.history(part).await.expect("history").len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_deleted_while_its_change_was_measured_is_not_revised(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part")
        .revision;
    assert!(
        PgParts(pool.clone())
            .delete(part)
            .await
            .expect("delete")
            .is_some()
    );

    let m = measurements();
    let error = revisions
        .record_revision(request(part, first, &blob_row(0x32), &m))
        .await
        .expect_err("a deleted part takes no revision");
    assert!(
        matches!(error, DbError::RevisionConflict { .. }),
        "got {error:?}"
    );
    assert!(
        revisions
            .current(library(), PATH)
            .await
            .expect("reads")
            .is_none(),
        "and the retry finds no part there, so it ingests the file as a new one"
    );
}

/// A revision whose file is somewhere else leaves the earlier file on disk: a part from before M4
/// keeps its store copy, and an earlier upload keeps its own inbox name.
#[sqlx::test(migrations = "./migrations")]
async fn a_revision_at_another_place_leaves_the_earlier_file_on_disk(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part")
        .revision;
    let blob = blob_row(0x32);
    let m = measurements();
    revisions
        .record_revision(RevisionRequest {
            root: FileRoot::Inbox,
            storage_path: "uploads/vee-block-lp-3072-02 (2).stl",
            ..request(part, first, &blob, &m)
        })
        .await
        .expect("records the upload's revision");
    let history = revisions.history(part).await.expect("history");
    assert!(history[0].on_disk && history[1].on_disk, "{history:?}");
}
