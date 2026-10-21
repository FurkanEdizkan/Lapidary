//! Accounts on this install (A1, owner 2026-10-17): the setup wizard's first account, signing in and
//! out, and the layer that keeps every other route behind a session.
//!
//! **A session is a random token in an HttpOnly cookie**, and the database keeps only its BLAKE3 hash.
//! The cookie is `SameSite=Strict`, and a request that changes something is refused when its `Origin`
//! names a different host than it was sent to, so another site's page cannot act as a signed-in person.
//!
//! **Until somebody has an account, every guarded route answers 409 `setup_required`**, and the browser
//! goes to the wizard. The wizard asks for the setup code the server logged when it started, so nobody
//! else on the network can make themselves the admin first.

use crate::AppState;
use crate::derive::internal_error;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use lapidary_core::UserId;
use lapidary_db::{PgAccounts, PgPool, PgSettings, UserRow};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use ts_rs::TS;

/// The cookie's name.
pub const COOKIE: &str = "lapidary_session";
/// How long a session lasts unused. In use, it slides.
const SESSION_DAYS: i32 = 30;
/// What a wrong password, username or setup code costs, so guessing is slow.
const WRONG_ANSWER: Duration = Duration::from_secs(1);
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
/// The setup code's letters: no 0/O, 1/I/L, so a code read off a terminal is typed right.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// The person a request is from, put in its extensions by [`require_user`].
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

impl From<UserRow> for CurrentUser {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.username,
            display_name: row.display_name,
            is_admin: row.is_admin,
        }
    }
}

/// A person, as the browser sees them.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

impl From<&CurrentUser> for SessionUser {
    fn from(user: &CurrentUser) -> Self {
        Self {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            is_admin: user.is_admin,
        }
    }
}

/// What the wizard needs before anybody has an account.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    /// `true` until the wizard has made the first account.
    pub required: bool,
    /// The name this install already used for its owner (`profile`, M2), offered to the first account.
    pub suggested_name: Option<String>,
    /// Where the models folder, the inbox and the store are on the host, when the deployment has said
    /// so. Shown, never changed: compose decides them.
    pub host_ingest_root: Option<String>,
    pub host_inbox_root: Option<String>,
    pub host_storage_root: Option<String>,
    /// Whether this install may import from model sites; `false` when `LAPIDARY_IMPORT_LINKS=false`
    /// forces it off, which the wizard then shows rather than offers.
    pub import_links_forced_off: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    pub code: String,
    pub username: String,
    pub display_name: Option<String>,
    pub password: String,
    /// Import from model sites; `None` leaves it on.
    pub import_links: Option<bool>,
    /// How many imports run at once (A41); `None` leaves the default. The worker takes it when it
    /// next starts, which the wizard asks for once setup is done.
    #[serde(default)]
    #[ts(optional)]
    pub import_workers: Option<u16>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SignIn {
    pub username: String,
    pub password: String,
}

