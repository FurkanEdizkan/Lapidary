//! Signed in, for every test that is about something other than signing in (A1).
//!
//! [`router`] is `lapidary_api::router` with a real session in front of it: the first request makes the
//! account and its session row, and every request carries that session's cookie, so the guard these
//! tests pass through is the one a browser does — there is no way around it in the crate.
//! `tests/auth.rs` is where signing in itself is tested, against `lapidary_api::router` directly.
#![allow(dead_code)]

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::{Next, from_fn};
use lapidary_api::{AppState, Role};
use sqlx::PgPool;

/// The account every signed-in test acts as: the admin, as the setup wizard would have made.
pub const USER: uuid::Uuid = uuid::uuid!("01931b6e-0000-7000-8000-00000000a001");
pub const USERNAME: &str = "lena.kowalski";
const TOKEN: [u8; 32] = [0x5a; 32];

pub fn router(state: AppState, role: Role) -> Router {
    let pool = state.db.clone();
    signed_in(lapidary_api::router(state, role), pool)
}

/// Any router, with the test account's cookie on every request.
pub fn signed_in(app: Router, pool: PgPool) -> Router {
    app.layer(from_fn(move |mut request: Request, next: Next| {
        let pool = pool.clone();
        async move {
            ensure_session(&pool).await;
            let cookie = format!(
                "{}={}",
                lapidary_api::auth::COOKIE,
                TOKEN.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            request.headers_mut().insert(
                header::COOKIE,
                HeaderValue::from_str(&cookie).expect("a hex cookie is a header value"),
            );
            next.run(request).await
        }
    }))
}

/// The account and its session, made once per test database. A write that fails is left to fail: a test
/// that closed its pool on purpose is testing what the server says then, and the guard says it.
pub async fn ensure_session(pool: &PgPool) {
    let _ = sqlx::query(
        "INSERT INTO app_user (id, username, display_name, password_hash, is_admin) \
         VALUES ($1, $2, 'Lena Kowalski', 'not-a-password-hash', true) ON CONFLICT DO NOTHING",
    )
    .bind(USER)
    .bind(USERNAME)
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "INSERT INTO session (token_hash, user_id, expires_at) \
         VALUES ($1, $2, now() + interval '1 day') ON CONFLICT DO NOTHING",
    )
    .bind(&blake3::hash(&TOKEN).as_bytes()[..])
    .bind(USER)
    .execute(pool)
    .await;
    // What setup does (A1): the account owns every library and model nobody owns yet — the seeded
    // library, and whatever a test filed before its first request. A test about a second person
    // makes that person's rows itself.
    for claim in [
        "UPDATE library SET owner_id = $1 WHERE owner_id IS NULL",
        "UPDATE part SET owner_id = $1 WHERE owner_id IS NULL",
    ] {
        let _ = sqlx::query(claim).bind(USER).execute(pool).await;
    }
}
