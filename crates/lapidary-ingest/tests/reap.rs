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
            // This fixture stages its bytes through the content-addressed writer, so the
            // row it makes is one that has not migrated: no folder, no storage path. That
            // is the state quarantine was built against and the one this file is about.
            folder: None,
            storage_path: None,
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

// ---------------------------------------------------------------------------------------
// A purged model directory, which the sweep does not reach.
// ---------------------------------------------------------------------------------------

/// The same bytes, written where a real ingest writes them since the folder tree: inside
/// the model's own directory, at `file.storage_path`, rather than under the hash fan-out.
fn stage_bytes_at(blob_root: &Path, seed: u8, rel: &str) -> StoredBlobRow {
    let store = SourceStore::open(blob_root, &WorkerRole::assume());
    let stored = store
        .put_at(rel, &[seed; 4096], lapidary_storage::Compression::AsIs)
        .expect("stage a model file");
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

async fn seed_part_at(pool: &PgPool, blob_root: &Path, seed: u8, path: &str, rel: &str) -> PartId {
    let blob = stage_bytes_at(blob_root, seed, rel);
    // Ingest writes `metadata.json` beside the file it describes (handler step 10), and a
    // sweep that removed the file and left the manifest would leave an orphan directory.
    // The fixture has to leave one for that to be testable at all.
    let model_dir = rel
        .rsplit_once('/')
        .expect("a model path has a directory")
        .0;
    std::fs::write(
        blob_root.join(model_dir).join("metadata.json"),
        br#"{"schema":1}"#,
    )
    .expect("a manifest beside the model file");
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: path,
            folder: None,
            storage_path: Some(rel),
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
        .expect("seed a migrated part")
}

/// A purged model directory goes, and it is the ordinary case rather than the edge one:
/// every part ingested since the folder tree keeps its bytes at `file.storage_path`, not
/// under the hash fan-out the sweep used to be the whole of.
///
/// This replaces `a_purged_model_directory_outlives_the_sweep_that_reports_removing_it`,
/// which pinned the gap while it was open -- same fixture, opposite assertion. The file,
/// the `metadata.json` beside it and the directory that held them all go together, because
/// a directory holding a stale manifest and nothing else is an orphan every walk of the
/// store has to explain.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_purged_model_directory_is_removed_once_its_thirty_days_are_up(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/brackets/lp-1042-03/lp-1042-03.stl";
    let part = seed_part_at(
        &pool,
        blob_root.path(),
        0xc7,
        "brackets/LP-1042-03.stl",
        rel,
    )
    .await;
    assert!(
        blob_root.path().join(rel).exists(),
        "the fixture has to put a file where a migrated part keeps one"
    );
    assert!(
        !bytes_exist(blob_root.path(), 0xc7),
        "and must not also leave one under the hash, or this proves nothing"
    );

    retire(&pool, part).await;
    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");

    assert_eq!(
        report.removed_files,
        vec![rel.to_owned()],
        "the sweep names the path it removed, not only the hash"
    );
    assert!(
        !blob_root.path().join(rel).exists(),
        "the model file is gone"
    );
    assert!(
        !blob_root
            .path()
            .join(model_dir(rel))
            .join("metadata.json")
            .exists(),
        "and its manifest with it -- a directory holding only a stale manifest is an orphan"
    );
    assert!(
        !blob_root.path().join(model_dir(rel)).exists(),
        "and the directory that held them, because nothing else was in it"
    );
    assert_eq!(
        quarantined_paths(&pool).await,
        Vec::<String>::new(),
        "the row is gone, not merely swept past"
    );
}

/// The production retention against a file quarantined seconds ago: what every sweep on a
/// new installation does, and it must do nothing at all. The hash-addressed half has the
/// same test, and the two retentions are one constant on purpose -- a second one would be
/// a second promise to explain in the same confirmation dialog.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_model_file_inside_its_thirty_days_is_not_touched(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/brackets/lp-1042-03/lp-1042-03.stl";
    let part = seed_part_at(
        &pool,
        blob_root.path(),
        0xc8,
        "brackets/LP-1042-03.stl",
        rel,
    )
    .await;
    retire(&pool, part).await;
    assert_eq!(quarantined_paths(&pool).await, vec![rel.to_owned()]);

    let report =
        lapidary_ingest::reap::sweep(&pool, blob_root.path(), lapidary_ingest::reap::QUARANTINE)
            .await
            .expect("sweep");

    assert!(report.removed_files.is_empty(), "{report:?}");
    assert_eq!(report.bytes, 0);
    assert!(
        blob_root.path().join(rel).exists(),
        "the bytes are the promise: still there for thirty days"
    );
    assert_eq!(quarantined_paths(&pool).await, vec![rel.to_owned()]);
}

