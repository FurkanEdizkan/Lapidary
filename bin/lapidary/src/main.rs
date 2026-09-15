//! `lapidary`, the desktop binary.
//!
//! `checkout`, `checkin` and `agent` are Phase 4 slice 1's round trip
//! (`docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md` §6): hand a part's
//! file out under a lock, watch it, and send each save back as a new revision. `open`,
//! `register` and `unregister` are slice 2's (`2026-09-15-phase-4-slice-2-design.md` §3): a
//! `lapidary://` link from a part's page, opened in the desktop's app for that file. Linux only
//! for now. `worker` and `up` are still to come.

mod checkout;
mod desktop;
mod link;
mod watch;

use anyhow::{Context, Result, anyhow, bail};
use checkout::Checkout;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use watch::{Seen, Verdict, Watch};

#[derive(Parser)]
#[command(
    name = "lapidary",
    version,
    about = "Lapidary — a visual index for 3D part libraries"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check a part out: take its lock and put its file in the workspace.
    Checkout {
        /// The part's id, from its page in Lapidary.
        part: String,
    },
    /// Check a part back in: hand its lock back. The folder and its files stay.
    Checkin {
        /// The checkout's folder in the workspace.
        folder: PathBuf,
    },
    /// Open a `lapidary://` link: this computer's checkout of the part, or a new one, in the app
    /// the desktop opens that kind of file with.
    Open {
        /// The link, `lapidary://open?part=<part id>`.
        link: String,
    },
    /// Make `lapidary open` the handler for `lapidary://` links on this desktop, with today's
    /// LAPIDARY_SERVER and LAPIDARY_WORKSPACE.
    Register,
    /// Remove what `lapidary register` set up.
    Unregister,
    /// Watch every checkout in the workspace, and send each save back as a new revision.
    Agent,
    /// Run a job worker against a Lapidary server.
    Worker,
    /// Start a local Lapidary stack.
    Up,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Checkout { part } => checkout(&part).await,
        Commands::Checkin { folder } => checkin(&folder).await,
        Commands::Open { link } => {
            let opened = open(&link).await;
            if let Err(error) = &opened {
                notify(&format!("{error:#}"));
            }
            opened
        }
        Commands::Register => desktop::register(&server(), &checkout::workspace()?),
        Commands::Unregister => desktop::unregister(),
        Commands::Agent => agent().await,
        Commands::Worker => later("worker"),
        Commands::Up => later("up"),
    }
}

fn later(name: &str) -> Result<()> {
    bail!(
        "`lapidary {name}` arrives in a later slice. Until then, run the stack with `podman compose -f deploy/compose.yaml up`."
    )
}

/// `$LAPIDARY_SERVER`, else the api on this machine.
fn server() -> String {
    std::env::var("LAPIDARY_SERVER")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

/// `$USER@$HOSTNAME`, as free text: there are no users yet (spec §5), so this names a person
/// for other people to read, and proves nothing.
fn holder() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "someone".to_owned());
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|host| host.trim().to_owned())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "this-computer".to_owned());
    format!("{user}@{host}")
}

// ---------------------------------------------------------------------------------------
// The wire, as far as this binary reads it. Local and minimal on purpose: linking
// `lapidary-api` for its types would link axum and sqlx into a desktop tool.
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct CheckedOut {
    lock: Lock,
}

