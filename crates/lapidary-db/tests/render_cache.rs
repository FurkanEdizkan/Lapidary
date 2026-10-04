//! "Free cache space" (Phase 4 slice 2 spec §4, `DATA.md` §1.5): which derivatives are cache, and
//! what removing them does to the blobs they named.
//!
//! **The exports alone since R3.** `cache_kinds` named the L1 and L2 rungs too, and after the
//! pivot nothing can rebuild a rung — so evicting one would be data loss dressed as a cache sweep,
//! and the wording of that button is a product rule: eviction must never read as data loss.
//!
//! **And R4's `0055` deleted the rung rows**, so in a live database the table holds exports and
//! nothing else. The rung below is still seeded, by hand, after the migration has run: what is
//! being pinned is that these two queries select *by kind* rather than by "every row in the
//! table", which is the edit `0055` makes tempting and which would turn the button into an
//! unconditional purge of whatever a later slice stores here. That no such row survives a
//! migrated database is `migrations.rs`'s assertion, not this file's.

use lapidary_core::{BlobHash, DerivativeKind, LibraryId, MeshMeasurements, PartId, RevisionId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

async fn part(pool: &sqlx::PgPool, name: &str, source: u8) -> PartId {
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
            format: "stl",
        })
        .await
        .expect("records")
}

/// One export onto a part's latest revision, as a `derive` job files one. Ingest writes none.
async fn export(pool: &sqlx::PgPool, part: PartId, kind: DerivativeKind, seed: u8, stored: u64) {
    let revision: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM revision WHERE part_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(part.as_uuid())
            .fetch_one(pool)
            .await
            .expect("the revision");
    PgIngest(pool.clone())
        .upsert_derivative(
            RevisionId::from_uuid(revision),
            kind,
            &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: stored,
                stored_bytes: stored,
                zstd_level: 0,
            },
            None,
            "mesh stl-1+glb-2+cpu-1",
        )
        .await
        .expect("the export lands");
}

/// A row of a kind nothing can rebuild, written as raw SQL because nothing in this build can name
/// one. It must be left exactly where it is by every sweep: unrebuildable bytes would be the
/// user's data, not our cache, and `0055` is the only thing that has ever removed one.
async fn stale_rung(pool: &sqlx::PgPool, part: PartId, kind: &str, seed: u8, stored: u64) {
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
         VALUES ($2, $3, $3, NULL, 0) ON CONFLICT (blake3) DO NOTHING",
    )
    .bind(part.as_uuid())
    .bind(BlobHash::from_bytes([seed; 32]).to_hex())
    .bind(stored as i64)
    .execute(pool)
    .await
    .expect("the rung's blob row");
    sqlx::query(
        "WITH r AS (SELECT id FROM revision WHERE part_id = $1 ORDER BY id DESC LIMIT 1), \
         ins AS (INSERT INTO derivative (id, revision_id, kind, blake3, kernel_version, params_json) \
                 SELECT uuidv7(), r.id, $2, $3, 'mesh stl-1+glb-1+cpu-1', '{}'::jsonb FROM r \
                 RETURNING blake3) \
         UPDATE blob SET ref_count = ref_count + 1 WHERE blake3 = (SELECT blake3 FROM ins)",
    )
    .bind(part.as_uuid())
    .bind(kind)
    .bind(BlobHash::from_bytes([seed; 32]).to_hex())
    .execute(pool)
    .await
    .expect("the rung's row");
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

/// A bracket nobody has opened in months, carrying both exports and an L1 rung a pre-pivot
/// pipeline left, beside a spacer whose 3MF somebody downloaded yesterday.
#[sqlx::test(migrations = "./migrations")]
async fn only_old_exports_are_cache_and_an_unrebuildable_rung_is_left_alone(pool: sqlx::PgPool) {
    let bracket = part(&pool, "bracket-lp-1042-03.stl", 0xa9).await;
    export(&pool, bracket, DerivativeKind::ExportStl, 0xa1, 48_210).await;
    export(&pool, bracket, DerivativeKind::Export3mf, 0xa2, 12_405).await;
    stale_rung(&pool, bracket, "tessellation_l1", 0xa0, 9_140).await;
    let spacer = part(&pool, "hex-spacer-m4x20-lp-2145-01.stl", 0xb9).await;
    export(&pool, spacer, DerivativeKind::Export3mf, 0xb1, 51_002).await;
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
        .expect("somebody downloaded the spacer's 3MF yesterday");

    let parts = PgParts(pool.clone());
    assert_eq!(
        parts
            .instance_storage()
            .await
            .expect("reads")
            .render_cache_bytes,
        48_210 + 12_405,
        "the bracket's two exports only: its rung is not cache, and the spacer's export was read \
         yesterday"
    );

    let freed = parts.free_render_cache().await.expect("frees");
    assert_eq!(freed.removed, 2, "the bracket's two exports");
    assert_eq!(freed.quarantined_bytes, 48_210 + 12_405);

    assert_eq!(
        kinds(&pool, bracket).await,
        ["tessellation_l1"],
        "**the rung stays.** Nothing can rebuild it, so a sweep that took it would be deleting \
         the only copy of something a person might still hold the hash of"
    );
    assert_eq!(kinds(&pool, spacer).await, ["export_3mf"]);
    assert_eq!(
        blob(&pool, 0xa1).await,
        (0, true),
        "quarantined, not deleted"
    );
    assert_eq!(
        blob(&pool, 0xa2).await,
        (0, true),
        "the other export too: it is written again when next asked for"
    );
    assert_eq!(
        blob(&pool, 0xa0).await,
        (1, false),
        "and the rung's blob is neither counted down nor quarantined"
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
