//! The dashboard (Phase 6; design in `docs/goals/phase-6.md`): what a widget is, and the one route that answers
//! all of them, `POST /api/dashboard/resolve` (goal G4).
//!
//! [`Widget`] is the dashboard's config schema: the web keys its widget registry by this enum's `kind`, so a
//! kind added here fails the web's type check until the web knows how to draw it. A layout is stored per browser
//! (until Phase 8 brings users), and opening the dashboard costs one resolve. There is no per-widget endpoint and
//! no polling: a widget is asked again when the event stream says its library changed.

use crate::AppState;
use crate::filters::FilterSearch;
use crate::parts::{
    FacetValue, GridRefusal, InstanceStorageView, LibraryStorage, PartCard, grid_rows, to_card,
};
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use lapidary_core::{LibraryId, SavedFilterId};
use lapidary_db::{DbError, PgDashboard, PgParts, PgPool, PgSavedFilters, Shows, Sort};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use ts_rs::TS;

/// The most widgets one resolve carries. A layout past this is refused whole rather than answered in
/// part: a runaway layout must not be able to hold the pool's connections.
const MAX_WIDGETS: usize = 32;

/// How many keys run at once. Below the pool's `max_connections(8)` on purpose — see [`resolve`].
const AT_ONCE: usize = 4;

/// How long one key gets, counted from when it starts rather than from when it was asked.
///
/// **The outer guard, not the mechanism** (goal L4). What ends a key's *read* is the api pool's
/// `lock_timeout` of two seconds and `statement_timeout` of five (`lapidary_db::INTERACTIVE`),
/// server-side, which is the only kind of timeout that frees the connection as well as the caller.
/// A widget's budget for a database read is therefore still the two seconds G4 gave it.
///
/// **Three seconds rather than two, deliberately, and the order is the whole reason.** Were this
/// equal to the lock ceiling, which of the two ended a blocked widget would come down to a few
/// milliseconds — and our clock would usually win, because it starts before the statement does. The
/// key would then be ended by a dropped future, which abandons a running statement, which is the bug
/// this goal exists to remove. A second of daylight puts the server first every time and leaves this
/// for what only it can catch: a key waiting for a permit or for a connection, where there is no
/// statement to abandon.
const PER_KEY: Duration = Duration::from_secs(3);

/// The most cards a card-carrying widget hands back. `phase-6.md` gives `recent` and `savedFilter`
/// twelve, and [`Widget`]'s `limit: u8` cannot carry that, so it is enforced here.
const MOST_CARDS: u8 = 12;

/// The most values a [`Widget::Facet`] hands back. The design sets no figure; a tile that lists more
/// than this is a list nobody reads, and a facet value is cheap but not free.
const MOST_FACET_VALUES: u8 = 24;

/// One widget, as a layout stores it and a resolve asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Widget {
    /// What one library costs on disk: [`LibraryStorage`].
    Storage { library: LibraryId },
    /// What the whole installation costs, the storage page's figures: [`InstanceStorageView`].
    InstanceStorage,
    /// A library's newest parts, at most 12.
    Recent { library: LibraryId, limit: u8 },
    /// The parts one saved filter finds, at most 12.
    SavedFilter {
        library: LibraryId,
        filter: SavedFilterId,
        limit: u8,
    },
    /// The commonest values of one facet in a library.
    Facet {
        library: LibraryId,
        facet: FacetKind,
        limit: u8,
    },
    /// A library's work in the queue.
    Queue { library: LibraryId },
    /// How many groups of near-duplicates a library holds, waiting for a person to decide.
    Duplicates { library: LibraryId },
}

/// Which facet a [`Widget::Facet`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FacetKind {
    Format,
    Material,
    Tag,
}

/// One widget asked for, under the key the layout knows it by.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct WidgetRequest {
    pub key: String,
    pub widget: Widget,
}

/// The `POST /api/dashboard/resolve` body: 1 to 32 widgets, each key once. Anything else is refused whole (422),
/// so a runaway layout cannot hold the database's connections.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ResolveRequest {
    pub widgets: Vec<WidgetRequest>,
}

