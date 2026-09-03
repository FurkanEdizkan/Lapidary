use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    status: &'static str,
    database: Database,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Database {
    major: i32,
    reachable: bool,
}

/// Proves the whole path: HTTP in, a real query against Postgres, JSON out.
pub async fn healthz(State(state): State<AppState>) -> Response {
    match lapidary_db::server_version_num(&state.db).await {
        Ok(num) => axum::Json(Health {
            status: "ok",
            database: Database { major: num / 10_000, reachable: true },
        })
        .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "status": "unavailable",
                "message": "Could not reach the database. Check that the `db` service is running and that DATABASE_URL matches it."
            })),
        )
            .into_response(),
    }
}