#[derive(Deserialize)]
struct Lock {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Detail {
    library: String,
    revision: String,
    rev_label: String,
    name: String,
    part_number: Option<String>,
    source_path: String,
    source_hash: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    have: Vec<String>,
    need_bytes: Vec<String>,
}

#[derive(Deserialize)]
struct Chunk {
    received: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Accepted {
    batch_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Batch {
    finished_at: Option<String>,
    revised: u32,
    failed: Vec<Failure>,
}

#[derive(Deserialize)]
struct Failure {
    reason: String,
}

fn json(request: reqwest::RequestBuilder, body: &serde_json::Value) -> reqwest::RequestBuilder {
    request
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
}

async fn send(request: reqwest::RequestBuilder, what: &str) -> Result<reqwest::Response> {
    request.send().await.with_context(|| {
        format!(
            "{what}: could not reach the Lapidary server at {}; check LAPIDARY_SERVER and that the api is running",
            server()
        )
    })
}

/// A refusal in the server's own words: every Lapidary refusal carries a `message` saying
/// what broke and what to do.
fn refusal(status: reqwest::StatusCode, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("the server answered {status}"))
}

async fn read<T: DeserializeOwned>(response: reqwest::Response, what: &str) -> Result<T> {
    let status = response.status();
    let body = response
        .bytes()
        .await
        .with_context(|| format!("{what}: the server's answer was cut off"))?;
    if !status.is_success() {
        bail!("{what}: {}", refusal(status, &body));
    }
    serde_json::from_slice(&body).with_context(|| {
        format!(
            "{what}: the server answered {status} with something this agent does not expect; check that both run the same Lapidary version"
        )
    })
}

// ---------------------------------------------------------------------------------------
// checkout, checkin
// ---------------------------------------------------------------------------------------

async fn checkout(part: &str) -> Result<()> {
    let part: lapidary_core::PartId = part.parse().map_err(|error| anyhow!("{error}"))?;
    let (folder, checkout) = take(part).await?;
    println!(
        "Checked out {} (revision {}) to {}.\nEdit it there: `lapidary agent` sends each save back as a new revision, and `lapidary checkin {}` hands the lock back.",
        checkout.file_name,
        checkout.rev_label,
        folder.display(),
        folder.display()
    );
    Ok(())
}

/// Take the part's lock and hand its file out. A checkout that does not finish hands the lock
/// back, so a failure leaves the part as it was.
async fn take(part: lapidary_core::PartId) -> Result<(PathBuf, Checkout)> {
    let (server, holder, workspace) = (server(), holder(), checkout::workspace()?);
    let client = reqwest::Client::new();

    let what = "checking the part out";
    let taken: CheckedOut = read(
        send(
            json(
                client.post(format!("{server}/api/parts/{part}/checkout")),
                &serde_json::json!({ "holder": holder }),
            ),
            what,
        )
        .await?,
        what,
    )
    .await?;

    // From here the part is locked, so anything that stops the checkout hands the lock back.
    match hand_out(&client, &server, part, &holder, &workspace, &taken.lock.id).await {
        Ok(done) => Ok(done),
        Err(error) => {
            let _ = send(
                json(
                    client.post(format!("{server}/api/parts/{part}/checkin")),
                    &serde_json::json!({ "lock": taken.lock.id }),
                ),
                "handing the lock back",
            )
            .await;
            Err(error.context("the checkout did not finish, so its lock was handed back"))
        }
    }
}

async fn hand_out(
    client: &reqwest::Client,
    server: &str,
    part: lapidary_core::PartId,
    holder: &str,
    workspace: &Path,
    lock: &str,
) -> Result<(PathBuf, Checkout)> {
    // Read after the lock is taken, so the revision named here is the one the lock protects.
    let what = "reading the part";
    let detail: Detail = read(
        send(client.get(format!("{server}/api/parts/{part}")), what).await?,
        what,
    )
    .await?;
    let folder = workspace.join(checkout::folder_name(
        detail.part_number.as_deref(),
        &detail.name,
        &detail.rev_label,
    ));
    if folder.exists() {
        bail!(
            "{} already exists. Check that checkout in (`lapidary checkin {}`) or move the folder aside, then check out again.",
            folder.display(),
            folder.display()
        );
    }
    let file_name = detail
        .source_path
        .rsplit('/')
        .next()
        .unwrap_or(&detail.source_path)
        .to_owned();

    let what = "downloading the file";
    let response = send(
        client.get(format!(
            "{server}/api/revisions/{}/download?variant=original",
            detail.revision
        )),
        what,
    )
    .await?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("{what}: the download was cut off"))?;
    if !status.is_success() {
        bail!("{what}: {}", refusal(status, &bytes));
    }
    // Hash before believing anything (DATA §6.2): these must be the bytes the revision holds.
    let blake3 = blake3::hash(&bytes).to_hex().to_string();
    if detail
        .source_hash
        .as_deref()
        .is_some_and(|known| known != blake3)
    {
        bail!(
            "{what}: the bytes that arrived are not the ones revision {} holds; check out again",
            detail.rev_label
        );
    }

