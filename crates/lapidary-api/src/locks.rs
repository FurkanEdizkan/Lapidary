//! Check-out locks (Phase 4 slice 1 spec §5): take one, check it in, release it.
//!
//! No auth, and the routes say so by what they do not ask for: a holder is free text, and
//! anyone who can reach the api can release any lock. A forced release is recorded, and the
//! holder's next save under it is refused naming who released it — that record is the whole
//! of the protection in this slice.

use crate::AppState;
use crate::derive::{internal_error, no_such_part};
use crate::folders::refused;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::{LockId, PartId, RevisionId};
use lapidary_db::{Checkout, LockRow, PgLocks, PgParts};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An active check-out, as a part's page shows it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartLock {
    pub id: LockId,
    pub holder: String,
    pub taken_at: Timestamp,
}

impl From<LockRow> for PartLock {
    fn from(row: LockRow) -> Self {
        Self {
            id: row.id,
            holder: row.holder,
            taken_at: row.taken_at,
        }
    }
}

/// `POST /api/parts/{id}/checkout`'s body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewCheckout {
    /// Who is checking the part out, as free text: the agent sends `$USER@$HOSTNAME`.
    pub holder: String,
}

/// What a check-out hands back: the lock, and the revision it was taken on — the one the
/// holder's copy is of.
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CheckedOut {
    pub lock: PartLock,
    pub revision: RevisionId,
}

/// `POST /api/parts/{id}/checkin`'s body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Checkin {
    pub lock: LockId,
}

/// `POST /api/parts/{id}/lock/release`'s body.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReleaseLock {
    /// Who is releasing it. Recorded, and named to the holder when their next save is refused.
    pub by: String,
}

/// The `holder` and `by` columns' own limit.
const MAX_NAME_CHARS: usize = 200;

fn named(name: Option<String>) -> Option<String> {
    let name = name?.trim().to_owned();
    (!name.is_empty() && name.chars().count() <= MAX_NAME_CHARS).then_some(name)
}

/// `POST /api/parts/{id}/checkout` — take the part's one lock.
pub async fn checkout(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    body: Result<Json<NewCheckout>, JsonRejection>,
) -> Response {
    let Some(holder) = named(body.ok().map(|Json(body)| body.holder)) else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badHolder",
            "Say who is checking this part out: a `holder` of 1 to 200 characters, such as \
             `mira@workshop-pc`.",
        );
    };
    let taken = match PgLocks(state.db.clone()).take(part, &holder).await {
        Ok(taken) => taken,
        Err(err) => return internal_error(&err, "checkout failed"),
    };
    match taken {
        Checkout::Taken(row) => match PgParts(state.db).latest_revision(part).await {
            Ok(Some(revision)) => (
                StatusCode::CREATED,
                Json(CheckedOut {
                    lock: row.into(),
                    revision,
                }),
            )
                .into_response(),
            Ok(None) => no_such_part(),
            Err(err) => internal_error(&err, "checkout revision lookup failed"),
        },
        Checkout::Held(row) => refused(
            StatusCode::CONFLICT,
            "checkedOut",
            &format!(
                "{} has had this part checked out since {}. Ask them to check it in, or release \
                 the lock if they cannot.",
                row.holder, row.taken_at
            ),
        ),
        Checkout::HobbyLibrary => refused(
            StatusCode::CONFLICT,
            "hobbyLibrary",
            "This part is in a hobby library, which keeps no revisions, so there is nothing to \
             check out. Switch the library to keep every change first.",
        ),
        Checkout::NoSuchPart => no_such_part(),
    }
}

/// `POST /api/parts/{id}/checkin` — the holder hands the lock back. Their files stay where
/// they are: checking in deletes nothing.
pub async fn checkin(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    body: Result<Json<Checkin>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badLock",
            "Send the `lock` id the check-out handed back.",
        );
    };
    match PgLocks(state.db).check_in(part, body.lock).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::CONFLICT,
            "notHeld",
            "That check-out is not held on this part any more: it was checked in or released. \
             Nothing changed.",
        ),
        Err(err) => internal_error(&err, "checkin failed"),
    }
}

/// `POST /api/parts/{id}/lock/release` — somebody other than the holder frees the part.
pub async fn release(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    body: Result<Json<ReleaseLock>, JsonRejection>,
) -> Response {
    let Some(by) = named(body.ok().map(|Json(body)| body.by)) else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badName",
            "Say who is releasing the lock: a `by` of 1 to 200 characters. It is recorded, and \
             the holder's next save names it.",
        );
    };
    match PgLocks(state.db).force_release(part, &by).await {
        Ok(Some(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(None) => refused(
            StatusCode::CONFLICT,
            "notCheckedOut",
            "This part is not checked out, so there is nothing to release.",
        ),
        Err(err) => internal_error(&err, "lock release failed"),
    }
}
