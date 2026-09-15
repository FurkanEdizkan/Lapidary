//! "Free cache space" (Phase 4 slice 2 spec §4, `DATA.md` §1.5): which rungs are cache, and what
//! removing them does to the blobs they named.

use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow, TessellationRow};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn rung(kind: &'static str, seed: u8, stored: u64) -> TessellationRow<'static> {
    TessellationRow {
        kind,
        blob: StoredBlobRow {
            hash: BlobHash::from_bytes([seed; 32]),
            size_bytes: stored,
            stored_bytes: stored,
            zstd_level: 0,
        },
        grid: Some(64),
    }
}

async fn part(
    pool: &sqlx::PgPool,
    name: &str,
    source: u8,
    rungs: &[TessellationRow<'_>],
) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            folder: None,
            storage_path: None,
            library: LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("uuid")),
            name,
            source_path: name,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([source; 32]),
                size_bytes: 204_800,
                stored_bytes: 204_800,
                zstd_level: 0,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: Some(b"the-thumbnail"),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: rungs,
        })
        .await
        .expect("records")
}

async fn kinds(pool: &sqlx::PgPool, part: PartId) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT d.kind FROM derivative d JOIN revision r ON r.id = d.revision_id \
         WHERE r.part_id = $1 ORDER BY d.kind",
    )
    .bind(part.as_uuid())
    .fetch_all(pool)
    .await
    .expect("kinds")
}

async fn blob(pool: &sqlx::PgPool, seed: u8) -> (i32, bool) {
    sqlx::query_as("SELECT ref_count, quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([seed; 32]).to_hex())
        .fetch_one(pool)
        .await
        .expect("the blob row is still there")
}

/// A bracket nobody has opened in months, whose L2 is the same bytes as its L0 (a small part's
/// rungs often are) and whose 3MF was written for a slicer, beside a spacer whose L1 somebody
/// read yesterday.
#[sqlx::test(migrations = "./migrations")]
async fn only_old_rungs_nothing_else_needs_are_cache_and_they_go_into_quarantine(
    pool: sqlx::PgPool,
) {
    let bracket = part(
        &pool,
        "bracket-lp-1042-03.stl",
        0xa9,
        &[
            rung("tessellation_l0", 0xa0, 9_140),
            rung("tessellation_l1", 0xa1, 48_210),
            rung("tessellation_l2", 0xa0, 9_140),
            rung("export_3mf", 0xa2, 12_405),
        ],
    )
    .await;
    let spacer = part(
        &pool,
        "hex-spacer-m4x20-lp-2145-01.stl",
        0xb9,
        &[
            rung("tessellation_l0", 0xb0, 3_016),
            rung("tessellation_l1", 0xb1, 51_002),
        ],
    )
    .await;
    sqlx::query(
        "UPDATE blob SET created_at = now() - interval '120 days', last_accessed_at = NULL",
    )
    .execute(&pool)
    .await
    .expect("ages every blob");
    sqlx::query("UPDATE blob SET last_accessed_at = now() - interval '1 day' WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([0xb1; 32]).to_hex())
        .execute(&pool)
        .await
        .expect("somebody opened the spacer yesterday");

    let parts = PgParts(pool.clone());
    assert_eq!(
        parts
            .instance_storage()
            .await
            .expect("reads")
            .render_cache_bytes,
        48_210 + 12_405,
        "the bracket's L1 and 3MF only: its L2 shares L0's blob, and the spacer's L1 was read yesterday"
    );

    let freed = parts.free_render_cache().await.expect("frees");
    assert_eq!(
        freed.rungs, 2,
        "the bracket's L1 and 3MF only: its L2 shares L0's blob, which the figure never counted, so it stays"
    );
    assert_eq!(freed.quarantined_bytes, 48_210 + 12_405);

    assert_eq!(
        kinds(&pool, bracket).await,
        ["tessellation_l0", "tessellation_l2", "thumbnail"]
    );
    assert_eq!(
        kinds(&pool, spacer).await,
        ["tessellation_l0", "tessellation_l1", "thumbnail"]
    );
    assert_eq!(
        blob(&pool, 0xa1).await,
        (0, true),
        "quarantined, not deleted"
    );
    assert_eq!(
        blob(&pool, 0xa2).await,
        (0, true),
        "the export too: it is written again when next asked for"
    );
    assert_eq!(
        blob(&pool, 0xa0).await,
        (2, false),
        "L0 and the L2 that shares its bytes both still point at it"
    );
    assert_eq!(
        blob(&pool, 0xa9).await,
        (1, false),
        "the source file is untouched"
    );
    assert_eq!(
        parts
            .instance_storage()
            .await
            .expect("reads")
            .render_cache_bytes,
        0
    );
}
