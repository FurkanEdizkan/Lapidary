//! The models folder, chosen in the wizard and Settings (A6, owner 2026-10-17).
//!
//! A container cannot change its own mounts, so compose mounts one **browse root** (`LAPIDARY_BROWSE_ROOT`,
//! the whole machine by default) at `LAPIDARY_HOST_DIR`, and the models folder is a folder inside it,
//! saved as a host path. Choosing one saves it and restarts this process, which then reads and writes
//! there. `LAPIDARY_MODELS_DIR` in `deploy/.env` fixes the folder instead, as `LAPIDARY_PORT` fixes
//! the port.
//!
//! **Choosing never orphans a model.** A model read where it is in the current folder, or a file or
//! folder somebody added from it, refuses the change. What Lapidary wrote under its own folders is
//! copied to the same place in the new one, so those rows read the same path there. Nothing is
//! removed from the old folder.

use anyhow::Result;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use lapidary_api::auth::CurrentUser;
use lapidary_api::server_port::{ChooseModelsFolder, HostFolders, ModelsFolder};
use lapidary_db::{PgPool, PgSettings, PgSources};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Where the chosen folder is kept, as a host path.
const SETTING: &str = "models_dir";
/// Lapidary's own top-level folders in the models folder; a new folder gets a copy of them.
const OWN: [&str; 3] = ["examples", "uploads", "imports"];
/// Never a models folder: the kernel's and the running system's own.
const OFF_LIMITS: [&str; 4] = ["/proc", "/sys", "/dev", "/run"];
/// `lapidary_storage::REMOVED_DIR`: never a folder to choose or open here.
const REMOVED: &str = "_removed";
/// The header the wizard sends its setup code in, before anybody can sign in.
const SETUP_CODE: &str = "x-setup-code";

/// The browse root: `dir` inside this container is `root` on the host.
#[derive(Debug, Clone)]
pub struct Host {
    pub dir: PathBuf,
    pub root: PathBuf,
}

impl Host {
    /// A host path, inside this container. `None` for one outside the browse root or with a `..`.
    fn inside(&self, shown: &Path) -> Option<PathBuf> {
        let rest = shown.strip_prefix(&self.root).ok()?;
        if rest
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return None;
        }
        Some(self.dir.join(rest))
    }

    /// A path inside this container, on the host.
    fn outside(&self, inner: &Path) -> String {
        match inner.strip_prefix(&self.dir) {
            // `join("")` would end the root in a slash.
            Ok(rest) if rest.as_os_str().is_empty() => self.root.display().to_string(),
            Ok(rest) => self.root.join(rest).display().to_string(),
            Err(_) => inner.display().to_string(),
        }
    }
}

/// The models folder this process runs on.
#[derive(Debug, Clone)]
pub struct Folder {
    pub dir: PathBuf,
    pub shown: Option<String>,
    pub locked_by: Option<String>,
}

/// At start: the operator's folder when `deploy/.env` fixes one, else the one chosen here, else the
/// deployment's mount (`default`).
pub async fn first(
    db: &PgPool,
    default: PathBuf,
    fixed: Option<String>,
    host: Option<&Host>,
) -> Folder {
    if fixed.is_some() {
        return Folder {
            dir: default,
            shown: fixed.filter(|shown| Path::new(shown).is_absolute()),
            locked_by: Some("LAPIDARY_MODELS_DIR".to_owned()),
        };
    }
    let saved = PgSettings(db.clone())
        .get(SETTING)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(str::to_owned));
    if let (Some(host), Some(saved)) = (host, saved) {
        match host.inside(Path::new(&saved)).filter(|dir| dir.is_dir()) {
            Some(dir) => {
                return Folder {
                    dir,
                    shown: Some(saved),
                    locked_by: None,
                };
            }
            None => tracing::warn!(
                folder = %saved,
                "the chosen models folder is not there; using the deployment's until it is, or another is chosen"
            ),
        }
    }
    Folder {
        dir: default,
        shown: None,
        locked_by: None,
    }
}

#[derive(Clone)]
struct Picker {
    db: PgPool,
    host: Option<Arc<Host>>,
    folder: Arc<Folder>,
    store: Option<String>,
    restart: CancellationToken,
}

