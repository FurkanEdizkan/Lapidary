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
use lapidary_api::server_port::{ChooseModelsFolder, HostFolders, MakeFolder, ModelsFolder};
use lapidary_db::{PgPool, PgSettings, PgSources};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Where the chosen folder is kept, as a host path.
const SETTING: &str = "models_dir";
/// And Lapidary's own files (A8).
const STORE_SETTING: &str = "store_dir";
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

/// Which folder: the models folder, or Lapidary's own files (A8). Each is a setting, fixed by its
/// `deploy/.env` variable.
#[derive(Debug, Clone, Copy)]
pub enum Which {
    Models,
    Store,
}

impl Which {
    fn setting(self) -> &'static str {
        match self {
            Self::Models => SETTING,
            Self::Store => STORE_SETTING,
        }
    }

    fn variable(self) -> &'static str {
        match self {
            Self::Models => "LAPIDARY_MODELS_DIR",
            Self::Store => "LAPIDARY_STORAGE_ROOT",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Models => "the models folder",
            Self::Store => "the folder for Lapidary's own files",
        }
    }
}

/// At start: the operator's folder when `deploy/.env` fixes one, else the one chosen here, else the
/// deployment's mount (`default`).
pub async fn first(
    db: &PgPool,
    which: Which,
    default: PathBuf,
    fixed: Option<String>,
    host: Option<&Host>,
) -> Folder {
    if fixed.is_some() {
        return Folder {
            dir: default,
            shown: fixed.filter(|shown| Path::new(shown).is_absolute()),
            locked_by: Some(which.variable().to_owned()),
        };
    }
    let saved = PgSettings(db.clone())
        .get(which.setting())
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
                which = which.name(),
                "the chosen folder is not there; using the deployment's until it is, or another is chosen"
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
    store: Arc<Folder>,
    restart: CancellationToken,
}

/// The routes. `restart` is what stops this process so it starts again on the new folder.
pub fn routes(
    db: PgPool,
    host: Option<Host>,
    folder: Folder,
    store: Folder,
    restart: CancellationToken,
) -> Router {
    let picker = Picker {
        db: db.clone(),
        host: host.map(Arc::new),
        folder: Arc::new(folder),
        store: Arc::new(store),
        restart,
    };
    let signed_in = Router::new()
        .route("/api/settings/storage", get(status).put(choose))
        .route("/api/settings/storage/folders", get(folders).post(make))
        .route_layer(axum::middleware::from_fn_with_state(
            db,
            lapidary_api::auth::require_user,
        ));
    // The wizard's, before anybody can sign in: they answer only with the setup code.
    let setup = Router::new()
        .route("/api/setup/storage", get(setup_status))
        .route(
            "/api/setup/storage/folders",
            get(setup_folders).post(setup_make),
        );
    signed_in
        .merge(setup)
        .layer(axum::middleware::from_fn(lapidary_api::auth::same_origin))
        .with_state(picker)
}

fn describe(picker: &Picker) -> ModelsFolder {
    ModelsFolder {
        path: picker.folder.shown.clone(),
        store: picker.store.shown.clone(),
        locked_by: picker.folder.locked_by.clone(),
        store_locked_by: picker.store.locked_by.clone(),
        can_choose: picker.host.is_some(),
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

async fn make(
    State(picker): State<Picker>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<MakeFolder>, JsonRejection>,
) -> Response {
    if !user.is_admin {
        return refused(StatusCode::FORBIDDEN, ADMINS_ONLY);
    }
    new_folder(&picker, body).await
}

async fn setup_make(
    State(picker): State<Picker>,
    headers: HeaderMap,
    body: Result<Json<MakeFolder>, JsonRejection>,
) -> Response {
    if !setup_open(&picker, &headers).await {
        return refused(StatusCode::FORBIDDEN, CODE_NEEDED);
    }
    new_folder(&picker, body).await
}

/// New folder (A9): one plain name inside a folder Lapidary may write, never over anything there.
/// It answers the new folder's listing.
async fn new_folder(picker: &Picker, body: Result<Json<MakeFolder>, JsonRejection>) -> Response {
    let Ok(Json(asked)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Say which folder to make, and its name.",
        );
    };
    let Some(host) = picker.host.clone() else {
        return refused(StatusCode::CONFLICT, NO_BROWSE_ROOT);
    };
    match tokio::task::spawn_blocking(move || make_in(&host, &asked.path, asked.name.trim())).await
    {
        Ok(Ok(listing)) => (StatusCode::CREATED, Json(listing)).into_response(),
        Ok(Err((status, sentence))) => refused(status, &sentence),
        Err(_) => refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Making the folder failed. Try again.",
        ),
    }
}

