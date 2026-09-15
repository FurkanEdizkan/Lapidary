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
mod folder;
mod link;
mod watch;

use anyhow::{Context, Result, anyhow, bail};
use checkout::Checkout;
use clap::{Parser, Subcommand};
use lapidary_targets::{Format, Handover};
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
    /// Watch a folder, and upload every model file in it into a library as it settles. A file deleted
    /// here changes nothing in the library.
    Watch {
        /// The folder to watch, and every folder under it.
        folder: PathBuf,
        /// The library to upload into: its id, from its page in Lapidary.
        #[arg(long)]
        library: String,
    },
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
        Commands::Watch { folder, library } => folder::watch(&folder, &library).await,
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
    /// The part's active check-out, if anybody holds one.
    lock: Option<Lock>,
    revision: String,
    rev_label: String,
    name: String,
    part_number: Option<String>,
    source_path: String,
    source_hash: Option<String>,
    source_format: Option<String>,
}

/// A file Lapidary builds when asked, already built.
#[derive(Deserialize)]
struct Ready {
    hash: String,
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
    #[serde(default)]
    ingested: u32,
    #[serde(default)]
    skipped: u32,
    revised: u32,
    #[serde(default)]
    unkept: u32,
    /// Every failure in the batch; `failed` lists the first 100.
    #[serde(default)]
    failed_total: u32,
    failed: Vec<Failure>,
}

