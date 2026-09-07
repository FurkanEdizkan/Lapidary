//! Seeding the bundled example parts.
//!
//! Two rules carry this whole feature, and both are about *not* doing it: never into a
//! library something has already run in, and never into the operator's ingest mount. The
//! first is what stops a restart putting back parts a user deleted; the second is what
//! stops a restart scanning a 320 GB corpus nobody asked about.

use lapidary_ingest::seed_examples;
use sqlx::PgPool;

/// The six committed example parts, which is what `deploy/Containerfile` copies into the
/// image. Read from the repository here so the test exercises the real files rather than
/// a fixture that could drift from them.
fn examples() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/parts")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_first_run_lands_the_bundled_parts_with_real_measurements(pool: PgPool) {
    // Through ingest, not through a migration, which is the point: a migration cannot
    // write a blob, so a seeded `file` row would name bytes the store does not hold and
    // the download would answer 500. These are real bytes, measured by the real kernel.
    let blobs = tempfile::tempdir().expect("temp dir");

    let added = seed_examples(pool.clone(), blobs.path(), &examples()).await;

    assert!(added >= 6, "the six committed example parts, got {added}");
    let (parts, measured): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(r.triangle_count) FROM part p \
         JOIN revision r ON r.part_id = p.id",
    )
    .fetch_one(&pool)
    .await
    .expect("query");
    assert_eq!(parts, i64::from(added));
    assert_eq!(
        measured, parts,
        "every seeded part carries the measurements a real ingest produces"
    );

    // And the bytes are on disk where the row says they are, so the download link works.
    //
    // `storage_path`, not the hash fan-out: a seeded part goes through the same ingest as
    // any other and lands in its own model directory, so the file this asserts is
    // `libraries/default/.../flange-dn40-pn16-lp-3310-02.stl` and not
    // `blobs/41/ed/41ed...`. Read off the row rather than reconstructed, because the row
    // is what the download route follows.
    let rel: String =
        sqlx::query_scalar("SELECT storage_path FROM file WHERE role = 'source' LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("query");
    let path = blobs.path().join(&rel);
    assert!(path.exists(), "no bytes at {}", path.display());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_start_seeds_nothing(pool: PgPool) {
    let blobs = tempfile::tempdir().expect("temp dir");
    let first = seed_examples(pool.clone(), blobs.path(), &examples()).await;
    let second = seed_examples(pool.clone(), blobs.path(), &examples()).await;

    assert!(first > 0);
    assert_eq!(second, 0, "restarting a worker must not re-seed");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_library_with_history_but_no_parts_is_not_seeded(pool: PgPool) {
    // The reason the question is "has anything ever run here" and not "is it empty", and
    // it is isolated deliberately: a library that has been scanned and holds no parts.
    //
    // An earlier version of this test seeded, soft-deleted every part, and asserted no
    // re-seed — and it passed with the guard replaced by an is-it-empty check, because
    // `library_holds` does not filter `deleted_at`, so the hash short-circuit settled
    // every file as `Skipped` and the count was zero either way. It was testing the
    // short-circuit while claiming to test the guard.
    //
    // A job row with no parts behind it cannot be answered by the short-circuit at all,
    // so this can only pass if the guard is the thing reading it.
    let blobs = tempfile::tempdir().expect("temp dir");
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES (gen_random_uuid(), gen_random_uuid(), $1, 'scan_directory', '{}'::jsonb, \
                 'done', 'skipped')",
    )
    .bind(
        "01931b6e-0000-7000-8000-000000000001"
            .parse::<uuid::Uuid>()
            .expect("library id"),
    )
    .execute(&pool)
    .await
    .expect("a scan has run here before");

    let added = seed_examples(pool.clone(), blobs.path(), &examples()).await;

    assert_eq!(
        added, 0,
        "a library that has been scanned is not a fresh one"
    );
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(parts, 0);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_every_seeded_part_does_not_bring_them_back(pool: PgPool) {
    // The user-facing half of the rule above. Two things stop this independently — the
    // history guard, and `library_holds` not filtering `deleted_at` — and both are
    // deliberate, so this asserts the outcome rather than which one did it.
    let blobs = tempfile::tempdir().expect("temp dir");
    seed_examples(pool.clone(), blobs.path(), &examples()).await;
    sqlx::query("UPDATE part SET deleted_at = now()")
        .execute(&pool)
        .await
        .expect("soft delete every part");

    let again = seed_examples(pool.clone(), blobs.path(), &examples()).await;

    assert_eq!(again, 0, "a restart must not undo a deliberate deletion");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_missing_examples_directory_is_not_a_failure(pool: PgPool) {
    // The normal case for a binary run outside the image: `deploy/Containerfile` is what
    // puts the examples there. Returning zero rather than erroring is what keeps a worker
    // from refusing to start over demo content.
    let blobs = tempfile::tempdir().expect("temp dir");
    let added = seed_examples(
        pool,
        blobs.path(),
        std::path::Path::new("/nonexistent-example-parts"),
    )
    .await;
    assert_eq!(added, 0);
}
