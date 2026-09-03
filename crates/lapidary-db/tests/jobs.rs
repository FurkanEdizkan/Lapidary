use lapidary_core::LibraryId;
use lapidary_db::PgJobs;
use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

#[sqlx::test(migrations = "./migrations")]
async fn enqueue_writes_one_pending_row_per_path_under_one_batch(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let paths = vec![
        "bracket-lp-1042-03.stl".to_owned(),
        "spacer-lp-2001-00.stl".to_owned(),
    ];

    let (batch, queued) = jobs.enqueue_scan(seeded(), &paths).await.expect("enqueues");
    assert_eq!(queued, 2);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT state, payload->>'path' FROM job WHERE batch_id = $1 ORDER BY payload->>'path'",
    )
    .bind(batch.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("reads back");

    assert_eq!(
        rows,
        vec![
            ("pending".to_owned(), "bracket-lp-1042-03.stl".to_owned()),
            ("pending".to_owned(), "spacer-lp-2001-00.stl".to_owned()),
        ]
    );

    // The assertion above pins path *values*, but never reads `id` back, so on its own
    // it cannot tell a correctly paired insert from one where `unnest`'s two arrays
    // were zipped out of step -- `id` is an opaque generated key with no relationship
    // to `path` that a test could independently recompute, so there is no oracle for
    // "this id belongs to this path". What *is* checkable, and what a mis-zip would
    // actually break, is row identity: two distinct paths must produce two distinct,
    // non-null primary keys, all still tagged with the batch `enqueue_scan` returned.
    // A shorter id array silently NULL-padded by `unnest` would violate `job`'s
    // `id uuid primary key` (NOT NULL) constraint and this test would already fail at
    // `.expect("enqueues")`; this assertion instead catches an implementation that
    // reused one id for every row, which no NOT NULL constraint would notice.
    let distinct_ids: i64 =
        sqlx::query_scalar("SELECT count(DISTINCT id) FROM job WHERE batch_id = $1")
            .bind(batch.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("counts distinct ids");
    assert_eq!(
        distinct_ids, 2,
        "each path must get its own row id, not a shared or dropped one"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn enqueueing_nothing_issues_a_batch_id_and_writes_no_rows(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, queued) = jobs.enqueue_scan(seeded(), &[]).await.expect("enqueues");

    assert_eq!(queued, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(count, 0, "an empty scan must not invent a job");
}