#[derive(Deserialize)]
struct Failure {
    #[serde(default)]
    path: String,
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
/// opened in the app this desktop has for its format.
async fn open(link: &str) -> Result<()> {
    let part = link::part(link).map_err(anyhow::Error::msg)?;
    let (server, workspace) = (server(), checkout::workspace()?);
    let (folder, checkout) = match checkout::find(&workspace, &server, &part.to_string()) {
        // Reused only while its lock is still the part's: a released or broken lock would have
        // every save from this folder refused.
        Some((folder, checkout)) => {
            let what = "reading the part";
            let detail: Detail = read(
                send(
                    reqwest::Client::new().get(format!("{server}/api/parts/{part}")),
                    what,
                )
                .await?,
                what,
            )
            .await?;
            if detail.lock.as_ref().map(|lock| lock.id.as_str()) != Some(checkout.lock.as_str()) {
                bail!(
                    "{} is a check-out of this part whose lock was released, so a save there would be refused. Check that folder in (`lapidary checkin {}`), then open the link again.",
                    folder.display(),
                    folder.display()
                );
            }
            (folder, checkout)
        }
        None => {
            let what = "reading the part";
            let detail: Detail = read(
                send(
                    reqwest::Client::new().get(format!("{server}/api/parts/{part}")),
                    what,
                )
                .await?,
                what,
            )
            .await?;
            // Negotiated only before a check-out exists: one taken already is the part's own file.
            if let Some(source) = detail.source_format.as_deref().and_then(Format::named)
                && let Handover::Export(export) =
                    desktop::handover(source, |format| desktop::default_app(format).is_some())
            {
                return open_export(&server, &workspace, part, &detail, source, export).await;
            }
            take(part).await?
        }
    };
    let file = folder.join(&checkout.file_name);
    let app = Path::new(&checkout.file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(Format::named)
        .and_then(desktop::default_app);
    open_in(app.as_deref(), &file)?;
    println!(
        "Opened {} (revision {}). `lapidary agent` sends each save back while it runs.",
        file.display(),
        checkout.rev_label
    );
    Ok(())
}

/// `file` in `app`, the desktop file `open` found for its format, else in whatever `xdg-open` picks.
///
/// The app by name, because `xdg-open` picks by the type it detects in the file, and a desktop's MIME
/// database may know none: shared-mime-info 2.4 has no STEP type, so a `.step` file reads as
/// `text/plain` and would open in a text editor, whichever app declared STEP.
fn open_in(app: Option<&str>, file: &Path) -> Result<()> {
    if let Some(entry) = app.and_then(desktop::entry_path)
        && Command::new("gio")
            .arg("launch")
            .arg(&entry)
            .arg(file)
            .status()
            .is_ok_and(|status| status.success())
    {
        return Ok(());
    }
    // No such app, or no `gio` to start it with: xdg-open may still open the file, and says what is
    // missing when it cannot.
    xdg_open(file)
}

/// `file` in the app this desktop opens its kind of file with.
fn xdg_open(file: &Path) -> Result<()> {
    // One argument, never a shell: the folder is ours, but a file name is still somebody's text.
    let status = Command::new("xdg-open")
        .arg(file)
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
    Ok(())
}

/// A part no app here opens, handed out as the export one does (`DATA.md` §5.1): written when it
/// is not yet, checked against the hash the server wrote it under, and saved read-only to
/// `exports/` in the workspace. Not a check-out: no lock is taken, and nothing saved from it
/// comes back.
async fn open_export(
    server: &str,
    workspace: &Path,
    part: lapidary_core::PartId,
    detail: &Detail,
    source: Format,
    export: Format,
) -> Result<()> {
    let client = reqwest::Client::new();
    let what = "writing the export";
    let mut written = None;
    // A second ask finds the export the first one's batch wrote.
    for _ in 0..2 {
        let response = send(
            client.post(format!(
                "{server}/api/parts/{part}/exports/{}",
                export.name()
            )),
            what,
        )
        .await?;
        if response.status() != reqwest::StatusCode::ACCEPTED {
            written = Some(read::<Ready>(response, what).await?.hash);
            break;
        }
        let accepted: Accepted = read(response, what).await?;
        let batch = follow(&client, server, &detail.library, &accepted.batch_id).await?;
        if let Some(failure) = batch.failed.first() {
            bail!("{what}: {}", failure.reason);
        }
    }
    let hash = written.with_context(|| {
        format!("{what}: its batch finished without writing one; open the link again")
    })?;

    let what = "downloading the export";
    let response = send(
        client.get(format!(
            "{server}/api/revisions/{}/download?variant={}",
            detail.revision,
            export.name()
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
    // Hash before believing anything (DATA §6.2): these must be the bytes the server wrote.
    if blake3::hash(&bytes).to_hex().as_str() != hash {
        bail!(
            "{what}: the bytes that arrived are not the export the server wrote; open the link again"
        );
    }

    let exports = workspace.join("exports");
    std::fs::create_dir_all(&exports)
        .with_context(|| format!("could not create {}", exports.display()))?;
    let file = exports.join(format!(
        "{}.lapidary.{}",
        checkout::folder_name(
            detail.part_number.as_deref(),
            &detail.name,
            &detail.rev_label
        ),
        export.name()
    ));
    // Renamed over rather than written in place, since one opened before is read-only.
    let mut partial = file.clone().into_os_string();
    partial.push(".part");
    std::fs::write(&partial, &bytes)
        .with_context(|| format!("could not write {}", file.display()))?;
    std::fs::rename(&partial, &file)
        .with_context(|| format!("could not write {}", file.display()))?;
    let mut permissions = std::fs::metadata(&file)
        .with_context(|| format!("could not read {}", file.display()))?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions)
        .with_context(|| format!("could not make {} read-only", file.display()))?;

    open_in(desktop::default_app(export).as_deref(), &file)?;
    let (from, to) = (source.name().to_uppercase(), export.name().to_uppercase());
    let note = format!(
        "No app on this computer opens {from} files, so revision {} opened as a {to} Lapidary wrote from its mesh, read-only: nothing saved from it comes back. To edit the part itself, make a CAD app the default for {from} files (`xdg-mime default <its .desktop file> {}`) and open the link again.",
        detail.rev_label,
        source.mimes()[0]
    );
    println!("{note} The file is {}.", file.display());
    notify(&note);
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
    let server = &checkout.server;
    let sent = send_files(
        client,
        server,
        &checkout.library,
        &[Outgoing {
            path: &checkout.source_path,
            bytes,
            blake3,
        }],
        Some(checkout.lock.as_str()),
    )
    .await?;
    // No batch: the part's current revision already holds these bytes.
    let Some(batch) = sent.batch else {
        return Ok(None);
    };
    let batch = follow(client, server, &checkout.library, &batch).await?;
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

/// A file on its way to a library: the path it is filed under, its bytes and their BLAKE3.
struct Outgoing<'a> {
    path: &'a str,
    bytes: &'a [u8],
    blake3: &'a str,
}

/// What a library did with files sent: the paths it already held as they are, and the id of the batch
/// taking the rest. No batch when there was nothing left to take.
struct Sent {
    have: Vec<String>,
    batch: Option<String>,
}

/// Files sent into a library the way the browser sends a drop: the probe, the bytes the server needs, and
/// one commit, under `lock` when a checkout sends them. Following the batch is the caller's: [`follow`] to
/// its end, or [`batch_status`] a round at a time.
async fn send_files(
    client: &reqwest::Client,
    server: &str,
    library: &str,
    files: &[Outgoing<'_>],
    lock: Option<&str>,
) -> Result<Sent> {
    let what = "asking what the server needs";
    let manifest: Vec<serde_json::Value> = files
        .iter()
        .map(|file| serde_json::json!({ "path": file.path, "blake3": file.blake3 }))
        .collect();
    let plan: Plan = read(
        send(
            json(
                client.post(format!("{server}/api/libraries/{library}/uploads/probe")),
                &serde_json::json!({ "files": manifest }),
            ),
            what,
        )
        .await?,
        what,
    )
    .await?;
    for file in files {
        if plan.need_bytes.iter().any(|path| path == file.path) {
            upload(client, server, library, file.bytes, file.blake3).await?;
        }
    }
    let committed: Vec<serde_json::Value> = files
        .iter()
        .filter(|file| !plan.have.iter().any(|path| path == file.path))
        .map(|file| serde_json::json!({ "path": file.path, "blake3": file.blake3, "lock": lock }))
        .collect();
    if committed.is_empty() {
        return Ok(Sent {
            have: plan.have,
            batch: None,
        });
    }

    let what = "committing the files";
    let accepted: Accepted = read(
        send(
            json(
                client.post(format!("{server}/api/libraries/{library}/uploads/commit")),
                &serde_json::json!({ "files": committed }),
            ),
            what,
        )
        .await?,
        what,
    )
    .await?;

    Ok(Sent {
        have: plan.have,
        batch: Some(accepted.batch_id),
    })
}

/// Where a batch has got to, asked once.
async fn batch_status(
    client: &reqwest::Client,
    server: &str,
    library: &str,
    batch: &str,
) -> Result<Batch> {
    let what = "following the batch";
    read(
        send(
            client.get(format!("{server}/api/libraries/{library}/jobs/{batch}")),
            what,
        )
        .await?,
        what,
    )
    .await
}

/// A batch followed to its end.
async fn follow(
    client: &reqwest::Client,
    server: &str,
    library: &str,
    batch: &str,
) -> Result<Batch> {
    loop {
        let status = batch_status(client, server, library, batch).await?;
        if status.finished_at.is_some() {
            return Ok(status);
        }
        tokio::time::sleep(watch::POLL).await;
    }
}

/// The bytes, in chunks the size the web client sends, under the server's 16 MiB limit.
async fn upload(
    client: &reqwest::Client,
    server: &str,
    library: &str,
    bytes: &[u8],
    blake3: &str,
) -> Result<()> {
    const CHUNK_BYTES: usize = 8 * 1024 * 1024;
    let url = format!("{server}/api/libraries/{library}/uploads/{blake3}");
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
