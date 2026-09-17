//! Pulling what somebody shares (sharing S3): the files of a mirrored share, into one of this installation's libraries.
//!
//! The api records a pull (`PgPulls::start`) and wakes the peer role, which alone holds the key a paired machine answers.
//! Each file this library does not already hold at its place is fetched from the sharer's blob route into the staging
//! volume as `<blake3>.part`, resumed from the staged length, and checked against its BLAKE3 when whole; a file that
//! does not match is dropped and the pull fails saying so. The files are then written into bundles of at most
//! [`BUNDLE_MAX`], stored with `SourceWriter` — this crate's one write to the store, and `cargo xtask check-deploy` keeps
//! it to this module — and queued as `ImportBundle` jobs into one batch, which the worker imports as it imports any
//! bundle. Once the batch settles, each part is named with who it came from.
//!
//! **Where parts land.** Import files a part by its source path, creating the categories its directories imply, so the
//! puller writes each one's as `Shared/<sharer's name> (<first group of their device id>)/<their source path>`
//! ([`shared_path`]). The device id's group keeps two sharers with one name apart.
//!
//! **What a pull again does.** A file this library holds at that place with those bytes is not fetched at all. A changed
//! file is fetched and imported as the destination's rules say: a hobby library keeps no revisions and counts it
//! unkept. A controlled destination refuses it, because a bundle carries only the sharer's current revision and import
//! will not graft that onto a history it does not start; recorded in `docs/ROADMAP.md`.

use crate::sync::{listen, wait};
use crate::{PeerIdentity, client_config};
use lapidary_core::{BlobHash, JobPayload, RevisionOrigin};
use lapidary_db::{
    DbError, Holder, MirroredPartRow, PgBlobs, PgJobs, PgMirror, PgPool, PgPulls, PullRow,
    StoredBlobRow,
};
use lapidary_storage::{Compression, SourceWriter};
use lapidary_targets::bundle::{
    self, Manifest, ManifestLibrary, ManifestPart, ManifestRevision, ManifestSource, StoreZip,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// The most file bytes one bundle carries. The worker reads a bundle whole into memory, once per part, so this is what
/// one import job costs it; a file larger than this travels in a bundle of its own.
pub const BUNDLE_MAX: u64 = 64 * 1024 * 1024;

/// The most files one bundle carries, well inside what an import reads.
const BUNDLE_FILES_MAX: usize = 1_000;

/// How long a connection to the sharer may take to open, and a read may wait for bytes. Not a whole request: a large
/// file over a slow link takes as long as it takes.
const PATIENCE: Duration = Duration::from_secs(30);

/// How often a pull whose bundles are queued asks whether their batch has settled.
const SETTLED_POLL: Duration = Duration::from_secs(2);

/// Where a part pulled from `sharer` is filed: `Shared/<name> (<first group of the device id>)/<source path>`. A name
/// is made one folder, whatever it holds; with no name, the group alone.
pub fn shared_path(
    sharer: Option<&str>,
    device: lapidary_core::DeviceId,
    source_path: &str,
) -> String {
    let id = device.to_string();
    let group = id.split('-').next().unwrap_or(&id);
    let name: String = sharer
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim();
    if name.is_empty() {
        format!("Shared/{group}/{source_path}")
    } else {
        format!("Shared/{name} ({group})/{source_path}")
    }
}

/// A file fetched whole and checked: where it is staged, and how many bytes crossed the network for it this time.
#[derive(Debug)]
pub struct Fetched {
    pub path: PathBuf,
    pub sent: u64,
}

#[derive(Debug, PartialEq)]
pub enum FetchError {
    /// Stopped for a reason another attempt may not meet: the connection dropped, the sharer is away. What was staged
    /// stays, and the next attempt resumes from it.
    Stalled(String),
    /// Refused, or the wrong bytes: another attempt would meet the same answer.
    Refused(String),
    /// The share is not shared with this installation any more.
    NotShared,
    /// The share asks first, and this installation's request is not granted: it waits, or was declined.
    NotGranted,
}

/// Fetch one file into `staging`: nothing when it is staged whole already, the rest when part of it is, and all of it
/// otherwise. Checked against `hash` once whole; a file that does not match is dropped.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    staging: &Path,
    hash: &BlobHash,
    size: u64,
) -> Result<Fetched, FetchError> {
    let hex = hash.to_hex();
    let whole = staging.join(&hex);
    if tokio::fs::try_exists(&whole).await.unwrap_or(false) {
        return Ok(Fetched {
            path: whole,
            sent: 0,
        });
    }
    let partial = staging.join(format!("{hex}.part"));
    let staged = tokio::fs::metadata(&partial)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    // More than the whole file is not the start of this one.
    let start = if staged > size { 0 } else { staged };
    let staging_failed = |err: std::io::Error| {
        FetchError::Stalled(format!(
            "Could not stage {hex}: {err}. Check the staging volume has room."
        ))
    };
    // A file staged to its last byte was stopped before its check: check it, rather than ask for a range that starts
    // at the end, which the sharer rightly refuses.
    let sent = if start > 0 && start == size {
        0
    } else {
        transfer(client, url, &partial, start, &hex).await?
    };

    let have = tokio::fs::metadata(&partial)
        .await
        .map(|meta| meta.len())
        .map_err(staging_failed)?;
    if have < size {
        return Err(FetchError::Stalled(format!(
            "The transfer of {hex} stopped at {have} of {size} bytes. The next attempt resumes there."
        )));
    }
    let checked = partial.clone();
    let actual = tokio::task::spawn_blocking(move || hash_file(&checked))
        .await
        .map_err(|err| FetchError::Stalled(err.to_string()))?
        .map_err(staging_failed)?;
    if have > size || actual != *hash {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(FetchError::Refused(format!(
            "The file sent for {hex} is not the file its sharer's catalogue names: its size or BLAKE3 differs. It was dropped. Pull again; if it happens again, the sharer's store needs checking."
        )));
    }
    tokio::fs::rename(&partial, &whole)
        .await
        .map_err(staging_failed)?;
    Ok(Fetched { path: whole, sent })
}

