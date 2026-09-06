//! The thirty-day sweep.
//!
//! This is the only code in Lapidary that destroys user data, so the tests here are mostly
//! about what it declines to do. The retention is injected — a test that waited thirty days
//! is a test that never runs — and the interesting cases are the ones where the database
//! and the disk disagree about what is safe to remove.

use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, PgPool, Purged, StoredBlobRow};
use lapidary_storage::{SourceStore, WorkerRole};
use std::path::Path;
use std::time::Duration;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// The real digest of the bytes `stage_bytes` writes, not a stand-in. A row whose hash
/// does not address the file on disk would let every assertion below pass while the sweep
/// unlinked nothing — the failure this whole file exists to catch.
fn hash_of(seed: u8) -> BlobHash {
    BlobHash::from_bytes(*blake3::hash(&[seed; 4096]).as_bytes())
}

/// Bytes on disk as well as a row, because a reaper tested only against rows is a reaper
/// whose unlink is never exercised. `put` writes through the real sharded layout, so what
/// the sweep removes is what a real ingest wrote.
fn stage_bytes(blob_root: &Path, seed: u8) -> StoredBlobRow {
    let store = SourceStore::open(blob_root, &WorkerRole::assume());
    let stored = store
        .put(&[seed; 4096], lapidary_storage::Compression::AsIs)
        .expect("stage blob bytes");
    assert_eq!(
        stored.hash,
        hash_of(seed),
        "the fixture addresses its own bytes"
    );
    StoredBlobRow {
        hash: stored.hash,
        size_bytes: 4096,
        stored_bytes: stored.stored_bytes,
        zstd_level: 0,
    }
}

async fn seed_part(pool: &PgPool, blob_root: &Path, seed: u8, path: &str) -> PartId {
    let blob = stage_bytes(blob_root, seed);
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: path,
            blob: &blob,
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            thumbnail_webp: Some(b"webp-preview"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed part")
}

/// Delete then purge, which is the only way a blob reaches quarantine.
async fn retire(pool: &PgPool, part: PartId) {
    let parts = PgParts(pool.clone());
    assert!(parts.soft_delete(part).await.expect("soft delete"));
    assert!(matches!(
        parts.purge(part).await.expect("purge"),
        Purged::Done(_)
    ));
}

fn bytes_exist(blob_root: &Path, seed: u8) -> bool {
    let hex = hash_of(seed).to_hex();
    blob_root
        .join("blobs")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex)
        .exists()
}

