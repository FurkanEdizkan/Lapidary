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