/// The answer: one [`KeyResult`] per key, in the order asked. Always 200 once the body is valid — a widget
/// that failed or ran out of time says so in its own result, and the others still arrive.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolveResponse {
    pub results: Vec<KeyResult>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyResult {
    pub key: String,
    pub result: WidgetResult,
}

/// How one widget came out.
#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum WidgetResult {
    Ok {
        value: WidgetValue,
    },
    /// It did not answer within its 2 seconds, counted from when it started rather than from when it was asked.
    TimedOut,
    /// It could not be answered: a library or saved filter that does not exist, or a query that failed. The
    /// message says which, in words a person can act on.
    Failed {
        message: String,
    },
}

/// A widget's value, tagged by the same `kind` as the [`Widget`] it answers.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum WidgetValue {
    Storage(LibraryStorage),
    InstanceStorage(InstanceStorageView),
    Recent(Vec<PartCard>),
    SavedFilter(FilteredParts),
    Facet(Vec<FacetValue>),
    Queue(QueueSummary),
    Duplicates(DuplicateSummary),
}

/// A saved filter's parts, with its name as it is now — renamed since the layout was saved, it shows the new
/// one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilteredParts {
    pub name: String,
    pub parts: Vec<PartCard>,
}

/// A library's jobs that are not finished, and the ones that failed.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSummary {
    pub pending: u32,
    pub running: u32,
    pub failed: u32,
}

/// The duplicates review queue in brief; the duplicates page holds the groups themselves.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateSummary {
    pub clusters: u32,
    /// Parts with no shape profile yet, so not compared.
    pub unprofiled: u32,
}

