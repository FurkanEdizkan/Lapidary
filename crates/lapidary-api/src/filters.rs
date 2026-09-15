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
use lapidary_db::{DbError, FilterMove, PgFolders, PgSavedFilters};
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
    /// One custom field this library offers as a filter, with `fieldValue` (`docs/DATA.md` §3.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field_value: Option<String>,
    /// With `field`, a number field's range in place of `fieldValue`: either bound, or both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field_min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field_max: Option<String>,
}

/// One saved filter.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SavedFilter {
    pub id: SavedFilterId,
    pub name: String,
    pub search: FilterSearch,
    /// It names a category that has been deleted since. The list marks it, and the grid opened on it
    /// says the category is gone instead of showing an empty grid.
    pub folder_gone: bool,
}

/// `PATCH /api/libraries/{library}/filters/{filter}`.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct RenameSavedFilter {
    pub name: String,
}

/// `POST /api/libraries/{library}/filters/{filter}/move`.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct MoveSavedFilter {
    pub direction: MoveDirection,
}

/// One place up the list, or one place down it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MoveDirection {
    Up,
    Down,
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
                folder_gone: row.folder_gone,
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
    let name = match tidy_name(&body.name) {
        Ok(name) => name,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };
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

    // A field this library does not offer as a filter, or a value not of its kind, would save a filter
    // that the grid then refuses.
    if let Err(refusal) = crate::fields::filter_of(
        &state.db,
        library,
        search.field.as_deref(),
        search.field_value.as_deref(),
        search.field_min.as_deref(),
        search.field_max.as_deref(),
    )
    .await
    {
        return refusal;
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
                // Its category was just found live in this library, or it has none.
                folder_gone: false,
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
        Ok(false) => no_such_filter(),
        Err(err) => internal_error(&err, "saved filter remove failed"),
    }
}

/// `PATCH /api/libraries/{library}/filters/{filter}`: a new name, by the rules a new filter's name
/// follows, and still unique in the library.
pub async fn rename(
    State(state): State<AppState>,
    Path((library, filter)): Path<(LibraryId, SavedFilterId)>,
    Json(body): Json<RenameSavedFilter>,
) -> Response {
    let name = match tidy_name(&body.name) {
        Ok(name) => name,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };
    match PgSavedFilters(state.db).rename(library, filter, name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_filter(),
        Err(err @ DbError::SavedFilterNameTaken { .. }) => {
            refused(StatusCode::CONFLICT, "nameTaken", &err.to_string())
        }
        Err(err) => internal_error(&err, "saved filter rename failed"),
    }
}

/// `POST /api/libraries/{library}/filters/{filter}/move`: one place up or down the list. At either end
/// nothing moves, and the answer is the same as for a move that did.
pub async fn move_filter(
    State(state): State<AppState>,
    Path((library, filter)): Path<(LibraryId, SavedFilterId)>,
    Json(body): Json<MoveSavedFilter>,
) -> Response {
    let direction = match body.direction {
        MoveDirection::Up => FilterMove::Up,
        MoveDirection::Down => FilterMove::Down,
    };
    match PgSavedFilters(state.db)
        .move_filter(library, filter, direction)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_filter(),
        Err(err) => internal_error(&err, "saved filter move failed"),
    }
}

/// A name trimmed, and refused with the reason and the sentence to show when it is empty or too long.
fn tidy_name(name: &str) -> Result<&str, (&'static str, String)> {
    let name = name.trim();
    if name.is_empty() {
        return Err((
            "emptyName",
            "A saved filter needs a name. Type one and save again.".to_owned(),
        ));
    }
    if name.chars().count() > NAME_MAX {
        return Err((
            "nameTooLong",
            format!(
                "A saved filter's name can be at most {NAME_MAX} characters. Shorten it and save again."
            ),
        ));
    }
    Ok(name)
}

fn no_such_filter() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchFilter",
        "This library has no saved filter with that id. It may have been removed, so reload the list.",
    )
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
        field: keep(search.field)?,
        field_value: keep(search.field_value)?,
        field_min: keep(search.field_min)?,
        field_max: keep(search.field_max)?,
    };
    // A field with neither a value nor a bound, or either without its field, filters nothing.
    let tidied = if tidied.field.is_none()
        || (tidied.field_value.is_none()
            && tidied.field_min.is_none()
            && tidied.field_max.is_none())
    {
        FilterSearch {
            field: None,
            field_value: None,
            field_min: None,
            field_max: None,
            ..tidied
        }
    } else {
        tidied
    };
    if tidied == FilterSearch::default() {
        return Err((
            "emptySearch",
            "There is nothing to save yet. Set a search, a category, a format, a material, a tag or a field on the grid, then save it."
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
