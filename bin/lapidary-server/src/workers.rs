//! How many files the job worker reads at once (A41, owner 2026-10-21): chosen in the setup wizard or
//! Settings, kept between runs, and fixed by `LAPIDARY_WORKER_CONCURRENCY` when the operator set it.
//!
//! The worker sizes its permits once, at start, so a change saves the number and restarts Lapidary the
//! way a models folder change does (`models_dir.rs`). Resizing the running worker is skipped: its
//! shutdown drain takes every permit, and a restart is a few seconds the person asked for.

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use lapidary_api::auth::CurrentUser;
use lapidary_api::server_port::{
    IMPORT_WORKERS as SETTING, IMPORT_WORKERS_RANGE as RANGE, ImportWorkers, SetImportWorkers,
};
use lapidary_db::{PgPool, PgSettings};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The same header the wizard's folder routes read the setup code from.
const SETUP_CODE: &str = "x-setup-code";

/// The number this run starts with: the operator's, else the saved one, else `default`.
pub async fn first(db: &PgPool, fixed: Option<usize>, default: usize) -> Workers {
    if let Some(workers) = fixed {
        return Workers {
            workers,
            locked_by: Some("LAPIDARY_WORKER_CONCURRENCY".to_owned()),
        };
    }
    let saved = match PgSettings(db.clone()).get(SETTING).await {
        Ok(value) => value
            .and_then(|v| v.as_u64())
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| RANGE.contains(n)),
        Err(error) => {
            tracing::warn!(%error, "could not read how many imports run at once; using the default");
            None
        }
    };
    Workers {
        workers: saved.map_or(default, usize::from),
        locked_by: None,
    }
}

#[derive(Clone)]
pub struct Workers {
    pub workers: usize,
    pub locked_by: Option<String>,
}

#[derive(Clone)]
struct Knob {
    db: PgPool,
    now: Workers,
    restart: CancellationToken,
}

/// `GET`/`PUT /api/settings/workers`, signed in, and `GET /api/setup/workers` for the wizard, which
/// answers only with the setup code.
pub fn routes(db: PgPool, now: Workers, restart: CancellationToken) -> Router {
    let knob = Knob {
        db: db.clone(),
        now,
        restart,
    };
    let signed_in = Router::new()
        .route("/api/settings/workers", get(status).put(set))
        .route_layer(axum::middleware::from_fn_with_state(
            db,
            lapidary_api::auth::require_user,
        ));
    signed_in
        .route("/api/setup/workers", get(setup_status))
        .layer(axum::middleware::from_fn(lapidary_api::auth::same_origin))
        .with_state(knob)
}

fn describe(now: &Workers) -> ImportWorkers {
    ImportWorkers {
        workers: u16::try_from(now.workers).unwrap_or(u16::MAX),
        locked_by: now.locked_by.clone(),
    }
}

async fn status(State(knob): State<Knob>) -> Response {
    Json(describe(&knob.now)).into_response()
}

async fn setup_status(State(knob): State<Knob>, headers: HeaderMap) -> Response {
    let code = headers
        .get(SETUP_CODE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !lapidary_api::auth::setup_open(&knob.db, code)
        .await
        .unwrap_or(false)
    {
        return refused(
            StatusCode::FORBIDDEN,
            "Setup is finished, or the setup code is wrong. Sign in and use Settings.",
        );
    }
    Json(describe(&knob.now)).into_response()
}

async fn set(
    State(knob): State<Knob>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<SetImportWorkers>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return refused(
            StatusCode::FORBIDDEN,
            "Only an admin can change how many imports run at once.",
        );
    }
    if let Some(by) = &knob.now.locked_by {
        return refused(
            StatusCode::CONFLICT,
            &format!(
                "How many imports run at once is set by {by} in deploy/.env. Change it there and run `docker compose up -d`."
            ),
        );
    }
    let Ok(Json(SetImportWorkers { workers })) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the number as `{\"workers\": 2}`.",
        );
    };
    if !RANGE.contains(&workers) {
        return refused(
            StatusCode::BAD_REQUEST,
            &format!(
                "Choose from {} to {} imports at once.",
                RANGE.start(),
                RANGE.end()
            ),
        );
    }
    if usize::from(workers) == knob.now.workers {
        return Json(describe(&knob.now)).into_response();
    }
    if let Err(error) = PgSettings(knob.db.clone())
        .set(SETTING, &serde_json::json!(workers))
        .await
    {
        tracing::error!(%error, "could not save how many imports run at once");
        return refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The number was not saved. Try again.",
        );
    }
    tracing::info!(workers, "imports at once changed; restarting on it");
    // The answer leaves first; the graceful stop waits for it either way.
    let restart = knob.restart.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        restart.cancel();
    });
    (
        StatusCode::ACCEPTED,
        Json(ImportWorkers {
            workers,
            locked_by: None,
        }),
    )
        .into_response()
}

fn refused(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "message": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(is_admin: bool) -> CurrentUser {
        CurrentUser {
            id: lapidary_core::UserId::new(),
            username: "mara".to_owned(),
            display_name: None,
            is_admin,
        }
    }

    async fn put(knob: &Knob, user: CurrentUser, workers: u16) -> StatusCode {
        set(
            State(knob.clone()),
            Extension(user),
            Ok(Json(SetImportWorkers { workers })),
        )
        .await
        .status()
    }

    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn a_saved_number_is_the_next_start_and_only_an_admin_changes_it(pool: PgPool) {
        assert_eq!(first(&pool, None, 2).await.workers, 2);
        let knob = Knob {
            db: pool.clone(),
            now: first(&pool, None, 2).await,
            restart: CancellationToken::new(),
        };
        assert_eq!(put(&knob, person(false), 3).await, StatusCode::FORBIDDEN);
        assert_eq!(put(&knob, person(true), 0).await, StatusCode::BAD_REQUEST);
        assert_eq!(put(&knob, person(true), 9).await, StatusCode::BAD_REQUEST);
        assert!(!knob.restart.is_cancelled());
        assert_eq!(put(&knob, person(true), 3).await, StatusCode::ACCEPTED);
        assert_eq!(first(&pool, None, 2).await.workers, 3);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(knob.restart.is_cancelled(), "a change restarts");
    }

    #[sqlx::test(migrations = "../../crates/lapidary-db/migrations")]
    async fn the_operators_number_wins_and_is_locked(pool: PgPool) {
        PgSettings(pool.clone())
            .set(SETTING, &serde_json::json!(5))
            .await
            .expect("saved");
        let now = first(&pool, Some(6), 2).await;
        assert_eq!(now.workers, 6);
        let knob = Knob {
            db: pool,
            now,
            restart: CancellationToken::new(),
        };
        assert_eq!(put(&knob, person(true), 3).await, StatusCode::CONFLICT);
    }
}