/// `POST /api/dashboard/resolve` — every widget on the screen, in one round trip.
///
/// The body is refused whole (422) when it carries no widget, more than [`MAX_WIDGETS`], or a key
/// twice; anything else answers 200, and a widget that failed or ran out of time says so in its own
/// result while the others still arrive. A key never fails the request: a dashboard of twelve tiles
/// where one library has been deleted shows eleven tiles and one message.
///
/// **The semaphore and the timeout are one mechanism, in this order.** A [`JoinSet`] spawns every key
/// at once, [`AT_ONCE`] of them hold a permit, and each key's [`PER_KEY`] starts *after* it has that
/// permit. Both halves matter: the pool is `max_connections(8)`, so without the permit limit twelve
/// keys would race for eight connections and the slow ones would report a timeout they spent waiting
/// for a connection rather than for their own query — and with the permit limit but a clock started at
/// spawn, a key that queued behind three slow ones would report the same lie from the other side.
/// `tests/dashboard.rs` holds both distinctions.
///
/// **What stops a key is the server, not [`PER_KEY`]** (goal L4). `lapidary_db::INTERACTIVE` puts
/// `lock_timeout = 2 s` and `statement_timeout = 5 s` on every connection of the api's pool, so a
/// widget held up by a locked table is cancelled *by PostgreSQL* at two seconds and gives its
/// connection straight back — which is what makes the twelfth simultaneous resolve of a locked table
/// leave the pool with connections in it. [`PER_KEY`] stays as the outer guard for a key that is
/// waiting on something other than its own statement (a permit, or a connection), where dropping the
/// future costs nothing because there is no statement running to abandon.
///
/// ponytail: a key stopped by `statement_timeout` rather than by `lock_timeout` — a read that is
/// genuinely slow instead of blocked — is reported at [`PER_KEY`] and its statement runs on for up to
/// three seconds more, still holding its connection. Bounded, where it used to be open-ended, and no
/// read in this file has ever been measured within an order of magnitude of either number. Closing it
/// would mean the resolve setting its own `statement_timeout` per key, which needs the resolvers to
/// hold a connection rather than take the pool, and that is `repo.rs`'s signature rather than this
/// file's.
async fn resolve(
    State(app): State<AppState>,
    body: Result<Json<ResolveRequest>, JsonRejection>,
) -> Response {
    let widgets = match body {
        Ok(Json(body)) => body.widgets,
        Err(rejection) => return bad_body(&rejection),
    };
    if widgets.is_empty() || widgets.len() > MAX_WIDGETS {
        return refused(&format!(
            "A dashboard resolve asks for 1 to {MAX_WIDGETS} widgets, and this one asks for \
             {}. Send the widgets on the screen, in one request.",
            widgets.len()
        ));
    }
    let mut seen = HashSet::with_capacity(widgets.len());
    if let Some(twice) = widgets
        .iter()
        .find(|asked| !seen.insert(asked.key.as_str()))
    {
        return refused(&format!(
            "Two widgets in this resolve share the key “{}”. The answers come back keyed, \
             so a repeated key could not be told apart — give every widget in the layout its own.",
            twice.key
        ));
    }
    drop(seen);

    let permits = Arc::new(Semaphore::new(AT_ONCE));
    let mut keys: Vec<String> = Vec::with_capacity(widgets.len());
    let mut running = JoinSet::new();
    for (at, WidgetRequest { key, widget }) in widgets.into_iter().enumerate() {
        keys.push(key.clone());
        let app = app.clone();
        let permits = Arc::clone(&permits);
        running.spawn(async move {
            // The permit first, then the clock: see this function's doc.
            let _permit = permits.acquire().await;
            let result = match tokio::time::timeout(PER_KEY, value_of(&app, &widget)).await {
                Ok(Ok(value)) => WidgetResult::Ok { value },
                Ok(Err(WidgetRefusal::TimedOut)) => WidgetResult::TimedOut,
                Ok(Err(WidgetRefusal::Failed(message))) => WidgetResult::Failed { message },
                // The outer guard fired, so this key was waiting on something that is not a
                // statement — `PER_KEY` is longer than the api's `lock_timeout` and a cancelled
                // statement comes back through `failed`, which logs its own line. Said out loud
                // anyway: an unexplained tile is what made the connection G4 found invisible.
                Err(_elapsed) => {
                    tracing::warn!(
                        key = %key,
                        widget = ?widget,
                        "a dashboard widget ran out of time without its read being stopped, so it \
                         was waiting for a permit or a connection rather than for the database"
                    );
                    WidgetResult::TimedOut
                }
            };
            (at, result)
        });
    }
    let mut answers: Vec<Option<WidgetResult>> = keys.iter().map(|_| None).collect();
    while let Some(finished) = running.join_next().await {
        match finished {
            Ok((at, result)) => answers[at] = Some(result),
            // A resolver that panicked takes its own key down and nothing else. Which key it was is
            // not recoverable from the join error, so every unanswered key gets the same message; the
            // log line below is where the panic itself is.
            Err(err) => tracing::error!(error = %err, "a dashboard widget panicked"),
        }
    }
    Json(ResolveResponse {
        results: keys
            .into_iter()
            .zip(answers)
            .map(|(key, result)| KeyResult {
                key,
                result: result.unwrap_or_else(|| WidgetResult::Failed {
                    message: "This widget could not be answered at all. Reload the dashboard, \
                              and check the server logs if it says this again."
                        .to_owned(),
                }),
            })
            .collect(),
    })
    .into_response()
}

/// Why a widget has no value.
///
/// Two answers rather than one string, because the wire type has had two answers since W0 and a read
/// the server cancelled belongs in the second one. A statement PostgreSQL stopped for passing
/// `lapidary_db::INTERACTIVE` **is** this widget running out of time — reporting it as `failed` with
/// a message would hide a timeout inside the wording of a fault, and would make which of the two a
/// locked table produces depend on whether our clock or the server's ran out first.
enum WidgetRefusal {
    /// The server stopped the read. `KeyResult` carries no message for this — the tile says it timed
    /// out, and the one line naming the widget is in the log.
    TimedOut,
    /// Anything else, as what a person can do about it.
    Failed(String),
}

