//! What this installation shares, as the people paired with it read it (sharing S2a):
//! `GET /peer/v1/shares`, `GET /peer/v1/shares/{share}/catalogue` and `GET /peer/v1/shares/{share}/thumbnail`.
//!
//! Every route asks which installation is asking ([`PeerDevice`], proved by the handshake) and whether it may,
//! before it reads anything: paired and not removed for the list, and `PgShares::access` for one share.
//! The wire types here are both ends' — the hello round that mirrors a catalogue reads these same structs.

use crate::PeerDevice;
use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use lapidary_core::{DeviceId, PartId, PeerShareId, ShareId};
use lapidary_db::{CatalogueRow, DbError, PgMirror, PgPool, PgShares, PgSharing};
use serde::{Deserialize, Serialize};

/// The most parts one catalogue page carries.
pub const CATALOGUE_MAX: i64 = 500;

/// A page's size when the request does not say.
const CATALOGUE_DEFAULT: i64 = 200;

/// A share, as the list names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub id: ShareId,
    pub name: String,
    pub part_count: i64,
    /// Changes whenever the catalogue does.
    pub digest: String,
    /// Whose folder it is, when this installation does not own it but holds it and may pass it on (S7). Left
    /// out for a folder of this installation's own, which is every folder a protocol-1 list carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// When this installation read that folder from its owner. Left out with `owner`, and what the reader
    /// compares against the copy it holds: the newer reading is the one worth taking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<jiff::Timestamp>,
}

/// One person a share goes to, as its owner publishes the folder's roster to the others in it.
///
/// Its members see each other by name, id and address, and the prompt that offers an introduction says so
/// (owner's decision, 2026-09-17). Nobody outside the folder reads this: the route asks the same question the
/// catalogue does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareMember {
    pub device_id: String,
    pub name: Option<String>,
    pub address: String,
    /// Whether the folder's owner lets them fetch its files. What another holder reads before serving them.
    pub may_fetch: bool,
}

/// One page of a share's catalogue. `next` is the `after` for the page that follows, `None` on the last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CataloguePage {
    pub parts: Vec<CataloguePart>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CataloguePart {
    pub part: PartId,
    pub source_path: String,
    pub name: String,
    pub part_number: Option<String>,
    pub tags: Vec<String>,
    pub licences: Vec<String>,
    pub blake3: Option<String>,
    pub size_bytes: Option<i64>,
    pub format: Option<String>,
    pub thumbnail: bool,
}

#[derive(Debug, Deserialize)]
pub struct CatalogueQuery {
    after: Option<String>,
    limit: Option<i64>,
    /// Whose folder is being asked for, when it is not this installation's (S7): the relayed copy held here.
    owner: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    part: PartId,
    owner: Option<String>,
}

/// The share routes, over the database the peer role holds.
pub fn shares_router(db: PgPool) -> axum::Router {
    axum::Router::new()
        .route("/peer/v1/shares", axum::routing::get(list))
        .route(
            "/peer/v1/shares/{share}/catalogue",
            axum::routing::get(catalogue),
        )
        .route(
            "/peer/v1/shares/{share}/thumbnail",
            axum::routing::get(thumbnail),
        )
        .route(
            "/peer/v1/shares/{share}/members",
            axum::routing::get(members),
        )
        .route(
            "/peer/v1/shares/{share}/request",
            axum::routing::post(request),
        )
        .with_state(db)
}

async fn list(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
) -> Response {
    match paired(&db, device).await {
        Ok(true) => {}
        Ok(false) => return not_paired(),
        Err(err) => return failed(&err),
    }
    // Paired, so the session named a device; the list it gets is the shares that reach that one.
    let Some(device) = device else {
        return not_paired();
    };
    let mut shares = match PgShares(db.clone()).offered_to(device).await {
        Ok(rows) => rows
            .into_iter()
            .map(|row| Share {
                id: row.id,
                name: row.name,
                part_count: row.part_count,
                digest: row.digest,
                owner: None,
                as_of: None,
            })
            .collect::<Vec<_>>(),
        Err(err) => return failed(&err),
    };
    // Folders of other people's that this installation holds and they are in (S7), so a folder stays
    // browsable while its owner is away. Only for a reader that says it understands them: an older one would
    // record a relayed folder as this installation's own, and then ask this installation for its files.
    // A reader this installation has never said hello to lists nothing either, which errs the same way.
    match relays(&db, device).await {
        Ok(relayed) => shares.extend(relayed),
        Err(err) => return failed(&err),
    }
    Json(shares).into_response()
}

