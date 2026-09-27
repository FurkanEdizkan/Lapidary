//! What a statement that gave up costs, against a real server (goal L4).
//!
//! The hole these tests close: a `tokio::time::timeout` bounds how long a caller waits and cancels
//! nothing on the server, so sqlx cannot hand that connection back to the pool until PostgreSQL has
//! finished. G4 stated it on a pool of one, as a ceiling it could not fix from its own file. Here
//! the same pool of one is the assertion that it is fixed: with `lapidary_db::INTERACTIVE` in the
//! connection's startup options, a read blocked on a table lock is cancelled *by the server*, and
//! the very next read answers on the same connection while the lock is still held.
//!
//! Every pool here is built the way a role's is — `Ceiling::applied_to` on the test database's own
//! connection options — because a test that builds a pool without them proves nothing about the
//! pool the api serves from.

use lapidary_db::{BACKGROUND, Ceiling, INTERACTIVE};
use sqlx::PgPool;
use std::time::{Duration, Instant};

/// A pool of `connections` on this test's database, carrying one role's ceiling.
async fn pool_with(pool: &PgPool, ceiling: Ceiling, connections: u32) -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(connections)
        // Well under the ceilings below, so a test that starves waits seconds rather than sqlx's
        // default half-minute: a starved pool should fail a test, not hang it.
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(ceiling.applied_to((*pool.connect_options()).clone()))
        .await
        .expect("a pool of its own")
}

