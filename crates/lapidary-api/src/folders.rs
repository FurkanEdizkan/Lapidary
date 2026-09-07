//! The category tree: read it, add to it, rearrange it, hide a branch of it.
//!
//! Four routes over `PgFolders`, and not one of them touches a file. That is the point
//! worth stating rather than leaving to be discovered: a category is a row, a model's
//! bytes live in a directory named after the categories above it, and the two are kept in
//! step only where a *model* moves — `moves.rs`, the one file in this crate allowed to
//! hold the store's rename handle at all.
//!
//! So **renaming a category does not rename its directory.** `file.storage_path` stays the
//! authoritative location of every model under it and stays correct, so nothing breaks;
//! what stops being true is that the directory names on disk mirror the category names on
//! screen for models ingested before the rename. That is a real wart, and it is the shape
//! the boundary gate chose: a folder route reaching for that rename handle to re-path a
//! whole subtree is exactly the copy-paste widening `xtask/src/deploy.rs`'s
//! `RELOCATE_MODULE` exists to refuse. Re-pathing a subtree belongs to a job that walks it
//! the way `migrate_storage` does, not to an HTTP handler holding a rename open.
//!
//! Delete is soft and cascading, and says so in its own body: the folder, its
//! subcategories and every model in any of them are marked deleted. Nothing leaves the
//! disk — `DATA.md` §1.6's purge is a separate, explicit action.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::slug::slugify;
use lapidary_core::{FolderId, LibraryId};
use lapidary_db::{DbError, PgFolders};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One node of the tree, flat. The client nests them by `parentId`.
///
/// Flat rather than nested, whole rather than a level at a time: at corpus scale the tree
/// is hundreds of rows, a lazy tree costs a round trip per expand on the one interaction
/// that has to feel instant (design §10), and a nested JSON shape built in SQL would be a
/// second structure to keep in step with the TypeScript one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderNode {
    pub id: FolderId,
    /// `null` at the library root. The tree has no synthetic root node — "All models" is a
    /// row the sidebar draws itself, because it stands for the absence of a filter and
    /// there is no id that means "no category".
    pub parent_id: Option<FolderId>,
    pub name: String,
    /// The directory this category owns, relative to its parent's.
    ///
    /// On the wire because a rename has to be able to name it. `folder.slug` is the
    /// category's *address*, allocated once at creation and never changed by a rename
    /// (`DATA.md` §1.1) — so after one rename the name on screen and the folder on disk
    /// differ, and the dialog that caused it is where a user should learn that, naming the
    /// folder they will actually find rather than describing the situation in the abstract.
    /// The store is meant to be opened in a file manager; this is what it will look like.
    pub slug: String,
    /// Live models in this category **and every category under it**.
    ///
    /// It rides along on every node so that one tree request answers the whole sidebar
    /// including every delete confirmation — the requirement is that a destructive
    /// confirmation names what it affects, and a count fetched per folder would be N
    /// requests for a panel that is one. Subtree-inclusive because the delete cascades that
    /// way, and it counts only models the grid would show, so the dialog cannot contradict
    /// the screen beside it.
    ///
    /// `number`, not `bigint`: ts-rs 12 maps a 64-bit integer to `bigint`, which
    /// `JSON.parse` never produces. Same override, same reason, as
    /// `PartSummary::sourceBytes`.
    #[ts(type = "number")]
    pub part_count: i64,
}

/// `POST /api/libraries/{id}/folders` — a category the user typed, not one a scan found.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewFolder {
    /// Absent or `null` puts it at the library root.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub parent_id: Option<FolderId>,
    pub name: String,
}