/// Fetch `url` into `partial` from byte `start`, adding to what is staged when the sharer resumes and replacing it when
/// the sharer sends the whole file. Answers how many bytes arrived.
async fn transfer(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    start: u64,
    hex: &str,
) -> Result<u64, FetchError> {
    let mut request = client.get(url);
    if start > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
    }
    let mut response = request.send().await.map_err(|err| {
        FetchError::Stalled(format!("Could not reach the sharer for {hex}: {err}."))
    })?;
    let status = response.status();
    let append = match status {
        reqwest::StatusCode::PARTIAL_CONTENT if start > 0 => true,
        reqwest::StatusCode::OK => false,
        _ => {
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                let _ = tokio::fs::remove_file(partial).await;
            }
            let body = response
                .bytes()
                .await
                .ok()
                .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
                .unwrap_or_default();
            let message = body["message"].as_str().map_or_else(
                || format!("The sharer answered {status} for {hex}."),
                str::to_owned,
            );
            // By the sharer's reason, not the status class: a share that asks first answers 403 until it is granted,
            // and a busy sharer 429, and neither is a refusal to fail a pull over.
            return Err(match body["reason"].as_str() {
                Some("notShared") => FetchError::NotShared,
                Some("askFirst" | "denied") => FetchError::NotGranted,
                _ if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE
                    || !status.is_client_error() =>
                {
                    FetchError::Stalled(message)
                }
                _ => FetchError::Refused(message),
            });
        }
    };

    let staging_failed = |err: std::io::Error| {
        FetchError::Stalled(format!(
            "Could not stage {hex}: {err}. Check the staging volume has room."
        ))
    };
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(partial)
        .await
        .map_err(staging_failed)?;
    let mut sent = 0u64;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).await.map_err(staging_failed)?;
                sent += chunk.len() as u64;
            }
            Ok(None) => break,
            Err(err) => {
                let _ = file.flush().await;
                return Err(FetchError::Stalled(format!(
                    "The transfer of {hex} stopped after {sent} bytes: {err}. The next attempt resumes there."
                )));
            }
        }
    }
    file.flush().await.map_err(staging_failed)?;
    Ok(sent)
}

