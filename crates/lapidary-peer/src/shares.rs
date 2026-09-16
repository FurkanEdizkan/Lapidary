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
use lapidary_core::{DeviceId, PartId, ShareId};
use lapidary_db::{CatalogueRow, DbError, PgPool, PgShares, PgSharing};
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
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    part: PartId,
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
    match PgShares(db).offered().await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| Share {
                    id: row.id,
                    name: row.name,
                    part_count: row.part_count,
                    digest: row.digest,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => failed(&err),
    }
}

async fn catalogue(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
    Query(query): Query<CatalogueQuery>,
) -> Response {
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    let limit = query
        .limit
        .unwrap_or(CATALOGUE_DEFAULT)
        .clamp(1, CATALOGUE_MAX);
    // `after=` with nothing after it is the first page, as the grid's own paging reads it.
    let after = query.after.as_deref().filter(|after| !after.is_empty());
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

async fn thumbnail(
    State(db): State<PgPool>,
    ConnectInfo(PeerDevice(device)): ConnectInfo<PeerDevice>,
    Path(share): Path<ShareId>,
    Query(query): Query<ThumbnailQuery>,
) -> Response {
    if let Err(refusal) = may_read(&db, device, share).await {
        return refusal;
    }
    match PgShares(db).thumbnail(share, query.part).await {
        Ok(Some(bytes)) => ([(header::CONTENT_TYPE, "image/webp")], bytes).into_response(),
        Ok(None) => refused(
            StatusCode::NOT_FOUND,
            "noThumbnail",
            "That part has no preview in this share: it may not have one yet, or it has left the category.",
        ),
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
async fn may_read(db: &PgPool, device: Option<DeviceId>, share: ShareId) -> Result<(), Response> {
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

fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
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

fn not_shared() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "notShared",
        "That category is not shared with you any more: its owner may have stopped sharing it, or removed you. Reload what they share.",
    )
}

fn failed(err: &DbError) -> Response {
    tracing::error!(error = %err, "a share route could not read the database");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
