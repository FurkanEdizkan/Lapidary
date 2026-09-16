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
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::{BatchId, DeviceId, LibraryId, PeerShareId, PullId};
use lapidary_db::{
    MirroredPartRow, MirroredShareRow, PeerRow, PgMirror, PgParts, PgPulls, PgSharing, PullRow,
};
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

/// Somebody else's share, as mirrored here.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MirroredShare {
    pub id: PeerShareId,
    pub device_id: String,
    /// What the sharer calls themselves, as their last hello said.
    pub sharer: Option<String>,
    pub name: String,
    #[ts(type = "number")]
    pub part_count: i64,
    /// When its whole catalogue was last read. `None` until it has been.
    pub synced_at: Option<Timestamp>,
}

/// A page of a mirrored share's parts. `next` is the `after` for the page that follows.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MirroredPartsPage {
    pub parts: Vec<MirroredPart>,
    pub next: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MirroredPart {
    pub source_path: String,
    pub name: String,
    pub part_number: Option<String>,
    pub tags: Vec<String>,
    pub licences: Vec<String>,
    #[ts(type = "number | null")]
    pub size_bytes: Option<i64>,
    pub format: Option<String>,
    pub thumbnail: bool,
}

#[derive(Debug, Deserialize)]
pub struct MirroredPartsQuery {
    after: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct MirroredThumbnailQuery {
    path: String,
}

/// The most parts one page of a shared library carries.
const MIRRORED_PAGE_MAX: i64 = 500;
const MIRRORED_PAGE_DEFAULT: i64 = 100;

pub async fn peer_shares(State(state): State<AppState>, Path(device): Path<String>) -> Response {
    let device = match device.parse::<DeviceId>() {
        Ok(device) => device,
        Err(err) => return bad_device_id(err),
    };
    match PgMirror(state.db).shares_of(device).await {
        Ok(rows) => Json(rows.into_iter().map(mirrored).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "mirrored shares failed"),
    }
}

pub async fn mirrored_share(
    State(state): State<AppState>,
    Path(share): Path<PeerShareId>,
) -> Response {
    match PgMirror(state.db).share(share).await {
        Ok(Some(row)) => Json(mirrored(row)).into_response(),
        Ok(None) => no_such_share(),
        Err(err) => internal_error(&err, "mirrored share failed"),
    }
}

pub async fn mirrored_parts(
    State(state): State<AppState>,
    Path(share): Path<PeerShareId>,
    Query(query): Query<MirroredPartsQuery>,
) -> Response {
    let mirror = PgMirror(state.db);
    // The share first, so a library whose sharer was removed is not found rather than an empty page that looks
    // like a share with nothing in it.
    match mirror.share(share).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_share(),
        Err(err) => return internal_error(&err, "mirrored parts failed"),
    }
    let limit = query
        .limit
        .unwrap_or(MIRRORED_PAGE_DEFAULT)
        .clamp(1, MIRRORED_PAGE_MAX);
    let after = query.after.as_deref().filter(|after| !after.is_empty());
    match mirror.parts(share, after, limit).await {
        Ok(rows) => {
            let full = usize::try_from(limit).is_ok_and(|limit| rows.len() == limit);
            let next = if full {
                rows.last().map(|row| row.source_path.clone())
            } else {
                None
            };
            Json(MirroredPartsPage {
                parts: rows.into_iter().map(mirrored_part).collect(),
                next,
            })
            .into_response()
        }
        Err(err) => internal_error(&err, "mirrored parts failed"),
    }
}

pub async fn mirrored_thumbnail(
    State(state): State<AppState>,
    Path(share): Path<PeerShareId>,
    Query(query): Query<MirroredThumbnailQuery>,
) -> Response {
    match PgMirror(state.db).thumbnail(share, &query.path).await {
        Ok(Some(bytes)) => ([(header::CONTENT_TYPE, "image/webp")], bytes).into_response(),
        Ok(None) => refused(
            StatusCode::NOT_FOUND,
            "noThumbnail",
            "That part has no preview in this shared library.",
        ),
        Err(err) => internal_error(&err, "mirrored thumbnail failed"),
    }
}

/// `POST /api/sharing/shares/{id}/pulls`'s body: the library the share's parts land in.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StartPull {
    pub library_id: LibraryId,
}

/// A pull, as its share's page follows it: fetching, then importing in `batch_id`, then done or failed.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Pull {
    pub id: PullId,
    pub library_id: LibraryId,
    /// `queued`, `fetching`, `importing`, `done` or `failed`.
    pub state: String,
    pub files_total: i32,
    pub files_done: i32,
    #[ts(type = "number")]
    pub bytes_total: i64,
    #[ts(type = "number")]
    pub bytes_done: i64,
    pub batch_id: Option<BatchId>,
    /// Why it failed, or why it is waiting to try again.
    pub error: Option<String>,
}

/// `POST /api/sharing/shares/{id}/pulls` — pull every part of a shared library into one of this installation's. The api
/// records it; the peer role, which alone can reach the sharer, fetches and queues the import.
pub async fn start_pull(
    State(state): State<AppState>,
    Path(share): Path<PeerShareId>,
    body: Result<Json<StartPull>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "badPull",
            "Say which library to pull into, as {\"libraryId\": \"…\"}.",
        );
    };
    match PgParts(state.db.clone())
        .auto_thumbnail(body.library_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refused(
                StatusCode::NOT_FOUND,
                "noSuchLibrary",
                "That library is not here any more. Choose another library to pull into.",
            );
        }
        Err(err) => return internal_error(&err, "pull library lookup failed"),
    }
    let pulls = PgPulls(state.db);
    match pulls.start(share, body.library_id).await {
        Ok(Some(_)) => match pulls.latest(share).await {
            Ok(Some(row)) => (StatusCode::ACCEPTED, Json(pull(row))).into_response(),
            Ok(None) => no_such_share(),
            Err(err) => internal_error(&err, "pull read failed"),
        },
        Ok(None) => no_such_share(),
        Err(err) => internal_error(&err, "pull start failed"),
    }
}

/// `GET /api/sharing/shares/{id}/pull` — the share's newest pull, or `null`.
pub async fn latest_pull(
    State(state): State<AppState>,
    Path(share): Path<PeerShareId>,
) -> Response {
    match PgPulls(state.db).latest(share).await {
        Ok(row) => Json(row.map(pull)).into_response(),
        Err(err) => internal_error(&err, "pull read failed"),
    }
}

fn pull(row: PullRow) -> Pull {
    Pull {
        id: row.id,
        library_id: row.library,
        state: row.state,
        files_total: row.files_total,
        files_done: row.files_done,
        bytes_total: row.bytes_total,
        bytes_done: row.bytes_done,
        batch_id: row.batch,
        error: row.error,
    }
}

fn no_such_share() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchShare",
        "That shared library is not here any more: its sharer stopped offering it, or you removed them. Reload the list of people you share with.",
    )
}

fn mirrored(row: MirroredShareRow) -> MirroredShare {
    MirroredShare {
        id: row.id,
        device_id: row.device.to_string(),
        sharer: row.sharer,
        name: row.name,
        part_count: row.part_count,
        synced_at: row.synced_at,
    }
}

fn mirrored_part(row: MirroredPartRow) -> MirroredPart {
    MirroredPart {
        source_path: row.source_path,
        name: row.name,
        part_number: row.part_number,
        tags: row.tags,
        licences: row.licences,
        size_bytes: row.size_bytes,
        format: row.format,
        thumbnail: row.thumbnail,
    }
}