fn hash_file(path: &Path) -> std::io::Result<BlobHash> {
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(BlobHash::from_bytes(*hasher.finalize().as_bytes()))
}

/// Pulls, oldest first, until `shutdown`: at start, when the api records one, and on the hello round's tick, which is
/// when a pull that stalled tries again.
///
/// ponytail: one pull at a time, so a pull whose sharer is away holds the ones behind it until they answer. Work pulls
/// per sharer if people pull from several at once.
pub async fn run(
    db: PgPool,
    identity: Arc<PeerIdentity>,
    staging: PathBuf,
    blob_root: PathBuf,
    shutdown: CancellationToken,
) {
    if let Err(error) = tokio::fs::create_dir_all(&staging).await {
        tracing::error!(%error, staging = %staging.display(), "could not create the staging directory; nothing can be pulled");
        return;
    }
    let mut listener = listen(&db).await;
    loop {
        loop {
            let pull = match PgPulls(db.clone()).next().await {
                Ok(Some(pull)) => pull,
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(%error, "could not read the pulls waiting; the next round tries again");
                    break;
                }
            };
            match work(&db, &identity, &staging, &blob_root, &pull).await {
                Ok(()) => {}
                Err(why) => {
                    tracing::warn!(pull = %pull.id.as_uuid(), %why, "a pull stalled; the next round tries again");
                    if let Err(error) = PgPulls(db.clone()).stalled(pull.id, &why).await {
                        tracing::warn!(%error, "could not record why a pull stalled");
                    }
                    break;
                }
            }
        }
        if !wait(&mut listener, &shutdown).await {
            return;
        }
    }
}

/// One file of a pull: the mirrored part, where it lands here, and its bytes' hash and size.
struct Wanted {
    part: MirroredPartRow,
    place: String,
    hash: BlobHash,
    size: u64,
}

fn db_error(err: DbError) -> String {
    err.to_string()
}