async fn quarantined(pool: &PgPool, seed: u8) -> Option<bool> {
    sqlx::query_scalar("SELECT quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(hash_of(seed).to_hex())
        .fetch_optional(pool)
        .await
        .expect("blob state reads")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_inside_its_thirty_days_is_not_touched(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let part = seed_part(&pool, blob_root.path(), 0xa1, "brackets/LP-1042-03.stl").await;
    retire(&pool, part).await;
    assert_eq!(quarantined(&pool, 0xa1).await, Some(true));

    // The production retention, against a blob quarantined seconds ago. This is what every
    // sweep on a new installation does, and it must do nothing at all.
    let report =
        lapidary_ingest::reap::sweep(&pool, blob_root.path(), lapidary_ingest::reap::QUARANTINE)
            .await
            .expect("sweep");
    assert!(report.removed.is_empty(), "{report:?}");
    assert_eq!(report.bytes, 0);
    assert_eq!(quarantined(&pool, 0xa1).await, Some(true));
    assert!(
        bytes_exist(blob_root.path(), 0xa1),
        "the bytes are the promise: reachable by hash for thirty days"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_past_its_cutoff_loses_its_row_and_its_bytes(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let part = seed_part(&pool, blob_root.path(), 0xb1, "brackets/LP-1042-03.stl").await;
    let stored = stage_bytes(blob_root.path(), 0xb1).stored_bytes;
    retire(&pool, part).await;

    // Zero retention: everything already quarantined is past its cutoff. The clock is the
    // only thing being skipped — every other check runs exactly as it does in production.
    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");
    assert_eq!(report.removed, vec![hash_of(0xb1)]);
    assert_eq!(report.bytes, stored);
    assert_eq!(
        quarantined(&pool, 0xb1).await,
        None,
        "the row is gone, not merely un-flagged"
    );
    assert!(!bytes_exist(blob_root.path(), 0xb1));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_blob_something_points_at_again_is_un_quarantined_rather_than_removed(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let part = seed_part(&pool, blob_root.path(), 0xc1, "brackets/LP-1042-03.stl").await;
    retire(&pool, part).await;
    assert_eq!(quarantined(&pool, 0xc1).await, Some(true));

    // Re-ingested: the person deleted it by mistake and re-scanned the folder. `record`
    // and not `link_existing`, because the blob row survived its own quarantine and
    // `ON CONFLICT DO NOTHING` leaves it exactly where it was.
    seed_part(&pool, blob_root.path(), 0xc1, "brackets/LP-1042-03.stl").await;

    // Past the cutoff, and it still must not be removed: the decision is `NOT EXISTS`
    // against the tables that hold the references, never `ref_count`, and there is a `file`
    // row again.
    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");
    assert!(
        report.removed.is_empty(),
        "bytes a part points at must survive their own cutoff: {report:?}"
    );
    assert!(bytes_exist(blob_root.path(), 0xc1));
    assert_eq!(
        quarantined(&pool, 0xc1).await,
        Some(false),
        "and the clock is cleared, not merely ignored"
    );
    // `un_quarantined` is 0 here and that is not a weaker result: the ingest above already
    // cleared the flag on the row it was incrementing anyway, so the sweep found nothing
    // left to clear. The sweep's own clearing still matters for a flag no ingest touched —
    // `one_wrongly_quarantined_blob_does_not_stop_the_sweep` is where that is asserted.
    assert_eq!(report.un_quarantined, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_sweep_ignores_ref_count_entirely(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let part = seed_part(&pool, blob_root.path(), 0xd1, "brackets/LP-1042-03.stl").await;
    retire(&pool, part).await;

    // A counter that drifted high on a blob nothing points at. If the sweep consulted it,
    // these bytes would be immortal — which is the harmless direction, and still wrong.
    sqlx::query("UPDATE blob SET ref_count = 9 WHERE blake3 = $1")
        .bind(hash_of(0xd1).to_hex())
        .execute(&pool)
        .await
        .expect("drift the counter");
    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");
    assert_eq!(
        report.removed,
        vec![hash_of(0xd1)],
        "reachability decides, not the counter"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_live_parts_bytes_survive_a_counter_that_says_nothing_points_at_them(pool: PgPool) {
    // The dangerous direction, and the reason the `NOT EXISTS` pair is redundant with
    // `ref_count` rather than replaced by it. A live part's blob whose counter was
    // corrupted to zero and whose flag was set by hand: nothing upstream would catch this,
    // and the sweep must still refuse. It is the one test in this file whose failure would
    // mean lost work rather than wasted disk.
    let blob_root = tempfile::tempdir().expect("temp dir");
    let live = seed_part(&pool, blob_root.path(), 0xd2, "brackets/LP-2210-01.stl").await;
    sqlx::query("UPDATE blob SET ref_count = 0, quarantined_at = now() WHERE blake3 = $1")
        .bind(hash_of(0xd2).to_hex())
        .execute(&pool)
        .await
        .expect("corrupt the counter");
    // Deliberately not `.expect("sweep")`. What must hold is that the bytes survive, and
    // there are three independent reasons they do: the `NOT EXISTS` pair declines the row;
    // `file.blake3`'s foreign key would refuse the delete even if that pair were dropped;
    // and the unlink happens inside the transaction, so a refused delete never reaches it.
    // A test that demanded `Ok` would be asserting which of the three fired, and the point
    // is that no single one of them has to be the one that does.
    let outcome = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO).await;
    if let Ok(report) = &outcome {
        assert!(
            report.removed.is_empty(),
            "a live part's bytes were removed on the strength of a wrong counter: {report:?}"
        );
    }
    assert!(
        bytes_exist(blob_root.path(), 0xd2),
        "a live part's bytes are gone from disk (sweep returned {outcome:?})"
    );
    // The part is still whole, which is the assertion a user would care about.
    assert!(
        PgParts(pool.clone())
            .detail(live)
            .await
            .expect("detail")
            .is_some()
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn one_wrongly_quarantined_blob_does_not_stop_the_sweep(pool: PgPool) {
    // What the `NOT EXISTS` pair is actually for.
    //
    // It is *not* what keeps a referenced blob's bytes safe — `file.blake3` and
    // `derivative.blake3` both have foreign keys to `blob`, so deleting a referenced row
    // raises a constraint violation whatever the `WHERE` clause says. The bytes survive
    // either way.
    //
    // What it buys is that the sweep declines that row instead of failing on it. The whole
    // sweep is one transaction, so a single wrongly-quarantined blob would otherwise roll
    // back every legitimate removal alongside it — and it would do that again every hour,
    // forever, because nothing about the bad row heals on its own. Quarantine would stop
    // collecting anything at all, and the only symptom would be a line in a log.
    let blob_root = tempfile::tempdir().expect("temp dir");

    // One blob that genuinely should go, and one that must not: a live part's, flagged and
    // zeroed by hand the way a drift would leave it.
    let doomed = seed_part(&pool, blob_root.path(), 0xe1, "brackets/LP-1042-03.stl").await;
    retire(&pool, doomed).await;
    seed_part(&pool, blob_root.path(), 0xe2, "brackets/LP-2210-01.stl").await;
    sqlx::query("UPDATE blob SET ref_count = 0, quarantined_at = now() WHERE blake3 = $1")
        .bind(hash_of(0xe2).to_hex())
        .execute(&pool)
        .await
        .expect("corrupt the counter");

    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("the sweep must survive a blob it cannot remove");
    assert_eq!(
        report.removed,
        vec![hash_of(0xe1)],
        "the collectable blob was collected in the same run as the un-collectable one"
    );
    assert!(!bytes_exist(blob_root.path(), 0xe1));
    assert!(bytes_exist(blob_root.path(), 0xe2));
    assert_eq!(
        quarantined(&pool, 0xe2).await,
        Some(false),
        "and the bad row heals: it is un-quarantined, so it is not a candidate next hour"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn re_ingesting_quarantined_bytes_clears_the_flag_without_waiting_for_a_sweep(pool: PgPool) {
    // Found by running the thing rather than by a test: re-scanning a purged file brought
    // the part back and put `ref_count` at 1, and the blob stayed flagged until the next
    // hourly sweep. Never unsafe — the reaper re-checks reachability and declines a
    // referenced blob — but it left "a referenced blob is never quarantined" true only
    // eventually, and an operator reading the column would have seen a lie for an hour.
    //
    // The fix rides on the `ref_count + 1` statement that every ingest already runs against
    // that row, so it costs nothing.
    let blob_root = tempfile::tempdir().expect("temp dir");
    let part = seed_part(&pool, blob_root.path(), 0xf1, "brackets/LP-1042-03.stl").await;
    retire(&pool, part).await;
    assert_eq!(quarantined(&pool, 0xf1).await, Some(true));

    seed_part(&pool, blob_root.path(), 0xf1, "brackets/LP-1042-03.stl").await;
    assert_eq!(
        quarantined(&pool, 0xf1).await,
        Some(false),
        "the flag must be gone the moment the reference exists, not an hour later"
    );
    assert!(bytes_exist(blob_root.path(), 0xf1));
}
