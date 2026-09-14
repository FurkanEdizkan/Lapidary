//! A check-out, as the worker enforces it: a changed file for a checked-out part is kept only
//! when it arrives under that part's lock.

use lapidary_core::{BatchId, BlobHash, JobId, JobPayload, LibraryId, LockId, Outcome, PartId};
use lapidary_db::{Checkout, JobRow, PgLocks, PgParts, StoredBlobRow};
use lapidary_ingest::WorkerHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
use std::path::Path;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &str = "bracket-lp-1042-03.stl";
const BRACKET_FIXTURE: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
const SPACER_FIXTURE: &[u8] = include_bytes!("../../../fixtures/spacer-lp-2001-00.stl");

fn seeded() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn job(payload: JobPayload) -> JobRow {
    JobRow {
        id: JobId::new(),
        batch_id: BatchId::new(),
        library_id: seeded(),
        kind: payload.kind().to_owned(),
        payload: payload.to_json(),
        attempts: 1,
        max_attempts: 3,
    }
}

/// What the agent's upload commits: the bytes, at the part's own path, under its lock.
async fn upload(pool: &PgPool, blob_root: &Path, bytes: &[u8]) -> BlobHash {
    let staging = tempfile::tempdir().expect("temp dir");
    let staged = staging.path().join("upload.part");
    std::fs::write(&staged, bytes).expect("stages the upload");
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let stored = lapidary_storage::SourceWriter::open(blob_root)
        .put_file(&staged, &hash, lapidary_storage::Compression::Zstd)
        .expect("stores");
    lapidary_db::PgBlobs(pool.clone())
        .record_unreferenced(&StoredBlobRow {
            hash: stored.hash,
            size_bytes: stored.size_bytes,
            stored_bytes: stored.stored_bytes,
            zstd_level: stored.zstd_level,
        })
        .await
        .expect("records the blob");
    hash
}

/// A bracket scanned into a controlled library and checked out by mira.
async fn checked_out(
    pool: &PgPool,
    ingest_dir: &Path,
    blob_root: &Path,
) -> (WorkerHandler, PartId, LockId) {
    assert!(
        PgParts(pool.clone())
            .make_controlled(seeded())
            .await
            .expect("switches the library")
    );
    let handler = WorkerHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
        cad: None,
    };
    std::fs::write(ingest_dir.join(BRACKET), BRACKET_FIXTURE).expect("stages the scan");
    assert_eq!(
        handler
            .handle(&job(JobPayload::IngestFile {
                path: BRACKET.to_owned()
            }))
            .await
            .expect("ingests"),
        Outcome::Ingested
    );
    let part: uuid::Uuid = sqlx::query_scalar("SELECT id FROM part")
        .fetch_one(pool)
        .await
        .expect("the one part");
    let part = PartId::from_uuid(part);
    let Checkout::Taken(lock) = PgLocks(pool.clone())
        .take(part, "mira@workshop-pc")
        .await
        .expect("takes")
    else {
        panic!("the check-out is taken");
    };
    (handler, part, lock.id)
}

/// Every revision, oldest first: its label and its origin.
async fn revisions(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as("SELECT rev_label, origin FROM revision ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .expect("revisions")
}

fn refused(result: Result<Outcome, HandlerError>) -> String {
    match result {
        Err(HandlerError::Permanent { message }) => message,
        other => panic!("a check-out's refusal is permanent until somebody acts on it: {other:?}"),
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_scan_of_a_checked_out_parts_changed_file_is_refused_naming_the_holder(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let (handler, _, _) = checked_out(&pool, ingest_dir.path(), blob_root.path()).await;

    std::fs::write(ingest_dir.path().join(BRACKET), SPACER_FIXTURE).expect("changes the file");
    let message = refused(
        handler
            .handle(&job(JobPayload::IngestFile {
                path: BRACKET.to_owned(),
            }))
            .await,
    );
    assert!(message.contains("mira@workshop-pc"), "{message}");
    assert_eq!(revisions(&pool).await.len(), 1, "no revision");

    let top: String = sqlx::query_scalar("SELECT storage_path FROM file")
        .fetch_one(&pool)
        .await
        .expect("the one file");
    assert_eq!(
        std::fs::read(blob_root.path().join(top)).expect("still on disk"),
        BRACKET_FIXTURE,
        "the refusal came before any file moved"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_holders_save_comes_back_as_a_revision_from_the_agent(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let (handler, _, lock) = checked_out(&pool, ingest_dir.path(), blob_root.path()).await;

    let hash = upload(&pool, blob_root.path(), SPACER_FIXTURE).await;
    let outcome = handler
        .handle(&job(JobPayload::IngestBlob {
            blake3: hash,
            source_path: BRACKET.to_owned(),
            lock: Some(lock),
        }))
        .await
        .expect("revises");
    assert_eq!(outcome, Outcome::Revised);
    assert_eq!(
        revisions(&pool).await,
        [
            ("1".to_owned(), "ingest".to_owned()),
            ("2".to_owned(), "agent".to_owned())
        ]
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_save_under_a_released_lock_is_refused_naming_who_released_it(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    let (handler, part, lock) = checked_out(&pool, ingest_dir.path(), blob_root.path()).await;
    PgLocks(pool.clone())
        .force_release(part, "jonas@laptop")
        .await
        .expect("releases")
        .expect("there was a check-out to release");

    let hash = upload(&pool, blob_root.path(), SPACER_FIXTURE).await;
    let message = refused(
        handler
            .handle(&job(JobPayload::IngestBlob {
                blake3: hash,
                source_path: BRACKET.to_owned(),
                lock: Some(lock),
            }))
            .await,
    );
    assert!(message.contains("jonas@laptop"), "{message}");
    assert_eq!(revisions(&pool).await.len(), 1, "no revision");
}
