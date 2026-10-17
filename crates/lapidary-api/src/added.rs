//! What a library holds (A2, owner 2026-10-17): the Import browser over the models folder, and the
//! files and folders a person added from it.
//!
//! **Adding does not scan.** `POST …/sources` records the file or folder (and its page, and whether
//! to fetch that page's details); pictures for it are posted next; then the browser asks for the
//! ordinary scan, which files everything the library's sources cover. In that order, a picture a
//! person chose is there before the part it is for.
//!
//! **Nothing here writes into the models folder.** The browser lists it; removing a source takes its
//! parts out of the library and leaves every file exactly where it is, because the folder is the
//! person's.

use crate::AppState;
use crate::derive::{internal_error, no_such_library};
use axum::Json;
use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::LibraryId;
use lapidary_db::{PgParts, PgSources, SourceRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// The most entries one listing answers with. A folder past it says so rather than being cut in
/// silence.
const MAX_ENTRIES: usize = 2000;

/// One entry of a folder in the models folder.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEntry {
    pub name: String,
    /// Relative to the models folder.
    pub path: String,
    pub is_folder: bool,
    /// A file's size; `None` for a folder.
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BrowseListing {
    /// The folder listed, relative to the models folder; `""` is the top.
    pub path: String,
    /// Folders first, then model files, each by name. Only STL, OBJ and 3MF files are listed.
    pub entries: Vec<BrowseEntry>,
    /// `true` when the folder holds more than this listing shows.
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    path: String,
}

/// An added file or folder, as the Import dialog shows it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySource {
    pub id: String,
    pub path: String,
    pub is_folder: bool,
    pub page_url: Option<String>,
    pub fetch_details: bool,
}

impl From<SourceRow> for LibrarySource {
    fn from(row: SourceRow) -> Self {
        Self {
            id: row.id.to_string(),
            is_folder: row.kind == "folder",
            path: row.path,
            page_url: row.page_url,
            fetch_details: row.fetch_details,
        }
    }
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewSourceRequest {
    /// Relative to the models folder; `""` is the whole of it.
    pub path: String,
    /// The model's page, when the person has one.
    pub page_url: Option<String>,
    /// Fetch the page's details for every part found under it.
    #[serde(default)]
    pub fetch_details: bool,
}

/// `GET /api/storage/browse?path=` — one folder of the models folder.
pub async fn browse(
    State(state): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::CurrentUser>,
    Query(query): Query<BrowseQuery>,
) -> Response {
    let path = query.path.trim_matches('/').to_owned();
    if let Err(refusal) = inside(&path) {
        return *refusal;
    }
    if let Err(refusal) = not_someone_elses(&path, &user.username) {
        return *refusal;
    }
    let only = (path == UPLOADS).then(|| user.username.clone());
    let dir = state.ingest_dir.join(&path);
    let listed = tokio::task::spawn_blocking(move || list(&dir)).await;
    match listed {
        Ok(Ok((entries, truncated))) => Json(BrowseListing {
            entries: entries
                .into_iter()
                // In `uploads/`, only the person's own folder (A3).
                .filter(|(name, _, _)| only.as_ref().is_none_or(|own| name == own))
                .map(|(name, is_folder, size)| BrowseEntry {
                    path: if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}/{name}")
                    },
                    name,
                    is_folder,
                    size_bytes: size,
                })
                .collect(),
            path,
            truncated,
        })
        .into_response(),
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => refused(
            StatusCode::NOT_FOUND,
            "That folder is not in your models folder any more. Go back up and choose again.",
        ),
        Ok(Err(error)) => {
            tracing::warn!(%error, "could not list a folder of the models folder");
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read that folder. Check that Lapidary may read your models folder, then try again.",
            )
        }
        Err(_) => refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read that folder. Try again.",
        ),
    }
}

/// `GET /api/libraries/{id}/sources`.
pub async fn list_sources(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
) -> Response {
    if let Some(refusal) = no_library(&state, library).await {
        return refusal;
    }
    match PgSources(state.db).list(library).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(LibrarySource::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "source list failed"),
    }
}