fn make_in(host: &Host, parent: &str, name: &str) -> Result<HostFolders, (StatusCode, String)> {
    let bad = |sentence: String| Err((StatusCode::BAD_REQUEST, sentence));
    if name.is_empty() || name.len() > 255 {
        return bad("Give the new folder a name.".to_owned());
    }
    if name.contains(['/', '\0']) || name.starts_with('.') || name == REMOVED {
        return bad(format!(
            "A folder can't be called {name}. Use a plain name, without / and not starting with a dot."
        ));
    }
    let Some(inner) = host.inside(Path::new(parent)) else {
        return bad(format!(
            "{parent} is outside the folders Lapidary can see, {}.",
            host.root.display()
        ));
    };
    // Where it really is, so a link can't put it outside the browse root.
    let listing = read(host, &inner)?;
    let real = host.inside(Path::new(&listing.path)).unwrap_or(inner);
    if off_limits(&listing.path) || !writable(&real) {
        return Err((StatusCode::BAD_REQUEST, cannot_make(&listing.path)));
    }
    let made = real.join(name);
    match std::fs::create_dir(&made) {
        Ok(()) => read(host, &made),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err((
            StatusCode::CONFLICT,
            format!(
                "{name} already exists in {}. Open it, or pick another name.",
                listing.path
            ),
        )),
        Err(_) => Err((StatusCode::BAD_REQUEST, cannot_make(&listing.path))),
    }
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
        root: host.outside(&host.dir),
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
        "Lapidary can't open {shown}. It runs as user {uid}; give it access with `sudo setfacl -m u:{uid}:rx \"{shown}\"` (and on each folder above it that it can't enter), then reload.",
        uid = rustix::process::getuid().as_raw()
    )
}

/// Not `cannot_write`'s recipe: that is for a folder somebody chose, never for `/` or `/proc`.
fn cannot_make(shown: &str) -> String {
    format!(
        "Lapidary can't make a folder in {shown}. It runs as user {uid}; open a folder that user owns and make it there.",
        uid = rustix::process::getuid().as_raw()
    )
}

fn cannot_write(shown: &str) -> String {
    format!(
        "Lapidary can't write to {shown}, so it can't keep uploads, removed models or Lapidary's own files there. To use this folder, run `sudo setfacl -R -m u:{uid}:rwX -m d:u:{uid}:rwX \"{shown}\"`, then reload.",
        uid = rustix::process::getuid().as_raw()
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
    let Some(host) = picker.host.clone() else {
        return refused(StatusCode::CONFLICT, NO_BROWSE_ROOT);
    };
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "Send the folders as `{\"path\": \"/srv/models\", \"store\": null}`.",
        );
    };
    let mut changes: Vec<(Which, String)> = Vec::new();
    // The models folder that will be in use, for the store's check that the two stay apart.
    let mut models_dir = picker.folder.dir.clone();
    if let Some(asked) = body.path {
        match choose_models(&picker, &host, asked).await {
            Ok(Some((shown, dir))) => {
                models_dir = dir;
                changes.push((Which::Models, shown));
            }
            Ok(None) => {}
            Err(refusal) => return refusal,
        }
    }
    if let Some(asked) = body.store {
        match choose_store(&picker, &host, asked, models_dir).await {
            Ok(Some(shown)) => changes.push((Which::Store, shown)),
            Ok(None) => {}
            Err(refusal) => return refusal,
        }
    }
    if changes.is_empty() {
        return Json(describe(&picker)).into_response();
    }
    let settings = PgSettings(picker.db.clone());
    for (which, shown) in &changes {
        if let Err(error) = settings
            .set(which.setting(), &serde_json::json!(shown))
            .await
        {
            tracing::error!(%error, which = which.name(), "could not save the folder");
            return refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The folder was not saved. Try again.",
            );
        }
        tracing::info!(folder = %shown, which = which.name(), "changed; restarting on it");
    }
    // The answer leaves first; the graceful stop waits for it either way.
    let restart = picker.restart.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        restart.cancel();
    });
    let mut now = describe(&picker);
    for (which, shown) in changes {
        match which {
            Which::Models => now.path = Some(shown),
            Which::Store => now.store = Some(shown),
        }
    }
    (StatusCode::ACCEPTED, Json(now)).into_response()
}