/// The routes. `restart` is what stops this process so it starts again on the new folder.
pub fn routes(
    db: PgPool,
    host: Option<Host>,
    folder: Folder,
    store: Option<String>,
    restart: CancellationToken,
) -> Router {
    let picker = Picker {
        db: db.clone(),
        host: host.map(Arc::new),
        folder: Arc::new(folder),
        store,
        restart,
    };
    let signed_in = Router::new()
        .route("/api/settings/storage", get(status).put(choose))
        .route("/api/settings/storage/folders", get(folders))
        .route_layer(axum::middleware::from_fn_with_state(
            db,
            lapidary_api::auth::require_user,
        ));
    // The wizard's, before anybody can sign in: they answer only with the setup code.
    let setup = Router::new()
        .route("/api/setup/storage", get(setup_status))
        .route("/api/setup/storage/folders", get(setup_folders));
    signed_in
        .merge(setup)
        .layer(axum::middleware::from_fn(lapidary_api::auth::same_origin))
        .with_state(picker)
}

fn describe(picker: &Picker) -> ModelsFolder {
    ModelsFolder {
        path: picker.folder.shown.clone(),
        store: picker.store.clone(),
        locked_by: picker.folder.locked_by.clone(),
        can_choose: picker.host.is_some() && picker.folder.locked_by.is_none(),
    }
}

async fn status(State(picker): State<Picker>) -> Response {
    Json(describe(&picker)).into_response()
}

async fn setup_open(picker: &Picker, headers: &HeaderMap) -> bool {
    let code = headers
        .get(SETUP_CODE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    lapidary_api::auth::setup_open(&picker.db, code)
        .await
        .unwrap_or(false)
}

async fn setup_status(State(picker): State<Picker>, headers: HeaderMap) -> Response {
    if !setup_open(&picker, &headers).await {
        return refused(StatusCode::FORBIDDEN, CODE_NEEDED);
    }
    Json(describe(&picker)).into_response()
}

const CODE_NEEDED: &str =
    "Setup is finished, or the setup code is wrong. Sign in and use Settings.";

#[derive(Deserialize)]
struct At {
    path: Option<String>,
}

async fn folders(
    State(picker): State<Picker>,
    Extension(user): Extension<CurrentUser>,
    Query(at): Query<At>,
) -> Response {
    if !user.is_admin {
        return refused(StatusCode::FORBIDDEN, ADMINS_ONLY);
    }
    list(&picker, at.path).await
}

const ADMINS_ONLY: &str = "Only an admin can choose the models folder.";

async fn setup_folders(
    State(picker): State<Picker>,
    headers: HeaderMap,
    Query(at): Query<At>,
) -> Response {
    if !setup_open(&picker, &headers).await {
        return refused(StatusCode::FORBIDDEN, CODE_NEEDED);
    }
    list(&picker, at.path).await
}

const NO_BROWSE_ROOT: &str = "This deployment mounts no folder to choose from. Set LAPIDARY_MODELS_DIR in deploy/.env and run `docker compose up -d`, or mount LAPIDARY_BROWSE_ROOT as deploy/compose.yaml does.";

async fn list(picker: &Picker, path: Option<String>) -> Response {
    let Some(host) = picker.host.clone() else {
        return refused(StatusCode::CONFLICT, NO_BROWSE_ROOT);
    };
    let shown = path
        .filter(|path| !path.trim().is_empty())
        .or_else(|| picker.folder.shown.clone())
        .unwrap_or_else(|| host.root.display().to_string());
    let Some(inner) = host.inside(Path::new(&shown)) else {
        return refused(
            StatusCode::BAD_REQUEST,
            &format!(
                "{shown} is outside the folders Lapidary can see, {}.",
                host.root.display()
            ),
        );
    };
    match tokio::task::spawn_blocking(move || read(&host, &inner)).await {
        Ok(Ok(listing)) => Json(listing).into_response(),
        Ok(Err((status, sentence))) => refused(status, &sentence),
        Err(_) => refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Reading the folder failed. Try again.",
        ),
    }
}

/// One folder's listing. A folder Lapidary can't open is still listed, with what to run.
fn read(host: &Host, inner: &Path) -> Result<HostFolders, (StatusCode, String)> {
    let real = inner.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            format!("There is no folder {}.", host.outside(inner)),
        )
    })?;
    if !real.starts_with(&host.dir) || !real.is_dir() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("There is no folder {}.", host.outside(inner)),
        ));
    }
    let shown = host.outside(&real);
    let parent = (real != host.dir)
        .then(|| real.parent().map(|parent| host.outside(parent)))
        .flatten();
    let (folders, mut problem) = match std::fs::read_dir(&real) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.') && name != REMOVED)
                .filter(|name| !off_limits(&host.outside(&real.join(name))))
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            (names, None)
        }
        Err(_) => (Vec::new(), Some(cannot_open(&shown))),
    };
    let choosable = real != host.dir && !off_limits(&shown);
    if choosable && problem.is_none() && !writable(&real) {
        problem = Some(cannot_write(&shown));
    }
    Ok(HostFolders {
        path: shown,
        parent,
        folders,
        choosable: choosable && problem.is_none(),
        problem,
    })
}

