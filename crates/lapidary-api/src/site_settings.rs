//! Importing from a model's page (F3, 2026-10-16): the Settings page's keys, the check that says
//! whether one works, and the route that queues an import.
//!
//! **A key, once saved, never comes back out.** The list says whether one is saved, its last four
//! characters, and what the last check said; the key itself goes from the database to its site and
//! nowhere else, and is never logged.
//!
//! **`LAPIDARY_IMPORT_LINKS=false` turns all of it off**, for an install that must make no outbound
//! request beyond a picture somebody pastes: the page says so, and every route here refuses.

use crate::AppState;
use crate::derive::{accept, internal_error, no_such_library};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::{JobPayload, LibraryId};
use lapidary_db::{PgParts, PgSiteKeys};
use lapidary_sites::{Site, SiteError, Sites, identify};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One site, as the Settings page shows it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SiteSetting {
    /// `printables`, `thingiverse` or `myminifactory`.
    pub site: String,
    pub name: String,
    pub needs_key: bool,
    /// The last four characters of the saved key, or `None` when there is none.
    pub hint: Option<String>,
    /// What checking the saved key last said; `None` until it is checked.
    pub checked_ok: Option<bool>,
    pub checked_at: Option<Timestamp>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettings {
    /// `false` when `LAPIDARY_IMPORT_LINKS=false` turned importing off for this install.
    pub enabled: bool,
    pub sites: Vec<SiteSetting>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetSiteKey {
    pub key: String,
}

/// A key to try before saving it; absent, the saved one is checked.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CheckSiteKey {
    #[serde(default)]
    #[ts(optional)]
    pub key: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SiteCheck {
    pub ok: bool,
    /// The account the key belongs to, when the site says.
    pub account: Option<String>,
    /// The site's answer in a sentence: who the key belongs to, or why it did not work.
    pub message: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportLink {
    pub url: String,
}

/// The `instance_setting` key the setup wizard writes (A1). Absent means on.
pub(crate) const IMPORT_LINKS: &str = "import_links";

/// `LAPIDARY_IMPORT_LINKS=false`: off whatever the wizard said, for an install that must make no
/// outbound request.
pub(crate) fn forced_off() -> bool {
    matches!(
        std::env::var("LAPIDARY_IMPORT_LINKS")
            .as_deref()
            .map(str::trim),
        Ok("false" | "0" | "no" | "off")
    )
}

/// Whether this install imports from model sites at all: not forced off, and not switched off in
/// setup. A setting that cannot be read counts as off — the side that contacts nobody.
async fn enabled(db: &lapidary_db::PgPool) -> bool {
    if forced_off() {
        return false;
    }
    match lapidary_db::PgSettings(db.clone()).get(IMPORT_LINKS).await {
        Ok(value) => value.and_then(|v| v.as_bool()).unwrap_or(true),
        Err(err) => {
            tracing::warn!(error = %err, "could not read the import switch; importing stays off until it can be");
            false
        }
    }
}

fn switched_off() -> Response {
    refused(
        StatusCode::FORBIDDEN,
        if forced_off() {
            "Importing from model sites is switched off on this install (LAPIDARY_IMPORT_LINKS=false), so nothing here contacts them. Whoever runs the server can turn it on in deploy/.env."
        } else {
            "Importing from model sites was switched off when this install was set up, so nothing here contacts them."
        },
    )
}

fn refused(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "message": message }))).into_response()
}

fn unknown_site() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "Lapidary keeps keys for printables, thingiverse and myminifactory, and that is none of them.",
    )
}

/// `GET /api/settings/sites`
pub async fn list(State(state): State<AppState>) -> Response {
    let rows = match PgSiteKeys(state.db.clone()).list().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(&err, "site key list failed"),
    };
    let sites = Site::ALL
        .into_iter()
        .map(|site| {
            let row = rows.iter().find(|row| row.site == site.slug());
            SiteSetting {
                site: site.slug().to_owned(),
                name: site.name().to_owned(),
                needs_key: site.needs_key(),
                hint: row.map(|row| row.hint.clone()),
                checked_ok: row.and_then(|row| row.checked_ok),
                checked_at: row
                    .and_then(|row| row.checked_at_us)
                    .and_then(|us| Timestamp::from_microsecond(us).ok()),
            }
        })
        .collect();
    Json(SiteSettings {
        enabled: enabled(&state.db).await,
        sites,
    })
    .into_response()
}

/// `PUT /api/settings/sites/{site}` — save a key, replacing the one there. `204`.
pub async fn set(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    body: Result<Json<SetSiteKey>, JsonRejection>,
) -> Response {
    let Some(site) = Site::from_slug(&slug) else {
        return unknown_site();
    };
    if !site.needs_key() {
        return refused(
            StatusCode::BAD_REQUEST,
            "Printables needs no key: its free models import as they are.",
        );
    }
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the key as `{\"key\": \"…\"}`.",
        );
    };
    if body.key.trim().is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "That key is empty. Paste the key itself, or use Remove to forget the saved one.",
        );
    }
    match PgSiteKeys(state.db).set(site.slug(), &body.key).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => internal_error(&err, "site key save failed"),
    }
}