/// **The guard, and the one assertion that fails loudly if it is ever dropped.**
///
/// The hash-addressed half gets its safety from a foreign key -- `file.blake3` references
/// `blob`, so a referenced row cannot be deleted whatever a query says, and the
/// reachability check merely lets the sweep decline. Nothing references
/// `file.storage_path`, so there the `NOT EXISTS` is the only thing standing between a
/// live part and its bytes.
///
/// Reaching this state takes a person: `model_dir_for` disambiguates against a directory
/// that exists, so a re-ingest after a purge lands somewhere else. What puts a quarantined
/// path back in play is the owner removing that directory in a file manager and re-adding
/// the file -- which a browsable store invites, and is the whole reason the layout exists.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_path_a_live_part_has_claimed_again_is_declined_rather_than_unlinked(pool: PgPool) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let rel = "libraries/default/brackets/lp-1042-03/lp-1042-03.stl";
    let gone = seed_part_at(
        &pool,
        blob_root.path(),
        0xc9,
        "brackets/LP-1042-03.stl",
        rel,
    )
    .await;
    retire(&pool, gone).await;
    assert_eq!(quarantined_paths(&pool).await, vec![rel.to_owned()]);

    // A second part now holds that exact path -- different bytes, same place.
    seed_part_at(
        &pool,
        blob_root.path(),
        0xca,
        "brackets/LP-1042-03-v2.stl",
        rel,
    )
    .await;

    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("sweep");

    assert!(
        report.removed_files.is_empty(),
        "a path a live file row names must not be unlinked: {report:?}"
    );
    assert_eq!(
        report.un_quarantined, 1,
        "it is reported as reclaimed, the way a re-referenced blob is"
    );
    assert!(
        blob_root.path().join(rel).exists(),
        "and the live part still has its bytes"
    );
    assert_eq!(
        quarantined_paths(&pool).await,
        Vec::<String>::new(),
        "the row is dropped rather than un-flagged: there is no ref_count to correct and \
         no state to go back to, so a record saying these bytes are doomed is just wrong"
    );
}

/// A model directory the owner has put something of their own into: a note beside the
/// model, a re-exported STL, a photo. The model file goes; their file does not, the
/// directory does not, and -- the part that matters -- the sweep does not fail over it and
/// stop reaping everything else for as long as that file sits there.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_directory_holding_something_of_the_owners_survives_and_does_not_stop_the_sweep(
    pool: PgPool,
) {
    let blob_root = tempfile::tempdir().expect("temp dir");
    let kept = "libraries/default/brackets/lp-1042-03/lp-1042-03.stl";
    let alone = "libraries/default/pumps/lp-5501-02/lp-5501-02.stl";
    let with_note = seed_part_at(&pool, blob_root.path(), 0xcb, "brackets/a.stl", kept).await;
    let plain = seed_part_at(&pool, blob_root.path(), 0xcc, "pumps/b.stl", alone).await;

    let note = blob_root
        .path()
        .join(model_dir(kept))
        .join("print-settings.txt");
    std::fs::write(&note, b"0.2 mm layers, 15% gyroid, no supports\n").expect("the owner's note");

    retire(&pool, with_note).await;
    retire(&pool, plain).await;

    let report = lapidary_ingest::reap::sweep(&pool, blob_root.path(), Duration::ZERO)
        .await
        .expect("a directory that will not empty must not fail the sweep");

    let mut removed = report.removed_files.clone();
    removed.sort();
    let mut expected = vec![kept.to_owned(), alone.to_owned()];
    expected.sort();
    assert_eq!(
        removed, expected,
        "both model files are removed, including the one whose directory has to stay"
    );
    assert!(!blob_root.path().join(kept).exists(), "the model file goes");
    assert!(note.exists(), "the owner's file does not");
    assert!(
        blob_root.path().join(model_dir(kept)).exists(),
        "nor the directory holding it"
    );
    assert!(
        !blob_root.path().join(model_dir(alone)).exists(),
        "and the directory with nothing left in it still goes -- one awkward neighbour \
         must not stop the tidying everywhere else"
    );
}

/// The parent of a store-relative file path: the model's own directory.
fn model_dir(rel: &str) -> &str {
    rel.rsplit_once('/')
        .expect("a model path has a directory")
        .0
}

async fn quarantined_paths(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT storage_path FROM quarantined_file ORDER BY storage_path")
        .fetch_all(pool)
        .await
        .expect("quarantined paths read")
}