    std::fs::create_dir_all(&folder)
        .with_context(|| format!("could not create {}", folder.display()))?;
    std::fs::write(folder.join(&file_name), &bytes)
        .with_context(|| format!("could not write {}", folder.join(&file_name).display()))?;
    let checkout = Checkout {
        server: server.to_owned(),
        library: detail.library,
        part: part.to_string(),
        source_path: detail.source_path,
        lock: lock.to_owned(),
        holder: holder.to_owned(),
        revision: detail.revision,
        rev_label: detail.rev_label,
        file_name,
        blake3,
    };
    checkout.write(&folder)?;
    Ok((folder, checkout))
}

async fn checkin(folder: &Path) -> Result<()> {
    let checkout = Checkout::read(folder)?;
    let response = send(
        json(
            reqwest::Client::new().post(format!(
                "{}/api/parts/{}/checkin",
                checkout.server, checkout.part
            )),
            &serde_json::json!({ "lock": checkout.lock }),
        ),
        "checking the part in",
    )
    .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.bytes().await.unwrap_or_default();
        // Somebody released it already: the lock is gone either way, so this checkout is over
        // here too. Anything else leaves the checkout as it was.
        if status != reqwest::StatusCode::CONFLICT {
            bail!("checking the part in: {}", refusal(status, &body));
        }
        println!("{}", refusal(status, &body));
    }
    std::fs::rename(
        folder.join(checkout::FILE),
        folder.join(checkout::CHECKED_IN),
    )
    .with_context(|| format!("could not mark {} checked in", folder.display()))?;
    println!(
        "Checked in. {} keeps its files; the agent no longer watches it.",
        folder.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------
// open
// ---------------------------------------------------------------------------------------

/// A `lapidary://` link from a part's page: this computer's checkout of the part, or a new one,
/// opened in whatever app the desktop opens that kind of file with.
async fn open(link: &str) -> Result<()> {
    let part = link::part(link).map_err(anyhow::Error::msg)?;
    let (server, workspace) = (server(), checkout::workspace()?);
    let (folder, checkout) = match checkout::find(&workspace, &server, &part.to_string()) {
        Some(found) => found,
        None => take(part).await?,
    };
    let file = folder.join(&checkout.file_name);
    // One argument, never a shell: the folder is ours, but a file name is still somebody's text.
    let status = Command::new("xdg-open")
        .arg(&file)
        .status()
        .with_context(|| {
            format!(
                "could not run xdg-open for {}; install xdg-utils, or open the file yourself",
                file.display()
            )
        })?;
    if !status.success() {
        bail!(
            "xdg-open could not open {} ({status}). Choose an app for this kind of file in your desktop's settings, or open it yourself.",
            file.display()
        );
    }
    println!(
        "Opened {} (revision {}). `lapidary agent` sends each save back while it runs.",
        file.display(),
        checkout.rev_label
    );
    Ok(())
}

/// A link is opened by a handler with no terminal, so a refusal is also shown as a desktop
/// notification where `notify-send` exists. A convenience only: nothing waits on it.
fn notify(text: &str) {
    // Quiet: without a session bus notify-send says so on stderr, under the refusal that matters.
    let _ = Command::new("notify-send")
        .arg("Lapidary")
        .arg(text)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

// ---------------------------------------------------------------------------------------
// agent
// ---------------------------------------------------------------------------------------

async fn agent() -> Result<()> {
    let workspace = checkout::workspace()?;
    std::fs::create_dir_all(&workspace)
        .with_context(|| format!("could not create {}", workspace.display()))?;
    println!(
        "Watching the checkouts in {}. Stop with Ctrl-C.",
        workspace.display()
    );
    let client = reqwest::Client::new();
    let mut watching: HashMap<PathBuf, (Checkout, Watch)> = HashMap::new();
    let mut unreadable: HashSet<PathBuf> = HashSet::new();

    loop {
        // The workspace is read again every round: a checkout made while the agent runs is
        // picked up, and a checked-in one is dropped.
        let folders = checkout::folders(&workspace);
        watching.retain(|folder, _| folders.contains(folder));
        for folder in folders {
            if watching.contains_key(&folder) || unreadable.contains(&folder) {
                continue;
            }
            match Checkout::read(&folder) {
                Ok(checkout) => {
                    let path = folder.join(&checkout.file_name);
                    println!("Watching {}", path.display());
                    let watch = Watch::new(look(&path));
                    watching.insert(folder, (checkout, watch));
                }
                Err(error) => {
                    eprintln!("{error:#}");
                    unreadable.insert(folder);
                }
            }
        }

        for (folder, (checkout, watch)) in &mut watching {
            let path = folder.join(&checkout.file_name);
            if watch.poll(Instant::now(), look(&path)) != Verdict::Hash {
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("could not read {}: {error}", path.display());
                    continue;
                }
            };
            let Some(blake3) = watch::changed(&checkout.blake3, &bytes) else {
                continue;
            };
            println!("{} was saved; sending it back.", path.display());
            match send_back(&client, checkout, &bytes, &blake3).await {
                Ok(Some((revision, rev_label))) => {
                    println!("Kept as revision {rev_label}.");
                    checkout.revision = revision;
                    checkout.rev_label = rev_label;
                    checkout.blake3 = blake3;
                    if let Err(error) = checkout.write(folder) {
                        eprintln!("{error:#}");
                    }
                }
                Ok(None) => println!(
                    "The part's current revision already holds these bytes; nothing new was kept."
                ),
                Err(error) => eprintln!("Not kept: {error:#}"),
            }
        }

        tokio::time::sleep(watch::POLL).await;
    }
}

fn look(path: &Path) -> Option<Seen> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Seen {
        size: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

/// One save, sent back: the probe, the bytes if the server needs them, the commit under the
/// checkout's lock, and the batch followed to its end. `Some` names the revision kept.
async fn send_back(
    client: &reqwest::Client,
    checkout: &Checkout,
    bytes: &[u8],
    blake3: &str,
) -> Result<Option<(String, String)>> {
    let (server, library) = (&checkout.server, &checkout.library);

    let what = "asking what the server needs";
    let plan: Plan = read(
        send(
            json(
                client.post(format!("{server}/api/libraries/{library}/uploads/probe")),
                &serde_json::json!({ "files": [{ "path": checkout.source_path, "blake3": blake3 }] }),
            ),
            what,
        )
        .await?,
        what,
    )
    .await?;
    if plan.have.contains(&checkout.source_path) {
        return Ok(None);
    }
    if plan.need_bytes.contains(&checkout.source_path) {
        upload(client, checkout, bytes, blake3).await?;
    }

    let what = "committing the save";
    let accepted: Accepted = read(
        send(
            json(
                client.post(format!("{server}/api/libraries/{library}/uploads/commit")),
                &serde_json::json!({ "files": [{
                    "path": checkout.source_path,
                    "blake3": blake3,
                    "lock": checkout.lock,
                }] }),
            ),
            what,
        )
        .await?,
        what,
    )
    .await?;

    let what = "following the save";
    let batch = loop {
        let batch: Batch = read(
            send(
                client.get(format!(
                    "{server}/api/libraries/{library}/jobs/{}",
                    accepted.batch_id
                )),
                what,
            )
            .await?,
            what,
        )
        .await?;
        if batch.finished_at.is_some() {
            break batch;
        }
        tokio::time::sleep(watch::POLL).await;
    };
    // The worker's refusal, verbatim: a released lock names who released it.
    if let Some(failure) = batch.failed.first() {
        bail!("{}", failure.reason);
    }
    if batch.revised == 0 {
        return Ok(None);
    }

    let what = "reading the new revision";
    let detail: Detail = read(
        send(
            client.get(format!("{server}/api/parts/{}", checkout.part)),
            what,
        )
        .await?,
        what,
    )
    .await?;
    Ok(Some((detail.revision, detail.rev_label)))
}

/// The bytes, in chunks the size the web client sends, under the server's 16 MiB limit.
async fn upload(
    client: &reqwest::Client,
    checkout: &Checkout,
    bytes: &[u8],
    blake3: &str,
) -> Result<()> {
    const CHUNK_BYTES: usize = 8 * 1024 * 1024;
    let url = format!(
        "{}/api/libraries/{}/uploads/{blake3}",
        checkout.server, checkout.library
    );
    let what = "sending the bytes";
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + CHUNK_BYTES).min(bytes.len());
        let response = send(
            client
                .put(format!("{url}?offset={offset}"))
                .body(bytes[offset..end].to_vec()),
            what,
        )
        .await?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .with_context(|| format!("{what}: the answer was cut off"))?;
        // A 409 is the server saying where it actually is, in the field a success uses.
        if !status.is_success() && status != reqwest::StatusCode::CONFLICT {
            bail!("{what}: {}", refusal(status, &body));
        }
        let chunk: Chunk = serde_json::from_slice(&body)
            .with_context(|| format!("{what}: the server's answer was not a byte count"))?;
        let received = usize::try_from(chunk.received).with_context(|| {
            format!("{what}: the server holds more bytes than this computer can count")
        })?;
        if received == offset {
            bail!("{what}: the server accepted nothing at byte {offset}; save the file again");
        }
        offset = received;
    }
    Ok(())
}
