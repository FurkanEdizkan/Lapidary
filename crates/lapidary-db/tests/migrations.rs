//! The schema constraints slice 2 adds are load-bearing, not decoration: one of them is
//! what makes at-least-once job delivery safe. Each is tested by trying to violate it.

use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

#[sqlx::test(migrations = "./migrations")]
async fn two_parts_with_one_name_in_one_library_are_refused(pool: PgPool) {
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let insert = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(library)
                .bind("bracket-lp-1042-03")
                .execute(&pool)
                .await
        }
    };

    insert(Uuid::now_v7())
        .await
        .expect("the first part inserts");
    let second = insert(Uuid::now_v7()).await;

    let err = second.expect_err("a second part with the same name must be refused");
    assert!(
        err.to_string().contains("part_name_unique_per_library"),
        "expected the named constraint to be what refused it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_that_claims_done_without_an_outcome_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'done')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("done without an outcome must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_that_claims_failed_without_a_reason_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'failed')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("failed without a reason must be refused");

    assert!(
        err.to_string().contains("job_failed_has_reason"),
        "expected job_failed_has_reason to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_carries_its_last_error_is_accepted(pool: PgPool) {
    // job_failed_has_reason is an implication (`state <> 'failed' or last_error is not
    // null`), not an equivalence: only the 'failed' direction is mandatory. Task 5's
    // reschedule sets state back to 'pending' while keeping last_error, so a retrying
    // job can say what went wrong last time without waiting for it to exhaust its
    // attempts. If this constraint ever regresses to a biconditional, this is the row
    // that starts getting refused, and nothing else in this file would catch it.
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, last_error) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'pending', 'the database was unreachable')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect("a retrying job may carry its last error while pending");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_done_job_that_claims_it_rendered_something_is_accepted(pool: PgPool) {
    // The positive case for 0005's re-added job_outcome_known: without this, a CHECK
    // written inverted would pass every negative case below and still be wrong.
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'done', 'rendered')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect("a done job may report that it rendered something");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_claims_it_rendered_something_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'pending', 'rendered')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("an outcome on a non-terminal row must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_claiming_an_unknown_outcome_is_still_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'done', 'polished')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("an outcome outside the known set must be refused");

    assert!(
        err.to_string().contains("job_outcome_known"),
        "expected job_outcome_known to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = false)]
async fn pre_existing_tessellation_rungs_survive_the_auto_thumbnail_migration(pool: PgPool) {
    // 0005 touches `library` and `job`, not `derivative` -- this is "we never delete user
    // data implicitly" made testable for the LOD ladder slice 3 wrote. Migrations are run
    // by hand here rather than via the attribute, so the rows genuinely pre-exist 0005
    // rather than being inserted into an already-migrated database.
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(4, &pool)
        .await
        .expect("migrations up to 0004 apply");

    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let part = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
        .bind(part)
        .bind(library)
        .bind("bracket-lp-1042-04")
        .execute(&pool)
        .await
        .expect("part inserts");

    let revision = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision)
    .bind(part)
    .execute(&pool)
    .await
    .expect("revision inserts");

    for kind in ["tessellation_l1", "tessellation_l2"] {
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
             VALUES ($1, $2, $3, $4, 'mesh stl-1+cpu-1', '{}')",
        )
        .bind(Uuid::now_v7())
        .bind(revision)
        .bind(kind)
        .bind(b"lod-rung".as_slice())
        .execute(&pool)
        .await
        .expect("tessellation rung inserts");
    }

    migrator.run(&pool).await.expect("0005 applies");

    let has_column: bool = sqlx::query_scalar(
        "SELECT exists (SELECT 1 FROM information_schema.columns \
         WHERE table_name = 'library' AND column_name = 'auto_thumbnail')",
    )
    .fetch_one(&pool)
    .await
    .expect("column check runs");
    assert!(has_column, "0005 must actually have run");

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative WHERE revision_id = $1 \
         AND kind IN ('tessellation_l1', 'tessellation_l2')",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .expect("count runs");

    assert_eq!(
        count, 2,
        "pre-existing tessellation rungs must survive migration 0005"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_claims_an_outcome_is_refused(pool: PgPool) {
    // The converse of `a_job_that_claims_done_without_an_outcome_is_refused`:
    // job_done_has_outcome is a genuine biconditional, so an outcome on a row that
    // isn't 'done' must be refused too, not just a 'done' row with no outcome.
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'pending', 'ingested')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("a pending job claiming an outcome must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}
