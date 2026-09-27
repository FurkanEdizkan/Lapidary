//! `GET /api/events` — the one stream the whole app watches (design: `docs/goals/phase-6.md`
//! § App-wide events).
//!
//! # The shape, and the thing it exists to avoid
//!
//! One `PgListener` per api process, never one per tab. `jobs.rs`'s per-batch stream wrote down why it
//! would not use NOTIFY — "one listener per connection is one Postgres backend per open tab… either a
//! listener per stream… or new plumbing to share one" — and this file is that plumbing. The listener is
//! opened once, and every tab is a [`tokio::sync::broadcast`] receiver off it. Ten tabs cost ten
//! in-process receivers and one database connection.
//!
//! # Why it has its own router
//!
//! `AppState` is built in 88 places, most of them tests that have no hub and want none. So the hub is not
//! a field on it: [`router`] takes the hub as its own state and `bin/lapidary-server` merges it for the
//! api role, the way it merges `lapidary-ingest`'s router for the worker.
//!
//! # What ends what
//!
//! The hub task owns the only `broadcast::Sender`. So when it returns — on shutdown, and only on
//! shutdown — that sender drops, every receiver sees the channel close, and every open stream completes
//! on its own. There is no second signal threaded through the router and nothing to remember to cancel;
//! [`Hub`] holds a *receiver* for precisely that reason (see its doc).

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderName, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use lapidary_core::{AppEvent, LibraryId};
use lapidary_db::{PgListener, PgPool};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_util::sync::CancellationToken;

/// How long notifications are gathered before the events they became go out.
///
/// A scan ingests one file per transaction, so a folder of four hundred parts arrives as four hundred
/// notifications over half a minute. The grid only ever answers one of them the same way — ask the server
/// again — so sending each one is asking the browser to re-fetch four hundred times to draw the same
/// page. The database already collapses a burst *inside* one transaction (migration `0049`); this
/// collapses across them.
const WINDOW: Duration = Duration::from_millis(250);

/// How many events a subscriber may fall behind before it is told to resync instead.
///
/// The buffer is per channel, not per subscriber, and one flush sends one event per library that changed
/// in the window, so the number that matters is how many libraries can change at once. 256 is far past
/// any real installation's library count and costs 256 pointer-sized slots for the process's whole life.
const BACKLOG: usize = 256;

/// How long the hub waits before trying to listen again, when the database would not give it a listener.
///
/// Only the *opening* is retried here. A connection that drops after it was working is sqlx's to
/// re-establish, which it does immediately and reports as a gap; see [`run`].
const RETRY: Duration = Duration::from_secs(5);

/// The fan-out: one listener behind it, one receiver per open stream.
///
/// Holds a `broadcast::Receiver` rather than the `Sender`, which is the whole shutdown story. The hub task
/// owns the only sender, so its return closes the channel and ends every stream; a `Sender` kept here
/// would keep the channel open after the task was gone, and every tab would hang on a stream that could
/// never say anything again. New subscribers come from `Receiver::resubscribe`, which starts them at the
/// current tail — a tab that opens now is not handed events from before it asked, and this idle receiver
/// never blocks a send, because a `broadcast` overwrites rather than waits.
#[derive(Debug)]
pub struct Hub(broadcast::Receiver<AppEvent>);

impl Clone for Hub {
    fn clone(&self) -> Self {
        Self(self.0.resubscribe())
    }
}

impl Hub {
    /// Start listening. `shutdown` ends the hub, and with it every stream it feeds.
    ///
    /// The first listener is opened **before this returns**, so a change made immediately afterwards is
    /// heard. Opening it inside the task would race the caller's next write, which is the difference
    /// between a test that passes and a test that passes most of the time. If it cannot be opened the hub
    /// still starts, and retries: an api that refuses to serve because live updates are unavailable would
    /// be a worse answer than one that serves and says so in its log.
    pub async fn spawn(db: PgPool, shutdown: CancellationToken) -> Self {
        let (events, subscriber) = broadcast::channel(BACKLOG);
        let listener = open(&db).await;
        tokio::spawn(run(db, events, listener, shutdown));
        Self(subscriber)
    }

    fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.0.resubscribe()
    }
}

/// The one listener, or `None` with the reason logged.
async fn open(db: &PgPool) -> Option<PgListener> {
    match lapidary_db::listen_for_changes(db).await {
        Ok(listener) => Some(listener),
        Err(error) => {
            tracing::warn!(
                %error,
                "could not listen for what changes in this installation, so open pages will not \
                 update themselves until the database takes the listener; every one of them is asked \
                 to refresh as soon as it does. Nothing is lost meanwhile — reloading a page shows \
                 the truth"
            );
            None
        }
    }
}

