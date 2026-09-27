//! `GET /api/events`, against a live Postgres: migration `0049`'s trigger, the hub that groups what it
//! says, and the stream a tab reads.
//!
//! The writes here are raw SQL on purpose, which is unusual for this crate's tests. What is under test is
//! a *trigger* — a rule the database enforces on every writer there is and every writer there will be —
//! so going through a repository would narrow the claim to the one writer that repository happens to be,
//! and would make a hundred rows a hundred ingests. `INSERT INTO part` is what every writer ultimately
//! does, and it is what the trigger sees.

use axum::body::{Body, BodyDataStream};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use lapidary_api::events::Hub;
use lapidary_core::{AppEvent, BatchId, JobId, LibraryId, PartId};
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

/// The measurement the goal asks for: a change reaches an open page inside a second.
const WITHIN_A_SECOND: Duration = Duration::from_secs(1);

/// Long enough for several of the hub's 250 ms windows to pass, for the assertions that nothing *more*
/// arrives. Deliberately not tight: a false pass here is a grouping bug shipped, and the price of being
/// generous is one second of test time.
const LONG_ENOUGH: Duration = Duration::from_millis(1_200);

/// A hub, the router over it, and the token that ends both.
async fn hub(pool: &sqlx::PgPool) -> (axum::Router, CancellationToken) {
    let shutdown = CancellationToken::new();
    let hub = Hub::spawn(pool.clone(), shutdown.clone()).await;
    (lapidary_api::events::router(hub), shutdown)
}

/// Ends the hub and waits until the database agrees its listener has gone.
///
/// Two jobs in one. It asserts the shutdown contract — the hub gives its connection back rather than
/// holding it past the process it belongs to — and it is what lets `#[sqlx::test]` tear the test database
/// down: `pool.close()` waits for every connection to come home, and a listener that outlived its test
/// would hold one for ten seconds and then be reported as a leak.
async fn stop(shutdown: &CancellationToken, pool: &sqlx::PgPool) {
    shutdown.cancel();
    for _ in 0..100 {
        if listeners(pool).await == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the hub kept its database connection after it was told to stop");
}

/// How many backends are listening for changes on this test's own database.
///
/// `datname` scopes it to this test: every lane's tests share one PostgreSQL, and `pg_stat_activity` is
/// cluster-wide. `query LIKE 'LISTEN%lapidary_events%'` and not `%lapidary_events%`, which would match
/// this very statement, and not `LISTEN%`, which in a real installation would also count the worker's
/// job channel and the peer role's sharing channel.
async fn listeners(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity \
         WHERE datname = current_database() AND query LIKE 'LISTEN%lapidary_events%'",
    )
    .fetch_one(pool)
    .await
    .expect("pg_stat_activity reads")
}

/// One part row, which is all the trigger needs to see. `source_path` is unique per library, so it is
/// the name again rather than a literal every caller would have to keep distinct.
async fn add_part(pool: &sqlx::PgPool, library: LibraryId, name: &str) {
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(PartId::new().as_uuid())
        .bind(library.as_uuid())
        .bind(name)
        .bind(format!("{name}.stl"))
        .execute(pool)
        .await
        .expect("a part is inserted");
}

/// A second library, so that grouping per library can be told from grouping altogether.
async fn add_library(pool: &sqlx::PgPool, name: &str) -> LibraryId {
    let library = LibraryId::new();
    sqlx::query("INSERT INTO library (id, name, slug) VALUES ($1, $2, $3)")
        .bind(library.as_uuid())
        .bind(name)
        .bind(name.to_lowercase())
        .execute(pool)
        .await
        .expect("a library is inserted");
    library
}

/// One queued job, in the state the queue puts it in.
async fn enqueue(pool: &sqlx::PgPool, library: LibraryId) -> JobId {
    let job = JobId::new();
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb)",
    )
    .bind(job.as_uuid())
    .bind(BatchId::new().as_uuid())
    .bind(library.as_uuid())
    .execute(pool)
    .await
    .expect("a job is enqueued");
    job
}

