//! The port Lapidary answers on (A5, owner 2026-10-17): a default, the first free one taken, and moved
//! from Settings without a restart.
//!
//! **This process owns its listeners**, so a container runs with host networking and binds the port
//! itself (`deploy/compose.yaml`). At start it takes `LAPIDARY_BIND` or `LAPIDARY_PORT` when the
//! operator set one — then the port is theirs and Settings says so — and otherwise the port this install
//! last used, or 3000, or the first free one of the twenty after it.
//!
//! **A move binds the new port beside the old one** and serves the same Router on both. The old one
//! keeps answering for a minute, for the other tabs, and then closes. The session cookie names no port,
//! so the browser that asked arrives signed in.

use anyhow::{Context, Result, bail};
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Extension, Json, Router};
use lapidary_api::auth::CurrentUser;
use lapidary_api::server_port::{MovePort, ServerPort};
use lapidary_db::{PgPool, PgSettings};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Where a port is kept between runs.
const SETTING: &str = "port";
/// Where an install that was never told a port starts looking.
const DEFAULT_PORT: u16 = 3000;
/// How many ports after it are tried before giving up.
const TRIES: u16 = 20;
/// How long the old port keeps answering after a move.
const OLD_PORT_GRACE: Duration = Duration::from_secs(60);

/// The listener the app starts on, and whether `deploy/.env` fixed it.
pub struct First {
    pub listener: TcpListener,
    pub port: u16,
    pub locked_by: Option<String>,
}

/// The first listener: the operator's address when they named one, else this install's port or the
/// first free one from it.
pub async fn first(db: &PgPool, bind: Option<&str>, port: Option<u16>) -> Result<First> {
    if let Some(bind) = bind {
        let listener = TcpListener::bind(bind).await.with_context(|| {
            format!(
                "Could not bind {bind} (LAPIDARY_BIND). Another process may already hold that port."
            )
        })?;
        let port = listener.local_addr()?.port();
        return Ok(First {
            listener,
            port,
            locked_by: Some("LAPIDARY_BIND".to_owned()),
        });
    }
    if let Some(port) = port {
        let listener = TcpListener::bind(("0.0.0.0", port)).await.with_context(|| {
            format!("Could not bind port {port} (LAPIDARY_PORT in deploy/.env). Another program is using it — choose another there.")
        })?;
        return Ok(First {
            listener,
            port,
            locked_by: Some("LAPIDARY_PORT".to_owned()),
        });
    }
    let settings = PgSettings(db.clone());
    let saved = settings
        .get(SETTING)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_u64())
        .and_then(|port| u16::try_from(port).ok());
    let start = saved.unwrap_or(DEFAULT_PORT);
    for port in start..=start.saturating_add(TRIES) {
        if let Ok(listener) = TcpListener::bind(("0.0.0.0", port)).await {
            if saved != Some(port)
                && let Err(error) = settings.set(SETTING, &serde_json::json!(port)).await
            {
                tracing::warn!(%error, port, "could not remember the port; the next start looks again");
            }
            return Ok(First {
                listener,
                port,
                locked_by: None,
            });
        }
    }
    bail!(
        "Could not start: ports {start} to {} are all in use. Free one, or set LAPIDARY_PORT in deploy/.env.",
        start.saturating_add(TRIES)
    )
}

/// What the routes ask the control task for.
pub struct Move {
    port: u16,
    reply: oneshot::Sender<Result<(), String>>,
}

/// The routes' handle on the listeners.
#[derive(Clone)]
pub struct Ports {
    moves: mpsc::Sender<Move>,
    current: Arc<AtomicU16>,
    locked_by: Option<String>,
}

/// One serving listener, and what stops it.
struct Serving {
    port: u16,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

fn serve(listener: TcpListener, port: u16, app: Router, shutdown: &CancellationToken) -> Serving {
    let stop = shutdown.child_token();
    let until = stop.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(async move { until.cancelled().await })
            .await
        {
            tracing::error!(%error, port, "a listener stopped unexpectedly");
        }
    });
    Serving { port, stop, task }
}

