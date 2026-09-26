//! Shape profiles, exercised the way the worker exercises them: through `handle`, through a real
//! ingest, and through the sweep a worker runs as it starts.
//!
//! The arithmetic itself is `lapidary-cad`'s to test, and it does — invariance, the noise floor,
//! the degenerate cases. What is tested here is the plumbing around it: that an ingest leaves a
//! row, that a rotated copy of one part is proposed as its near-duplicate, that a missing row comes
//! back identical, that a new revision moves it, and that a revision with no viewer mesh fails once
//! and says why rather than being retried for ever.

use lapidary_cad::{Mesh, parse_stl, write_stl};
use lapidary_core::{
    BatchId, JobId, JobPayload, LibraryId, Outcome, PartId, RevisionId, SHAPE_VERSION,
    shape::is_near_duplicate,
};
use lapidary_db::{JobRow, PgShapes, ShapeRow};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
use std::path::Path;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &str = "brackets/bracket-lp-1042-03.stl";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
const SPACER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/spacer-lp-2001-00.stl");

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

fn handler_over(pool: &PgPool, ingest_dir: &Path, blob_root: &Path) -> WorkerHandler {
    WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
        cad: None,
    }
}

fn job_row(library: LibraryId, payload: &JobPayload) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: library,
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

fn ingest_job(path: &str) -> JobRow {
    job_row(
        seeded(),
        &JobPayload::IngestFile {
            path: path.to_owned(),
        },
    )
}

fn stage(dir: &Path, at: &str, bytes: &[u8]) {
    let path = dir.join(at);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("stages the directory");
    }
    std::fs::write(path, bytes).expect("stages the fixture");
}

/// The bracket fixture turned 37° about (1, 2, 3), written back out as binary STL: the same part,
/// exported from a CAD tool in another orientation, which is the duplicate a person most often has
/// twice without knowing it.
fn turned_bracket() -> Vec<u8> {
    let mesh = parse_stl(BRACKET_FIXTURE).expect("the bracket fixture parses");
    let n = 14.0f64.sqrt();
    let (a, b, c) = (1.0 / n, 2.0 / n, 3.0 / n);
    let (s, t) = (0.6458f64.sin(), 0.6458f64.cos());
    let k = 1.0 - t;
    let m = [
        [t + a * a * k, a * b * k - c * s, a * c * k + b * s],
        [b * a * k + c * s, t + b * b * k, b * c * k - a * s],
        [c * a * k - b * s, c * b * k + a * s, t + c * c * k],
    ];
    let turn = |v: [f32; 3]| {
        let v = v.map(f64::from);
        [0, 1, 2].map(|row| (m[row][0] * v[0] + m[row][1] * v[1] + m[row][2] * v[2]) as f32)
    };
    write_stl(&Mesh {
        triangles: mesh
            .triangles
            .iter()
            .map(|t| [turn(t[0]), turn(t[1]), turn(t[2])])
            .collect(),
        parts: vec![],
    })
    .expect("writes an STL")
}

async fn only_part(pool: &PgPool) -> PartId {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM part")
        .fetch_one(pool)
        .await
        .expect("exactly one part");
    PartId::from_uuid(id)
}

async fn part_at(pool: &PgPool, source_path: &str) -> PartId {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM part WHERE source_path = $1")
        .bind(source_path)
        .fetch_one(pool)
        .await
        .expect("a part at this path");
    PartId::from_uuid(id)
}

async fn latest_revision(pool: &PgPool, part: PartId) -> RevisionId {
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM revision WHERE part_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(part.as_uuid())
    .fetch_one(pool)
    .await
    .expect("a revision");
    RevisionId::from_uuid(id)
}

async fn shape_of(pool: &PgPool, part: PartId) -> ShapeRow {
    PgShapes(pool.clone())
        .of_part(part)
        .await
        .expect("the profile reads back")
        .unwrap_or_else(|| panic!("part {part} has no shape profile"))
}