/// Work one pull as far as it goes. `Ok` once it is finished, done or failed; `Err` when it stalled, with why.
pub async fn work(
    db: &PgPool,
    identity: &PeerIdentity,
    staging: &Path,
    blob_root: &Path,
    pull: &PullRow,
) -> Result<(), String> {
    let pulls = PgPulls(db.clone());
    let who = sharer_named(pull);
    if pull.removed && pull.state != "importing" {
        pulls
            .finish(
                pull.id,
                Some(&format!(
                    "You removed {who}, so {} is not pulled from them any more. Parts already pulled stay.",
                    pull.share_name
                )),
            )
            .await
            .map_err(db_error)?;
        return Ok(());
    }
    let (Some(share), Some(remote)) = (pull.share, pull.remote) else {
        return stopped_sharing(&pulls, pull).await;
    };
    let mirror = PgMirror(db.clone());
    // One part when the pull names one (S9), the whole folder when it does not.
    let catalogue = match pull.source_path.as_deref() {
        Some(path) => mirror
            .part(share, path)
            .await
            .map_err(db_error)?
            .into_iter()
            .collect(),
        None => mirror
            .parts(share, None, i64::MAX)
            .await
            .map_err(db_error)?,
    };
    let mut files = Vec::new();
    for part in catalogue {
        let (Some(hex), Some(size)) = (part.blake3.as_deref(), part.size_bytes) else {
            continue;
        };
        let (Ok(hash), Ok(size)) = (BlobHash::parse_hex(hex), u64::try_from(size)) else {
            continue;
        };
        let place = shared_path(pull.sharer.as_deref(), pull.device, &part.source_path);
        // The sharer's paths are another machine's word: one that would escape the library is left out, not filed.
        if lapidary_core::slug::reject_escaping_path(&place).is_err() {
            tracing::warn!(source_path = %part.source_path, "left a shared part out of a pull: its path leaves the library");
            continue;
        }
        files.push(Wanted {
            part,
            place,
            hash,
            size,
        });
    }
    let places: Vec<String> = files.iter().map(|file| file.place.clone()).collect();

    if pull.state == "importing"
        && let Some(batch) = pull.batch
    {
        return settle(db, pull, batch, &places).await;
    }

    let asked: Vec<(&str, BlobHash)> = files
        .iter()
        .map(|file| (file.place.as_str(), file.hash))
        .collect();
    let held = PgBlobs(db.clone())
        .library_holds_each(pull.library, &asked)
        .await
        .map_err(db_error)?;
    let wanted: Vec<Wanted> = files
        .into_iter()
        .zip(held)
        .filter_map(|(file, held)| (!held).then_some(file))
        .collect();
    let bytes_total: u64 = wanted.iter().map(|file| file.size).sum();
    if wanted.is_empty() {
        if !pulls.fetching(pull.id, 0, 0).await.map_err(db_error)? {
            return Ok(());
        }
        return settled(db, pull, &places).await;
    }

    // Everybody who may be asked for this folder's files: its owner, and the people on the roster its owner
    // published that this installation is paired with (S9). The owner comes first while they are answering.
    let holders = mirror.holders(share).await.map_err(db_error)?;
    let mut clients: Vec<(Holder, reqwest::Client)> = Vec::with_capacity(holders.len());
    for holder in holders {
        let tls = client_config(identity, holder.device).map_err(|err| err.to_string())?;
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .connect_timeout(PATIENCE)
            .read_timeout(PATIENCE)
            .build()
            .map_err(|err| {
                format!(
                    "Could not set up a connection to {}: {err}.",
                    holder.address
                )
            })?;
        clients.push((holder, client));
    }
    let owner = clients.iter().position(|(holder, _)| holder.owner);
    let shared = format!(
        "https://{}/peer/v1/shares/{}",
        pull.address,
        remote.as_uuid()
    );
    // Asked before every attempt, since asking again changes nothing: a share that asks first answers where this
    // installation stands, and nothing is fetched until it is granted.
    //
    // Only its owner can answer it, so an owner that cannot be reached is not a refusal when somebody else in the
    // folder holds the files: the other holders were told the owner's answer with the roster, and enforce it.
    let mut ask_owner = owner;
    if let Some(index) = owner {
        match ask(&clients[index].1, &format!("{shared}/request")).await {
            Ok(Standing::MayFetch) => {}
            Ok(Standing::Waiting) => {
                pulls.waiting(pull.id).await.map_err(db_error)?;
                return Err(format!(
                    "Waiting for {who} to let you pull {}.",
                    pull.share_name
                ));
            }
            Ok(Standing::Denied) => {
                let why = format!(
                    "{who} declined your request to pull {}. Parts already pulled stay.",
                    pull.share_name
                );
                pulls.finish(pull.id, Some(&why)).await.map_err(db_error)?;
                return Ok(());
            }
            Ok(Standing::NotShared) => return stopped_sharing(&pulls, pull).await,
            // One failed request is not an owner being away, and the roster's answer about fetching is a
            // snapshot: an owner who is answering hellos and did not answer this must be asked again, or an
            // owner who has just closed a folder's files would be fetched from through its other people.
            Err(why) if clients.len() == 1 || clients[index].0.online => return Err(why),
            // Silent for three hello rounds: away, and somebody else in the folder may hold what it holds.
            Err(why) => {
                tracing::info!(pull = %pull.id.as_uuid(), %why, "a folder's owner is away; asking its other people");
                ask_owner = None;
            }
        }
    }
    if !pulls
        .fetching(
            pull.id,
            i32::try_from(wanted.len()).unwrap_or(i32::MAX),
            i64::try_from(bytes_total).unwrap_or(i64::MAX),
        )
        .await
        .map_err(db_error)?
    {
        return Ok(());
    }
    let (mut files_done, mut bytes_done, mut sent) = (0i32, 0i64, 0u64);
    let mut staged = Vec::with_capacity(wanted.len());
    for file in &wanted {
        // Asked one at a time, in the order the holders came in, and the first yes is the one fetched from. A
        // holder that stalls or is busy is passed over for the next; nobody reachable leaves the pull to say so.
        let Some((holder, client)) =
            holder_for(&clients, ask_owner, file, remote, pull.device).await
        else {
            let why = format!(
                "Nobody reachable has {} yet. It is tried again shortly.",
                file.part.name
            );
            tracing::info!(pull = %pull.id.as_uuid(), files_done, "no holder answered for a file");
            return Err(why);
        };
        let url = format!(
            "https://{}/peer/v1/shares/{}/blob/{}{}",
            holder.address,
            remote.as_uuid(),
            file.hash.to_hex(),
            if holder.owner {
                String::new()
            } else {
                format!("?owner={}", pull.device)
            }
        );
        match fetch(client, &url, staging, &file.hash, file.size).await {
            Ok(fetched) => {
                sent += fetched.sent;
                files_done += 1;
                bytes_done =
                    bytes_done.saturating_add(i64::try_from(file.size).unwrap_or(i64::MAX));
                staged.push(fetched.path);
                if !pulls
                    .progress(pull.id, files_done, bytes_done)
                    .await
                    .map_err(db_error)?
                {
                    // Paused. What is staged stays for the resume.
                    tracing::info!(pull = %pull.id.as_uuid(), files_done, sent_bytes = sent, "a pull paused");
                    return Ok(());
                }
            }
            Err(FetchError::Stalled(why)) => {
                tracing::info!(pull = %pull.id.as_uuid(), files_done, sent_bytes = sent, "a pull's fetch stopped");
                return Err(why);
            }
            Err(FetchError::Refused(why)) => {
                pulls.finish(pull.id, Some(&why)).await.map_err(db_error)?;
                return Ok(());
            }
            Err(FetchError::NotShared) => return stopped_sharing(&pulls, pull).await,
            // A grant taken back mid-pull: the next attempt asks again, and waits or is told it was declined.
            Err(FetchError::NotGranted) => {
                return Err(format!(
                    "{who} has not granted you {} any more.",
                    pull.share_name
                ));
            }
        }
    }
    // What the measurement reads: the bytes this attempt moved, which after a restart is the remainder alone.
    tracing::info!(pull = %pull.id.as_uuid(), files = wanted.len(), sent_bytes = sent, "fetched a pull's files");

    let mut jobs = Vec::new();
    for (index, group) in bundles(&wanted).into_iter().enumerate() {
        let name = format!("pull-{}-{index}.zip", pull.id.as_uuid());
        let manifest = manifest(&pull.share_name, &group);
        let entries: Vec<(String, PathBuf)> = group
            .iter()
            .map(|file| (file.place.clone(), staging.join(file.hash.to_hex())))
            .collect();
        let (path, root) = (staging.join(&name), blob_root.to_path_buf());
        let stored = tokio::task::spawn_blocking(move || {
            let hash = write_bundle(&path, &manifest, &entries)?;
            let stored = SourceWriter::open(&root)
                .put_file(&path, &hash, Compression::for_source_format("zip"))
                .map_err(std::io::Error::other);
            let _ = std::fs::remove_file(&path);
            stored
        })
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| {
            format!("Could not store a bundle of pulled files: {err}. Check the store has room.")
        })?;
        PgBlobs(db.clone())
            .record_unreferenced(&StoredBlobRow {
                hash: stored.hash,
                size_bytes: stored.size_bytes,
                stored_bytes: stored.stored_bytes,
                zstd_level: stored.zstd_level,
            })
            .await
            .map_err(db_error)?;
        jobs.push(JobPayload::ImportBundle {
            blake3: stored.hash,
            path: name,
        });
    }
    let (batch, _) = PgJobs(db.clone())
        .enqueue(pull.library, &jobs)
        .await
        .map_err(db_error)?;
    pulls.importing(pull.id, batch).await.map_err(db_error)?;
    // Only once the batch is recorded: a restart before this fetches nothing again, and bundles once more.
    for path in staged {
        let _ = tokio::fs::remove_file(path).await;
    }
    settle(db, pull, batch, &places).await
}