fn locked(which: Which, by: &str) -> Response {
    refused(
        StatusCode::CONFLICT,
        &format!(
            "{} is set by {by} in deploy/.env. Change it there and run `docker compose up -d`.",
            capital(which.name())
        ),
    )
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The models folder, checked, with Lapidary's own files in it copied over: its host path and its
/// path here, or `None` when it is the one in use.
async fn choose_models(
    picker: &Picker,
    host: &Arc<Host>,
    asked: String,
) -> Result<Option<(String, PathBuf)>, Response> {
    if let Some(by) = &picker.folder.locked_by {
        return Err(locked(Which::Models, by));
    }
    let current = picker.folder.dir.clone();
    let checked = {
        let host = host.clone();
        tokio::task::spawn_blocking(move || prepare(&host, &asked, &current, Which::Models)).await
    };
    let (shown, new_dir) = match checked {
        Ok(Ok(Some(chosen))) => chosen,
        Ok(Ok(None)) => return Ok(None),
        Ok(Err((status, sentence))) => return Err(refused(status, &sentence)),
        Err(_) => return Err(failed("Checking the folder failed. Try again.")),
    };
    let used = match PgSources(picker.db.clone()).folder_use(&OWN).await {
        Ok(used) => used,
        Err(error) => {
            tracing::error!(%error, "could not read what the models folder holds");
            return Err(failed("Reading the library failed. Try again."));
        }
    };
    if used.sources > 0 || used.in_place > 0 {
        let now = picker
            .folder
            .shown
            .clone()
            .unwrap_or_else(|| "the current models folder".to_owned());
        return Err(refused(
            StatusCode::CONFLICT,
            &format!(
                "{} model(s) and {} added file(s) or folder(s) are read from {now}, and would be lost track of in another folder. Remove them from the library first, or keep this folder.",
                used.in_place, used.sources
            ),
        ));
    }
    let from = picker.folder.dir.clone();
    let files = used.own_files;
    let copied = {
        let to = new_dir.clone();
        tokio::task::spawn_blocking(move || copy_own(&from, &to, &files)).await
    };
    match copied {
        Ok(Ok(())) => Ok(Some((shown, new_dir))),
        Ok(Err(sentence)) => Err(refused(StatusCode::CONFLICT, &sentence)),
        Err(_) => Err(failed("Copying Lapidary's files failed. Try again.")),
    }
}

/// Lapidary's own files (A8), checked and copied whole to the new folder: its host path, or `None`
/// when it is the one in use. Never inside the models folder, nor holding it.
async fn choose_store(
    picker: &Picker,
    host: &Arc<Host>,
    asked: String,
    models_dir: PathBuf,
) -> Result<Option<String>, Response> {
    if let Some(by) = &picker.store.locked_by {
        return Err(locked(Which::Store, by));
    }
    let from = picker.store.dir.clone();
    let checked = {
        let (host, from) = (host.clone(), from.clone());
        tokio::task::spawn_blocking(move || {
            let chosen = prepare(&host, &asked, &from, Which::Store)?;
            if let Some((shown, real)) = &chosen {
                let models = models_dir.canonicalize().unwrap_or(models_dir);
                if real.starts_with(&models) || models.starts_with(real) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("{shown} and the models folder would be one inside the other. Keep Lapidary's own files in a folder of their own."),
                    ));
                }
                copy_tree(&from, real).map_err(|sentence| (StatusCode::CONFLICT, sentence))?;
            }
            Ok(chosen)
        })
        .await
    };
    match checked {
        Ok(Ok(chosen)) => Ok(chosen.map(|(shown, _)| shown)),
        Ok(Err((status, sentence))) => Err(refused(status, &sentence)),
        Err(_) => Err(failed("Copying Lapidary's files failed. Try again.")),
    }
}

fn failed(sentence: &str) -> Response {
    refused(StatusCode::INTERNAL_SERVER_ERROR, sentence)
}

/// The chosen folder, checked: `None` when it is the one in use.
type Chosen = Result<Option<(String, PathBuf)>, (StatusCode, String)>;

fn prepare(host: &Host, asked: &str, current: &Path, which: Which) -> Chosen {
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
            format!(
                "{shown} can't be {}. Choose a folder of your own inside it.",
                which.name()
            ),
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
        if !copy_one(&from.join(rel), &to.join(rel), rel)? {
            tracing::warn!(file = %rel, "a file Lapidary wrote is no longer in the models folder; not copied");
        }
    }
    Ok(())
}

