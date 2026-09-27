//! `POST /api/dashboard/resolve`: twelve widgets in one round trip, and what happens when one of them
//! cannot answer.
//!
//! Two of the rules here are about time rather than about SQL, and they are the reason this file holds
//! a table lock. `LOCK TABLE job IN ACCESS EXCLUSIVE MODE` makes the `queue` widget — the only one that
//! reads `job` — block for as long as the test wants, which is how "one key times out and the other
//! eleven answer", "each key's two seconds start once it holds a permit" and "four keys run at a time"
//! are asserted rather than asserted about.
//!
//! Every test builds its own pool with a stated `max_connections`, because the distinction between
//! waiting for a permit and waiting for a connection cannot be read off `#[sqlx::test]`'s default.
//! Production is `max_connections(8)` (`lapidary_db::connect`), and the semaphore of 4 is set
//! against it.
//!
//! Those pools also carry `lapidary_db::INTERACTIVE`, the api role's own `statement_timeout` and
//! `lock_timeout` (goal L4) — so what ends a blocked widget here is what ends it in the api: the
//! server, which gives the connection straight back. A pool built without them would still pass
//! every test about ordering and refusals and would prove nothing at all about the pool.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{
    BlobHash, JobPayload, LibraryId, MeasurementProvenance, MeshMeasurements, PartId,
    RevisionOrigin, SavedFilterId,
};
use lapidary_db::{
    IngestRequest, PgFolders, PgIngest, PgJobs, PgParts, PgSavedFilters, StoredBlobRow,
};
use std::time::{Duration, Instant};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// A library id that names nothing. Every widget that takes a library must fail its own key on it.
const NO_SUCH_LIBRARY: &str = "01931b6e-0000-7000-8000-0000000000ff";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // A dashboard reads metadata and derivatives and never a byte off disk. A root that does not
        // exist is what makes that an assertion rather than a coincidence — the `instanceStorage`
        // widget deliberately skips the disk walk the storage page offers.
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
        host_storage_root: None,
        touches: Default::default(),
    }
}

/// A pool of `connections` on this test's own database, so the test states the limit instead of
/// inheriting one — and carrying the api role's ceiling, so a statement is stopped here the way it
/// is in the api.
async fn pool_of(pool: &sqlx::PgPool, connections: u32) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(connections)
        // Two seconds under the semaphore's worst round, so a starved pool fails a test on the
        // assertion rather than hanging it for sqlx's default half-minute.
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(lapidary_db::INTERACTIVE.applied_to((*pool.connect_options()).clone()))
        .await
        .expect("a pool of its own")
}

async fn resolve(
    pool: sqlx::PgPool,
    widgets: Vec<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let body = serde_json::json!({ "widgets": widgets });
    let request = Request::builder()
        .method("POST")
        .uri("/api/dashboard/resolve")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    let response = router(state(pool), Role::Api)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    (status, json)
}

/// The 200 every valid body answers, with its results.
async fn resolved(pool: sqlx::PgPool, widgets: Vec<serde_json::Value>) -> serde_json::Value {
    let (status, json) = resolve(pool, widgets).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

fn ask(key: &str, widget: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "key": key, "widget": widget })
}

fn keys(json: &serde_json::Value) -> Vec<String> {
    json["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|one| one["key"].as_str().expect("a key").to_owned())
        .collect()
}

fn statuses(json: &serde_json::Value) -> Vec<String> {
    json["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|one| {
            one["result"]["status"]
                .as_str()
                .expect("a status")
                .to_owned()
        })
        .collect()
}

/// One key's result.
fn answer<'a>(json: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    json["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|one| one["key"] == key)
        .map(|one| &one["result"])
        .unwrap_or_else(|| panic!("no result for key {key}: {json}"))
}

/// One key's value, asserting it answered.
fn value<'a>(json: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    let result = answer(json, key);
    assert_eq!(result["status"], "ok", "{key}: {result}");
    &result["value"]
}

/// One key's failure message, asserting it failed.
fn message(json: &serde_json::Value, key: &str) -> String {
    let result = answer(json, key);
    assert_eq!(result["status"], "failed", "{key}: {result}");
    result["message"].as_str().expect("a message").to_owned()
}