/// One `UPDATE job SET state = …`, the shape every one of `PgJobs`'s transitions has.
async fn set_job_state(pool: &sqlx::PgPool, job: JobId, state: &str) {
    let outcome = (state == "done").then_some("ingested");
    let error = (state == "failed").then_some("the file could not be read");
    sqlx::query("UPDATE job SET state = $2, outcome = $3, last_error = $4 WHERE id = $1")
        .bind(job.as_uuid())
        .bind(state)
        .bind(outcome)
        .bind(error)
        .execute(pool)
        .await
        .expect("the job's state is written");
}

/// Opens `GET /api/events` and hands back the response, unread.
async fn open(app: &axum::Router) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri("/api/events")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds")
}

/// What the next read off a stream found.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Event(AppEvent),
    /// Nothing arrived in the time allowed. The stream is still open.
    Quiet,
    /// The response completed: the hub is gone.
    Ended,
}

/// One open stream — one tab — read an event at a time.
struct Tab {
    body: BodyDataStream,
    text: String,
}

impl Tab {
    fn of(response: Response) -> Self {
        Self {
            body: response.into_body().into_data_stream(),
            text: String::new(),
        }
    }

    /// The next event, or why there was not one, within `within`.
    ///
    /// Keep-alive comments and the blank lines between frames are skipped: what is under test is what
    /// the hub says, not how axum spaces it out.
    async fn next(&mut self, within: Duration) -> Next {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(event) = self.buffered() {
                return Next::Event(event);
            }
            match tokio::time::timeout_at(deadline, self.body.next()).await {
                Err(_) => return Next::Quiet,
                Ok(None) => return Next::Ended,
                Ok(Some(chunk)) => {
                    let bytes = chunk.expect("the stream's body reads");
                    self.text.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
        }
    }

    fn buffered(&mut self) -> Option<AppEvent> {
        while let Some(end) = self.text.find('\n') {
            let line: String = self.text.drain(..=end).collect();
            if let Some(json) = line.trim_end().strip_prefix("data: ") {
                return Some(serde_json::from_str(json).expect("an event is JSON"));
            }
        }
        None
    }
}

/// Stage 1 and the goal's first measurement: a part arrives, and the page hears about it inside a second.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_new_part_reaches_an_open_page_within_a_second(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let mut stream = Tab::of(open(&app).await);

    let started = std::time::Instant::now();
    add_part(&pool, seeded(), "idler-pulley-lp-4820-00").await;

    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "inserting a part notifies its library"
    );
    let took = started.elapsed();
    assert!(
        took < WITHIN_A_SECOND,
        "a change has to reach an open page inside a second; this took {took:?}"
    );

    stop(&shutdown, &pool).await;
}

/// Stage 1's other two writes on `part`. Update and delete notify as well, so a rename and a purge move
/// a grid that is already open.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_rename_and_a_removal_notify_too(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let mut stream = Tab::of(open(&app).await);
    add_part(&pool, seeded(), "flange-dn40-lp-3310-02").await;
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() })
    );

    sqlx::query("UPDATE part SET name = 'flange-dn40-lp-3310-03' WHERE name = $1")
        .bind("flange-dn40-lp-3310-02")
        .execute(&pool)
        .await
        .expect("the part is renamed");
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "an update notifies"
    );

    sqlx::query("DELETE FROM part WHERE name = $1")
        .bind("flange-dn40-lp-3310-03")
        .execute(&pool)
        .await
        .expect("the part is deleted");
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "a delete notifies, from OLD — a purged part is as much a change as a new one"
    );

    stop(&shutdown, &pool).await;
}

/// The half the database does by itself: identical notifications inside one transaction are delivered
/// once, so a hundred rows are one event rather than a hundred.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_hundred_parts_in_one_transaction_are_one_event(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let mut stream = Tab::of(open(&app).await);

    let mut transaction = pool.begin().await.expect("a transaction begins");
    for number in 0..100 {
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
            .bind(PartId::new().as_uuid())
            .bind(seeded().as_uuid())
            .bind(format!("spacer-lp-2001-{number:02}"))
            .bind(format!("spacers/spacer-lp-2001-{number:02}.stl"))
            .execute(&mut *transaction)
            .await
            .expect("a part is inserted");
    }
    transaction.commit().await.expect("the transaction commits");

    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() })
    );
    assert_eq!(
        stream.next(LONG_ENOUGH).await,
        Next::Quiet,
        "a hundred rows in one transaction is one event, not a hundred"
    );

    stop(&shutdown, &pool).await;
}