/// One widget's value, or the reason there is none.
///
/// Every arm is a read that already exists somewhere in this crate, called with the dashboard's own
/// limits. Nothing here writes, and nothing here touches a source file.
async fn value_of(app: &AppState, widget: &Widget) -> Result<WidgetValue, WidgetRefusal> {
    match widget {
        Widget::Storage { library } => storage(&app.db, *library).await,
        Widget::InstanceStorage => instance_storage(app).await,
        Widget::Recent { library, limit } => recent(&app.db, *library, *limit).await,
        Widget::SavedFilter {
            library,
            filter,
            limit,
        } => saved_filter(&app.db, *library, *filter, *limit).await,
        Widget::Facet {
            library,
            facet,
            limit,
        } => one_facet(&app.db, *library, *facet, *limit).await,
        Widget::Queue { library } => queue(&app.db, *library).await,
        Widget::Duplicates { library } => duplicates(&app.db, *library).await,
    }
}

/// [`Widget::Storage`] — `GET /api/libraries/{id}/storage`'s figures. Its own read says when the
/// library is not there, so it needs no probe.
async fn storage(db: &PgPool, library: LibraryId) -> Result<WidgetValue, WidgetRefusal> {
    match PgParts(db.clone()).storage_totals(library).await {
        Ok(Some(totals)) => Ok(WidgetValue::Storage(crate::parts::library_storage(totals))),
        Ok(None) => Err(no_such_library()),
        Err(err) => Err(failed(&err, "dashboard library storage read failed")),
    }
}

/// [`Widget::InstanceStorage`] — the storage page's figures without its disk walk, and without its
/// flush of the pending reads: a widget is a read, and the render-cache figure is the one it affects,
/// at most five minutes behind. `GET /api/storage` is where an exact one is.
async fn instance_storage(app: &AppState) -> Result<WidgetValue, WidgetRefusal> {
    match PgParts(app.db.clone()).instance_storage().await {
        Ok(totals) => Ok(WidgetValue::InstanceStorage(
            crate::parts::instance_storage_view(totals, None, app.host_storage_root.clone()),
        )),
        Err(err) => Err(failed(&err, "dashboard instance storage read failed")),
    }
}

/// [`Widget::Recent`] — the newest parts, the grid's own read with no filters.
async fn recent(db: &PgPool, library: LibraryId, limit: u8) -> Result<WidgetValue, WidgetRefusal> {
    exists(db, library).await?;
    let rows = grid_rows(
        db,
        library,
        &FilterSearch::default(),
        None,
        u16::from(capped(limit, MOST_CARDS)),
        Shows::Live,
        Sort::Newest,
    )
    .await
    .map_err(grid_failed)?;
    Ok(WidgetValue::Recent(rows.into_iter().map(to_card).collect()))
}

/// [`Widget::SavedFilter`] — what one saved filter finds, and its name as it is now.
///
/// The library's filters are read in full and this one picked out, which is `GET
/// /api/libraries/{id}/filters`'s own query: it is the read that already knows whether the category a
/// filter names is still there, and a library holds a handful of filters, not a page of them.
async fn saved_filter(
    db: &PgPool,
    library: LibraryId,
    filter: SavedFilterId,
    limit: u8,
) -> Result<WidgetValue, WidgetRefusal> {
    let saved = PgSavedFilters(db.clone())
        .list(library)
        .await
        .map_err(|err| failed(&err, "dashboard saved filter read failed"))?;
    // An unknown library has no filters, so this one answer covers both: neither a deleted library nor
    // a deleted filter leaves anything for the widget to show.
    let Some(saved) = saved.into_iter().find(|row| row.id == filter) else {
        return Err(WidgetRefusal::Failed(
            "This saved filter is no longer in the library — it, or the library, has been \
             deleted. Point the widget at another filter, or remove it from the dashboard."
                .to_owned(),
        ));
    };
    // The grid opened on such a filter says the category is gone rather than showing an empty grid
    // (`filters.rs`), and `FilteredParts` has no room to say it — so the key fails instead of showing
    // an empty tile, which would read as "nothing matches".
    if saved.folder_gone {
        return Err(WidgetRefusal::Failed(format!(
            "“{}” filters on a category that has been deleted, so it can show nothing. \
             Edit the filter in the grid, or point this widget at another one.",
            saved.name
        )));
    }
    let search: FilterSearch = serde_json::from_str(&saved.search).map_err(|err| {
        tracing::error!(error = %err, filter = %saved.id, "a saved filter holds a search this build cannot read");
        WidgetRefusal::Failed(
            "This saved filter holds settings this version cannot read. Check the server logs \
             for which one, and save it again from the grid."
                .to_owned(),
        )
    })?;
    let rows = grid_rows(
        db,
        library,
        &search,
        None,
        u16::from(capped(limit, MOST_CARDS)),
        Shows::Live,
        Sort::Newest,
    )
    .await
    .map_err(grid_failed)?;
    Ok(WidgetValue::SavedFilter(FilteredParts {
        name: saved.name,
        parts: rows.into_iter().map(to_card).collect(),
    }))
}