/// Who shares a pull's share: their name, or their device id's first group when they gave none.
fn sharer_named(pull: &PullRow) -> String {
    pull.sharer.clone().unwrap_or_else(|| {
        let id = pull.device.to_string();
        id.split('-').next().unwrap_or(&id).to_owned()
    })
}

/// A pull whose share is not shared with this installation any more fails, naming it. The sharer's own answer names
/// nothing, so that nobody learns what is shared by asking; this installation knows which share it was pulling.
async fn stopped_sharing(pulls: &PgPulls, pull: &PullRow) -> Result<(), String> {
    let why = format!(
        "{} no longer shares {} with you. Parts already pulled stay.",
        sharer_named(pull),
        pull.share_name
    );
    pulls.finish(pull.id, Some(&why)).await.map_err(db_error)
}

/// Who to fetch one file from: the first holder that answers that it has it (S9).
///
/// One request a holder, in the order they came: the folder's owner while they are answering, then whoever
/// answered a hello most recently. Asking is cheap and a refused fetch is not, which is what `/have` is for.
async fn holder_for<'a>(
    clients: &'a [(Holder, reqwest::Client)],
    ask_owner: Option<usize>,
    file: &Wanted,
    remote: lapidary_core::ShareId,
    owner: lapidary_core::DeviceId,
) -> Option<(&'a Holder, &'a reqwest::Client)> {
    for (index, (holder, client)) in clients.iter().enumerate() {
        if holder.owner && ask_owner != Some(index) {
            continue;
        }
        let request = client
            .get(format!(
                "https://{}/peer/v1/shares/{}/have",
                holder.address,
                remote.as_uuid()
            ))
            .query(&[("blake3", file.hash.to_hex().as_str())]);
        let request = if holder.owner {
            request
        } else {
            request.query(&[("owner", owner.to_string())])
        };
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let body = match response.bytes().await {
                    Ok(body) => body,
                    Err(_) => continue,
                };
                match serde_json::from_slice::<serde_json::Value>(&body) {
                    Ok(answer) if answer["have"] == true => return Some((holder, client)),
                    _ => continue,
                }
            }
            // Busy, refused or unreachable: the next holder is asked, and this one on the next attempt.
            _ => continue,
        }
    }
    None
}