/// A part's source bytes, one hash per `seed`. Four bytes of it, because the measurement corpus is
/// 10,000 parts and two parts sharing a hash are "identical" — which would put them in a cluster and
/// measure G3's duplicate-heavy case instead of this one.
fn hash_of(seed: u32) -> BlobHash {
    let mut bytes = [0x5au8; 32];
    bytes[..4].copy_from_slice(&seed.to_le_bytes());
    BlobHash::from_bytes(bytes)
}

/// A part with a real ingest record behind it: a `blob` row, a revision and a source file.
async fn seed(pool: &sqlx::PgPool, name: &str, path: &str, format: &str, blob: u32) -> PartId {
    seed_with(pool, name, path, format, blob, None).await
}

async fn seed_with(
    pool: &sqlx::PgPool,
    name: &str,
    path: &str,
    format: &str,
    blob: u32,
    thumbnail: Option<&[u8]>,
) -> PartId {
    let stored = format!("libraries/default/{path}");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: path,
            folder: None,
            storage_path: Some(&stored),
            blob: &StoredBlobRow {
                hash: hash_of(blob),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            provenance: MeasurementProvenance::TESSELLATED,
            thumbnail_webp: thumbnail,
            kernel_version: "mesh stl-1+cpu-1",
            format,
            tessellations: &[],
        })
        .await
        .expect("seeds the part")
}

/// Three parts, tagged and typed so every facet has a commonest value and a second one: two PETG STLs
/// somebody printed, and one steel STEP waiting on review.
async fn corpus(pool: &sqlx::PgPool) {
    let parts = PgParts(pool.clone());
    let bracket = seed(
        pool,
        "Bracket, LP-1042-03",
        "bracket-lp-1042-03.stl",
        "stl",
        0xa1,
    )
    .await;
    let spacer = seed(pool, "Spacer, 20 mm", "spacer-20.stl", "stl", 0xa2).await;
    let housing = seed(
        pool,
        "Housing, LP-2210-01",
        "housing-lp-2210-01.step",
        "step",
        0xa3,
    )
    .await;
    for (part, tag, material) in [
        (bracket, "printed", "PETG"),
        (spacer, "printed", "PETG"),
        (housing, "review", "Steel, 316L"),
    ] {
        assert!(
            parts
                .set_tags(part, &[tag.to_owned()])
                .await
                .expect("tags the part")
        );
        assert!(
            parts
                .set_materials(part, &[material.to_owned()])
                .await
                .expect("gives the part a material")
        );
    }
}

/// A saved filter on the two PETG parts.
async fn petg_filter(pool: &sqlx::PgPool) -> SavedFilterId {
    PgSavedFilters(pool.clone())
        .create(
            library(),
            "PETG, printed",
            &serde_json::json!({ "material": "PETG" }),
        )
        .await
        .expect("saves the filter")
}

/// Three pending jobs, one of them taken and one of them failed, so the queue widget has a figure in
/// each of its three numbers.
async fn queued(pool: &sqlx::PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue(
        library(),
        &[
            JobPayload::IngestFile {
                path: "bracket-lp-1042-03.stl".to_owned(),
            },
            JobPayload::IngestFile {
                path: "spacer-20.stl".to_owned(),
            },
            JobPayload::IngestFile {
                path: "housing-lp-2210-01.step".to_owned(),
            },
        ],
    )
    .await
    .expect("queues three jobs");
    let running = jobs
        .dequeue("lane-1-test", Duration::from_secs(60))
        .await
        .expect("takes one")
        .expect("there is one to take");
    let doomed = jobs
        .dequeue("lane-1-test", Duration::from_secs(60))
        .await
        .expect("takes another")
        .expect("there is another to take");
    jobs.fail(
        doomed.id,
        "Could not read this STL file — re-export it and scan again.",
    )
    .await
    .expect("fails the second");
    assert_ne!(running.id, doomed.id);
}

/// The widget bodies, by kind. `library` is a string so a test can pass an id that names nothing.
fn storage(library: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "storage", "library": library })
}

fn instance_storage() -> serde_json::Value {
    serde_json::json!({ "kind": "instanceStorage" })
}

fn recent(library: &str, limit: u32) -> serde_json::Value {
    serde_json::json!({ "kind": "recent", "library": library, "limit": limit })
}

fn saved_filter(library: &str, filter: &str, limit: u32) -> serde_json::Value {
    serde_json::json!({
        "kind": "savedFilter", "library": library, "filter": filter, "limit": limit
    })
}

