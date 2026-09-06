//! Moving a model between categories.
//!
//! The one module allowed to name `SourceRelocator` — `xtask/src/deploy.rs`'s
//! `RELOCATE_MODULE` names this file exactly, so splitting the route across two files
//! fails the gate rather than quietly widening the capability. The handle it opens can
//! create a directory and rename one, and cannot read a byte of a source file; that is the
//! whole reason a rename earns a handle of its own rather than borrowing one that can
//! reach content. The gate is textual, so it reads doc comments too — which is why the
//! wider handles are described here and never named.
//!
//! **Ordering: rename first, inside the transaction, commit only if it succeeded.** A
//! failed rename rolls back and nothing moved. The window that remains is a rename that
//! succeeds and a commit that then fails, leaving the disk ahead of the database — which
//! `metadata.json` makes repairable, because every model directory identifies itself.
//!
//! What separates the two orderings is *how often each window opens*, not what it leaves
//! behind. Both leave a row naming a directory that reads fail on: commit-first leaves one
//! naming a path the rename never created, and rename-first leaves one naming the directory
//! the rename just emptied. The outcomes are symmetric. The frequencies are not. A rename
//! fails for ordinary reasons — a full volume, a permission, a destination that appeared —
//! and this ordering answers every one of those with a clean refusal and nothing moved. A
//! commit failing *after* a successful rename needs the connection to drop in the gap
//! between `COMMIT` and its acknowledgement, which is rare. So the common failure is made
//! total and the rare one is made repairable, rather than the other way round.
//!
//! The transaction itself lives in `PgParts::move_to_folder` (no SQL outside
//! `lapidary-db`), and the rename travels there as a closure — this crate holds the storage
//! handle, that crate holds the transaction, and neither may depend on the other's half.
//!
//! What a move does **not** touch is as load-bearing as what it does. `part.source_path` is
//! identity: where the file sat in the ingest directory, immutable, and the reason a
//! re-scan after a move recognises the same file and leaves it where the user put it.
//! `metadata.json` carries no path either, so a moved directory's manifest is still correct
//! and is not rewritten.

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use lapidary_core::slug::disambiguate;
use lapidary_core::{FolderId, PartId};
use lapidary_db::{DbError, MoveSource, PgFolders, PgParts};
use lapidary_storage::SourceRelocator;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MovePart {
    /// The category to file it under, or `null` for the library root. `null` is a real
    /// target and not the absence of one.
    pub folder_id: Option<FolderId>,
    /// The client has seen the collision warning and wants it anyway. Slice 6a decided two
    /// parts called `bracket` are the truth; this is how the UI says it knows.
    #[serde(default)]
    pub acknowledge_duplicate: bool,
}

/// One row of `GET /api/parts/{id}/moves`. Serialize-only and no ts-rs export: nothing on
/// the web side reads a history yet, and an exported type with no consumer is a file the
/// bindings gate then polices for nobody.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRecord {
    pub from_folder: Option<FolderId>,
    pub to_folder: Option<FolderId>,
    pub moved_at: Timestamp,
}