/// `POST /api/libraries/{id}/sources` — add a file or folder of the models folder. `201` with the
/// source; the caller then posts its pictures, then asks for a scan.
pub async fn add_source(
    State(state): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::CurrentUser>,
    Path(library): Path<LibraryId>,
    body: Result<Json<NewSourceRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the file or folder as `{\"path\": \"…\"}`, relative to your models folder.",
        );
    };
    if let Some(refusal) = no_library(&state, library).await {
        return refusal;
    }
    let path = body.path.trim_matches('/').to_owned();
    if let Err(refusal) = inside(&path) {
        return *refusal;
    }
    if path == UPLOADS {
        return refused(
            StatusCode::BAD_REQUEST,
            "uploads holds everybody's uploads. Open it and add your own folder instead.",
        );
    }
    if let Err(refusal) = not_someone_elses(&path, &user.username) {
        return *refusal;
    }
    let full = state.ingest_dir.join(&path);
    let kind = if full.is_dir() {
        "folder"
    } else if full.is_file() && lapidary_core::is_model_file(&path) {
        "file"
    } else {
        return refused(
            StatusCode::BAD_REQUEST,
            "That is not a folder or an STL, OBJ or 3MF file in your models folder. Choose one from the list.",
        );
    };
    let page_url = body
        .page_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty());
    if let Some(url) = page_url
        && let Err(error) = lapidary_sites::identify(url)
    {
        return refused(StatusCode::BAD_REQUEST, &error.to_string());
    }
    match PgSources(state.db)
        .add(
            library,
            &path,
            kind,
            page_url,
            body.fetch_details && page_url.is_some(),
        )
        .await
    {
        Ok(row) => (StatusCode::CREATED, Json(LibrarySource::from(row))).into_response(),
        Err(err) => internal_error(&err, "adding a source failed"),
    }
}

/// `POST /api/libraries/{library}/sources/{source}/pictures` — the body is one picture, for every
/// part found under the source. Through the same `normalize` every upload takes.
pub async fn add_source_picture(
    State(state): State<AppState>,
    Path((library, source)): Path<(LibraryId, Uuid)>,
    body: Bytes,
) -> Response {
    match PgSources(state.db.clone()).get(library, source).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_source(),
        Err(err) => return internal_error(&err, "source lookup failed"),
    }
    let image = match lapidary_storage::images::normalize(&body) {
        Ok(image) => image,
        Err(error) => return refused(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    match PgSources(state.db).add_image(source, &image.webp).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => internal_error(&err, "saving a source picture failed"),
    }
}

/// `DELETE /api/libraries/{library}/sources/{source}` — take the file or folder out of the library,
/// and every part under it. The files stay where they are.
pub async fn remove_source(
    State(state): State<AppState>,
    Path((library, source)): Path<(LibraryId, Uuid)>,
) -> Response {
    match PgSources(state.db).remove(library, source).await {
        Ok(Some(parts)) => Json(serde_json::json!({ "partsRemoved": parts })).into_response(),
        Ok(None) => no_such_source(),
        Err(err) => internal_error(&err, "removing a source failed"),
    }
}

/// Where uploads are written, one folder per person (A2).
const UPLOADS: &str = "uploads";

/// Not inside another person's uploads (A3), which is that person's to see. Said as a folder that is
/// not there, which is what it is to this person.
fn not_someone_elses(path: &str, username: &str) -> Result<(), Box<Response>> {
    let mut segments = path.split('/');
    if segments.next() == Some(UPLOADS)
        && let Some(owner) = segments.next()
        && owner != username
    {
        return Err(Box::new(refused(
            StatusCode::NOT_FOUND,
            "That folder is not in your models folder any more. Go back up and choose again.",
        )));
    }
    Ok(())
}

/// A path in the models folder a person may look at or add: not out of it, and not into `_removed/`.
fn inside(path: &str) -> Result<(), Box<Response>> {
    if !path.is_empty() && lapidary_core::path_escapes(path) {
        return Err(Box::new(refused(
            StatusCode::BAD_REQUEST,
            "That path leaves your models folder. Choose a folder from the list.",
        )));
    }
    if path == lapidary_storage::REMOVED_DIR
        || path.starts_with(&format!("{}/", lapidary_storage::REMOVED_DIR))
    {
        return Err(Box::new(refused(
            StatusCode::BAD_REQUEST,
            "_removed holds the files of models you removed. Move one out of it to add it again.",
        )));
    }
    Ok(())
}

/// A folder's folders and model files: `(name, is_folder, size)`, folders first, each by name.
/// A listed entry: its name, whether it is a folder, and a file's size.
type Listed = (String, bool, Option<u64>);

fn list(dir: &std::path::Path) -> std::io::Result<(Vec<Listed>, bool)> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if lapidary_core::is_ignored(&name)
            || name.starts_with('.')
            || name == lapidary_storage::REMOVED_DIR
        {
            continue;
        }
        // Follows a symlink, as the scan does for a file; a symlinked folder is listed but the scan
        // does not descend into it.
        let Ok(meta) = std::fs::metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            entries.push((name, true, None));
        } else if meta.is_file() && lapidary_core::is_model_file(&name) {
            entries.push((name, false, Some(meta.len())));
        }
    }
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    let truncated = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    Ok((entries, truncated))
}

async fn no_library(state: &AppState, library: LibraryId) -> Option<Response> {
    match PgParts(state.db.clone()).library_exists(library).await {
        Ok(true) => None,
        Ok(false) => Some(no_such_library()),
        Err(err) => Some(internal_error(&err, "library lookup failed")),
    }
}

fn no_such_source() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "That file or folder is not in this library any more. Reload to see what it holds.",
    )
}

fn refused(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "message": message }))).into_response()
}