fn facet(library: &str, which: &str, limit: u32) -> serde_json::Value {
    serde_json::json!({ "kind": "facet", "library": library, "facet": which, "limit": limit })
}

fn queue(library: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "queue", "library": library })
}

fn duplicates(library: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "duplicates", "library": library })
}

/// A full dashboard: every one of the seven kinds, twelve tiles, three kinds twice — a real layout
/// carries more than one facet tile.
fn twelve(filter: &str) -> Vec<serde_json::Value> {
    let library = SEEDED_LIBRARY;
    vec![
        ask("a", storage(library)),
        ask("b", instance_storage()),
        ask("c", recent(library, 6)),
        ask("d", saved_filter(library, filter, 6)),
        ask("e", facet(library, "format", 5)),
        ask("f", facet(library, "material", 5)),
        ask("g", facet(library, "tag", 5)),
        ask("h", queue(library)),
        ask("i", duplicates(library)),
        ask("j", storage(library)),
        ask("k", recent(library, 3)),
        ask("l", facet(library, "format", 1)),
    ]
}

/// The eleven tiles of [`twelve`] that do not read `job`, so a locked `job` table leaves them alone.
fn eleven_that_never_read_the_queue(filter: &str) -> Vec<serde_json::Value> {
    twelve(filter)
        .into_iter()
        .filter(|asked| asked["widget"]["kind"] != "queue")
        .collect()
}