/// The folders held here that `device` may be passed, when its last hello said it reads them.
async fn relays(db: &PgPool, device: DeviceId) -> Result<Vec<Share>, DbError> {
    if !PgSharing(db.clone())
        .features(device)
        .await?
        .iter()
        .any(|feature| feature == crate::RELAY)
    {
        return Ok(Vec::new());
    }
    Ok(PgMirror(db.clone())
        .relayable_to(device)
        .await?
        .into_iter()
        .map(|row| Share {
            id: row.remote,
            name: row.name,
            part_count: row.part_count,
            digest: row.digest,
            owner: Some(row.owner.to_string()),
            as_of: row.as_of,
        })
        .collect())
}

/// Whose folder a request is about, when it says. `Err(())` is an id that is not one, which every caller
/// answers with the refusal a stranger gets: a request naming a machine that cannot exist learns nothing.
pub(crate) fn asked_owner(owner: Option<&str>) -> Result<Option<DeviceId>, ()> {
    match owner {
        None | Some("") => Ok(None),
        Some(owner) => owner.parse().map(Some).map_err(|_| ()),
    }
}

/// The folder held here that `device` may read, when the request named an owner other than this installation.
///
/// `Ok(None)` means the request is about this installation's own folder, and the ordinary path answers it.
async fn may_relay(
    db: &PgPool,
    device: Option<DeviceId>,
    owner: Option<DeviceId>,
    share: ShareId,
) -> Result<Option<PeerShareId>, Response> {
    let (Some(device), Some(owner)) = (device, owner) else {
        return Ok(None);
    };
    if PgSharing(db.clone())
        .identity()
        .await
        .map_err(|err| failed(&err))?
        .is_some_and(|identity| identity.device_id == owner)
    {
        return Ok(None);
    }
    match PgMirror(db.clone()).relayed_to(owner, share, device).await {
        Ok(Some(held)) => Ok(Some(held)),
        Ok(None) => Err(not_shared()),
        Err(err) => Err(failed(&err)),
    }
}

async fn catalogue(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
    Query(query): Query<CatalogueQuery>,
) -> Response {
    let Ok(owner) = asked_owner(query.owner.as_deref()) else {
        return not_shared();
    };
    let limit = query
        .limit
        .unwrap_or(CATALOGUE_DEFAULT)
        .clamp(1, CATALOGUE_MAX);
    // `after=` with nothing after it is the first page, as the grid's own paging reads it.
    let after = query.after.as_deref().filter(|after| !after.is_empty());
    // Somebody else's folder, held here and asked for by one of its people (S7): the copy this installation
    // read is what it answers with, and their being on its owner's roster is what allows it.
    match may_relay(&db, device, owner, share).await {
        Ok(Some(held)) => return relayed_catalogue(&db, held, after, limit).await,
        Ok(None) => {}
        Err(refusal) => return refusal,
    }
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    match PgShares(db).catalogue(share, after, limit).await {
        Ok(rows) => {
            let full = usize::try_from(limit).is_ok_and(|limit| rows.len() == limit);
            let next = if full {
                rows.last().map(|row| row.source_path.clone())
            } else {
                None
            };
            Json(CataloguePage {
                parts: rows.into_iter().map(catalogue_part).collect(),
                next,
            })
            .into_response()
        }
        Err(err) => failed(&err),
    }
}

/// `GET /peer/v1/shares/{share}/members` — who else this folder goes to, so the people in it can reach each
/// other when its owner is away. Behind the same question as the catalogue: somebody the folder does not reach
/// is told it is not shared with them, and learns nothing about who is in it.
async fn members(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
) -> Response {
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    match PgShares(db).roster(share).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| ShareMember {
                    device_id: row.device.to_string(),
                    name: row.name,
                    address: row.address,
                    may_fetch: row.may_fetch,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => failed(&err),
    }
}