/// Every route but the public ones goes through this. A request with a live session gets a
/// [`CurrentUser`]; one without gets 409 `setup_required` while nobody has an account, else 401.
pub async fn require_user(State(db): State<PgPool>, mut request: Request, next: Next) -> Response {
    match who(db.clone(), request.headers()).await {
        Ok(user) => {
            if let Some(refusal) = scope(&db, &user, request.method(), request.uri().path()).await {
                return refusal;
            }
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(refusal) => refusal,
    }
}

/// **A person reaches their own models and nobody else's** (A3). Every route that names a library, a
/// part, a category or a revision in its path is checked here, once, before its handler runs: a
/// library or category must be theirs, and a part (or a revision of one) must be theirs to change it,
/// or theirs or shared with them to read it.
///
/// Somebody else's is refused with the same `404` an unknown library or part gets, so a person learns
/// nothing about anybody else's models — except a shared part they try to change, which is a `403`,
/// because they can already see it. An id that names nothing at all is left to the handler, which
/// answers it exactly as it always has. A path whose id is not a plain uuid is a 404 (A38), never let through.
async fn scope(db: &PgPool, user: &CurrentUser, method: &Method, path: &str) -> Option<Response> {
    use lapidary_core::{FolderId, LibraryId, PartId, RevisionId};
    let mut segments = path.trim_start_matches('/').split('/');
    let (Some("api"), Some(kind), Some(id)) = (segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    let scoped = matches!(kind, "libraries" | "parts" | "revisions" | "folders");
    // Fail closed (A38): an id written any other way — percent-encoded, above all — is decoded by the router into the
    // same id, so letting it through unchecked handed the handler somebody else's part. Every id here is a plain uuid.
    let Ok(id) = uuid::Uuid::parse_str(id) else {
        return scoped.then(|| {
            answer(
                StatusCode::NOT_FOUND,
                "not_found",
                "There is nothing at that address. Go back to your library and open it from there.",
            )
        });
    };
    let parts = lapidary_db::PgParts(db.clone());
    let reads = matches!(*method, Method::GET | Method::HEAD);
    let checked = async {
        match kind {
            "libraries" => library_refusal(&parts, LibraryId::from_uuid(id), user).await,
            "parts" => part_refusal(&parts, PartId::from_uuid(id), user, reads).await,
            "revisions" => match parts.revision_part(RevisionId::from_uuid(id)).await? {
                Some(part) => part_refusal(&parts, part, user, reads).await,
                None => Ok(None),
            },
            "folders" => match parts.folder_library(FolderId::from_uuid(id)).await? {
                Some(library) => Ok(library_refusal(&parts, library, user)
                    .await?
                    .map(|_| crate::folders::no_such_folder())),
                None => Ok(None),
            },
            _ => Ok(None),
        }
    };
    match checked.await {
        Ok(refusal) => refusal,
        Err(err) => Some(internal_error(&err, "access check failed")),
    }
}

/// `Some` refusal when the library exists and is somebody else's.
async fn library_refusal(
    parts: &lapidary_db::PgParts,
    library: lapidary_core::LibraryId,
    user: &CurrentUser,
) -> Result<Option<Response>, lapidary_db::DbError> {
    if parts.library_owner(library).await? == Some(user.id)
        || !parts.library_exists(library).await?
    {
        return Ok(None);
    }
    Ok(Some(crate::derive::no_such_library()))
}

/// `Some` refusal when the part exists and this person may not do this to it.
async fn part_refusal(
    parts: &lapidary_db::PgParts,
    part: lapidary_core::PartId,
    user: &CurrentUser,
    reads: bool,
) -> Result<Option<Response>, lapidary_db::DbError> {
    Ok(match parts.part_access(part, user.id).await? {
        Some(lapidary_db::Access::Owner) => None,
        Some(lapidary_db::Access::Reader) if reads => None,
        Some(lapidary_db::Access::Reader) => Some(answer(
            StatusCode::FORBIDDEN,
            "shared",
            "This model was shared with you to look at and download. Only its owner can change it.",
        )),
        None if parts.part_exists(part).await? => Some(crate::derive::no_such_part()),
        None => None,
    })
}

/// The person a request's cookie names, or the answer for nobody: 409 `setup_required` while nobody
/// has an account, else 401 `signed_out`.
async fn who(db: PgPool, headers: &HeaderMap) -> Result<CurrentUser, Response> {
    let accounts = PgAccounts(db);
    if let Some(token) = token(headers) {
        match accounts.session_user(&hash(&token), SESSION_DAYS).await {
            Ok(Some(user)) => return Ok(CurrentUser::from(user)),
            Ok(None) => {}
            Err(err) => return Err(internal_error(&err, "session lookup failed")),
        }
    }
    Err(match accounts.any_user().await {
        Ok(false) => setup_required(),
        Ok(true) => answer(
            StatusCode::UNAUTHORIZED,
            "signed_out",
            "Sign in to use Lapidary.",
        ),
        Err(err) => internal_error(&err, "account lookup failed"),
    })
}

/// A request that changes something must come from a page on this same host. Browsers send `Origin`
/// on every such request; a client that sends none is not a browser riding someone's cookie.
pub async fn same_origin(request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && let Some(origin) = request.headers().get(header::ORIGIN) {
        let host = request
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok());
        let origin_host = origin
            .to_str()
            .ok()
            .and_then(|o| o.split_once("://"))
            .map(|(_, rest)| rest.trim_end_matches('/'));
        if host.is_none() || origin_host != host {
            return answer(
                StatusCode::FORBIDDEN,
                "cross_origin",
                "This request came from another site, so Lapidary refused it. Open Lapidary itself and try again.",
            );
        }
    }
    next.run(request).await
}

/// `GET /api/setup`.
pub async fn setup_status(State(state): State<AppState>) -> Response {
    let required = match PgAccounts(state.db.clone()).any_user().await {
        Ok(any) => !any,
        Err(err) => return internal_error(&err, "account lookup failed"),
    };
    Json(SetupStatus {
        required,
        // 0060's profile row, which offered a name here, went with A3: the wizard asks for one.
        suggested_name: None,
        host_ingest_root: state.host_ingest_root.clone().filter(|_| required),
        host_inbox_root: state.host_inbox_root.clone().filter(|_| required),
        host_storage_root: state.host_storage_root.clone().filter(|_| required),
        import_links_forced_off: crate::site_settings::forced_off(),
    })
    .into_response()
}

/// `POST /api/setup` — the first account, signed in.
pub async fn setup(
    State(state): State<AppState>,
    body: Result<Json<SetupRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send the setup code, a username and a password.");
    };
    let accounts = PgAccounts(state.db.clone());
    match accounts.any_user().await {
        Ok(true) => return already_set_up(),
        Ok(false) => {}
        Err(err) => return internal_error(&err, "account lookup failed"),
    }
    match code_matches(&state.db, &body.code).await {
        Ok(true) => {}
        Ok(false) => {
            return answer(
                StatusCode::FORBIDDEN,
                "wrong_code",
                "That setup code is not the one this server printed. Run `docker compose logs app` and copy the line that says \"setup code\".",
            );
        }
        Err(err) => return internal_error(&err, "setup code lookup failed"),
    }
    let username = match username(&body.username) {
        Ok(name) => name,
        Err(response) => return *response,
    };
    if let Err(response) = password_ok(&body.password) {
        return *response;
    }
    let display_name = match display_name(body.display_name.as_deref()) {
        Ok(name) => name,
        Err(response) => return *response,
    };
    let range = crate::server_port::IMPORT_WORKERS_RANGE;
    if body.import_workers.is_some_and(|n| !range.contains(&n)) {
        return refused(&format!(
            "Choose from {} to {} imports at once.",
            range.start(),
            range.end()
        ));
    }
    let hash = match hash_password(body.password).await {
        Ok(hash) => hash,
        Err(response) => return *response,
    };
    let user = match accounts
        .create_first_admin(&username, display_name.as_deref(), &hash)
        .await
    {
        Ok(Some(user)) => CurrentUser::from(user),
        Ok(None) => return already_set_up(),
        Err(err) => return internal_error(&err, "first account failed"),
    };
    if let Some(on) = body.import_links
        && let Err(err) = PgSettings(state.db.clone())
            .set(
                crate::site_settings::IMPORT_LINKS,
                &serde_json::Value::Bool(on),
            )
            .await
    {
        // The account is made; a switch left at its default is not a reason to say setup failed.
        tracing::warn!(error = %err, "could not save the import switch the wizard set; it stays on");
    }
    if let Some(workers) = body.import_workers
        && let Err(err) = PgSettings(state.db.clone())
            .set(
                crate::server_port::IMPORT_WORKERS,
                &serde_json::json!(workers),
            )
            .await
    {
        tracing::warn!(error = %err, "could not save how many imports run at once; the default stays");
    }
    tracing::info!(username = %user.username, "setup made the first account");
    signed_in(&accounts, &user).await
}