/// A transaction holding `job` under `ACCESS EXCLUSIVE`, which conflicts with the `ACCESS SHARE` a
/// plain `SELECT` takes: every read of that table blocks until it is rolled back.
async fn lock_the_queue(pool: &sqlx::PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut held = pool.begin().await.expect("a transaction");
    sqlx::query("LOCK TABLE job IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *held)
        .await
        .expect("locks the job table");
    held
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn twelve_widgets_answer_in_the_order_they_were_asked(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let filter = petg_filter(&pool).await;

    let json = resolved(pool.clone(), twelve(&filter.to_string())).await;

    assert_eq!(
        keys(&json),
        ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"],
        "the results come back in the order asked, whatever order they finished in"
    );
    assert_eq!(statuses(&json), ["ok"; 12], "{json}");

    // One value per kind, so the response is asserted and not only its shape.
    assert_eq!(value(&json, "a")["kind"], "storage");
    assert_eq!(value(&json, "a")["value"]["sourceBytes"], 3 * 91_204);
    assert_eq!(value(&json, "b")["kind"], "instanceStorage");
    assert_eq!(
        value(&json, "b")["value"]["onDiskBytes"],
        serde_json::Value::Null,
        "the widget skips the disk walk the storage page offers"
    );
    assert_eq!(value(&json, "c")["kind"], "recent");
    assert_eq!(
        value(&json, "c")["value"].as_array().expect("cards").len(),
        3
    );
    assert_eq!(value(&json, "d")["kind"], "savedFilter");
    assert_eq!(value(&json, "d")["value"]["name"], "PETG, printed");
    assert_eq!(
        value(&json, "d")["value"]["parts"]
            .as_array()
            .expect("cards")
            .len(),
        2,
        "the two PETG parts, and not the steel one"
    );
    assert_eq!(value(&json, "e")["kind"], "facet");
    assert_eq!(
        value(&json, "e")["value"],
        serde_json::json!([
            { "value": "stl", "count": 2 },
            { "value": "step", "count": 1 },
        ]),
        "commonest first"
    );
    assert_eq!(
        value(&json, "f")["value"],
        serde_json::json!([
            { "value": "PETG", "count": 2 },
            { "value": "Steel, 316L", "count": 1 },
        ])
    );
    assert_eq!(
        value(&json, "g")["value"],
        serde_json::json!([
            { "value": "printed", "count": 2 },
            { "value": "review", "count": 1 },
        ])
    );
    assert_eq!(value(&json, "h")["kind"], "queue");
    assert_eq!(
        value(&json, "h")["value"],
        serde_json::json!({ "pending": 1, "running": 1, "failed": 1 })
    );
    assert_eq!(value(&json, "i")["kind"], "duplicates");
    assert_eq!(
        value(&json, "i")["value"],
        serde_json::json!({ "clusters": 0, "unprofiled": 3 }),
        "nothing is profiled yet, and the tile says so rather than claiming no duplicates"
    );
    assert_eq!(
        value(&json, "l")["value"],
        serde_json::json!([{ "value": "stl", "count": 2 }]),
        "a facet limit of one is the commonest value alone"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_body_that_asks_for_no_widget_is_refused_whole(pool: sqlx::PgPool) {
    let (status, json) = resolve(pool, vec![]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert!(json["results"].is_null(), "{json}");
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("1 to 32"),
        "{json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn thirty_three_widgets_are_refused_whole(pool: sqlx::PgPool) {
    corpus(&pool).await;
    let widgets: Vec<serde_json::Value> = (0..33)
        .map(|n| ask(&format!("tile-{n}"), storage(SEEDED_LIBRARY)))
        .collect();

    let (status, json) = resolve(pool, widgets).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert!(json["results"].is_null(), "not one key is answered: {json}");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn one_key_asked_twice_is_refused_whole(pool: sqlx::PgPool) {
    corpus(&pool).await;
    let widgets = vec![
        ask("storage", storage(SEEDED_LIBRARY)),
        ask("queue", queue(SEEDED_LIBRARY)),
        ask("storage", instance_storage()),
    ];

    let (status, json) = resolve(pool, widgets).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert!(json["results"].is_null(), "{json}");
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("storage"),
        "the message names the key that repeated: {json}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_unknown_library_fails_only_its_own_keys(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let widgets = vec![
        // `duplicates` first, because it is the one that would not have noticed: G3's `clusters`
        // answers an empty queue for a library that does not exist rather than an error, so a tile
        // without its own existence check would read as "no duplicates here".
        ask("gone-duplicates", duplicates(NO_SUCH_LIBRARY)),
        ask("gone-storage", storage(NO_SUCH_LIBRARY)),
        ask("gone-queue", queue(NO_SUCH_LIBRARY)),
        ask("gone-recent", recent(NO_SUCH_LIBRARY, 6)),
        ask("gone-facet", facet(NO_SUCH_LIBRARY, "tag", 6)),
        ask("here-recent", recent(SEEDED_LIBRARY, 6)),
        ask("here-queue", queue(SEEDED_LIBRARY)),
    ];

    let json = resolved(pool, widgets).await;

    for key in [
        "gone-duplicates",
        "gone-storage",
        "gone-queue",
        "gone-recent",
        "gone-facet",
    ] {
        assert!(
            message(&json, key).contains("No library with that id exists"),
            "{key}"
        );
    }
    assert_eq!(
        value(&json, "here-recent")["value"]
            .as_array()
            .expect("cards")
            .len(),
        3,
        "the library that is there still answers"
    );
    assert_eq!(
        value(&json, "here-queue")["value"],
        serde_json::json!({ "pending": 1, "running": 1, "failed": 1 })
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_saved_filter_that_is_gone_fails_only_its_key(pool: sqlx::PgPool) {
    corpus(&pool).await;
    let filter = petg_filter(&pool).await;
    assert!(
        PgSavedFilters(pool.clone())
            .remove(library(), filter)
            .await
            .expect("removes the filter")
    );
    let widgets = vec![
        ask(
            "filter",
            saved_filter(SEEDED_LIBRARY, &filter.to_string(), 6),
        ),
        ask("recent", recent(SEEDED_LIBRARY, 6)),
    ];

    let json = resolved(pool, widgets).await;

    assert!(
        message(&json, "filter").contains("no longer in the library"),
        "{json}"
    );
    assert_eq!(statuses(&json), ["failed", "ok"]);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_saved_filter_on_a_deleted_category_fails_its_key_rather_than_showing_nothing(
    pool: sqlx::PgPool,
) {
    corpus(&pool).await;
    let folder = PgFolders(pool.clone())
        .create(library(), None, "Fixtures", "fixtures")
        .await
        .expect("makes a category");
    let filter = PgSavedFilters(pool.clone())
        .create(
            library(),
            "Fixtures, printed",
            &serde_json::json!({ "folderId": folder.to_string(), "tag": "printed" }),
        )
        .await
        .expect("saves the filter");
    PgFolders(pool.clone())
        .soft_delete_subtree(folder)
        .await
        .expect("deletes the category");

    let json = resolved(
        pool,
        vec![ask(
            "filter",
            saved_filter(SEEDED_LIBRARY, &filter.to_string(), 6),
        )],
    )
    .await;

    let said = message(&json, "filter");
    assert!(said.contains("Fixtures, printed"), "{said}");
    assert!(
        said.contains("category that has been deleted"),
        "an empty tile would read as “nothing matches”: {said}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_saved_filter_answers_under_the_name_it_has_now(pool: sqlx::PgPool) {
    corpus(&pool).await;
    let filter = petg_filter(&pool).await;
    assert!(
        PgSavedFilters(pool.clone())
            .rename(library(), filter, "PETG only")
            .await
            .expect("renames the filter")
    );

    let json = resolved(
        pool,
        vec![ask(
            "filter",
            saved_filter(SEEDED_LIBRARY, &filter.to_string(), 6),
        )],
    )
    .await;

    assert_eq!(value(&json, "filter")["value"]["name"], "PETG only");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_card_widget_asking_past_twelve_gets_twelve(pool: sqlx::PgPool) {
    for n in 1..=14u8 {
        seed(
            &pool,
            &format!("Washer, M{n}"),
            &format!("hardware/washer-m{n}.stl"),
            "stl",
            u32::from(n),
        )
        .await;
    }
    let filter = PgSavedFilters(pool.clone())
        .create(
            library(),
            "Every washer",
            &serde_json::json!({ "q": "Washer" }),
        )
        .await
        .expect("saves the filter");

    let json = resolved(
        pool,
        vec![
            ask("recent", recent(SEEDED_LIBRARY, 200)),
            ask(
                "filter",
                saved_filter(SEEDED_LIBRARY, &filter.to_string(), 200),
            ),
            ask("none", recent(SEEDED_LIBRARY, 0)),
        ],
    )
    .await;

    assert_eq!(
        value(&json, "recent")["value"]
            .as_array()
            .expect("cards")
            .len(),
        12,
        "phase-6 caps `recent` at twelve, and `limit: u8` cannot carry that"
    );
    assert_eq!(
        value(&json, "filter")["value"]["parts"]
            .as_array()
            .expect("cards")
            .len(),
        12,
        "and `savedFilter` at twelve"
    );
    assert_eq!(
        value(&json, "none")["value"]
            .as_array()
            .expect("cards")
            .len(),
        1,
        "a limit of zero is one card, not an empty tile"
    );
}

/// G4 stage 2: one key blocked on a lock times out after about two seconds, and the other eleven
/// answer. The whole response is not held to the slowest tile's budget plus the others' — the eleven
/// went through the three free permits while the locked one held the fourth.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_locked_queue_times_out_its_own_key_while_the_other_eleven_answer(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let filter = petg_filter(&pool).await;
    let app = pool_of(&pool, 8).await;
    let held = lock_the_queue(&pool).await;

    let mut widgets = eleven_that_never_read_the_queue(&filter.to_string());
    widgets.insert(0, ask("queue", queue(SEEDED_LIBRARY)));
    let began = Instant::now();
    let json = resolved(app.clone(), widgets).await;
    let took = began.elapsed();

    held.rollback().await.expect("releases the lock");
    app.close().await;

    assert_eq!(answer(&json, "queue")["status"], "timedOut", "{json}");
    let answered = statuses(&json)
        .iter()
        .filter(|s| s.as_str() == "ok")
        .count();
    assert_eq!(answered, 11, "the other eleven answered: {json}");
    assert!(
        took >= Duration::from_millis(1_900) && took < Duration::from_secs(4),
        "one key's two seconds, not eleven keys' worth: {took:?}"
    );
}

/// Each key's clock starts once it holds a permit. Eight keys block on the lock and take every permit
/// for two rounds; the ninth gets one only when they have all given up, about four seconds in, and
/// must still have its own [`PER_KEY`] then. Counted from when it was asked, it would report a timeout
/// it never had.
///
/// **Eight blocked keys rather than the four this test was written with** (goal L4). The wait the
/// ninth key survives has to be longer than one key's own budget, or a clock started at spawn would
/// still have time left when the permit arrived and the test would pass on the mutation it exists to
/// catch. With the lock ceiling at two seconds and `PER_KEY` at three, four blocked keys are a
/// two-second wait and not enough; two rounds of four are four seconds and are.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_keys_own_budget_starts_when_it_starts_and_not_when_it_was_asked(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    // Eight connections, as production has: the blocked keys must not be able to starve the last one
    // of a connection, or this would be asserting the pool rather than the clock.
    let app = pool_of(&pool, 8).await;
    let held = lock_the_queue(&pool).await;

    let mut widgets: Vec<serde_json::Value> = (1..=8)
        .map(|n| ask(&format!("blocked-{n}"), queue(SEEDED_LIBRARY)))
        .collect();
    widgets.push(ask("last", storage(SEEDED_LIBRARY)));
    let began = Instant::now();
    let json = resolved(app.clone(), widgets).await;
    let took = began.elapsed();

    held.rollback().await.expect("releases the lock");
    app.close().await;

    let mut expected = vec!["timedOut"; 8];
    expected.push("ok");
    assert_eq!(
        statuses(&json),
        expected,
        "the last key waited about four seconds for a permit and still got its own budget: {json}"
    );
    assert!(took >= Duration::from_millis(3_900), "{took:?}");
    assert_eq!(value(&json, "last")["kind"], "storage");
}

/// Four keys at a time, and the semaphore is what makes that true. Twelve keys all blocked on one lock
/// go in three rounds of four, each round spending its own two seconds — about six seconds in all.
/// Without the permit limit every key would start at once and the whole thing would be over in two.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn twelve_blocked_keys_go_four_at_a_time(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let app = pool_of(&pool, 8).await;
    let held = lock_the_queue(&pool).await;

    let widgets: Vec<serde_json::Value> = (0..12)
        .map(|n| ask(&format!("tile-{n}"), queue(SEEDED_LIBRARY)))
        .collect();
    let began = Instant::now();
    let json = resolved(app.clone(), widgets).await;
    let took = began.elapsed();

    held.rollback().await.expect("releases the lock");
    app.close().await;

    assert_eq!(statuses(&json), ["timedOut"; 12], "{json}");
    // Five seconds rather than the 3.5 this was written with (goal L4). Without the permit limit the
    // twelve keys race for eight connections, and where they used to all give up at two seconds —
    // ours, on the pool — the four that wait now get a connection when the first eight are cancelled
    // and block for two more, landing at about three. Six seconds against three wants a threshold
    // between them with room on both sides, not one just under the lower number.
    assert!(
        took >= Duration::from_millis(5_000),
        "twelve keys four at a time is three rounds of two seconds, not one: {took:?}"
    );
}

/// **G4's ceiling, closed.** `a_key_that_gave_up_still_holds_its_connection_until_the_lock_clears`
/// asserted the opposite of this on the same pool of one: our two seconds cancelled the future, the
/// statement went on running on the server, and the next read had nowhere to go until the lock
/// cleared.
///
/// Now the resolve itself is the thing being asked, on a pool of **one**, against a locked table: the
/// widget times out and the connection is back in the pool immediately, so a second resolve of a
/// widget that reads something else answers while the lock is *still held*. Before the ceiling the
/// second resolve could only have timed out too — on the pool, not on the lock.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_widget_that_timed_out_leaves_the_pool_the_connection_it_was_using(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let one = pool_of(&pool, 1).await;
    let held = lock_the_queue(&pool).await;

    let blocked = resolved(one.clone(), vec![ask("queue", queue(SEEDED_LIBRARY))]).await;

    let began = Instant::now();
    let after = resolved(one.clone(), vec![ask("storage", storage(SEEDED_LIBRARY))]).await;
    let answered = began.elapsed();

    held.rollback().await.expect("releases the lock");
    one.close().await;

    assert_eq!(statuses(&blocked), ["timedOut"], "{blocked}");
    assert_eq!(
        statuses(&after),
        ["ok"],
        "the one connection was free again while the lock was still held: {after}"
    );
    assert!(
        answered < Duration::from_millis(1_500),
        "and free immediately rather than at the next ceiling: {answered:?}"
    );
}

/// **The thing that could not be tested before.** A locked table, resolve after resolve, and the
/// pool still has connections in it at the end.
///
/// What the old arrangement did was *accumulate*: each key that gave up kept its connection until
/// the lock cleared, so four keys a round became four, then eight, then twelve held at once, and the
/// api ran out. So the shape of this test is rounds, not volume — one resolve of four blocked keys at
/// a time, four times over, with somebody else reading throughout. Sixteen keys give up in all,
/// against a pool of eight holding a stand-in for `events.rs`'s `PgListener`, which costs the api one
/// of its eight for as long as the process runs (found by G1). Four of the seven usable connections
/// are busy at any moment and three are not, and the probe asserts exactly that: a plain `SELECT 1`
/// answers in milliseconds every time. On the old code it would have answered for the first round and
/// then waited on the pool until its `acquire_timeout`.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn however_many_resolves_meet_a_locked_table_the_pool_still_has_connections(
    pool: sqlx::PgPool,
) {
    corpus(&pool).await;
    queued(&pool).await;
    let app = pool_of(&pool, 8).await;
    // The api's event listener, as a cost: one connection, held until the test is over.
    let listener = app.acquire().await.expect("the listener's connection");
    let held = lock_the_queue(&pool).await;

    let probing = app.clone();
    let stop = tokio_util::sync::CancellationToken::new();
    let until = stop.clone();
    let probe = tokio::spawn(async move {
        let mut worst = Duration::ZERO;
        let mut taken = 0u32;
        while !until.is_cancelled() {
            let began = Instant::now();
            let answered = sqlx::query("SELECT 1").fetch_one(&probing).await;
            let took = began.elapsed();
            assert!(
                answered.is_ok(),
                "the pool starved after {taken} answered probes: {answered:?}"
            );
            worst = worst.max(took);
            taken += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        (taken, worst)
    });

    const ROUNDS: usize = 4;
    for round in 0..ROUNDS {
        let widgets: Vec<serde_json::Value> = (0..4)
            .map(|n| ask(&format!("tile-{round}-{n}"), queue(SEEDED_LIBRARY)))
            .collect();
        let json = resolved(app.clone(), widgets).await;
        assert_eq!(
            statuses(&json),
            ["timedOut"; 4],
            "round {round}: every key is blocked on the lock, and none of them failed on the \
             pool: {json}"
        );
    }

    stop.cancel();
    let (probes, worst) = probe.await.expect("the probe did not panic");
    drop(listener);
    held.rollback().await.expect("releases the lock");
    app.close().await;

    assert!(
        probes >= u32::try_from(ROUNDS).expect("four"),
        "the probe should have run throughout every round, not {probes} times"
    );
    // The probe erroring is what the pool's 10 s `acquire_timeout` would turn starvation into, and it
    // is asserted inside the loop. This says the pool was never even close: four cancelled lock waits
    // at a time leave three of the seven connections free, so nobody else waits at all.
    assert!(
        worst < Duration::from_millis(500),
        "somebody else's read waited {worst:?} for one of the three connections the resolves were \
         not using"
    );
}

/// Three dashboards open at once. Three resolves of twelve keys share the pool of eight, and all three
/// answer every key. The permit limit is per request, so twelve keys can be in flight against eight
/// connections: this passes because these reads are fast, not because the pool is held back — see
/// `a_key_that_gave_up_still_holds_its_connection_until_the_lock_clears` for what a slow one costs.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn three_resolves_at_once_all_answer_every_key(pool: sqlx::PgPool) {
    corpus(&pool).await;
    queued(&pool).await;
    let filter = petg_filter(&pool).await;
    let app = pool_of(&pool, 8).await;

    let (one, two, three) = tokio::join!(
        resolved(app.clone(), twelve(&filter.to_string())),
        resolved(app.clone(), twelve(&filter.to_string())),
        resolved(app.clone(), twelve(&filter.to_string())),
    );

    app.close().await;
    for json in [one, two, three] {
        assert_eq!(statuses(&json), ["ok"; 12], "{json}");
    }
}

/// The p50 of a twelve-widget resolve on a library of 10,000 parts, which is G4's measured figure.
/// `#[ignore]`d for G3's reason: the gate should not pay the seeding to learn a number that belongs in
/// the goal's Record. Run it with
/// `cargo xtask heavy -- cargo test --release -p lapidary-api --test dashboard -- --ignored --nocapture`.
///
/// The corpus is deliberately duplicate-free — every descriptor is 0.05 apart and
/// `NEAR_DUPLICATE_DISTANCE` is 0.04 — while the sizes are 0.05 mm apart, so the near-duplicate sweep
/// still compares every neighbour inside the ±2% size band and then finds nothing. A library that *has*
/// duplicates pays what G3 measured on top of this: cards for every part in every cluster.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
#[ignore = "a measurement; see the goal's Record for the numbers"]
async fn ten_thousand_parts_resolve_twelve_widgets_inside_the_budget(pool: sqlx::PgPool) {
    use lapidary_core::{DESCRIPTOR_LEN, ShapeProfile};
    use lapidary_db::{PgRevisions, PgShapes};

    const PARTS: usize = 10_000;

    let seeding = Instant::now();
    // In chunks, not all at once: 10,000 tasks queueing on a five-connection pool would pass sqlx's
    // acquire timeout before they ever ran (G3 found this).
    for chunk in (0..PARTS).collect::<Vec<usize>>().chunks(32) {
        let mut seeds = tokio::task::JoinSet::new();
        for n in chunk.iter().copied() {
            let pool = pool.clone();
            seeds.spawn(async move {
                // A preview of about the size a real one is, so a card carries the base64 a real
                // card carries.
                let thumbnail = vec![0x57u8; 3_072];
                let path = format!("bay-{:02}/washer-{n:05}.stl", n % 64);
                let part = seed_with(
                    &pool,
                    &format!("Washer, {n:05}"),
                    &path,
                    if n % 7 == 0 { "step" } else { "stl" },
                    n as u32,
                    Some(&thumbnail),
                )
                .await;
                let mut descriptor = [0.12_f32; DESCRIPTOR_LEN];
                descriptor[0] += n as f32 * 0.05;
                let revision = PgRevisions(pool.clone())
                    .current(library(), &path)
                    .await
                    .expect("reads the current revision")
                    .expect("the part has one")
                    .revision;
                assert!(
                    PgShapes(pool.clone())
                        .record(
                            part,
                            revision,
                            BlobHash::from_bytes([0x71; 32]),
                            &ShapeProfile {
                                size_mm: 10.0 + n as f64 * 0.05,
                                descriptor,
                            },
                        )
                        .await
                        .expect("records the profile")
                );
                // A third of the library carries a tag and a material, so the facet tiles have values
                // to rank. Every part would be 20,000 more round trips for the same three numbers.
                if n % 3 == 0 && n < 900 {
                    let parts = PgParts(pool.clone());
                    let tag = ["printed", "review", "wip"][n % 9 / 3];
                    assert!(parts.set_tags(part, &[tag.to_owned()]).await.expect("tags"));
                    assert!(
                        parts
                            .set_materials(
                                part,
                                &[["PETG", "PLA", "Steel, 316L"][n % 9 / 3].to_owned()]
                            )
                            .await
                            .expect("materials")
                    );
                }
            });
        }
        while let Some(seeded) = seeds.join_next().await {
            seeded.expect("the seed task finishes");
        }
    }
    // A filter that matches the whole library, so the tile hands back its full twelve cards.
    let filter = PgSavedFilters(pool.clone())
        .create(
            library(),
            "Every washer",
            &serde_json::json!({ "q": "Washer" }),
        )
        .await
        .expect("saves the filter");
    queued(&pool).await;
    let seeded = seeding.elapsed();

    let app = pool_of(&pool, 8).await;
    // Warm first: the first resolve pays for the connections and the plans.
    let first = resolved(app.clone(), twelve(&filter.to_string())).await;
    assert_eq!(statuses(&first), ["ok"; 12], "{first}");
    println!(
        "seeded {PARTS} profiled parts in {:.1?}; duplicates tile {}, savedFilter cards {}",
        seeded,
        value(&first, "i")["value"],
        value(&first, "d")["value"]["parts"]
            .as_array()
            .expect("cards")
            .len()
    );

    let mut runs = Vec::new();
    for _ in 0..7 {
        let began = Instant::now();
        let json = resolved(app.clone(), twelve(&filter.to_string())).await;
        runs.push(began.elapsed());
        assert_eq!(statuses(&json), ["ok"; 12], "{json}");
    }
    app.close().await;

    runs.sort();
    let p50 = runs[runs.len() / 2];
    println!(
        "twelve widgets: p50 {p50:.1?} over {} runs ({runs:.1?})",
        runs.len()
    );
    assert!(
        p50 < Duration::from_millis(1_500),
        "a dashboard settles in one round trip, and every key has 2 s of its own: p50 {p50:.1?}"
    );
}