/// Copy every file under `from` to the same place under `to` (A8, Lapidary's own files), the same
/// way. Symbolic links are left out.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let mut folders = vec![PathBuf::new()];
    while let Some(rel) = folders.pop() {
        let entries = std::fs::read_dir(from.join(&rel)).map_err(|error| {
            format!(
                "Could not read {} to copy it ({error}).",
                from.join(&rel).display()
            )
        })?;
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let inner = rel.join(entry.file_name());
            if kind.is_dir() {
                folders.push(inner);
            } else if kind.is_file() {
                let shown = inner.display().to_string();
                copy_one(&from.join(&inner), &to.join(&inner), &shown)?;
            }
        }
    }
    Ok(())
}

/// One file, keeping its modified time, which a download compares. A file already there with the
/// same size is taken as copied; one that differs refuses. `false` when the source is gone.
fn copy_one(source: &Path, target: &Path, rel: &str) -> Result<bool, String> {
    let Ok(meta) = std::fs::metadata(source) else {
        return Ok(false);
    };
    if let Ok(there) = std::fs::metadata(target) {
        if there.len() == meta.len() {
            return Ok(true);
        }
        return Err(format!(
            "{rel} is already in the new folder and is a different file. Move it aside, or choose another folder."
        ));
    }
    let copy = || -> std::io::Result<()> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        std::io::copy(&mut std::fs::File::open(source)?, &mut out)?;
        out.set_modified(meta.modified()?)?;
        out.sync_all()
    };
    copy().map_err(|error| {
        format!("Could not copy {rel} into the new folder ({error}). Check that Lapidary may write there, then try again.")
    })?;
    Ok(true)
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
    fn a_new_folder_is_one_plain_name_and_never_over_anything() {
        let root = tempfile::tempdir().expect("a folder");
        let host = host(root.path());
        let made = make_in(&host, "/", "Printables").expect("made");
        assert_eq!(made.path, "/Printables");
        assert!(root.path().join("Printables").is_dir());
        assert_eq!(
            make_in(&host, "/", "Printables")
                .map(|_| ())
                .expect_err("a second one is refused")
                .0,
            StatusCode::CONFLICT
        );
        for name in ["", "a/b", "..", ".hidden", REMOVED] {
            assert_eq!(
                make_in(&host, "/", name)
                    .map(|_| ())
                    .expect_err("refused")
                    .0,
                StatusCode::BAD_REQUEST,
                "{name:?}"
            );
        }
        assert!(make_in(&host, "/../x", "y").is_err());
        // Where it may not write, it says so, and never hands out a setfacl for the folder.
        let shut = root.path().join("shut");
        std::fs::create_dir(&shut).expect("shut");
        std::fs::set_permissions(&shut, std::os::unix::fs::PermissionsExt::from_mode(0o555))
            .expect("read-only");
        if !writable(&shut) {
            let (status, said) = make_in(&host, "/shut", "x").expect_err("refused");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(said.contains("can't make a folder in /shut") && !said.contains("setfacl"));
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
        let models_dir = Which::Models;
        assert!(prepare(&host, "/", root.path(), models_dir).is_err());
        assert!(prepare(&host, "/missing", root.path(), models_dir).is_err());
        assert!(prepare(&host, "/models/../..", root.path(), models_dir).is_err());
        let chosen = prepare(&host, "/models", root.path(), models_dir).expect("chosen");
        assert_eq!(chosen.map(|(shown, _)| shown), Some("/models".to_owned()));
        // The one in use is no change.
        assert_eq!(prepare(&host, "/models", &models, models_dir), Ok(None));
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

    #[test]
    fn the_store_is_copied_whole_with_its_times() {
        let (from, to) = (
            tempfile::tempdir().expect("from"),
            tempfile::tempdir().expect("to"),
        );
        let rel = "libraries/default/flange-dn40-lp-3310-02_554fb0/flange-dn40-lp-3310-02.stl";
        std::fs::create_dir_all(from.path().join(rel).parent().expect("parent")).expect("folders");
        std::fs::write(from.path().join(rel), b"solid flange\nendsolid flange\n").expect("written");
        copy_tree(from.path(), to.path()).expect("copied");
        let (a, b) = (
            std::fs::metadata(from.path().join(rel)).expect("source"),
            std::fs::metadata(to.path().join(rel)).expect("copy"),
        );
        assert_eq!((a.len(), a.modified().ok()), (b.len(), b.modified().ok()));
        // Again is a no-op.
        copy_tree(from.path(), to.path()).expect("again");
    }
}