/// The half the hub does: a burst of *separate* transactions, which PostgreSQL cannot collapse because
/// each commits on its own, still costs a page one question per library.
///
/// This is the test that fails if the grouping window is deleted — the transaction test above would not,
/// because the database collapses that one without any help.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_burst_of_separate_writes_is_grouped_per_library(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let other = add_library(&pool, "Fixtures").await;
    // The library insert is not a `part` write, so it notifies nothing. Opening the stream after it
    // keeps that out of the count either way.
    let mut stream = Tab::of(open(&app).await);

    for number in 0..10 {
        add_part(&pool, seeded(), &format!("bracket-lp-1042-{number:02}")).await;
        add_part(&pool, other, &format!("jig-plate-lp-6110-{number:02}")).await;
    }

    // Twenty commits become at most two events per library: the burst either lands inside one window or
    // straddles the boundary between two. Never ten, which is what no grouping at all would give.
    let mut seen = Vec::new();
    while let Next::Event(event) = stream.next(LONG_ENOUGH).await {
        seen.push(event);
    }
    let one = AppEvent::Changed { library: seeded() };
    let another = AppEvent::Changed { library: other };
    let for_seeded = seen.iter().filter(|event| **event == one).count();
    let for_other = seen.iter().filter(|event| **event == another).count();
    assert!(
        (1..=2).contains(&for_seeded) && (1..=2).contains(&for_other),
        "ten writes a library, grouped over 250 ms, is one event each or two across a window \
         boundary; got {seen:?}"
    );
    assert_eq!(
        seen.len(),
        for_seeded + for_other,
        "and nothing else: no resync, and no library that did not change"
    );

    stop(&shutdown, &pool).await;
}

/// Why the hub exists. Five tabs are five receivers in this process and one backend in the database.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn five_pages_share_one_listening_backend(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    assert_eq!(
        listeners(&pool).await,
        1,
        "the hub opens its listener before it returns, so a write straight afterwards is heard"
    );

    let mut streams = Vec::new();
    for _ in 0..5 {
        streams.push(Tab::of(open(&app).await));
    }
    add_part(&pool, seeded(), "shaft-collar-lp-5230-01").await;
    for (tab, stream) in streams.iter_mut().enumerate() {
        assert_eq!(
            stream.next(WITHIN_A_SECOND).await,
            Next::Event(AppEvent::Changed { library: seeded() }),
            "tab {tab} sees the change"
        );
    }
    assert_eq!(
        listeners(&pool).await,
        1,
        "five open streams, one LISTEN backend — one listener per process, never one per tab"
    );

    stop(&shutdown, &pool).await;
}

/// A job is worth a wake-up when it ends, and not before. `pending` to `running` is invisible to anybody
/// looking at a page; `done` is the moment the part has its thumbnail and its shape profile, and `failed`
/// the moment a file's failure is worth showing.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_notifies_when_it_ends_and_not_before(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let job = enqueue(&pool, seeded()).await;
    let failing = enqueue(&pool, seeded()).await;
    let mut stream = Tab::of(open(&app).await);

    set_job_state(&pool, job, "running").await;
    assert_eq!(
        stream.next(LONG_ENOUGH).await,
        Next::Quiet,
        "a job starting changes nothing anybody can see"
    );

    set_job_state(&pool, job, "done").await;
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "a job reaching done notifies: its part has its derivatives and its profile by then"
    );

    set_job_state(&pool, failing, "failed").await;
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "and so does a job that failed"
    );

    stop(&shutdown, &pool).await;
}