fn off_limits(shown: &str) -> bool {
    OFF_LIMITS
        .iter()
        .any(|bad| shown == *bad || shown.starts_with(&format!("{bad}/")))
}

/// May this process write here and open what is inside? Asked of the system, ACLs included;
/// nothing is created to find out.
fn writable(dir: &Path) -> bool {
    use rustix::fs::{Access, access};
    access(dir, Access::WRITE_OK | Access::EXEC_OK).is_ok()
}

fn cannot_open(shown: &str) -> String {
    format!(
        "Lapidary can't open {shown}. It runs as user 10001; give it access with `sudo setfacl -m u:10001:rx \"{shown}\"` (and on each folder above it that it can't enter), then reload."
    )
}

fn cannot_write(shown: &str) -> String {
    format!(
        "Lapidary can't write to {shown}, so it couldn't put uploads there or move a removed model into _removed. Run `sudo setfacl -R -m u:10001:rwX -m d:u:10001:rwX \"{shown}\"`, then reload."
    )
}

async fn choose(
    State(picker): State<Picker>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<ChooseModelsFolder>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return refused(StatusCode::FORBIDDEN, ADMINS_ONLY);
    }
    if let Some(by) = &picker.folder.locked_by {
        return refused(
            StatusCode::CONFLICT,
            &format!(
                "The models folder is set by {by} in deploy/.env. Change it there and run `docker compose up -d`."
            ),
        );
    }
    let Some(host) = picker.host.clone() else {
        return refused(StatusCode::CONFLICT, NO_BROWSE_ROOT);
    };
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the folder as `{\"path\": \"/srv/models\"}`.",
        );
    };
    let current = picker.folder.dir.clone();
    let db = picker.db.clone();
    let chosen = tokio::task::spawn_blocking(move || prepare(&host, &body.path, &current)).await;
    let (shown, new_dir) = match chosen {
        Ok(Ok(Some(chosen))) => chosen,
        // The folder it already runs on: nothing to do.
        Ok(Ok(None)) => return Json(describe(&picker)).into_response(),
        Ok(Err((status, sentence))) => return refused(status, &sentence),
        Err(_) => {
            return refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Checking the folder failed. Try again.",
            );
        }
    };
    let used = match PgSources(db.clone()).folder_use(&OWN).await {
        Ok(used) => used,
        Err(error) => {
            tracing::error!(%error, "could not read what the models folder holds");
            return refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Reading the library failed. Try again.",
            );
        }
    };
    if used.sources > 0 || used.in_place > 0 {
        let now = picker
            .folder
            .shown
            .clone()
            .unwrap_or_else(|| "the current models folder".to_owned());
        return refused(
            StatusCode::CONFLICT,
            &format!(
                "{} model(s) and {} added file(s) or folder(s) are read from {now}, and would be lost track of in another folder. Remove them from the library first, or keep this folder.",
                used.in_place, used.sources
            ),
        );
    }
    let from = picker.folder.dir.clone();
    let files = used.own_files;
    let copied = tokio::task::spawn_blocking({
        let new_dir = new_dir.clone();
        move || copy_own(&from, &new_dir, &files)
    })
    .await;
    match copied {
        Ok(Ok(())) => {}
        Ok(Err(sentence)) => return refused(StatusCode::CONFLICT, &sentence),
        Err(_) => {
            return refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Copying Lapidary's files failed. Try again.",
            );
        }
    }
    if let Err(error) = PgSettings(db).set(SETTING, &serde_json::json!(shown)).await {
        tracing::error!(%error, "could not save the models folder");
        return refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The folder was not saved. Try again.",
        );
    }
    tracing::info!(folder = %shown, "the models folder changed; restarting on it");
    // The answer leaves first; the graceful stop waits for it either way.
    let restart = picker.restart.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        restart.cancel();
    });
    (
        StatusCode::ACCEPTED,
        Json(ModelsFolder {
            path: Some(shown),
            store: picker.store.clone(),
            locked_by: None,
            can_choose: true,
        }),
    )
        .into_response()
}

/// The chosen folder, checked: `None` when it is the one in use.
type Chosen = Result<Option<(String, PathBuf)>, (StatusCode, String)>;