/// `PATCH /api/folders/{id}` — rename, reparent, or both.
///
/// `parentId` is a double `Option` because the two absences are different requests:
/// leaving the field out means "do not move it", and sending `null` means "move it to the
/// library root". Collapsing them would make every rename also move the folder to the root,
/// which is the kind of silent data change this project treats as a defect, not a default.
///
/// **Both fields are `#[ts(optional = nullable)]`, and for `parentId` that is the whole
/// point.** TypeScript can express the three states this type has — `{}`, `{parentId:
/// null}` and `{parentId: id}` — only as an *optional* property, because an omitted key
/// and a `null` one are different values there in a way they are not in most languages.
/// Exported as `parentId?: FolderId | null`, a client that spreads an object with
/// `parentId: undefined` sends nothing, and one that means the root has to write `null` on
/// purpose. Bound to `T | null` instead — ts-rs's default for `Option` — the two states
/// would collapse and every rename would quietly move its category to the library root.
// A `//` comment and not a `///` one: this is about the Rust build, and a doc comment here
// is copied verbatim into `web/src/bindings/FolderPatch.ts`, where a frontend reader has no
// use for it. The build prints `ts-rs failed to parse this attribute. It will be ignored.`
// for `deserialize_with` below — expected and harmless. ts-rs has no use for a deserializer
// name, and what it ignores is the serde attribute, not the `ts` ones above it. The
// exported type is committed and gated by `cargo xtask verify`, so a regression fails a
// build rather than hiding in a warning.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FolderPatch {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[ts(optional = nullable)]
    pub parent_id: Option<Option<FolderId>>,
}

/// Tell "the field was not sent" from "the field was sent as `null`".
///
/// `Option<Option<T>>` alone does not: serde hands `null` to the *outer* option, which
/// makes an explicit null indistinguishable from an absent key — a rename would then also
/// move the category to the library root, which is precisely the silent data change
/// [`FolderPatch`] exists to avoid. `#[serde(default)]` covers the absent case and this
/// covers the present one, so only a key that is actually there reaches here at all.
fn present_or_absent<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer).map(Some)
}

/// `GET /api/libraries/{id}/folders` — the whole tree, with counts.
///
/// An unknown library answers an empty list rather than a 404, matching the grid: an empty
/// library and an id that names nothing look alike to somebody browsing, and the sidebar's
/// "No categories yet" is the right thing to say about both.
pub async fn tree(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgFolders(state.db).tree(library).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| FolderNode {
                    id: row.id,
                    parent_id: row.parent_id,
                    name: row.name,
                    slug: row.slug,
                    part_count: row.part_count,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "folder tree query failed"),
    }
}

/// `POST /api/libraries/{id}/folders` — create one category.
///
/// The slug is derived here and never sent by the client: it is what the filesystem gets,
/// `slugify` is the one place that decides what a filesystem can hold, and a client that
/// could name the directory could name one outside the store.
pub async fn create(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(body): Json<NewFolder>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "emptyName",
            "A category needs a name. Type one and try again.",
        );
    }

    // A parent in another library would put this category in a tree it does not belong to,
    // and the directory it implies under `libraries/<other>/` is one no model here would
    // ever be written into.
    if let Some(parent) = body.parent_id {
        match PgFolders(state.db.clone()).library_of(parent).await {
            Ok(Some(owner)) if owner == library => {}
            Ok(_) => return cross_library_parent(),
            Err(err) => return internal_error(&err, "folder parent lookup failed"),
        }
    }

    match PgFolders(state.db)
        .create(library, body.parent_id, name, &slugify(name))
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(FolderNode {
                id,
                parent_id: body.parent_id,
                name: name.to_owned(),
                // The same `slugify(name)` the create above was given, not a second call:
                // one derivation, so the row and the response cannot describe different
                // directories.
                slug: slugify(name),
                // Brand new, so nothing is in it yet. Stated rather than re-queried.
                part_count: 0,
            }),
        )
            .into_response(),
        Err(err) => folder_error(&err, "folder create failed"),
    }
}

/// `PATCH /api/folders/{id}` — rename and/or reparent.
///
/// When a request asks for both, the move is applied first and the rename second, because
/// a name has to be unique among its *destination* siblings and applying them the other way
/// round would check it against the parent the folder is leaving. Each half is atomic on
/// its own; a rename refused after the move landed says so rather than reporting a failure
/// that would read as "nothing happened".
pub async fn patch(
    State(state): State<AppState>,
    Path(folder): Path<FolderId>,
    Json(body): Json<FolderPatch>,
) -> Response {
    let folders = PgFolders(state.db);

    let library = match folders.library_of(folder).await {
        Ok(Some(library)) => library,
        Ok(None) => return no_such_folder(),
        Err(err) => return internal_error(&err, "folder lookup failed"),
    };

    let mut moved = false;
    if let Some(parent) = body.parent_id {
        if let Some(parent) = parent {
            match folders.library_of(parent).await {
                Ok(Some(owner)) if owner == library => {}
                Ok(_) => return cross_library_parent(),
                Err(err) => return internal_error(&err, "folder parent lookup failed"),
            }
        }
        match folders.reparent(folder, parent).await {
            Ok(true) => moved = true,
            Ok(false) => return no_such_folder(),
            Err(err) => return folder_error(&err, "folder reparent failed"),
        }
    }

    if let Some(name) = body.name {
        let name = name.trim();
        if name.is_empty() {
            return refused(
                StatusCode::BAD_REQUEST,
                "emptyName",
                "A category needs a name. Type one and try again.",
            );
        }
        match folders.rename(folder, name).await {
            Ok(true) => {}
            Ok(false) => return no_such_folder(),
            Err(err) if moved => return partly_applied(&err),
            Err(err) => return folder_error(&err, "folder rename failed"),
        }
    }

    StatusCode::OK.into_response()
}