/// Where this installation stands with a share's files.
enum Standing {
    MayFetch,
    Waiting,
    Denied,
    NotShared,
}

/// Ask for a share's files. `Err` when the sharer could not be asked, with why.
async fn ask(client: &reqwest::Client, url: &str) -> Result<Standing, String> {
    let response = client
        .post(url)
        .send()
        .await
        .map_err(|err| format!("Could not reach the sharer: {err}."))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .ok()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
        .unwrap_or_default();
    if status == reqwest::StatusCode::NOT_FOUND || body["reason"] == "notShared" {
        return Ok(Standing::NotShared);
    }
    match body["grant"].as_str() {
        Some("open" | "granted") => Ok(Standing::MayFetch),
        Some("asked") => Ok(Standing::Waiting),
        Some("denied") => Ok(Standing::Denied),
        _ => Err(body["message"].as_str().map_or_else(
            || format!("The sharer answered {status} when asked."),
            str::to_owned,
        )),
    }
}

/// Files in fetch order, grouped into bundles of at most [`BUNDLE_MAX`] bytes and [`BUNDLE_FILES_MAX`] files.
fn bundles(files: &[Wanted]) -> Vec<Vec<&Wanted>> {
    let mut groups: Vec<Vec<&Wanted>> = Vec::new();
    let mut bytes = 0u64;
    for file in files {
        let fits = groups.last().is_some_and(|group| {
            group.len() < BUNDLE_FILES_MAX && bytes.saturating_add(file.size) <= BUNDLE_MAX
        });
        if fits {
            bytes += file.size;
            if let Some(group) = groups.last_mut() {
                group.push(file);
            }
        } else {
            bytes = file.size;
            groups.push(vec![file]);
        }
    }
    groups
}