/// The handle the routes use, and the task that owns every listener. `app` is given to the task
/// once the routes are layered into it, through the returned sender.
pub fn handle(locked_by: Option<String>, port: u16) -> (Ports, mpsc::Receiver<Move>) {
    let (moves, receiver) = mpsc::channel(4);
    (
        Ports {
            moves,
            current: Arc::new(AtomicU16::new(port)),
            locked_by,
        },
        receiver,
    )
}

/// Serve `app` on `first`, and move it when asked, until `shutdown`. Returns once the last listener
/// has drained.
pub async fn run(
    first: TcpListener,
    ports: Ports,
    mut moves: mpsc::Receiver<Move>,
    app: Router,
    db: PgPool,
    shutdown: CancellationToken,
) {
    let mut serving = serve(
        first,
        ports.current.load(Ordering::SeqCst),
        app.clone(),
        &shutdown,
    );
    let mut retiring: Vec<tokio::task::AbortHandle> = Vec::new();
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            asked = moves.recv() => {
                let Some(Move { port, reply }) = asked else { break };
                if port == serving.port {
                    let _ = reply.send(Ok(()));
                    continue;
                }
                match TcpListener::bind(("0.0.0.0", port)).await {
                    Err(_) => {
                        let _ = reply.send(Err(format!(
                            "Port {port} is in use by another program — pick another."
                        )));
                    }
                    Ok(listener) => {
                        if let Err(error) = PgSettings(db.clone()).set(SETTING, &serde_json::json!(port)).await {
                            tracing::warn!(%error, port, "could not remember the new port; the next start looks again");
                        }
                        let old = std::mem::replace(&mut serving, serve(listener, port, app.clone(), &shutdown));
                        ports.current.store(port, Ordering::SeqCst);
                        tracing::info!(from = old.port, to = port, "Lapidary moved to http://localhost:{port}");
                        let _ = reply.send(Ok(()));
                        // The old port's minute, then a graceful stop; an open event stream would keep
                        // it draining for ever, so five seconds after that it is closed outright.
                        let (stop, task) = (old.stop.clone(), old.task.abort_handle());
                        tokio::spawn(async move {
                            tokio::time::sleep(OLD_PORT_GRACE).await;
                            stop.cancel();
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            task.abort();
                        });
                        retiring.push(old.task.abort_handle());
                    }
                }
            }
        }
    }
    // Shutting down: every listener's token is a child of `shutdown`, so the current one drains; an
    // old one still in its minute is closed.
    for old in retiring {
        old.abort();
    }
    let _ = serving.task.await;
}

/// `GET /api/settings/server` and `PUT /api/settings/port`, behind the same sign-in guard as every
/// other route. Changing the port is an admin's.
pub fn routes(db: PgPool, ports: Ports) -> Router {
    Router::new()
        .route("/api/settings/server", get(status))
        .route("/api/settings/port", put(move_to))
        .route_layer(axum::middleware::from_fn_with_state(
            db,
            lapidary_api::auth::require_user,
        ))
        .layer(axum::middleware::from_fn(lapidary_api::auth::same_origin))
        .with_state(ports)
}

async fn status(State(ports): State<Ports>) -> Response {
    Json(ServerPort {
        port: ports.current.load(Ordering::SeqCst),
        locked_by: ports.locked_by.clone(),
    })
    .into_response()
}

