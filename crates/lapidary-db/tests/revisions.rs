//! `PgRevisions`: the next revision is recorded against the current one, with its files
//! moved inside the transaction, or not at all.

use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, RevisionOrigin};
use lapidary_db::{
    DbError, IngestRequest, PgBlobs, PgIngest, PgParts, PgRevisions, RevisionRequest, StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const PATH: &str = "vee-block-lp-3072-02.stl";
const STORED: &str = "libraries/default/vee-block-lp-3072-02/vee-block-lp-3072-02.stl";
const ASIDE: &str = "libraries/default/vee-block-lp-3072-02/revisions/1/vee-block-lp-3072-02.stl";

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
            blob: &blob_row(0x31),
            measurements: &measurements(),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: Some(b"webp-preview"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
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
        lock: None,
        blob,
        measurements,
        provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
        thumbnail_webp: None,
        kernel_version: "mesh stl-1+cpu-1",
        format: "stl",
        tessellations: &[],
    }
}

/// `metadata.json` is read while the part's row is held: a custom value being written waits the read out,
/// and the manifest handed over holds that value, so the file is never written from rows a commit replaced.
#[sqlx::test(migrations = "./migrations")]
async fn a_manifest_waits_for_a_change_to_its_part_and_holds_it(pool: sqlx::PgPool) {
    let part = seed(&pool).await;
    let mut writing = pool.begin().await.expect("begins");
    sqlx::query(
        "UPDATE part SET metadata_json = jsonb_set(metadata_json, '{custom}', '{\"supplier\": \"Misumi\"}') WHERE id = $1",
    )
    .bind(part.as_uuid())
    .execute(&mut *writing)
    .await
    .expect("writes Misumi");
    let describing = tokio::spawn({
        let revisions = PgRevisions(pool.clone());
        async move { revisions.write_manifest(part, |manifest| manifest).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !describing.is_finished(),
        "the manifest waits for the value's commit"
    );
    writing.commit().await.expect("commits");

    let manifest = describing
        .await
        .expect("joins")
        .expect("reads")
        .expect("the part exists");
    assert_eq!(
        manifest.part.metadata["custom"],
        serde_json::json!({ "supplier": "Misumi" })
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_revision_goes_on_top_and_the_previous_file_is_set_aside_under_its_own_label(
    pool: sqlx::PgPool,
) {
    let part = seed(&pool).await;
    let revisions = PgRevisions(pool.clone());
    let first = revisions
        .current(library(), PATH)
        .await
        .expect("reads")
        .expect("the seeded part");
    let blob = blob_row(0x32);
    let m = measurements();

    let mut moved = None;
    let second = revisions
        .record_revision(
            request(part, first.revision, &blob, &m),
            |current, aside| {
                moved = Some((current.to_owned(), aside.to_owned()));
                Ok(())
            },
        )
        .await
        .expect("records the second revision");
    assert_eq!(
        moved,
        Some((STORED.to_owned(), ASIDE.to_owned())),
        "the closure is told where the current file is and where it goes"
    );

    let history = revisions.history(part).await.expect("history");
    let labels: Vec<&str> = history.iter().map(|r| r.rev_label.as_str()).collect();
    assert_eq!(labels, ["2", "1"], "newest first");
    assert_eq!(history[0].id, second);
    assert_eq!(history[0].parent, Some(first.revision));
    assert_eq!(history[0].origin, RevisionOrigin::Upload);
    assert_eq!(history[1].origin, RevisionOrigin::Ingest);
    assert_eq!(history[0].storage_path.as_deref(), Some(STORED));
    assert_eq!(history[1].storage_path.as_deref(), Some(ASIDE));
    assert_eq!(history[1].thumbnail.as_deref(), Some(&b"webp-preview"[..]));

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
async fn a_parent_that_is_no_longer_current_is_a_conflict_and_moves_no_file(pool: sqlx::PgPool) {
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
        .record_revision(request(part, first, &blob_row(0x32), &m), |_, _| Ok(()))
        .await
        .expect("the second revision");

    let mut ran = false;
    let error = revisions
        .record_revision(request(part, first, &blob_row(0x33), &m), |_, _| {
            ran = true;
            Ok(())
        })
        .await
        .expect_err("revision 1 is no longer current");
    assert!(
        matches!(error, DbError::RevisionConflict { .. }),
        "got {error:?}"
    );
    assert!(!ran, "no file may move for a refused revision");
    assert_eq!(revisions.history(part).await.expect("history").len(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_failed_file_move_records_nothing(pool: sqlx::PgPool) {
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

    let error = revisions
        .record_revision(request(part, first, &blob, &m), |_, _| {
            Err("No space left on device".to_owned())
        })
        .await
        .expect_err("the move failed");
    assert!(
        matches!(&error, DbError::RevisionFilesFailed { detail } if detail == "No space left on device"),
        "got {error:?}"
    );

    let history = revisions.history(part).await.expect("history");
    assert_eq!(history.len(), 1, "no second revision");
    assert_eq!(
        history[0].storage_path.as_deref(),
        Some(STORED),
        "the previous file's row still names where it is"
    );
    assert!(
        !PgBlobs(pool.clone())
            .exists(&blob.hash)
            .await
            .expect("reads"),
        "and no blob row for bytes that were never kept"
    );
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
            .soft_delete(part)
            .await
            .expect("soft delete")
    );

    let mut ran = false;
    let m = measurements();
    let error = revisions
        .record_revision(request(part, first, &blob_row(0x32), &m), |_, _| {
            ran = true;
            Ok(())
        })
        .await
        .expect_err("a deleted part takes no revision");
    assert!(
        matches!(error, DbError::RevisionConflict { .. }),
        "got {error:?}"
    );
    assert!(!ran);
    assert!(
        revisions
            .current(library(), PATH)
            .await
            .expect("reads")
            .expect("still there")
            .deleted,
        "and the retry sees it deleted"
    );
}