/// Losing the listener is the one thing this stream cannot hide, so it says so: a page told to resync
/// asks for everything again, which is the only honest answer about notifications nobody heard.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_lost_listener_becomes_a_resync(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let mut stream = Tab::of(open(&app).await);

    let killed: Option<bool> = sqlx::query_scalar(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = current_database() AND query LIKE 'LISTEN%lapidary_events%'",
    )
    .fetch_optional(&pool)
    .await
    .expect("pg_stat_activity reads");
    assert_eq!(
        killed,
        Some(true),
        "there was one listening backend, and terminating it worked"
    );

    // Generous: which arm of the hub fires depends on whether sqlx sees the connection go at once or
    // fails its next acquire, and the second path waits out the retry.
    assert_eq!(
        stream.next(Duration::from_secs(8)).await,
        Next::Event(AppEvent::Resync),
        "a connection that went away and came back is a gap, and the page is told to ask again"
    );

    // And it is listening again, proved by a change rather than by a sleep.
    add_part(&pool, seeded(), "gasket-lp-7740-00").await;
    assert_eq!(
        stream.next(WITHIN_A_SECOND).await,
        Next::Event(AppEvent::Changed { library: seeded() }),
        "the hub listens again after it lost the connection"
    );
    assert_eq!(
        listeners(&pool).await,
        1,
        "and it is still one listener, not two"
    );

    stop(&shutdown, &pool).await;
}

/// Both of this crate's streams carry both headers, **on the router the api process actually serves**.
///
/// `docs/ARCHITECTURE.md` calls a buffering proxy the most common works-in-dev-breaks-in-prod bug in
/// this stack, and until this goal the batch stream had neither header.
///
/// The router here is the merge `bin/lapidary-server` performs for the api role, not `events::router`
/// on its own, because that merge is the one seam nothing else covers: axum panics at merge time on a
/// path both routers claim or a fallback both set, and the first place that would otherwise show is a
/// container that will not start.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn both_streams_ask_not_to_be_buffered(pool: sqlx::PgPool) {
    let shutdown = CancellationToken::new();
    let hub = Hub::spawn(pool.clone(), shutdown.clone()).await;
    let app = lapidary_api::router(
        lapidary_api::AppState {
            db: pool.clone(),
            blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
            upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
            host_storage_root: None,
            touches: Default::default(),
        },
        lapidary_api::Role::Api,
    )
    .merge(lapidary_api::events::router(hub));

    let response = open(&app).await;
    assert_eq!(response.status(), StatusCode::OK);
    for header in ["cache-control", "x-accel-buffering"] {
        let values: Vec<_> = response.headers().get_all(header).iter().collect();
        assert_eq!(
            values.len(),
            1,
            "/api/events carries exactly one {header}: {values:?}"
        );
    }
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert_eq!(response.headers()["x-accel-buffering"], "no");

    // The per-batch stream, on the same merged router. A batch that does not exist still answers 200
    // with its headers — the status is sent before the first read and an `EventSource` could not read a
    // body anyway, which is `jobs.rs`'s own reasoning.
    let batch = BatchId::new();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/libraries/{}/jobs/{batch}/events", seeded()))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-cache");
    assert_eq!(
        response.headers()["x-accel-buffering"],
        "no",
        "the batch stream gets the same treatment: it goes through the same proxy"
    );
    stop(&shutdown, &pool).await;
}

/// The hub ends on shutdown, and its end is what ends every stream. Nothing in the router is cancelled
/// by hand: the hub task owns the only sender, so its return closes the channel.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn shutdown_ends_every_open_stream(pool: sqlx::PgPool) {
    let (app, shutdown) = hub(&pool).await;
    let mut first = Tab::of(open(&app).await);
    let mut second = Tab::of(open(&app).await);

    shutdown.cancel();
    assert_eq!(
        first.next(WITHIN_A_SECOND).await,
        Next::Ended,
        "the response completes rather than hanging open on a hub that is gone"
    );
    assert_eq!(second.next(WITHIN_A_SECOND).await, Next::Ended);

    // A page that opens afterwards gets a stream that ends at once rather than one that waits forever.
    assert_eq!(
        Tab::of(open(&app).await).next(WITHIN_A_SECOND).await,
        Next::Ended
    );
    stop(&shutdown, &pool).await;
}