async fn move_to(
    State(ports): State<Ports>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<MovePort>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return refused(StatusCode::FORBIDDEN, "Only an admin can change the port.");
    }
    if let Some(by) = &ports.locked_by {
        return refused(
            StatusCode::CONFLICT,
            &format!(
                "The port is set by {by} in deploy/.env. Change it there and run `docker compose up -d`."
            ),
        );
    }
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the port as `{\"port\": 3005}`.",
        );
    };
    if body.port < 1024 {
        return refused(
            StatusCode::BAD_REQUEST,
            "Choose a port from 1024 to 65535; the ones below need the system's administrator.",
        );
    }
    let (reply, answer) = oneshot::channel();
    if ports
        .moves
        .send(Move {
            port: body.port,
            reply,
        })
        .await
        .is_err()
    {
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "The server is stopping. Try again once it is back.",
        );
    }
    match answer.await {
        Ok(Ok(())) => Json(ServerPort {
            port: body.port,
            locked_by: None,
        })
        .into_response(),
        Ok(Err(sentence)) => refused(StatusCode::CONFLICT, &sentence),
        Err(_) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "The server is stopping. Try again once it is back.",
        ),
    }
}

fn refused(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "message": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A port nothing is listening on, found by asking the system for one and letting it go.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("0.0.0.0:0").expect("a free port");
        listener.local_addr().expect("an address").port()
    }

    /// Whether `port` answers `GET /ping` with the test app's `pong`.
    async fn pongs(port: u16) -> bool {
        tokio::task::spawn_blocking(move || {
            let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
                return false;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            if stream
                .write_all(b"GET /ping HTTP/1.1\r\nHost: lapidary\r\nConnection: close\r\n\r\n")
                .is_err()
            {
                return false;
            }
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            answer.contains("pong")
        })
        .await
        .unwrap_or(false)
    }

    async fn ask(ports: &Ports, port: u16) -> Result<(), String> {
        let (reply, answer) = oneshot::channel();
        ports
            .moves
            .send(Move { port, reply })
            .await
            .expect("the control task is running");
        answer.await.expect("an answer")
    }

    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn a_busy_port_is_skipped_and_a_move_serves_the_new_port_beside_the_old(pool: PgPool) {
        // This install last used a port something else holds now.
        let holder = std::net::TcpListener::bind("0.0.0.0:0").expect("a port to hold");
        let busy = holder.local_addr().expect("an address").port();
        PgSettings(pool.clone())
            .set(SETTING, &serde_json::json!(busy))
            .await
            .expect("a remembered port");
        let first = first(&pool, None, None).await.expect("starts anyway");
        assert!(
            first.port > busy && first.port <= busy + TRIES,
            "{} after {busy}",
            first.port
        );
        assert!(first.locked_by.is_none());
        assert_eq!(
            PgSettings(pool.clone()).get(SETTING).await.expect("reads"),
            Some(serde_json::json!(first.port)),
            "and remembers the one it took"
        );

        let started = first.port;
        let (ports, moves) = handle(None, started);
        let app = Router::new().route("/ping", get(|| async { "pong" }));
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(run(
            first.listener,
            ports.clone(),
            moves,
            app,
            pool.clone(),
            shutdown.clone(),
        ));
        assert!(pongs(started).await);

        // A move: the new port answers, and so does the old one, for its minute.
        let to = free_port();
        assert_eq!(ask(&ports, to).await, Ok(()));
        assert!(pongs(to).await, "the new port");
        assert!(pongs(started).await, "the old port, still");
        assert_eq!(ports.current.load(Ordering::SeqCst), to);
        assert_eq!(
            PgSettings(pool.clone()).get(SETTING).await.expect("reads"),
            Some(serde_json::json!(to))
        );
        // A port another program holds is refused in a sentence, and nothing moves.
        let refused = ask(&ports, busy).await.expect_err("busy");
        assert!(refused.contains("in use"), "{refused}");
        assert_eq!(ports.current.load(Ordering::SeqCst), to);

        shutdown.cancel();
        task.await.expect("drains");
        drop(holder);
    }

    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn a_port_the_operator_fixed_is_taken_as_it_is_and_said_so(pool: PgPool) {
        let port = free_port();
        let first = first(&pool, None, Some(port)).await.expect("binds");
        assert_eq!(
            (first.port, first.locked_by.as_deref()),
            (port, Some("LAPIDARY_PORT"))
        );
        // Taken, it refuses rather than looking elsewhere.
        assert!(super::first(&pool, None, Some(port)).await.is_err());
    }
}
