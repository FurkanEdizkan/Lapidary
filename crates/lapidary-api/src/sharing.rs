//! Sharing (S1b): this installation's identity and the people it shares with. `GET` and `PUT`
//! `/api/sharing/identity`, `GET` and `POST` `/api/sharing/peers`, and `DELETE
//! /api/sharing/peers/{device}`.
//!
//! The api never speaks to another installation. It edits the list; the peer role's hello round takes
//! the list up and records who answered (`lapidary_peer::sync`), and this crate may not depend on that
//! one (`xtask/src/layers.rs`).

use crate::AppState;
use crate::folders::{internal_error, refused};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::DeviceId;
use lapidary_db::{PeerRow, PgSharing};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest name, in characters, as the table holds names.
const NAME_MAX: usize = 64;
/// The longest address, as the table holds addresses.
const ADDRESS_MAX: usize = 255;

/// This installation, as the people it shares with know it.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SharingIdentity {
    /// `None` until the peer role has run here, which is what sharing switched off looks like.
    pub device_id: Option<String>,
    pub name: Option<String>,
}

/// The `PUT` body. `None`, or nothing but spaces, clears the name.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetSharingName {
    pub name: Option<String>,
}

/// Somebody this installation is paired with.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Peer {
    pub device_id: String,
    pub address: String,
    /// What they call themselves, as their last hello said.
    pub name: Option<String>,
    pub added_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
    /// Why the last hello failed, in words. `None` once one succeeds.
    pub last_error: Option<String>,
    pub online: bool,
}

/// The `POST` body: what two people paste to each other.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AddPeer {
    pub device_id: String,
    pub address: String,
}

pub async fn identity(State(state): State<AppState>) -> Response {
    match PgSharing(state.db).identity().await {
        Ok(identity) => Json(SharingIdentity {
            device_id: identity.as_ref().map(|row| row.device_id.to_string()),
            name: identity.and_then(|row| row.name),
        })
        .into_response(),
        Err(err) => internal_error(&err, "sharing identity read failed"),
    }
}

pub async fn set_name(
    State(state): State<AppState>,
    body: Result<Json<SetSharingName>, JsonRejection>,
) -> Response {
    let Ok(Json(SetSharingName { name })) = body else {
        return bad_name();
    };
    let name = name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if name.is_some_and(|name| name.chars().count() > NAME_MAX) {
        return bad_name();
    }
    match PgSharing(state.db).set_name(name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::CONFLICT,
            "sharingOff",
            "Sharing is not switched on for this installation yet, so it has no id to give a name to. Start the sharing service with deploy/compose.sharing.yaml, then name it once this page shows its device id.",
        ),
        Err(err) => internal_error(&err, "sharing name set failed"),
    }
}

pub async fn peers(State(state): State<AppState>) -> Response {
    match PgSharing(state.db).peers().await {
        Ok(rows) => Json(rows.into_iter().map(peer).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "peer list failed"),
    }
}

pub async fn add(
    State(state): State<AppState>,
    body: Result<Json<AddPeer>, JsonRejection>,
) -> Response {
    let Ok(Json(AddPeer { device_id, address })) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badPeer",
            "Pairing needs the other installation's device id and where to reach it, such as 192.168.1.24:8082. Fill in both and try again.",
        );
    };
    let device = match device_id.parse::<DeviceId>() {
        Ok(device) => device,
        Err(err) => return bad_device_id(err),
    };
    let Some(address) = reachable_at(&address) else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badAddress",
            "Where to reach them is a host and a port, such as 192.168.1.24:8082 or workshop.tailnet.ts.net:8082, with an IPv6 address in brackets. Leave out https:// and any path; their sharing page shows the port.",
        );
    };
    let sharing = PgSharing(state.db);
    match sharing.identity().await {
        Ok(Some(own)) if own.device_id == device => {
            return refused(
                StatusCode::BAD_REQUEST,
                "ownDeviceId",
                "That is this installation's own device id. Paste the id shown on the sharing page of the other machine.",
            );
        }
        Ok(_) => {}
        Err(err) => return internal_error(&err, "sharing identity read failed"),
    }
    match sharing.add_peer(device, address).await {
        Ok(row) => Json(peer(row)).into_response(),
        Err(err) => internal_error(&err, "peer add failed"),
    }
}

pub async fn remove(State(state): State<AppState>, Path(device): Path<String>) -> Response {
    let device = match device.parse::<DeviceId>() {
        Ok(device) => device,
        Err(err) => return bad_device_id(err),
    };
    match PgSharing(state.db).remove_peer(device).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "notPaired",
            "This installation is not paired with that device id, so there is nobody to remove. Reload the list to see who it shares with.",
        ),
        Err(err) => internal_error(&err, "peer remove failed"),
    }
}

/// A device id refused in `CoreError`'s own words, which say what to do about it.
fn bad_device_id(err: impl std::fmt::Display) -> Response {
    refused(StatusCode::BAD_REQUEST, "badDeviceId", &err.to_string())
}

fn bad_name() -> Response {
    refused(
        StatusCode::BAD_REQUEST,
        "badName",
        "A name for this installation is up to 64 characters, such as \"Furkan's workbench\". Shorten it and try again, or leave it empty to go by the device id alone.",
    )
}

fn peer(row: PeerRow) -> Peer {
    Peer {
        device_id: row.device_id.to_string(),
        address: row.address,
        name: row.name,
        added_at: row.added_at,
        last_seen_at: row.last_seen_at,
        last_error: row.last_error,
        online: row.online,
    }
}

/// Where to reach another installation: a host and a port, as its owner reads them off their own
/// sharing page. No scheme and no path, since the peer protocol decides both.
fn reachable_at(raw: &str) -> Option<&str> {
    let address = raw.trim();
    let (host, port) = address.rsplit_once(':')?;
    let port_ok = port.parse::<u16>().is_ok_and(|port| port != 0);
    // An IPv6 address has colons of its own, so it is written in brackets, as a URL writes it.
    let bracketed = host.len() > 2 && host.starts_with('[') && host.ends_with(']');
    let host_ok = !host.is_empty()
        && (bracketed || !host.contains(':'))
        && !host.contains(|c: char| c.is_whitespace() || c == '/' || c == '@');
    (port_ok && host_ok && address.len() <= ADDRESS_MAX).then_some(address)
}