/// `PATCH /api/parts/{id}` — file a model under a category, or under none.
///
/// `200` when it moved, `404` when there is no such live part, and `409` for the three
/// things that are answers rather than failures: the storage migration has not reached this
/// model, the target is in another library, or a model of the same name is already there.
/// Those three share a status and are told apart by `reason` in the body — a client cannot
/// be asked to tell them apart by matching prose written for a person.
pub async fn move_part(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<MovePart>,
) -> Response {
    let parts = PgParts(state.db.clone());

    let source = match parts.move_source(part).await {
        Ok(Some(source)) => source,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "move source lookup failed"),
    };

    // Same folder, nothing to do — and short-circuited *before* the destination is
    // computed, not after. The disambiguation below asks whether the destination directory
    // already exists, and for a move into the category the model is already in the answer
    // is yes, because the model itself is standing in it: without this guard a no-op move
    // would rename `rock` to `rock_a1b2c3` for no reason at all.
    if source.folder == body.folder_id {
        return StatusCode::OK.into_response();
    }

    let (Some(old_directory), Some(hash)) = (source.directory.clone(), source.source_hash) else {
        return migration_pending();
    };

    if let Some(target) = body.folder_id {
        match PgFolders(state.db.clone()).library_of(target).await {
            Ok(Some(owner)) if owner == source.library => {}
            Ok(Some(_)) => return other_library(),
            Ok(None) => return no_such_folder(),
            Err(err) => return internal_error(&err, "move target lookup failed"),
        }
    }

    if !body.acknowledge_duplicate {
        match parts
            .name_taken_in_folder(source.library, body.folder_id, &source.name, part)
            .await
        {
            Ok(true) => return duplicate(&source.name),
            Ok(false) => {}
            Err(err) => return internal_error(&err, "duplicate name check failed"),
        }
    }

    let destination =
        match destination(&state, &source, body.folder_id, &old_directory, &hash).await {
            Ok(destination) => destination,
            Err(response) => return *response,
        };

    let relocator = SourceRelocator::open(&state.blob_root);
    // The category's own directory, created before the transaction opens: `rename` requires
    // the destination's parent to exist, and `create_dir` is `mkdir -p`, so a move into a
    // category nothing has been ingested into yet works the first time. Left behind if the
    // move then fails — an empty category directory, which is what a category with nothing
    // in it looks like anyway.
    if let Some((parent, _)) = destination.rsplit_once('/')
        && let Err(err) = relocator.create_dir(parent)
    {
        return storage_failure(
            &err,
            "storageUnwritable",
            "move destination directory create failed",
            "Could not create the category's directory in the storage folder, so nothing \
             was moved. Check that the storage volume is mounted and writable, then try \
             again.",
        );
    }

    match parts
        .move_to_folder(
            part,
            source.folder,
            body.folder_id,
            &old_directory,
            &destination,
            || {
                relocator
                    .rename(&old_directory, &destination)
                    .map_err(|err| err.to_string())
            },
        )
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        // `DbError::RenameFailed`'s own text is what the caller gets — `client_message`
        // passes it through, audited, because the storage layer composed it for an operator
        // — but the log entry is not optional either. Every 500 this route can answer now
        // leaves one.
        Err(err @ DbError::RenameFailed { .. }) => {
            tracing::error!(error = %err, "part move rename failed");
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "renameFailed",
                &err.client_message(),
            )
        }
        Err(err) => internal_error(&err, "part move failed"),
    }
}

/// `GET /api/parts/{id}/moves` — where this model has been filed, newest first.
///
/// A part that has never moved and a part id that names nothing both answer `[]`. That is
/// deliberate rather than lazy about the 404: nothing acts differently on the two, and the
/// second query it would take to tell them apart would be paid by every read of a history
/// that is empty for the honest reason.
pub async fn history(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).moves(part).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| MoveRecord {
                    from_folder: row.from_folder,
                    to_folder: row.to_folder,
                    moved_at: row.moved_at,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "move history query failed"),
    }
}