/// `POST /api/session` — sign in.
pub async fn sign_in(
    State(state): State<AppState>,
    body: Result<Json<SignIn>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused("Send a username and a password.");
    };
    let accounts = PgAccounts(state.db);
    let username = body.username.trim().to_lowercase();
    let found = match accounts.credentials(&username).await {
        Ok(found) => found,
        Err(err) => return internal_error(&err, "sign-in lookup failed"),
    };
    let verified = match found {
        Some((user, hash)) => verify_password(body.password, hash).await.then_some(user),
        None => None,
    };
    let Some(user) = verified else {
        tokio::time::sleep(WRONG_ANSWER).await;
        return answer(
            StatusCode::UNAUTHORIZED,
            "wrong_password",
            "That username and password don't match. Check both and try again.",
        );
    };
    signed_in(&accounts, &CurrentUser::from(user)).await
}

/// `DELETE /api/session` — sign out. Always 204: signed out twice is still signed out.
pub async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = token(&headers)
        && let Err(err) = PgAccounts(state.db).end_session(&hash(&token)).await
    {
        return internal_error(&err, "sign-out failed");
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie("", 0));
    response
}

/// `GET /api/session` — who is signed in, or the guard's own 401 or 409 for nobody.
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match who(state.db, &headers).await {
        Ok(user) => Json(SessionUser::from(&user)).into_response(),
        Err(refusal) => refusal,
    }
}