/// Holds `ACCESS EXCLUSIVE` on `job` until the returned transaction is rolled back, which is what
/// makes every read of that table block for exactly as long as a test wants. G4's
/// `tests/dashboard.rs` holds the same lock for the same reason.
async fn lock_the_queue(pool: &PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut held = pool.begin().await.expect("a transaction to hold the lock");
    sqlx::query("LOCK TABLE job IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *held)
        .await
        .expect("takes the lock");
    held
}

/// The api's ceiling, as the server sees it — not as our own struct spells it. A startup option
/// PostgreSQL silently ignored would leave every other test here passing for the wrong reason.
///
/// It also asserts that an operator's own `options` survive ours, because `applied_to` appends: a
/// connection string asking for `geqo=off` still gets it.
#[sqlx::test(migrations = "./migrations")]
async fn a_pool_carries_its_roles_ceiling_and_keeps_what_the_url_already_asked_for(pool: PgPool) {
    let operators = (*pool.connect_options()).clone().options([("geqo", "off")]);
    let api = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(INTERACTIVE.applied_to(operators))
        .await
        .expect("the api's pool");

    let shown: (String, String, String) = sqlx::query_as(
        "SELECT current_setting('statement_timeout'), current_setting('lock_timeout'), \
         current_setting('geqo')",
    )
    .fetch_one(&api)
    .await
    .expect("the server says what it was asked for");
    assert_eq!(shown.0, "5s", "statement_timeout");
    assert_eq!(shown.1, "2s", "lock_timeout");
    assert_eq!(shown.2, "off", "the operator's own option survived ours");
    api.close().await;

    let worker = pool_with(&pool, BACKGROUND, 1).await;
    let shown: (String, String) = sqlx::query_as(
        "SELECT current_setting('statement_timeout'), current_setting('lock_timeout')",
    )
    .fetch_one(&worker)
    .await
    .expect("the server says what it was asked for");
    assert_eq!(shown.0, "10min", "the worker's statement ceiling");
    assert_eq!(shown.1, "2min", "the worker's lock ceiling");
    worker.close().await;
}

/// **G4's test, inverted.** `a_key_that_gave_up_still_holds_its_connection_until_the_lock_clears`
/// asserted, on a pool of one, that the connection was *not* free again while the lock was held.
/// With the ceiling in place the server is what ends the read, at `lock_timeout`, and the
/// connection comes straight back: the second read answers on it while the lock is still held.
///
/// No `tokio::time::timeout` anywhere in here, deliberately. Wrapping the blocked read in one would
/// drop the future before the server's own cancellation arrived and pin the connection again — the
/// exact bug this closes, re-introduced by the test that was supposed to prove it gone.
#[sqlx::test(migrations = "./migrations")]
async fn a_read_the_server_cancelled_gives_its_connection_straight_back(pool: PgPool) {
    let one = pool_with(&pool, INTERACTIVE, 1).await;
    let held = lock_the_queue(&pool).await;

    let began = Instant::now();
    let blocked = sqlx::query("SELECT count(*) FROM job")
        .fetch_one(&one)
        .await;
    let waited = began.elapsed();

    let began = Instant::now();
    let after = sqlx::query("SELECT 1").fetch_one(&one).await;
    let answered = began.elapsed();

    held.rollback().await.expect("releases the lock");
    one.close().await;

    let err = blocked.expect_err("a read of a locked table cannot finish");
    let err = lapidary_db::DbError::from(err);
    assert!(
        err.gave_up(),
        "the server should have cancelled this itself, and said so in its SQLSTATE: {err}"
    );
    assert!(
        (Duration::from_millis(1_800)..Duration::from_millis(4_000)).contains(&waited),
        "cancelled at the lock ceiling, not at the statement ceiling and not at once: {waited:?}"
    );
    assert!(
        after.is_ok(),
        "the one connection is usable again: {after:?}"
    );
    assert!(
        answered < Duration::from_millis(500),
        "and it was free immediately rather than when the lock cleared: {answered:?}"
    );
}

/// The whole point of two ceilings, asserted as one difference: the same statement, too long for
/// the api and fine for the worker. A job that tessellates for a minute is not a stuck query.
///
/// Both run at once, so the file pays six seconds rather than twelve.
#[sqlx::test(migrations = "./migrations")]
async fn a_statement_past_the_apis_ceiling_is_cancelled_and_the_same_one_finishes_for_the_worker(
    pool: PgPool,
) {
    let api = pool_with(&pool, INTERACTIVE, 1).await;
    let worker = pool_with(&pool, BACKGROUND, 1).await;

    // Six seconds: past the api's five and nowhere near the worker's ten minutes. `pg_sleep`
    // rather than a real read, because what is being asserted is the ceiling and not the query —
    // a read slow enough to cross five seconds would need a corpus this suite has no reason to
    // build.
    let sleep = || sqlx::query("SELECT pg_sleep(6)");
    let (refused, finished) = tokio::join!(sleep().execute(&api), sleep().execute(&worker));

    api.close().await;
    worker.close().await;

    let err = lapidary_db::DbError::from(refused.expect_err("the api's ceiling stops this"));
    assert!(err.gave_up(), "cancelled by the server: {err}");
    assert!(
        finished.is_ok(),
        "a worker statement longer than the api's ceiling still finishes: {finished:?}"
    );
}

/// A lock wait is cancelled at the *lock* ceiling and a slow statement at the statement one, and
/// the pool recovers from both. Without this, a pool that only ever had `statement_timeout` would
/// pass the test above and still hold a connection for five seconds on every locked read.
#[sqlx::test(migrations = "./migrations")]
async fn a_lock_wait_and_a_slow_statement_are_two_different_ceilings(pool: PgPool) {
    let api = pool_with(&pool, INTERACTIVE, 1).await;
    let held = lock_the_queue(&pool).await;

    let began = Instant::now();
    let locked = sqlx::query("SELECT count(*) FROM job")
        .fetch_one(&api)
        .await;
    let lock_wait = began.elapsed();
    held.rollback().await.expect("releases the lock");

    let began = Instant::now();
    let slow = sqlx::query("SELECT pg_sleep(30)").execute(&api).await;
    let statement = began.elapsed();
    api.close().await;

    assert!(locked.is_err() && slow.is_err(), "both are cancelled");
    assert!(
        lock_wait < statement,
        "a lock wait ends sooner than a slow statement: {lock_wait:?} against {statement:?}"
    );
    assert!(
        (Duration::from_millis(4_500)..Duration::from_millis(8_000)).contains(&statement),
        "the statement ceiling is five seconds, not thirty: {statement:?}"
    );
}