/// Where the model directory is going: `libraries/{library}/{category…}/{model}`.
///
/// The model directory keeps the name it already has rather than being re-slugified from
/// the part name. The name on disk was decided once, at ingest, possibly with a
/// disambiguating suffix, and `metadata.json` inside it is what identifies it — recomputing
/// it here would rename directories on a move for reasons having nothing to do with the
/// move.
///
/// If the destination is taken, the same rule ingest uses applies: `_` and six hex
/// characters of the source hash (`slug::disambiguate`), deterministic so the same bytes
/// land on the same name. That check is also what keeps `SourceRelocator::rename` away from
/// its one destructive behaviour — `std::fs::rename` silently *replaces* an empty existing
/// destination directory on Unix, and fails outright on Windows, which the Tauri shell
/// targets. Never handing it an existing destination is what makes the route behave the
/// same on both. A directory created by somebody else in the window between this check and
/// the rename is still possible, and is the narrow case where the Unix behaviour could
/// discard an empty directory; nothing with bytes in it can be lost that way, because
/// `rename` refuses a destination that is not empty.
async fn destination(
    state: &AppState,
    source: &MoveSource,
    target: Option<FolderId>,
    old_directory: &str,
    hash: &lapidary_core::BlobHash,
) -> Result<String, Box<Response>> {
    let library_slug = match PgParts(state.db.clone()).library_slug(source.library).await {
        Ok(Some(slug)) => slug,
        Ok(None) => return Err(Box::new(no_such_part())),
        Err(err) => {
            return Err(Box::new(internal_error(&err, "library slug lookup failed")));
        }
    };

    // Read back through `slug_path` rather than joined from the names the client sent:
    // a folder carries the slug it was created with, which need not match a re-slug of
    // today's name, and every other reader of this tree resolves the directory this way.
    let category = match target {
        Some(folder) => match PgFolders(state.db.clone()).slug_path(folder).await {
            Ok(path) => path,
            Err(err) => return Err(Box::new(internal_error(&err, "slug path lookup failed"))),
        },
        None => String::new(),
    };

    let base = format!("libraries/{library_slug}/{category}");
    let base = base.trim_end_matches('/');
    let model = old_directory
        .rsplit_once('/')
        .map_or(old_directory, |(_parent, model)| model);

    let mut destination = format!("{base}/{model}");
    if std::path::Path::new(&state.blob_root)
        .join(&destination)
        .exists()
    {
        destination = format!("{base}/{}", disambiguate(model, hash));
    }
    Ok(destination)
}

/// A refusal with a machine-readable `reason` beside its prose.
///
/// `409` has three distinct causes on this route and the status alone cannot tell them
/// apart, so the client would otherwise be reading the message text — which is written for
/// a person, and gets rewritten — to decide whether to offer "move anyway" or explain that
/// a migration is still running.
fn refused(status: StatusCode, reason: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "message": message, "reason": reason })),
    )
        .into_response()
}

fn no_such_part() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchPart",
        "There is no model with that id. It may have been deleted — reload the grid and try \
         again.",
    )
}

fn no_such_folder() -> Response {
    refused(
        StatusCode::CONFLICT,
        "noSuchFolder",
        "There is no category with that id, so there is nowhere to file this model. Reload \
         the category tree and try again.",
    )
}

/// Wording fixed by the plan, and it is the wording the card's own "not migrated yet"
/// message mirrors.
fn migration_pending() -> Response {
    refused(
        StatusCode::CONFLICT,
        "migrationPending",
        "This model has not finished moving into the new storage layout yet. Wait for the \
         storage migration to finish, then try again.",
    )
}

fn other_library() -> Response {
    refused(
        StatusCode::CONFLICT,
        "crossLibrary",
        "That category is in a different library. A model can only be filed under a \
         category in its own library — pick one from this library's tree.",
    )
}

/// Named, because the plan says name it: a warning that will not say which model it is
/// about is a warning the user has to go and check for themselves.
fn duplicate(name: &str) -> Response {
    refused(
        StatusCode::CONFLICT,
        "duplicateName",
        &format!(
            "A model called `{name}` is already in that category. Two models can share a \
             name — they are told apart by where they came from — so send the same move \
             again with acknowledgeDuplicate set if that is what you meant."
        ),
    )
}

/// [`internal_error`] for the one failure on this route that is not a `DbError`.
///
/// `StorageError` has no `client_message` to defer to, and its `Io` variant carries the
/// store's absolute path — the server's filesystem layout, which is the operator's business
/// and not the caller's. So the real error goes to the log and the caller gets a fixed
/// message, and the `reason` field is what a client matches on, exactly as for the two
/// refusals that share this status.
fn storage_failure(
    err: &lapidary_storage::StorageError,
    reason: &'static str,
    what: &'static str,
    message: &'static str,
) -> Response {
    tracing::error!(error = %err, "{what}");
    refused(StatusCode::INTERNAL_SERVER_ERROR, reason, message)
}

/// Same shape and reasoning as `parts.rs`'s: the operator gets the real error through the
/// log, the caller gets whatever `client_message` has decided is safe to show.
fn internal_error(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}
