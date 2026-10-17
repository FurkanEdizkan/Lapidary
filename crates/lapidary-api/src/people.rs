//! The people on an install, and what they share (A4, owner 2026-10-17).
//!
//! **The admin makes accounts**; there is no sign-up. Each new account starts with its own empty
//! library. Deleting an account gives its libraries and models to the admin who deletes it, so nothing
//! anybody added is lost with them.
//!
//! **A model is shared with chosen people or with everybody.** A share lets them look and download;
//! changing it stays the owner's (`auth::scope` refuses anything else with a `403`).

use crate::AppState;
use crate::auth::{CurrentUser, hash_password, password_ok};
use crate::derive::internal_error;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use lapidary_core::{PartId, UserId};
use lapidary_db::{Access, PgAccounts, PgParts, UserRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The library every new account starts with.
const FIRST_LIBRARY: &str = "My models";

/// A person, as another person sees them.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

impl From<UserRow> for Person {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.username,
            display_name: row.display_name,
            is_admin: row.is_admin,
        }
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewPerson {
    pub username: String,
    pub display_name: Option<String>,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewPassword {
    pub password: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PasswordChange {
    pub current: String,
    pub password: String,
}

/// Who a model is shared with.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Shares {
    /// Everybody on this install.
    pub everyone: bool,
    pub people: Vec<UserId>,
}

/// `GET /api/people` — everybody on the install, for choosing who to share with and for the admin's
/// list.
pub async fn list(State(state): State<AppState>) -> Response {
    match PgAccounts(state.db).list().await {
        Ok(rows) => Json(rows.into_iter().map(Person::from).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "people list failed"),
    }
}

/// `POST /api/people` — admin only: a new account, with an empty library of its own.
pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<NewPerson>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return admins_only();
    }
    let Ok(Json(body)) = body else {
        return refused("Send a username and a password.");
    };
    let username = match crate::auth::username(&body.username) {
        Ok(name) => name,
        Err(refusal) => return *refusal,
    };
    if let Err(refusal) = password_ok(&body.password) {
        return *refusal;
    }
    let display_name = match crate::auth::display_name(body.display_name.as_deref()) {
        Ok(name) => name,
        Err(refusal) => return *refusal,
    };
    let hash = match hash_password(body.password).await {
        Ok(hash) => hash,
        Err(refusal) => return *refusal,
    };
    let made = match PgAccounts(state.db.clone())
        .create(&username, display_name.as_deref(), &hash, body.is_admin)
        .await
    {
        Ok(Some(made)) => made,
        Ok(None) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "message": format!("Somebody is already called {username}. Choose another username.") })),
            )
                .into_response();
        }
        Err(err) => return internal_error(&err, "making an account failed"),
    };
    if let Err(err) = PgParts(state.db)
        .create_library_for(FIRST_LIBRARY, "", Some(made.id))
        .await
    {
        tracing::warn!(error = %err, username = %made.username, "the new account has no first library; it can make one");
    }
    tracing::info!(username = %made.username, by = %user.username, "an account was made");
    (StatusCode::CREATED, Json(Person::from(made))).into_response()
}

/// `PUT /api/people/{id}/password` — admin only: a new password for somebody, who is signed out
/// everywhere.
pub async fn reset_password(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<UserId>,
    body: Result<Json<NewPassword>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return admins_only();
    }
    let Ok(Json(body)) = body else {
        return refused("Send the new password as `{\"password\": \"…\"}`.");
    };
    if let Err(refusal) = password_ok(&body.password) {
        return *refusal;
    }
    let hash = match hash_password(body.password).await {
        Ok(hash) => hash,
        Err(refusal) => return *refusal,
    };
    match PgAccounts(state.db)
        .set_password_by_id(id, &hash, None)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_person(),
        Err(err) => internal_error(&err, "password reset failed"),
    }
}

/// `DELETE /api/people/{id}` — admin only, never yourself: the account goes, and its libraries and
/// models become the admin's.
pub async fn delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<UserId>,
) -> Response {
    if !user.is_admin {
        return admins_only();
    }
    if id == user.id {
        return refused("You cannot delete your own account. Ask another admin to.");
    }
    match PgAccounts(state.db).delete_into(id, user.id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_person(),
        Err(err) => internal_error(&err, "deleting an account failed"),
    }
}

/// `PUT /api/profile/password` — change your own password, given the one you have now. Your other
/// sessions are signed out; this one stays.
pub async fn change_password(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    headers: axum::http::HeaderMap,
    body: Result<Json<PasswordChange>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send your current password and the new one.");
    };
    let accounts = PgAccounts(state.db);
    let hash = match accounts.password_hash(user.id).await {
        Ok(Some(hash)) => hash,
        Ok(None) => return no_such_person(),
        Err(err) => return internal_error(&err, "password lookup failed"),
    };
    if !crate::auth::verify_password(body.current, hash).await {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "message": "That is not your current password. Type it again." })),
        )
            .into_response();
    }
    if let Err(refusal) = password_ok(&body.password) {
        return *refusal;
    }
    let new = match hash_password(body.password).await {
        Ok(hash) => hash,
        Err(refusal) => return *refusal,
    };
    let keep = crate::auth::session_hash(&headers);
    match accounts
        .set_password_by_id(user.id, &new, keep.as_ref())
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => internal_error(&err, "password change failed"),
    }
}

/// `GET /api/parts/{id}/shares` — the owner's answer to who sees this model.
pub async fn shares(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(part): Path<PartId>,
) -> Response {
    let parts = PgParts(state.db);
    match parts.part_access(part, user.id).await {
        Ok(Some(Access::Owner)) => {}
        Ok(_) => return crate::derive::no_such_part(),
        Err(err) => return internal_error(&err, "access check failed"),
    }
    match parts.shares(part).await {
        Ok((everyone, people)) => Json(Shares {
            everyone,
            people: people.into_iter().map(|person| person.id).collect(),
        })
        .into_response(),
        Err(err) => internal_error(&err, "share list failed"),
    }
}

/// `PUT /api/parts/{id}/shares` — share the model with exactly these. The guard has checked it is the
/// caller's.
pub async fn set_shares(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    body: Result<Json<Shares>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send `{\"everyone\": false, \"people\": [\"…\"]}`.");
    };
    match PgParts(state.db)
        .set_shares(part, body.everyone, &body.people)
        .await
    {
        Ok(()) => Json(body).into_response(),
        Err(err) => internal_error(&err, "sharing failed"),
    }
}

/// `GET /api/shared` — the models other people shared with you, newest first: Shared with me.
pub async fn shared_with_me(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match PgParts(state.db).shared_with(user.id, 500).await {
        Ok(rows) => Json(crate::PartsPage {
            parts: rows.into_iter().map(crate::parts::to_card).collect(),
            next: None,
        })
        .into_response(),
        Err(err) => internal_error(&err, "shared list failed"),
    }
}

fn admins_only() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "message": "Only an admin can manage accounts." })),
    )
        .into_response()
}

fn no_such_person() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(
            serde_json::json!({ "message": "There is no such account any more. Reload the list." }),
        ),
    )
        .into_response()
}

fn refused(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}