/// The code the wizard asks for, made at start-up while nobody has an account. `None` when there is
/// nothing to set up, or when `LAPIDARY_SETUP_CODE` names the code instead.
pub async fn prepare_setup(db: &PgPool) -> Result<Option<String>, lapidary_db::DbError> {
    if PgAccounts(db.clone()).any_user().await? || env_code().is_some() {
        return Ok(None);
    }
    let mut bytes = [0u8; 12];
    OsRng.fill_bytes(&mut bytes);
    let letters: String = bytes
        .iter()
        .map(|b| CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()] as char)
        .collect();
    let code = format!("{}-{}-{}", &letters[..4], &letters[4..8], &letters[8..]);
    PgSettings(db.clone())
        .set(
            "setup_code",
            &serde_json::json!({ "blake3": blake3::hash(normal(&code).as_bytes()).to_hex().to_string() }),
        )
        .await?;
    Ok(Some(code))
}

/// A random password for `reset-password` to print: 20 letters from the setup code's alphabet.
pub fn new_password() -> String {
    let mut bytes = [0u8; 20];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()] as char)
        .collect()
}

/// A password as the database keeps it: an argon2id PHC string. Off the async threads, because it is
/// deliberately slow.
pub async fn hash_password(password: String) -> Result<String, Box<Response>> {
    let hashed = tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await;
    match hashed {
        Ok(Ok(hash)) => Ok(hash),
        _ => {
            tracing::error!("hashing a password failed");
            Err(Box::new(answer(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Could not save the password. Try again; if it keeps failing, the server log says why.",
            )))
        }
    }
}

/// The same checks the wizard makes, for every other place that takes a new password.
pub fn password_ok(password: &str) -> Result<(), Box<Response>> {
    let length = password.chars().count();
    if length < MIN_PASSWORD {
        return Err(Box::new(refused(&format!(
            "A password needs at least {MIN_PASSWORD} characters. Choose a longer one."
        ))));
    }
    if length > MAX_PASSWORD {
        return Err(Box::new(refused(&format!(
            "A password is at most {MAX_PASSWORD} characters. Choose a shorter one."
        ))));
    }
    Ok(())
}

pub(crate) async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

async fn signed_in(accounts: &PgAccounts, user: &CurrentUser) -> Response {
    let mut token = [0u8; 32];
    OsRng.fill_bytes(&mut token);
    if let Err(err) = accounts
        .start_session(user.id, &hash(&token), SESSION_DAYS)
        .await
    {
        return internal_error(&err, "starting a session failed");
    }
    let mut response = Json(SessionUser::from(user)).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie(&hex(&token), i64::from(SESSION_DAYS) * 86_400),
    );
    response
}