fn prepare(host: &Host, asked: &str, current: &Path) -> Chosen {
    let asked = asked.trim();
    let outside = || {
        (
            StatusCode::BAD_REQUEST,
            format!(
                "Choose a folder inside {}, the folder Lapidary can see.",
                host.root.display()
            ),
        )
    };
    let inner = host.inside(Path::new(asked)).ok_or_else(outside)?;
    let real = inner.canonicalize().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("There is no folder {asked}."),
        )
    })?;
    if !real.starts_with(&host.dir) || !real.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("There is no folder {asked}."),
        ));
    }
    let shown = host.outside(&real);
    if real == host.dir || off_limits(&shown) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{shown} can't be the models folder. Choose a folder of your own inside it."),
        ));
    }
    if !writable(&real) {
        return Err((StatusCode::BAD_REQUEST, cannot_write(&shown)));
    }
    if current.canonicalize().is_ok_and(|now| now == real) {
        return Ok(None);
    }
    Ok(Some((shown, real)))
}

/// Copy what Lapidary wrote under its own folders to the same place in `to`, keeping each file's
/// modified time, which a download compares. A file already there with the same size is taken as
/// copied; one that differs refuses, and nothing in either folder is removed.
fn copy_own(from: &Path, to: &Path, files: &[String]) -> Result<(), String> {
    for rel in files {
        let (source, target) = (from.join(rel), to.join(rel));
        let Ok(meta) = std::fs::metadata(&source) else {
            tracing::warn!(file = %rel, "a file Lapidary wrote is no longer in the models folder; not copied");
            continue;
        };
        if let Ok(there) = std::fs::metadata(&target) {
            if there.len() == meta.len() {
                continue;
            }
            return Err(format!(
                "{} is already in the new folder and is a different file. Move it aside, or choose another folder.",
                rel
            ));
        }
        let copy = || -> std::io::Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            std::io::copy(&mut std::fs::File::open(&source)?, &mut out)?;
            out.set_modified(meta.modified()?)?;
            out.sync_all()
        };
        copy().map_err(|error| {
            format!("Could not copy {rel} into the new folder ({error}). Check that Lapidary may write there, then try again.")
        })?;
    }
    Ok(())
}

fn refused(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "message": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(dir: &Path) -> Host {
        Host {
            dir: dir.to_path_buf(),
            root: PathBuf::from("/"),
        }
    }

    #[test]
    fn host_paths_map_into_the_mount_and_never_out_of_it() {
        let host = Host {
            dir: PathBuf::from("/host"),
            root: PathBuf::from("/"),
        };
        assert_eq!(
            host.inside(Path::new("/mnt/STL Files")),
            Some(PathBuf::from("/host/mnt/STL Files"))
        );
        assert_eq!(host.inside(Path::new("/mnt/../etc")), None);
        assert_eq!(host.inside(Path::new("relative")), None);
        assert_eq!(host.outside(Path::new("/host/mnt/x")), "/mnt/x");
        assert!(off_limits("/proc") && off_limits("/run/user") && !off_limits("/running"));
    }

    #[test]
    fn a_folder_is_chosen_only_inside_the_root_and_never_the_root_itself() {
        let root = tempfile::tempdir().expect("a folder");
        let models = root.path().join("models");
        std::fs::create_dir(&models).expect("models");
        let host = host(root.path());
        // The root itself, or a path the mount does not have.
        assert!(prepare(&host, "/", root.path()).is_err());
        assert!(prepare(&host, "/missing", root.path()).is_err());
        assert!(prepare(&host, "/models/../..", root.path()).is_err());
        let chosen = prepare(&host, "/models", root.path()).expect("chosen");
        assert_eq!(chosen.map(|(shown, _)| shown), Some("/models".to_owned()));
        // The one in use is no change.
        assert_eq!(prepare(&host, "/models", &models), Ok(None));
    }

    #[test]
    fn lapidarys_own_files_are_copied_with_their_time_and_never_over_a_different_file() {
        let (from, to) = (
            tempfile::tempdir().expect("from"),
            tempfile::tempdir().expect("to"),
        );
        std::fs::create_dir_all(from.path().join("examples")).expect("examples");
        let rel = "examples/flange-dn40-lp-3310-02.stl".to_owned();
        std::fs::write(from.path().join(&rel), b"solid flange\nendsolid flange\n")
            .expect("written");
        copy_own(from.path(), to.path(), std::slice::from_ref(&rel)).expect("copied");
        let (a, b) = (
            std::fs::metadata(from.path().join(&rel)).expect("source"),
            std::fs::metadata(to.path().join(&rel)).expect("copy"),
        );
        assert_eq!(a.len(), b.len());
        assert_eq!(a.modified().ok(), b.modified().ok());
        // Again is a no-op; a different file in the way refuses and stays as it was.
        copy_own(from.path(), to.path(), std::slice::from_ref(&rel)).expect("again");
        std::fs::write(to.path().join(&rel), b"other").expect("other");
        assert!(copy_own(from.path(), to.path(), std::slice::from_ref(&rel)).is_err());
        assert_eq!(std::fs::read(to.path().join(&rel)).expect("kept"), b"other");
    }
}