/// `DELETE /api/folders/{id}` — soft-delete the category and everything under it.
///
/// The counts come back so the caller can check what it warned about against what
/// happened. Nothing here removes a file, and the wording says so in the one place a user
/// might reasonably fear otherwise.
pub async fn delete(State(state): State<AppState>, Path(folder): Path<FolderId>) -> Response {
    match PgFolders(state.db).soft_delete_subtree(folder).await {
        // Nothing hidden at all means there was nothing there to hide: an unknown id, or a
        // folder somebody else already deleted.
        Ok((0, 0)) => no_such_folder(),
        Ok((folders, parts)) => Json(serde_json::json!({
            "foldersHidden": folders,
            "partsHidden": parts,
        }))
        .into_response(),
        Err(err) => internal_error(&err, "folder delete failed"),
    }
}

/// The refusals that carry a machine-readable `reason` beside their prose. The status is
/// the same for several distinct causes, so the client needs something other than the
/// message text — which is written for a person and will be rewritten — to tell them apart.
fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message, "reason": reason })),
    )
        .into_response()
}

fn no_such_folder() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchFolder",
        "There is no category with that id. It may have been deleted — reload the tree and \
         try again.",
    )
}

fn cross_library_parent() -> Response {
    refused(
        StatusCode::CONFLICT,
        "crossLibrary",
        "That parent category is in a different library, and a category can only be filed \
         under one in its own. Pick a parent from this library's tree.",
    )
}

/// A move that landed followed by a rename that did not. Reported as its own thing because
/// the two obvious wordings are both wrong here: "nothing changed" is false, and the plain
/// collision message would leave the caller believing the move was rolled back too.
fn partly_applied(err: &DbError) -> Response {
    refused(
        StatusCode::CONFLICT,
        "renamedAfterMove",
        &format!(
            "The category was moved, but keeping its name there was refused, so it still \
             carries the old one: {err} Rename it separately once you have picked a name \
             that is free."
        ),
    )
}

/// The `DbError`s these routes raise that are answers rather than failures — a name or
/// directory a sibling already holds, a move that would put a category inside itself, and a
/// library id that names nothing. Everything else is a 500.
fn folder_error(err: &DbError, what: &'static str) -> Response {
    match err {
        // `create` and `tree` deliberately answer an unknown library differently, and the
        // difference is read versus write. `tree` returns `[]` because an empty library and
        // an id naming nothing look alike to somebody browsing, and the sidebar's "No
        // categories yet" is true of both. A create cannot borrow that: reporting a category
        // made inside a library that does not exist is a lie the caller then builds on. So
        // this is the 404 an unknown *category* already gets, and the write says no.
        DbError::NoSuchLibrary { .. } => {
            refused(StatusCode::NOT_FOUND, "noSuchLibrary", &err.to_string())
        }
        DbError::FolderNameTaken { .. } => {
            refused(StatusCode::CONFLICT, "nameTaken", &err.to_string())
        }
        DbError::FolderSlugTaken { .. } => {
            refused(StatusCode::CONFLICT, "slugTaken", &err.to_string())
        }
        DbError::WouldCreateCycle { .. } => {
            refused(StatusCode::CONFLICT, "wouldCycle", &err.to_string())
        }
        other => internal_error(other, what),
    }
}

/// The query itself failed. Same shape and same reasoning as `parts.rs`'s: the operator
/// gets the real error through the log, the caller gets whatever `client_message` has
/// decided is safe to show.
fn internal_error(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
