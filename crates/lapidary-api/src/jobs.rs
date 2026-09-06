//! Batch status: what a scan turned into. `api` role only — it reads job rows, touches
//! no source file and invokes no kernel, so it belongs on the open path.
//!
//! The route is scoped under its library rather than being a bare `/api/jobs/{id}`.
//! CLAUDE.md: content addressing is not authorization, and a batch id is no different —
//! it is a uuid a caller might hold from anywhere. Scoping the route makes the
//! reachability check structural instead of a step someone can forget, and
//! `PgJobs::batch_status` filters on `library_id` for that reason rather than for
//! performance.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use lapidary_core::{BatchId, LibraryId};
use lapidary_db::{DbError, PgJobs};
use std::convert::Infallible;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// `GET /api/libraries/{library}/jobs/{batch}` — how a scan is going, and how it ended.
pub async fn batch_status(
    State(state): State<AppState>,
    Path((library, batch)): Path<(LibraryId, BatchId)>,
) -> Response {
    match PgJobs(state.db).batch_status(library, batch).await {
        Ok(Some(status)) => Json(status).into_response(),
        Ok(None) => no_such_batch(),
        Err(err) => internal_error(&err),
    }
}

/// How often the stream re-reads the batch while it is running.
///
/// The same second the poll it replaces used, and for the same reason slice 2 gave for
/// `LAPIDARY_JOB_POLL_SECS`: this is the floor that makes delivery *correct*, not an
/// optimization. A `LISTEN/NOTIFY` wake would make it faster and is deliberately not here
/// — see the stream's own doc.
const TICK: Duration = Duration::from_secs(1);

/// How long the stream stays open on a batch that never finishes.
///
/// A batch whose worker died mid-run leaves rows that never reach a terminal state, and
/// without this the connection is held open forever by both ends. Ending it hands the
/// client back to its fallback poll, which is the thing that can report a stalled batch
/// honestly; `EventSource` will also reconnect on its own, so this is a ceiling on one
/// connection rather than on watching.
const MAX_STREAM: Duration = Duration::from_secs(30 * 60);

/// `GET /api/libraries/{library}/jobs/{batch}/events` — the same status, streamed.
///
/// # Why this exists
///
/// `2026-09-05-phase-1-slice-5-HANDOFF.md` records the risk this closes: *"The progress
/// line freezes on a hidden tab — react-query does not poll a hidden document."* A user
/// drops a thousand files, switches tabs while they ingest, and comes back to a progress
/// line stopped where they left it. A browser keeps an `EventSource` open on a hidden tab,
/// so the line keeps moving and the grid keeps filling.
///
/// # Why it is a timer and not `LISTEN/NOTIFY`
///
/// Because one listener per connection is one Postgres backend per open tab. Slice 2's
/// worker holds a single `PgListener` for the whole process and there is no fan-out from
/// it, so wiring NOTIFY in here would mean either a listener per stream — a hundred tabs
/// holding a hundred backends against a small api pool — or new plumbing to share one.
///
/// The timer is the half that makes delivery *correct*; NOTIFY only makes it faster, and
/// this route's whole purpose is the hidden tab rather than latency. Deferred, and stated
/// so nobody reads the absence as an oversight.
///
/// # Why the stream ends itself
///
/// `EventSource` reconnects automatically, so a stream that merely goes quiet on a
/// finished batch is one the browser re-opens forever — the same shape of leak
/// `refetchInterval` returning `false` closed for the poll, one layer down. So the last
/// event goes out, and the response completes.
///
/// A batch that does not exist ends the stream immediately rather than answering 404: the
/// status code is already sent by the time the first read happens, and an `EventSource`
/// cannot read a body anyway. The client's fallback poll is what reports that honestly.
pub async fn batch_events(
    State(state): State<AppState>,
    Path((library, batch)): Path<(LibraryId, BatchId)>,
) -> impl IntoResponse {
    // A task and a bounded channel, the shape `download.rs` already uses, and cancellation
    // comes free with it: a closed tab drops the receiver, the next `send` fails, and the
    // task returns. There is nothing to remember to wire up.
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(1);
    tokio::spawn(async move {
        let jobs = PgJobs(state.db);
        let started = Instant::now();
        loop {
            let status = match jobs.batch_status(library, batch).await {
                Ok(Some(status)) => status,
                // No such batch, or the query failed. Either way there is nothing further
                // to send, and the client's fallback poll is what reports it honestly — a
                // stream held open would just be re-opened by the browser forever.
                Ok(None) => return,
                Err(err) => {
                    tracing::error!(error = %err, "batch status stream query failed");
                    return;
                }
            };
            let done = status.finished_at.is_some() || started.elapsed() > MAX_STREAM;
            let event = match Event::default().json_data(&status) {
                Ok(event) => event,
                Err(err) => {
                    tracing::error!(error = %err, "a batch status would not serialise");
                    return;
                }
            };
            // The finished status is SENT, and only then does the loop end. Ending first
            // would leave the client's last known state one tick behind the truth, at the
            // one moment the number on screen matters most.
            if tx.send(Ok(event)).await.is_err() || done {
                return;
            }
            tokio::time::sleep(TICK).await;
        }
    });
    // Comments on an idle connection, which is what stops a proxy closing a stream that
    // has nothing to say. `TICK` is a second, so this only fires if a query hangs.
    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

/// No job rows for that batch in that library. Three different situations arrive here and
/// all three are honestly described by "no scan with that id has run in this library":
/// an id that was never issued, an id belonging to another library (which must not be
/// distinguishable from the first — that is the authorization point above), and a scan
/// that enqueued nothing at all. The last one is not a lost batch: `ScanAccepted.queued`
/// already told the client it was zero, and its doc says such a batch has no status
/// resource and must not be polled.
fn no_such_batch() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No scan with that id has run in this library. Check the id, or \
                        start a new scan."
        })),
    )
        .into_response()
}

/// The query itself failed. Same asymmetry `parts::internal_error` keeps: the operator
/// gets the real error through the log, the client gets whatever `client_message` decides
/// is safe to hand back.
fn internal_error(err: &DbError) -> Response {
    tracing::error!(error = %err, "batch status query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