/// The `profile_shape` job the worker's start-up sweep queued, as a row the handler can run.
async fn queued_profile_job(pool: &PgPool) -> JobRow {
    let (id, library, payload): (Uuid, Uuid, serde_json::Value) = sqlx::query_as(
        "SELECT id, library_id, payload FROM job WHERE kind = 'profile_shape' \
         AND state = 'pending' ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("the sweep queued a profile_shape job");
    JobRow {
        id: JobId::from_uuid(id),
        batch_id: BatchId::new(),
        library_id: LibraryId::from_uuid(library),
        kind: JobPayload::PROFILE_SHAPE.to_owned(),
        payload,
        attempts: 1,
        max_attempts: 3,
    }
}

async fn profile_jobs(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'profile_shape'")
        .fetch_one(pool)
        .await
        .expect("count query")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn ingesting_a_part_records_the_shape_of_the_rung_it_wrote(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);

    assert_eq!(
        handler.handle(&ingest_job(BRACKET)).await.expect("ingests"),
        Outcome::Ingested
    );

    let part = only_part(&pool).await;
    let row = shape_of(&pool, part).await;
    assert_eq!(
        row.version, SHAPE_VERSION,
        "stamped with this build's version"
    );
    assert_eq!(
        row.revision,
        latest_revision(&pool, part).await,
        "against the revision whose rung it was computed from"
    );
    let l0: String = sqlx::query_scalar(
        "SELECT blake3 FROM derivative WHERE kind = 'tessellation_l0' AND blake3 IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("ingest wrote an L0 rung");
    assert_eq!(
        row.l0.to_hex(),
        l0,
        "and from that rung's bytes, so a rebuilt rung makes the row stale"
    );
    assert!(
        row.profile.size_mm > 1.0 && row.profile.size_mm < 200.0,
        "the bracket is 88x40x25 mm, so its mean surface-point distance is tens of mm, got {}",
        row.profile.size_mm
    );
    // The D2 block holds square roots of probabilities, so its squares sum to 1.
    let mass: f32 = row.profile.descriptor[..32].iter().map(|p| p * p).sum();
    assert!(
        (mass - 1.0).abs() < 1e-5,
        "the 32 D2 bins must hold a whole distribution, got {mass}"
    );
    assert_eq!(
        profile_jobs(&pool).await,
        0,
        "ingest profiles in line; the job kind is for backfills"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_part_exported_the_other_way_up_is_a_near_duplicate(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    const TURNED: &str = "brackets/spares/bracket-lp-1042-03-rotated.stl";
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    stage(ingest_dir.path(), TURNED, &turned_bracket());

    for path in [BRACKET, TURNED] {
        assert_eq!(
            handler.handle(&ingest_job(path)).await.expect("ingests"),
            Outcome::Ingested,
            "{path} is a part of its own: other bytes at another path"
        );
    }

    let upright = shape_of(&pool, part_at(&pool, BRACKET).await).await.profile;
    let turned = shape_of(&pool, part_at(&pool, TURNED).await).await.profile;
    assert!(
        is_near_duplicate(&upright, &turned),
        "one bracket at two orientations must be proposed as a near-duplicate: {} apart, \
         sizes {} and {}",
        lapidary_core::shape::distance(&upright.descriptor, &turned.descriptor),
        upright.size_mm,
        turned.size_mm
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_whose_row_went_missing_gets_an_identical_one_when_a_worker_starts(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");

    let part = only_part(&pool).await;
    let before = shape_of(&pool, part).await;
    sqlx::query("DELETE FROM part_shape")
        .execute(&pool)
        .await
        .expect("takes the row away");

    // What a worker does as it comes up.
    handler.enqueue_stale_derivatives().await;
    let job = queued_profile_job(&pool).await;
    assert_eq!(
        handler.handle(&job).await.expect("profiles"),
        Outcome::Profiled,
        "a profile job is its own outcome: it indexes nothing and renders nothing"
    );

    let after = shape_of(&pool, part).await;
    assert_eq!(
        before.profile.descriptor.map(f32::to_bits),
        after.profile.descriptor.map(f32::to_bits),
        "the same rung must give the same 35 floats, bit for bit: a stored profile is compared \
         against a freshly computed one"
    );
    assert_eq!(before.profile.size_mm, after.profile.size_mm);
    assert_eq!((before.revision, before.l0), (after.revision, after.l0));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_current_profile_is_not_queued_again(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");

    handler.enqueue_stale_derivatives().await;
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        profile_jobs(&pool).await,
        0,
        "a part whose row names this build's version, its latest revision and its current rung \
         is not stale, and a sweep that queued it would queue every part on every start"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_new_revision_moves_the_row_to_its_own_rung(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    sqlx::query("UPDATE library SET mode = 'controlled' WHERE id = $1")
        .bind(seeded().as_uuid())
        .execute(&pool)
        .await
        .expect("switches the library to controlled");

    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");
    let part = only_part(&pool).await;
    let first = shape_of(&pool, part).await;

    // Other bytes at the same path: a second revision of the one part, and a different shape.
    stage(ingest_dir.path(), BRACKET, SPACER_FIXTURE);
    assert_eq!(
        handler.handle(&ingest_job(BRACKET)).await.expect("revises"),
        Outcome::Revised
    );

    let second = shape_of(&pool, part).await;
    assert_eq!(
        second.revision,
        latest_revision(&pool, part).await,
        "the row follows the part's current revision"
    );
    assert!(
        second.revision.as_uuid() > first.revision.as_uuid(),
        "and that is the newer one: revision ids are UUID v7 and sort by time"
    );
    assert_ne!(second.l0, first.l0, "computed from the new revision's rung");
    assert!(
        !is_near_duplicate(&first.profile, &second.profile),
        "a bracket and a spacer are not one shape"
    );
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        profile_jobs(&pool).await,
        0,
        "and the revision left nothing for the sweep to do"
    );

    // The sweep's `revision_id` branch, which needs two revisions for a row to be behind: a row
    // left at revision 1 while revision 2 is current is stale, whatever its version and rung say.
    sqlx::query("UPDATE part_shape SET revision_id = $1")
        .bind(first.revision.as_uuid())
        .execute(&pool)
        .await
        .expect("puts the row back a revision");
    handler.enqueue_stale_derivatives().await;
    assert_eq!(
        profile_jobs(&pool).await,
        1,
        "a row naming a revision that is no longer the latest must be swept up"
    );
    let job = queued_profile_job(&pool).await;
    handler.handle(&job).await.expect("profiles");
    assert_eq!(
        shape_of(&pool, part).await.revision,
        second.revision,
        "and the row it writes names the current revision again"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_revision_with_no_viewer_mesh_fails_once_and_says_what_to_do(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");
    let revision = latest_revision(&pool, only_part(&pool).await).await;

    // First, while the rung is still there: the same revision named by another library's job. A
    // revision id is a uuid a caller might hold from anywhere, and content addressing is not
    // authorization — asked before the rung is deleted, because a missing rung would refuse this
    // job for the wrong reason and prove nothing.
    let elsewhere = LibraryId::from_uuid(
        Uuid::parse_str("01931b6e-0000-7000-8000-0000000000e2").expect("an id parses"),
    );
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, 'Fixture jigs', 'fixture jigs')")
        .bind(elsewhere.as_uuid())
        .execute(&pool)
        .await
        .expect("seeds a second library");
    let other = job_row(elsewhere, &JobPayload::ProfileShape { revision });
    assert!(
        matches!(
            handler.handle(&other).await,
            Err(HandlerError::Permanent { .. })
        ),
        "another library's job must not reach this revision"
    );
    sqlx::query("DELETE FROM part_shape")
        .execute(&pool)
        .await
        .expect("clears the row the refused job must not have written");

    sqlx::query("DELETE FROM derivative WHERE kind = 'tessellation_l0'")
        .execute(&pool)
        .await
        .expect("takes the rung away");

    let job = job_row(seeded(), &JobPayload::ProfileShape { revision });
    let error = handler
        .handle(&job)
        .await
        .expect_err("there is no rung to profile");
    let HandlerError::Permanent { message } = error else {
        panic!("a revision with no rung will not grow one on a retry: {error:?}");
    };
    assert!(
        message.contains("re-scan the part"),
        "the error must say what to do, not only what is missing: {message}"
    );
    assert!(
        PgShapes(pool.clone())
            .of_part(only_part(&pool).await)
            .await
            .expect("reads")
            .is_none(),
        "and nothing was recorded for a revision that could not be profiled"
    );
}

/// Two more ways a row goes stale: the algorithm moved, and the rung was rebuilt. The third — the
/// part gained a revision the row never followed — is at the end of the revision test above, which
/// is where there are two revisions for a row to be behind. All three are one query, and a reader
/// ignores a stale row, so a branch that stops finding one leaves the part out of the duplicate
/// review for good.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_row_of_another_version_or_another_rung_is_swept_up(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");
    let part = only_part(&pool).await;
    let current = shape_of(&pool, part).await;

    for (what, spoil) in [
        (
            "an older SHAPE_VERSION",
            "UPDATE part_shape SET version = version - 1",
        ),
        (
            "another rung's bytes",
            "UPDATE part_shape SET l0_blake3 = repeat('0', 64)",
        ),
    ] {
        sqlx::query("DELETE FROM job")
            .execute(&pool)
            .await
            .expect("clears the queue");
        sqlx::query(spoil)
            .execute(&pool)
            .await
            .expect("spoils the row");
        handler.enqueue_stale_derivatives().await;
        assert_eq!(
            profile_jobs(&pool).await,
            1,
            "a row naming {what} must be swept up"
        );
        let job = queued_profile_job(&pool).await;
        assert_eq!(
            handler.handle(&job).await.expect("profiles"),
            Outcome::Profiled
        );
        let fresh = shape_of(&pool, part).await;
        assert_eq!(
            (fresh.version, fresh.l0, fresh.revision),
            (current.version, current.l0, current.revision),
            "and the row that replaces it names this build's version and the current rung"
        );
    }
}

/// A rebuilt L0 is a different set of triangles, so the profile computed from the old one is not
/// this part's shape any more. `derive_one` profiles again from the bytes it just wrote rather than
/// leaving a stale row for the next worker start.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn rebuilding_the_viewer_mesh_profiles_the_part_again(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    stage(ingest_dir.path(), BRACKET, BRACKET_FIXTURE);
    handler.handle(&ingest_job(BRACKET)).await.expect("ingests");
    let part = only_part(&pool).await;
    let revision = latest_revision(&pool, part).await;
    let before = shape_of(&pool, part).await;
    sqlx::query("DELETE FROM part_shape")
        .execute(&pool)
        .await
        .expect("takes the row away");

    let rebuild = job_row(
        seeded(),
        &JobPayload::Derive {
            revision,
            produce: lapidary_core::DerivativeKind::TessellationL0,
        },
    );
    assert_eq!(
        handler.handle(&rebuild).await.expect("rebuilds the rung"),
        Outcome::Rendered,
        "a derive reports what it built, not the profile it recorded on the way"
    );
    assert_eq!(
        shape_of(&pool, part)
            .await
            .profile
            .descriptor
            .map(f32::to_bits),
        before.profile.descriptor.map(f32::to_bits),
        "the rebuilt rung is the same rung, so the row it writes is the same row"
    );
}