/// A bundle's manifest: each part at its place here, with the sharer's name, number, tags and licences, and its current
/// file as its one revision.
fn manifest(share_name: &str, files: &[&Wanted]) -> Manifest {
    Manifest {
        format: bundle::FORMAT.to_owned(),
        version: bundle::VERSION,
        library: ManifestLibrary {
            name: share_name.to_owned(),
            mode: "hobby".to_owned(),
        },
        parts: files
            .iter()
            .map(|file| ManifestPart {
                name: file.part.name.clone(),
                part_number: file.part.part_number.clone(),
                source_path: file.place.clone(),
                tags: file.part.tags.clone(),
                sources: file
                    .part
                    .licences
                    .iter()
                    .map(|licence| ManifestSource {
                        url: None,
                        vendor: None,
                        external_id: None,
                        title: None,
                        license: Some(licence.clone()),
                    })
                    .collect(),
                revisions: vec![ManifestRevision {
                    rev_label: "A".to_owned(),
                    parent_label: None,
                    origin: RevisionOrigin::Ingest.as_str().to_owned(),
                    // The catalogue carries no date, and import reads none.
                    created_at: String::new(),
                    blake3: file.hash.to_hex(),
                    size_bytes: file.size,
                    format: file.part.format.clone().unwrap_or_default(),
                    path: file.place.clone(),
                }],
            })
            .collect(),
    }
}

/// Write a bundle of staged files to `path`, and answer its hash.
fn write_bundle(
    path: &Path,
    manifest: &Manifest,
    entries: &[(String, PathBuf)],
) -> std::io::Result<BlobHash> {
    let mut zip = StoreZip::new(std::io::BufWriter::new(std::fs::File::create(path)?));
    let text = serde_json::to_vec(manifest).map_err(std::io::Error::other)?;
    zip.add(bundle::MANIFEST, &mut text.as_slice())?;
    for (entry, staged) in entries {
        zip.add(entry, &mut std::fs::File::open(staged)?)?;
    }
    zip.finish()?;
    hash_file(path)
}

/// Wait for the import batch to settle, then name the sharer on what landed and finish.
async fn settle(
    db: &PgPool,
    pull: &PullRow,
    batch: lapidary_core::BatchId,
    places: &[String],
) -> Result<(), String> {
    loop {
        match PgJobs(db.clone())
            .batch_status(pull.library, batch)
            .await
            .map_err(db_error)?
        {
            Some(status) if status.pending + status.running > 0 => {
                tokio::time::sleep(SETTLED_POLL).await;
            }
            _ => break,
        }
    }
    settled(db, pull, places).await
}

async fn settled(db: &PgPool, pull: &PullRow, places: &[String]) -> Result<(), String> {
    let pulls = PgPulls(db.clone());
    pulls
        .record_provenance(pull.library, pull.device, pull.sharer.as_deref(), places)
        .await
        .map_err(db_error)?;
    pulls.finish(pull.id, None).await.map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lapidary_core::DeviceId;

    fn device(seed: &[u8]) -> DeviceId {
        DeviceId::from_public_key(seed)
    }

    #[test]
    fn a_name_with_a_slash_is_one_folder() {
        let ayse = device(b"ed25519 public key of the workshop pc in Ayse's garage");
        let group = ayse.to_string()[..5].to_owned();
        assert_eq!(
            shared_path(Some("AC/DC\\Tribute"), ayse, "Terrain/Rocks/cliff-face.stl"),
            format!("Shared/AC-DC-Tribute ({group})/Terrain/Rocks/cliff-face.stl")
        );
        assert_eq!(
            shared_path(None, ayse, "cliff-face.stl"),
            format!("Shared/{group}/cliff-face.stl")
        );
    }

    #[test]
    fn two_sharers_with_one_name_do_not_share_a_folder() {
        let workshop = device(b"ed25519 public key of the workshop pc in Ayse's garage");
        let laptop = device(b"ed25519 public key of Ayse's laptop");
        assert_ne!(
            shared_path(Some("Ayşe's workshop"), workshop, "Terrain/cliff-face.stl"),
            shared_path(Some("Ayşe's workshop"), laptop, "Terrain/cliff-face.stl")
        );
    }
}