/// The hub: hear, group, send. Returns only when `shutdown` is cancelled.
async fn run(
    db: PgPool,
    events: broadcast::Sender<AppEvent>,
    mut listener: Option<PgListener>,
    shutdown: CancellationToken,
) {
    let mut window = tokio::time::interval(WINDOW);
    let mut changed: HashSet<LibraryId> = HashSet::new();
    loop {
        let Some(listening) = listener.as_mut() else {
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(RETRY) => {}
            }
            listener = open(&db).await;
            if listener.is_some() {
                // Asked when listening resumes rather than when it stopped: a page told to ask again
                // while the database is unreachable would only ask a question nothing can answer.
                let _ = events.send(AppEvent::Resync);
            }
            continue;
        };
        // `try_recv` and not `recv`: they differ in exactly the thing this stream cares about. Both
        // reconnect, but `recv` does it silently — the notifications that arrived while the connection
        // was down are simply gone, and every page is left showing something that is no longer true,
        // with nothing anywhere saying so. `try_recv` reports the reconnect as `Ok(None)`, which is
        // what makes `Resync` possible at all.
        let lost = tokio::select! {
            () = shutdown.cancelled() => return,
            _ = window.tick() => {
                for library in changed.drain() {
                    // An error here is every subscriber having gone — nobody has the app open. The
                    // hub keeps listening regardless: the next tab to open resubscribes.
                    let _ = events.send(AppEvent::Changed { library });
                }
                false
            }
            heard = listening.try_recv() => match heard {
                Ok(Some(notification)) => {
                    match notification.payload().parse::<LibraryId>() {
                        Ok(library) => {
                            changed.insert(library);
                        }
                        // Only migration `0049`'s trigger writes this channel, so this is
                        // unreachable short of somebody issuing NOTIFY by hand. Logged rather than
                        // ignored, because "the dashboard stopped updating" is otherwise a silent
                        // symptom, and dropped rather than fanned out as a resync, because a
                        // stranger on our channel must not be able to make every tab re-fetch.
                        Err(error) => tracing::warn!(
                            %error,
                            payload = notification.payload(),
                            "something on the change channel did not name a library; ignoring it. \
                             Only migration 0049's trigger should write this channel"
                        ),
                    }
                    false
                }
                Ok(None) => {
                    // Reconnected. Whatever was notified while it was down is gone, so the window's
                    // contents are no longer the whole story and one resync replaces them.
                    changed.clear();
                    let _ = events.send(AppEvent::Resync);
                    false
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "stopped hearing what changes in this installation; listening again shortly, \
                         and every open page is asked to refresh when it works"
                    );
                    changed.clear();
                    true
                }
            },
        };
        if lost {
            listener = None;
        }
    }
}

/// `GET /api/events`, and the hub as its state. Merged by `bin/lapidary-server` for the api role.
pub fn router(hub: Hub) -> Router {
    Router::new()
        .route("/api/events", get(events))
        .with_state(hub)
}

/// Which libraries changed, for as long as the page is open.
///
/// No 404 and no error body anywhere in here: an `EventSource` cannot read one. A stream that has nothing
/// to say says nothing, and the keep-alive comment is what stops a proxy closing it for being quiet.
async fn events(State(hub): State<Hub>) -> impl IntoResponse {
    let events = BroadcastStream::new(hub.subscribe()).map(frame);
    (
        no_buffering(),
        Sse::new(events).keep_alive(KeepAlive::default()),
    )
}

/// One event as an SSE frame. The error is axum's own, which is how `Sse` reports a body that broke.
fn frame(heard: Result<AppEvent, BroadcastStreamRecvError>) -> Result<Event, axum::Error> {
    Event::default().json_data(resolve(heard))
}

/// What this subscriber is told, given what the broadcast handed it.
///
/// A subscriber that fell out of the buffer is told to resync rather than handed a gap it cannot see.
/// `AppEvent::Resync` means exactly "ask again for everything", which is the only honest answer about
/// events that were overwritten before this stream read them. Its own function because it is the one
/// decision in this file with no database in it, and so the one a test can make without a stack.
fn resolve(heard: Result<AppEvent, BroadcastStreamRecvError>) -> AppEvent {
    match heard {
        Ok(event) => event,
        Err(BroadcastStreamRecvError::Lagged(missed)) => {
            tracing::debug!(
                missed,
                "a page stopped reading its event stream long enough to fall behind it; asking that \
                 one page to refresh instead"
            );
            AppEvent::Resync
        }
    }
}

/// The two headers a stream needs to survive the proxy in front of it.
///
/// `X-Accel-Buffering: no` is the nginx family's per-response "do not buffer this", and
/// `docs/ARCHITECTURE.md` names it the single most common works-in-dev-breaks-in-prod bug in this stack:
/// default buffering holds events until the buffer fills, and ingest appears frozen. `Cache-Control:
/// no-cache` axum's own `Sse` already sets; naming it here as well puts the whole contract of a streaming
/// response in one place, and `IntoResponseParts` *inserts* headers rather than appending, so the value
/// is written once either way.
///
/// Used by both streams in this crate — this one and `jobs.rs`'s per-batch one.
pub(crate) fn no_buffering() -> [(HeaderName, &'static str); 2] {
    [
        (header::CACHE_CONTROL, "no-cache"),
        (HeaderName::from_static("x-accel-buffering"), "no"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule the route depends on, on the function the route reaches through `frame`: a subscriber
    /// that fell behind is sent `resync`, not silence and not a gap.
    #[test]
    fn a_subscriber_that_fell_behind_is_asked_to_resync() {
        let library = LibraryId::new();
        assert_eq!(
            resolve(Err(BroadcastStreamRecvError::Lagged(7))),
            AppEvent::Resync
        );
        assert_eq!(
            resolve(Ok(AppEvent::Changed { library })),
            AppEvent::Changed { library },
            "and anything that did arrive is passed through as itself"
        );
    }

    /// The same thing through the stream wrapper, so the arm above is checked against what `broadcast`
    /// really does when it overwrites rather than against an assumption about which error that raises.
    #[tokio::test]
    async fn overflowing_the_backlog_is_what_produces_that() {
        let library = LibraryId::new();
        let (events, subscriber) = broadcast::channel(2);
        let mut stream = BroadcastStream::new(subscriber);
        for _ in 0..3 {
            events
                .send(AppEvent::Changed { library })
                .expect("a subscriber is listening");
        }
        let heard = stream.next().await.expect("the stream has not ended");
        assert_eq!(
            resolve(heard),
            AppEvent::Resync,
            "three events into a buffer of two overwrites the oldest, and the reader is told so"
        );
    }
}
