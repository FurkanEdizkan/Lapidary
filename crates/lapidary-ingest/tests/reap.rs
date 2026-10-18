//! The thirty-day sweep.
//!
//! This is the only code in Lapidary that removes stored bytes, so the tests here are mostly
//! about what it declines to do. Since delete became final (2026-10-17) nothing queues a model
//! file — delete takes rows, never files — and since M4 the sweep has no way to name one: it
//! removes content-addressed blobs in Lapidary's own store and nothing else. `retire` deletes a
//! part and recounts its blob the way the old purge did.
//!
//! The retention is injected — a test that waited thirty days is a test that never runs — and the interesting cases are the ones where the database
//! and the disk disagree about what is safe to remove.

use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, PgPool, StoredBlobRow};
use lapidary_storage::DerivativeStore;
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
    let stored = DerivativeStore::open(blob_root)
        .put(&[seed; 4096])
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
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: path,
            // This fixture stages its bytes through the content-addressed writer, so the
            // row it makes is one that has not migrated: no folder, no storage path. That
            // is the state quarantine was built against and the one this file is about.
            folder: None,
            storage_path: None,
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            blob: &blob,
            measurements: Some(&MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            }),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("seed part")
}

/// What a purge left behind before delete became final: the part's rows gone, its bytes
/// queued for the sweep. Delete itself never queues a source file, so the queue is written here
/// by hand, exactly as the old purge wrote it — the sweep still drains such rows, and its blob
/// half is the same one cache eviction uses.
async fn retire(pool: &PgPool, part: PartId) {
    #[allow(clippy::type_complexity)]
    let files: Vec<String> = sqlx::query_scalar(
        "SELECT f.blake3 FROM file f \
         JOIN revision r ON r.id = f.revision_id WHERE r.part_id = $1",
    )
    .bind(part.as_uuid())
    .fetch_all(pool)
    .await
    .expect("the part's files");
    assert!(
        PgParts(pool.clone())
            .delete(part)
            .await
            .expect("delete")
            .is_some()
    );
    for hash in files {
        sqlx::query(
            "UPDATE blob SET ref_count = (SELECT count(*) FROM file WHERE blake3 = $1), \
                 quarantined_at = CASE WHEN EXISTS (SELECT 1 FROM file WHERE blake3 = $1) \
                                       THEN NULL ELSE coalesce(quarantined_at, now()) END \
             WHERE blake3 = $1",
        )
        .bind(&hash)
        .execute(pool)
        .await
        .expect("the old purge's recount");
    }
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

/// The same bytes at a path under `root` — a part from before M4 in the store's folder tree, or a
/// file in the owner's folders.
fn stage_bytes_at(root: &Path, seed: u8, rel: &str) -> StoredBlobRow {
    let file = root.join(rel);
    std::fs::create_dir_all(file.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&file, [seed; 4096]).expect("stage a model file");
    StoredBlobRow {
        hash: hash_of(seed),
        size_bytes: 4096,
        stored_bytes: 4096,
        zstd_level: 0,
    }
}

/// **Delete is delete, and the file stays** (owner, 2026-10-17). A filed part with a cached export
/// is deleted and the sweep then runs with no retention at all: the export, which Lapidary wrote
/// and can write again, enters quarantine and leaves the disk; the model file is never queued and
/// is still there, byte for byte.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_parts_model_file_survives_every_sweep_and_only_its_export_leaves(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/spacers/lp-2145-01/lp-2145-01.stl";
    let source = stage_bytes_at(blob_root.path(), 0xd1, rel);
    let rung_bytes = vec![0x77u8; 3_292];
    let rung = lapidary_storage::DerivativeStore::open(blob_root.path())
        .put(&rung_bytes)
        .expect("stores the rung");
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Hex spacer, M4x20, LP-2145-01",
            source_path: "spacers/LP-2145-01.stl",
            folder: None,
            storage_path: Some(rel),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            blob: &source,
            measurements: Some(&MeshMeasurements {
                bbox_mm: [7.0, 8.1, 20.0],
                triangle_count: 1_204,
                surface_area_mm2: 612.5,
                volume_mm3: Some(1_060.0),
                is_watertight: true,
            }),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("a filed part");
    // The derivative is written after the part now: ingest carries none since R3, and a derive
    // job is the only thing that files one. An export rather than a rung, because an export is
    // what a worker still builds.
    let revision: uuid::Uuid = sqlx::query_scalar("SELECT id FROM revision WHERE part_id = $1")
        .bind(part.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the revision");
    PgIngest(pool.clone())
        .upsert_derivative(
            lapidary_core::RevisionId::from_uuid(revision),
            lapidary_core::DerivativeKind::ExportStl,
            &StoredBlobRow {
                hash: rung.hash,
                size_bytes: rung.size_bytes,
                stored_bytes: rung.stored_bytes,
                zstd_level: rung.zstd_level,
            },
            None,
            "mesh stl-1+glb-1+cpu-1",
        )
        .await
        .expect("the export lands");

    let parts = PgParts(pool.clone());
    assert!(parts.delete(part).await.expect("delete").is_some());
    assert_eq!(
        parts
            .instance_storage()
            .await
            .expect("totals")
            .quarantined_bytes,
        rung.stored_bytes,
        "only the export is queued, never the model file"
    );

    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");
    assert_eq!(report.bytes, rung.stored_bytes, "the export left");

    assert_eq!(
        std::fs::read(blob_root.path().join(rel)).expect("the model file is still there"),
        vec![0xd1u8; 4096],
        "and it is byte for byte what it was"
    );
}

/// (b) A deleted part's file in the owner's folders: delete takes rows, and no sweep, however
/// short its hold, can reach anything outside Lapidary's own store. Every file, size, time and
/// byte under the folder is what it was.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_deleted_parts_file_in_the_owners_folders_is_beyond_every_sweep(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("the store");
    let folders = tempfile::tempdir().expect("the owner's folders");
    let rel = "Spacers/LP-2145-01.stl";
    let source = stage_bytes_at(folders.path(), 0xd2, rel);
    let before = listing(folders.path());
    let part = PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Hex spacer, M4x20, LP-2145-01",
            source_path: rel,
            folder: None,
            storage_path: Some(rel),
            root: lapidary_core::FileRoot::Library,
            mtime_ns: Some(1_760_000_000_000_000_000),
            blob: &source,
            measurements: Some(&MeshMeasurements {
                bbox_mm: [7.0, 8.1, 20.0],
                triangle_count: 1_204,
                surface_area_mm2: 612.5,
                volume_mm3: Some(1_060.0),
                is_watertight: true,
            }),
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("a part in the owner's folders");
    retire(&pool, part).await;
    lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");
    assert_eq!(listing(folders.path()), before);
}

/// Every file under `root` with its size, modification time and bytes.
fn listing(root: &Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime, Vec<u8>)> {
    let mut seen = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir).expect("reads") {
            let path = entry.expect("an entry").path();
            let meta = std::fs::metadata(&path).expect("stats");
            if meta.is_dir() {
                queue.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("reads");
                seen.push((path, meta.len(), meta.modified().expect("mtime"), bytes));
            }
        }
    }
    seen.sort();
    seen
}