fn cookie(value: &str, max_age: i64) -> HeaderValue {
    let secure = std::env::var("LAPIDARY_COOKIE_SECURE")
        .is_ok_and(|v| matches!(v.trim(), "true" | "1" | "yes" | "on"));
    let cookie = format!(
        "{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    );
    // Hex and fixed words only, so this is always a valid header value.
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// The session token from the `Cookie` header, or `None` for a missing or malformed one.
fn token(headers: &HeaderMap) -> Option<[u8; 32]> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .and_then(|(_, value)| unhex(value))
}

/// The hash of this request's own session token, so changing a password can keep this session.
pub(crate) fn session_hash(headers: &HeaderMap) -> Option<[u8; 32]> {
    token(headers).map(|token| hash(&token))
}

fn hash(token: &[u8; 32]) -> [u8; 32] {
    *blake3::hash(token).as_bytes()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn env_code() -> Option<String> {
    std::env::var("LAPIDARY_SETUP_CODE")
        .ok()
        .map(|code| normal(&code))
        .filter(|code| !code.is_empty())
}

/// A code as typed: case, spaces and dashes don't matter.
fn normal(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Whether a request may use the wizard's own routes outside this crate (A6's folder picker): nobody
/// has an account yet, and it carries the setup code.
pub async fn setup_open(db: &PgPool, typed: &str) -> Result<bool, lapidary_db::DbError> {
    Ok(!PgAccounts(db.clone()).any_user().await? && code_matches(db, typed).await?)
}

/// Wrong setup codes, one at a time (A38): each holds this for its `WRONG_ANSWER`, so guesses sent in parallel still
/// come one a second, from the wizard's form and its folder picker alike.
static WRONG_CODES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn code_matches(db: &PgPool, typed: &str) -> Result<bool, lapidary_db::DbError> {
    let matches = code_is(db, typed).await?;
    if !matches {
        let _one_at_a_time = WRONG_CODES.lock().await;
        tokio::time::sleep(WRONG_ANSWER).await;
    }
    Ok(matches)
}

async fn code_is(db: &PgPool, typed: &str) -> Result<bool, lapidary_db::DbError> {
    let typed = normal(typed);
    if typed.is_empty() {
        return Ok(false);
    }
    if let Some(code) = env_code() {
        return Ok(typed == code);
    }
    let stored = PgSettings(db.clone()).get("setup_code").await?;
    let want = stored
        .as_ref()
        .and_then(|value| value.get("blake3"))
        .and_then(|value| value.as_str());
    Ok(want == Some(blake3::hash(typed.as_bytes()).to_hex().as_str()))
}

pub(crate) fn username(raw: &str) -> Result<String, Box<Response>> {
    let name = raw.trim().to_lowercase();
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if !first_ok || !rest_ok || name.len() > 40 {
        return Err(Box::new(refused(
            "A username is 1 to 40 letters, digits, dots, dashes or underscores, starting with a letter or digit.",
        )));
    }
    Ok(name)
}

pub(crate) fn display_name(raw: Option<&str>) -> Result<Option<String>, Box<Response>> {
    let name = raw.map(str::trim).filter(|name| !name.is_empty());
    if name.is_some_and(|name| name.chars().count() > 80) {
        return Err(Box::new(refused(
            "A display name is at most 80 characters. Shorten it and try again.",
        )));
    }
    Ok(name.map(str::to_owned))
}

fn setup_required() -> Response {
    answer(
        StatusCode::CONFLICT,
        "setup_required",
        "Nobody has an account on this install yet. Open Lapidary in a browser to run setup.",
    )
}

fn already_set_up() -> Response {
    answer(
        StatusCode::CONFLICT,
        "already_set_up",
        "This install is already set up. Sign in instead, or ask its admin for an account.",
    )
}

fn refused(message: &str) -> Response {
    answer(StatusCode::BAD_REQUEST, "refused", message)
}

fn answer(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "code": code, "message": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_survives_the_cookie_and_nothing_else_reads_as_one() {
        let token = [7u8; 32];
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("theme=dark; {COOKIE}={}", hex(&token)))
                .expect("header"),
        );
        assert_eq!(super::token(&headers), Some(token));
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("lapidary_session=zz"),
        );
        assert_eq!(super::token(&headers), None);
    }

    #[test]
    fn a_username_is_folded_and_a_strange_one_refused() {
        assert_eq!(username("  Lena.K ").ok().as_deref(), Some("lena.k"));
        assert!(username("-lena").is_err());
        assert!(username("le na").is_err());
        assert!(username("").is_err());
    }

    #[test]
    fn a_setup_code_ignores_case_spaces_and_dashes() {
        assert_eq!(normal("abcd-efgh 2345"), "ABCDEFGH2345");
    }
}