/// `DELETE /api/settings/sites/{site}` — forget the key. `204` whether or not there was one.
pub async fn clear(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let Some(site) = Site::from_slug(&slug) else {
        return unknown_site();
    };
    match PgSiteKeys(state.db).clear(site.slug()).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => internal_error(&err, "site key clear failed"),
    }
}

/// `POST /api/settings/sites/{site}/check` — ask the site whether a key works.
///
/// With a key in the body, that key is tried and nothing is saved, so a person can check before they
/// save. Without one, the saved key is tried and the answer recorded beside it. A `200` either way:
/// "the key does not work" is the check working.
pub async fn check(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    body: Option<Json<CheckSiteKey>>,
) -> Response {
    if !enabled(&state.db).await {
        return switched_off();
    }
    let Some(site) = Site::from_slug(&slug) else {
        return unknown_site();
    };
    let offered = body
        .and_then(|Json(body)| body.key)
        .filter(|key| !key.trim().is_empty());
    let keys = PgSiteKeys(state.db.clone());
    let (key, saved) = match offered {
        Some(key) => (Some(key), false),
        None => match keys.secret(site.slug()).await {
            Ok(key) => (key, true),
            Err(err) => return internal_error(&err, "site key read failed"),
        },
    };
    let answer = Sites::public().check(site, key.as_deref()).await;
    if saved
        && key.is_some()
        && let Err(err) = keys.record_check(site.slug(), answer.is_ok()).await
    {
        tracing::warn!(error = %err, site = site.slug(), "could not record a key check");
    }
    Json(match answer {
        Ok(_) if !site.needs_key() => SiteCheck {
            ok: true,
            message: format!(
                "{} answers. Its free models import without a key.",
                site.name()
            ),
            account: None,
        },
        Ok(account) => SiteCheck {
            ok: true,
            message: format!("{} answers: the key works for {account}.", site.name()),
            account: Some(account),
        },
        Err(error) => SiteCheck {
            ok: false,
            account: None,
            message: error.to_string(),
        },
    })
    .into_response()
}

/// `POST /api/libraries/{id}/imports/link` — queue a model's import from its page. `202` with the
/// batch to watch, or a refusal that names the site or the missing key before anything is queued.
pub async fn import(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    body: Result<Json<ImportLink>, JsonRejection>,
) -> Response {
    if !enabled(&state.db).await {
        return switched_off();
    }
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the model's address as `{\"url\": \"…\"}`.",
        );
    };
    let site = match identify(&body.url) {
        Ok((site, _)) => site,
        Err(error) => return refused(StatusCode::UNPROCESSABLE_ENTITY, &error.to_string()),
    };
    if site.needs_key() {
        match PgSiteKeys(state.db.clone()).secret(site.slug()).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return refused(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &SiteError::NoKey { site }.to_string(),
                );
            }
            Err(err) => return internal_error(&err, "site key read failed"),
        }
    }
    match PgParts(state.db.clone()).library_exists(library).await {
        Ok(true) => {}
        Ok(false) => return no_such_library(),
        Err(err) => return internal_error(&err, "library lookup failed"),
    }
    accept(
        state.db,
        library,
        &[JobPayload::ImportLink {
            url: body.url.trim().to_owned(),
        }],
    )
    .await
}

/// `POST /api/parts/{id}/describe {url}` — "Fetch details" (A2): fill a part from its model's page —
/// source, tags, pictures — and download nothing. `202` with the batch, or the same refusals an import
/// gives before anything is queued.
pub async fn describe(
    State(state): State<AppState>,
    Path(part): Path<lapidary_core::PartId>,
    body: Result<Json<ImportLink>, JsonRejection>,
) -> Response {
    if !enabled(&state.db).await {
        return switched_off();
    }
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the model's page as `{\"url\": \"…\"}`.",
        );
    };
    let site = match identify(&body.url) {
        Ok((site, _)) => site,
        Err(error) => return refused(StatusCode::UNPROCESSABLE_ENTITY, &error.to_string()),
    };
    if site.needs_key() {
        match PgSiteKeys(state.db.clone()).secret(site.slug()).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return refused(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &SiteError::NoKey { site }.to_string(),
                );
            }
            Err(err) => return internal_error(&err, "site key read failed"),
        }
    }
    let detail = match PgParts(state.db.clone()).detail(part).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return crate::derive::no_such_part(),
        Err(err) => return internal_error(&err, "part lookup failed"),
    };
    accept(
        state.db,
        detail.library,
        &[JobPayload::DescribePage {
            path: detail.source_path,
            url: body.url.trim().to_owned(),
        }],
    )
    .await
}