/// [`Widget::Facet`] — the commonest values of one facet, unfiltered.
///
/// The facet reads answer in value order, so the sort here is what makes "commonest" true. Past
/// [`lapidary_db::EXACT_FACET_ROWS`] matching parts there is no count to sort by at all (`DATA.md`
/// §3.4 withholds them), and a stable sort then leaves the query's own order, which is by value — the
/// only order left.
async fn one_facet(
    db: &PgPool,
    library: LibraryId,
    facet: FacetKind,
    limit: u8,
) -> Result<WidgetValue, WidgetRefusal> {
    exists(db, library).await?;
    let parts = PgParts(db.clone());
    let read = match facet {
        FacetKind::Format => {
            parts
                .format_facet(library, None, None, Shows::Live, None, None, None, None)
                .await
        }
        FacetKind::Material => {
            parts
                .material_facet(library, None, None, Shows::Live, None, None, None, None)
                .await
        }
        FacetKind::Tag => {
            parts
                .tag_facet(library, None, None, Shows::Live, None, None, None, None)
                .await
        }
    };
    let mut values = read.map_err(|err| failed(&err, "dashboard facet read failed"))?;
    values.sort_by_key(|value| std::cmp::Reverse(value.count));
    values.truncate(usize::from(capped(limit, MOST_FACET_VALUES)));
    Ok(WidgetValue::Facet(
        values
            .into_iter()
            .map(|value| FacetValue {
                value: value.value,
                count: value.count,
            })
            .collect(),
    ))
}

/// [`Widget::Queue`] — what this library has waiting, running and failed.
async fn queue(db: &PgPool, library: LibraryId) -> Result<WidgetValue, WidgetRefusal> {
    exists(db, library).await?;
    let counts = PgDashboard(db.clone())
        .queue(library)
        .await
        .map_err(|err| failed(&err, "dashboard queue read failed"))?;
    Ok(WidgetValue::Queue(QueueSummary {
        pending: counts.pending,
        running: counts.running,
        failed: counts.failed,
    }))
}

/// [`Widget::Duplicates`] — how many groups of near-duplicates are waiting for somebody to decide.
///
/// ponytail: `likeness::clusters` builds a card, inline thumbnail and all, for every part in every
/// cluster, and this widget counts the groups and throws the cards away. One definition of a cluster
/// is worth that: G3 measured the whole read at 234 ms on a deliberately duplicate-heavy library of
/// 10,000 parts, mostly in the cards. A count-only read is the upgrade if a real library measures slow.
async fn duplicates(db: &PgPool, library: LibraryId) -> Result<WidgetValue, WidgetRefusal> {
    // This widget's own probe, and not belt-and-braces: `clusters` answers an empty queue for a
    // library that does not exist rather than an error (G3's Record says so), so without this an
    // unknown library would read as "no duplicates here" instead of failing its key.
    exists(db, library).await?;
    let found = crate::likeness::clusters(db, library, None)
        .await
        .map_err(|err| failed(&err, "dashboard duplicates read failed"))?;
    Ok(WidgetValue::Duplicates(DuplicateSummary {
        clusters: u32::try_from(found.clusters.len()).unwrap_or(u32::MAX),
        unprofiled: found.unprofiled,
    }))
}