/// A page of a folder held here, as this installation read it from its owner.
///
/// The parts are the mirrored ones, so `part` is the owner's id for each — the same id its owner's own
/// catalogue gives — and the reader cannot tell one copy from the other, which is the point.
async fn relayed_catalogue(
    db: &PgPool,
    held: PeerShareId,
    after: Option<&str>,
    limit: i64,
) -> Response {
    match PgMirror(db.clone()).parts(held, after, limit).await {
        Ok(rows) => {
            let full = usize::try_from(limit).is_ok_and(|limit| rows.len() == limit);
            let next = if full {
                rows.last().map(|row| row.source_path.clone())
            } else {
                None
            };
            Json(CataloguePage {
                parts: rows
                    .into_iter()
                    .map(|row| CataloguePart {
                        part: row.remote_part,
                        source_path: row.source_path,
                        name: row.name,
                        part_number: row.part_number,
                        tags: row.tags,
                        licences: row.licences,
                        blake3: row.blake3,
                        size_bytes: row.size_bytes,
                        format: row.format,
                        thumbnail: row.thumbnail,
                    })
                    .collect(),
                next,
            })
            .into_response()
        }
        Err(err) => failed(&err),
    }
}

async fn relayed_thumbnail(db: &PgPool, held: PeerShareId, part: PartId) -> Response {
    match PgMirror(db.clone()).thumbnail_of(held, part).await {
        Ok(Some(bytes)) => ([(header::CONTENT_TYPE, "image/webp")], bytes).into_response(),
        Ok(None) => no_thumbnail(),
        Err(err) => failed(&err),
    }
}

async fn thumbnail(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
    Query(query): Query<ThumbnailQuery>,
) -> Response {
    let Ok(owner) = asked_owner(query.owner.as_deref()) else {
        return not_shared();
    };
    match may_relay(&db, device, owner, share).await {
        Ok(Some(held)) => return relayed_thumbnail(&db, held, query.part).await,
        Ok(None) => {}
        Err(refusal) => return refusal,
    }
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    match PgShares(db).thumbnail(share, query.part).await {
        Ok(Some(bytes)) => ([(header::CONTENT_TYPE, "image/webp")], bytes).into_response(),
        Ok(None) => no_thumbnail(),
        Err(err) => failed(&err),
    }
}

/// Paired with this installation, and not removed.
async fn paired(db: &PgPool, device: Option<DeviceId>) -> Result<bool, DbError> {
    let Some(device) = device else {
        return Ok(false);
    };
    Ok(PgSharing(db.clone())
        .paired()
        .await?
        .iter()
        .any(|(id, _)| *id == device))
}

/// The one question every route about a single share asks first. A stranger is told the same as somebody
/// whose share was withdrawn, so nobody learns what is shared by asking.
/// `POST /peer/v1/shares/{share}/request` — ask for a share's files. Answers where the asker stands: `open` when the share
/// does not ask first, else `asked`, `granted` or `denied`. Asking again changes nothing, so a puller asks before every
/// attempt. Browsing never needs this: the catalogue stays readable to everyone paired, so a share that asks first can
/// still be seen, and asked for.
async fn request(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
) -> Response {
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    let Some(device) = device else {
        return not_shared();
    };
    match PgShares(db).ask(device, share).await {
        Ok(Some(grant)) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "grant": grant.as_str() })),
        )
            .into_response(),
        Ok(None) => not_shared(),
        Err(err) => failed(&err),
    }
}

pub(crate) async fn may_read(
    db: &PgPool,
    device: Option<DeviceId>,
    share: ShareId,
) -> Result<(), Response> {
    let Some(device) = device else {
        return Err(not_shared());
    };
    match PgShares(db.clone()).access(device, share).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(not_shared()),
        Err(err) => Err(failed(&err)),
    }
}

fn catalogue_part(row: CatalogueRow) -> CataloguePart {
    CataloguePart {
        part: row.part,
        source_path: row.source_path,
        name: row.name,
        part_number: row.part_number,
        tags: row.tags,
        licences: row.licences,
        blake3: row.blake3,
        size_bytes: row.size_bytes,
        format: row.format,
        thumbnail: row.thumbnail,
    }
}

pub(crate) fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message, "reason": reason })),
    )
        .into_response()
}

fn not_paired() -> Response {
    refused(
        StatusCode::FORBIDDEN,
        "notPaired",
        "This installation is not paired with yours. Ask its owner to add your device id on their sharing page.",
    )
}

fn no_thumbnail() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noThumbnail",
        "That part has no preview in this share: it may not have one yet, or it has left the category.",
    )
}

fn not_shared() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "notShared",
        "That category is not shared with you any more: its owner may have stopped sharing it, or removed you. Reload what they share.",
    )
}

pub(crate) fn failed(err: &DbError) -> Response {
    tracing::error!(error = %err, "a share route could not read the database");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
