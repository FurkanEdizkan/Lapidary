//! Saved filters: a name for a set of the grid's filters, kept per library.
//!
//! Three routes over `PgSavedFilters`. A saved filter keeps what the grid's URL carries -- the
//! search, the category, the format, the material and the tag -- and never the part a quick look
//! is open on or a batch, which describe a moment rather than a filter. There are no users yet,
//! so a library's saved filters are everyone's who opens it, which is what a collection is.
//!
//! Removing one takes a name and the filters it kept off a list. No part is touched.

use crate::AppState;
use crate::folders::{internal_error, refused};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{FolderId, LibraryId, SavedFilterId};
use lapidary_db::{DbError, PgFolders, PgSavedFilters};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest name a saved filter takes, in characters. The table's check says the same.
const NAME_MAX: usize = 80;

/// The longest single value a filter keeps, in characters.
const VALUE_MAX: usize = 512;

/// The grid's filters, as its URL carries them. An absent field is no filter on it, and a field
/// the grid does not filter by is refused rather than kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct FilterSearch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder_id: Option<FolderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tag: Option<String>,
}

/// One saved filter.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SavedFilter {
    pub id: SavedFilterId,
    pub name: String,
    pub search: FilterSearch,
}

/// `POST /api/libraries/{id}/filters`.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewSavedFilter {
    pub name: String,
    pub search: FilterSearch,
}

/// `GET /api/libraries/{id}/filters`: the library's saved filters, by name. An id naming no
/// library answers `[]`, as the category tree does, because to somebody browsing the two look alike.
pub async fn list(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    let rows = match PgSavedFilters(state.db).list(library).await {
        Ok(rows) => rows,
        Err(err) => return internal_error(&err, "saved filter list failed"),
    };
    let mut filters = Vec::with_capacity(rows.len());
    for row in rows {
        match serde_json::from_str::<FilterSearch>(&row.search) {
            Ok(search) => filters.push(SavedFilter {
                id: row.id,
                name: row.name,
                search,
            }),
            // Only `create` below writes the column, through this same type, so a row it cannot
            // read was written by something else.
            Err(err) => {
                tracing::error!(error = %err, filter = %row.id, "a saved filter holds a search this build cannot read");
                return server_error(
                    "A saved filter in this library could not be read. Check the server logs for which one.",
                );
            }
        }
    }
    Json(filters).into_response()
}

/// `POST /api/libraries/{id}/filters`: save the grid's filters under a name.
pub async fn create(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(body): Json<NewSavedFilter>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "emptyName",
            "A saved filter needs a name. Type one and save again.",
        );
    }
    if name.chars().count() > NAME_MAX {
        return refused(
            StatusCode::BAD_REQUEST,
            "nameTooLong",
            &format!(
                "A saved filter's name can be at most {NAME_MAX} characters. Shorten it and save again."
            ),
        );
    }
    let search = match tidy(body.search) {
        Ok(search) => search,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };

    // A category from another library would narrow this library's grid to a folder it does not have.
    if let Some(folder) = search.folder_id {
        match PgFolders(state.db.clone()).library_of(folder).await {
            Ok(Some(owner)) if owner == library => {}
            Ok(_) => {
                return refused(
                    StatusCode::BAD_REQUEST,
                    "crossLibraryFolder",
                    "That category is not in this library, so a filter here cannot keep it. Choose a category from this library's tree and save again.",
                );
            }
            Err(err) => return internal_error(&err, "saved filter category lookup failed"),
        }
    }

    let stored = match serde_json::to_value(&search) {
        Ok(stored) => stored,
        Err(err) => {
            tracing::error!(error = %err, "a saved filter's search did not serialise");
            return server_error(
                "The filter could not be saved. Check the server logs for detail.",
            );
        }
    };
    match PgSavedFilters(state.db)
        .create(library, name, &stored)
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(SavedFilter {
                id,
                name: name.to_owned(),
                search,
            }),
        )
            .into_response(),
        Err(err @ DbError::NoSuchLibrary { .. }) => {
            refused(StatusCode::NOT_FOUND, "noSuchLibrary", &err.to_string())
        }
        Err(err @ DbError::SavedFilterNameTaken { .. }) => {
            refused(StatusCode::CONFLICT, "nameTaken", &err.to_string())
        }
        Err(err) => internal_error(&err, "saved filter create failed"),
    }
}

/// `DELETE /api/libraries/{library}/filters/{filter}`: take one saved filter off the list.
pub async fn remove(
    State(state): State<AppState>,
    Path((library, filter)): Path<(LibraryId, SavedFilterId)>,
) -> Response {
    match PgSavedFilters(state.db).remove(library, filter).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(
            StatusCode::NOT_FOUND,
            "noSuchFilter",
            "This library has no saved filter with that id. It may have been removed already, so reload the list.",
        ),
        Err(err) => internal_error(&err, "saved filter remove failed"),
    }
}

/// Each value trimmed and an empty one dropped. Refused, with the reason and the sentence to show,
/// when a value is too long or when nothing is left to filter by.
fn tidy(search: FilterSearch) -> Result<FilterSearch, (&'static str, String)> {
    let keep = |value: Option<String>| -> Result<Option<String>, (&'static str, String)> {
        let Some(value) = value else { return Ok(None) };
        let value = value.trim();
        if value.chars().count() > VALUE_MAX {
            return Err((
                "valueTooLong",
                format!(
                    "A saved filter keeps values of at most {VALUE_MAX} characters. Shorten the search and save again."
                ),
            ));
        }
        Ok((!value.is_empty()).then(|| value.to_owned()))
    };
    let tidied = FilterSearch {
        q: keep(search.q)?,
        folder_id: search.folder_id,
        format: keep(search.format)?,
        material: keep(search.material)?,
        tag: keep(search.tag)?,
    };
    if tidied == FilterSearch::default() {
        return Err((
            "emptySearch",
            "There is nothing to save yet. Set a search, a category, a format, a material or a tag on the grid, then save it."
                .to_owned(),
        ));
    }
    Ok(tidied)
}

/// A failure that is not the database's, with the sentence to show and the detail left to the log.
fn server_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}