/// Whether the library is there at all, through `PgParts::auto_thumbnail` — the existence probe
/// `derive.rs` and G3's routes already use.
///
/// Only the widgets whose own read cannot tell the difference ask this. `recent`, `facet` and `queue`
/// each answer emptily for an id naming nothing, and `duplicates` does too; `storage` and
/// `savedFilter` say so themselves, and `instanceStorage` names no library.
async fn exists(db: &PgPool, library: LibraryId) -> Result<(), WidgetRefusal> {
    match PgParts(db.clone()).auto_thumbnail(library).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(no_such_library()),
        Err(err) => Err(failed(&err, "dashboard library lookup failed")),
    }
}

/// `limit` inside `1..=most`. Clamped rather than refused: the body is otherwise valid, a widget is a
/// read, and 422ing a whole dashboard because one tile asked for twenty cards would take down eleven
/// tiles that were fine. Zero is one for `parts.rs`'s reason — a limit of nothing is nobody's ask.
fn capped(limit: u8, most: u8) -> u8 {
    limit.clamp(1, most)
}

/// A library id that names nothing. It fails its own key and no other.
fn no_such_library() -> WidgetRefusal {
    WidgetRefusal::Failed(
        "No library with that id exists. It may have been deleted since this dashboard was \
         arranged — point the widget at another library, or remove it."
            .to_owned(),
    )
}

/// A read that did not answer. The operator gets the detail through the log — once, naming this
/// widget, and at warn rather than error when the server stopped it on purpose
/// (`crate::error::log_db_error`); the key carries what a person can act on, exactly as `parts.rs`'s
/// `internal_error` decides.
///
/// **This is where the log line for an abandoned statement comes from.** Before the api's pool had a
/// ceiling, a widget held up by a lock was ended by our own clock and this function was never
/// reached: the only line was sqlx's own "slow statement" warning, which carries the SQL and names
/// neither the widget nor the fact that the connection was gone. Two lines now, and between them they
/// say which widget, which statement, and that the server stopped it.
fn failed(err: &DbError, what: &'static str) -> WidgetRefusal {
    crate::error::log_db_error(err, what);
    if err.gave_up() {
        WidgetRefusal::TimedOut
    } else {
        WidgetRefusal::Failed(err.client_message())
    }
}

/// A grid read that could not be answered, as a message. The refused half is a whole response composed
/// by `fields::filter_of`, whose text cannot be read back out here, so this says what a person can do
/// about either shape of it.
fn grid_failed(refusal: GridRefusal) -> WidgetRefusal {
    match refusal {
        GridRefusal::Db(err) => failed(&err, "dashboard grid read failed"),
        GridRefusal::Field(_) => WidgetRefusal::Failed(
            "This saved filter filters on a custom field the library no longer offers as a \
             filter. Open the filter in the grid to see which, then edit or remove it."
                .to_owned(),
        ),
    }
}

/// A body that is well-formed JSON and still not a resolve, or not JSON at all. The rejection's own
/// status is kept — 415 for the wrong content type, 400 for broken syntax, 422 for a shape serde read
/// and refused — and its text is wrapped in what to send instead.
fn bad_body(rejection: &JsonRejection) -> Response {
    (
        rejection.status(),
        Json(serde_json::json!({
            "message": format!(
                "Could not read the dashboard resolve body: {rejection}. It is \
                 `{{\"widgets\": [{{\"key\": \"…\", \"widget\": {{\"kind\": \"…\"}}}}]}}`, \
                 1 to {MAX_WIDGETS} widgets, each key once."
            )
        })),
    )
        .into_response()
}

/// A body that read cleanly and still cannot be carried out.
fn refused(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}

/// This file's routes, merged for the api role. One route: there is deliberately no per-widget
/// endpoint (`FEATURES.md` §8 calls per-widget polling a self-inflicted DoS).
pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/api/dashboard/resolve", post(resolve))
}
