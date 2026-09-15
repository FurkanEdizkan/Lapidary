//! `lapidary watch <folder> --library <id>`: every model file under a folder, uploaded into a library as
//! it settles (the local product spec §5).
//!
//! Polled, like the checkout agent, on Linux like the rest of it. Every file under the folder is watched
//! here, so `docs/DATA.md` §6.2's ignore list applies whole. A settled change is hashed before anything is
//! believed, and sent the way the browser sends a drop, so the server decides what it is: ingested,
//! skipped, revised or unkept. A file deleted here changes nothing in the library.

use crate::watch::{Seen, Verdict, Watch};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// How often the folder is listed (spec §5.2).
///
/// ponytail: one listing and a `stat` per file every interval. Over the STL corpus's tree, 2,778 files in
/// 546 directories, that took 12 ms warm and 109 ms cold (2026-09-15). `notify` (inotify), which
/// ARCHITECTURE names, replaces the poll once a tree is large enough for that to matter.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// How long a file the library refused waits before it is sent again, unless it changes first. A part
/// checked out to somebody refuses every upload until it is checked in, and each try fails a job.
pub const RETRY: Duration = Duration::from_secs(5 * 60);

/// The most bytes one upload holds in memory. Files that settle together go up together (spec §5.3),
/// in as many uploads as keep each under this; a file larger than it goes up by itself.
const BATCH_BYTES: u64 = 64 * 1024 * 1024;

/// `docs/DATA.md` §6.2's ignore list, whole: hidden files and folders (`.DS_Store` among them), an
/// office lock file, and the backups, temporaries, locks and autosaves editors leave beside a model.
pub fn ignored(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || lower == "thumbs.db"
        || [".bak", ".tmp", ".lck", ".autosave"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

/// A file's source path in the library: its path under the folder, with `/` between the parts. `None`
/// for anything not plainly under it.
pub fn source_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// What was last sent for one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    pub size: u64,
    pub modified_ms: u64,
    pub blake3: String,
}

/// A look's modification time, in milliseconds since the epoch, as the state file keeps it.
pub fn stamp(seen: &Seen) -> u64 {
    seen.modified
        .duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// One round's decisions: the files to hash now, and the files gone since they were last sent.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Round {
    pub hash: Vec<String>,
    pub deleted: Vec<String>,
}

/// The round, decided from this listing, what was last sent, each file's watch, and when each refused
/// file was refused.
///
/// A file whose size and modification time are what was last sent starts as seen, so a restart sends
/// nothing for having noticed it. Anything else starts unseen, and is hashed once it has settled: a new
/// file, one changed while the watch was stopped, and one refused [`RETRY`] ago.
pub fn plan(
    known: &BTreeMap<String, Known>,
    listing: &BTreeMap<String, Seen>,
    watches: &mut HashMap<String, Watch>,
    refused: &mut HashMap<String, Instant>,
    now: Instant,
) -> Round {
    let mut round = Round::default();
    refused.retain(|path, at| {
        let waiting = listing.contains_key(path) && now.duration_since(*at) < RETRY;
        if !waiting {
            watches.remove(path);
        }
        waiting
    });
    watches.retain(|path, _| listing.contains_key(path));
    for (path, seen) in listing {
        let watch = watches.entry(path.clone()).or_insert_with(|| {
            let unchanged = known
                .get(path)
                .is_some_and(|sent| sent.size == seen.size && sent.modified_ms == stamp(seen));
            Watch::new(unchanged.then_some(*seen))
        });
        if watch.poll(now, Some(*seen)) == Verdict::Hash {
            round.hash.push(path.clone());
        }
    }
    round.deleted = known
        .keys()
        .filter(|path| !listing.contains_key(*path))
        .cloned()
        .collect();
    round
}

/// The paths committed that the library did not keep, by the batch's failures. All of them when the
/// failures cannot all be matched to a path: more than the batch lists, or one naming a path not sent.
fn refusals(committed: &[String], batch: Option<&crate::Batch>) -> Vec<String> {
    let Some(batch) = batch else {
        return Vec::new();
    };
    let listed = usize::try_from(batch.failed_total).unwrap_or(usize::MAX);
    let matched = batch
        .failed
        .iter()
        .all(|failure| committed.contains(&failure.path));
    if listed > batch.failed.len() || !matched {
        return committed.to_vec();
    }
    committed
        .iter()
        .filter(|path| batch.failed.iter().any(|failure| &failure.path == *path))
        .cloned()
        .collect()
}

/// An accepted upload's files, settled: each one kept is recorded as sent, and each one refused waits
/// [`RETRY`] to be sent again, recorded as nothing, so a restart sends it too.
fn settle(
    known: &mut BTreeMap<String, Known>,
    refused: &mut HashMap<String, Instant>,
    sent: Vec<(String, Known)>,
    refusals: &[String],
    now: Instant,
) {
    for (path, last) in sent {
        if refusals.contains(&path) {
            refused.insert(path, now);
        } else {
            refused.remove(&path);
            known.insert(path, last);
        }
    }
}

/// The files to hash, in uploads of at most [`BATCH_BYTES`] by their listed sizes, in order.
fn batches(paths: Vec<String>, listing: &BTreeMap<String, Seen>) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut bytes = 0;
    for path in paths {
        let size = listing.get(&path).map_or(0, |seen| seen.size);
        match groups.last_mut() {
            Some(group) if bytes + size <= BATCH_BYTES => group.push(path),
            _ => {
                groups.push(vec![path]);
                bytes = 0;
            }
        }
        bytes += size;
    }
    groups
}

/// Every model file under the folder that the ignore list lets through, by source path. Symlinks are not
/// followed, and an entry that cannot be read is left out of this round rather than stopping it.
fn list(root: &Path) -> BTreeMap<String, Seen> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if ignored(name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            if !kind.is_file() || !lapidary_core::is_model_file(name) {
                continue;
            }
            let (Ok(metadata), Some(source)) = (entry.metadata(), source_path(root, &path)) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            found.insert(
                source,
                Seen {
                    size: metadata.len(),
                    modified,
                },
            );
        }
    }
    found
}

/// `$XDG_STATE_HOME/lapidary/<state_name>`, falling back to `~/.local/state`. Nothing is ever written
/// inside the watched folder.
fn state_path(library: &str, root: &Path) -> Result<PathBuf> {
    Ok(crate::desktop::xdg_dir("XDG_STATE_HOME", ".local/state")?
        .join("lapidary")
        .join(state_name(library, root)))
}

/// `watch-<library>-<folder>.json`, the folder as the first 16 hex digits of its canonical path's hash.
/// One state per folder: two watches into one library would otherwise each read the other's files as
/// deleted here, and whichever saved last would leave the other to send everything again.
pub fn state_name(library: &str, root: &Path) -> String {
    let folder = blake3::hash(root.as_os_str().as_encoded_bytes()).to_hex();
    format!("watch-{library}-{}.json", &folder[..16])
}

pub async fn watch(folder: &Path, library: &str) -> Result<()> {
    if library.is_empty() || !library.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        bail!(
            "`{library}` is not a library id. Copy the id from the library's page in Lapidary, then run `lapidary watch` again."
        );
    }
    let root = folder.canonicalize().with_context(|| {
        format!(
            "could not read {}; check that the folder exists and that you can open it",
            folder.display()
        )
    })?;
    let state = state_path(library, &root)?;
    let mut known: BTreeMap<String, Known> = match std::fs::read_to_string(&state) {
        Ok(text) => serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not a watch state this agent can read; remove it, and every file in the folder is sent again, which the server skips where it already holds the bytes",
                state.display()
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", state.display()));
        }
    };
    if let Some(parent) = state.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let client = reqwest::Client::new();
    let server = crate::server();
    println!(
        "Watching {} for library {library}. Stop with Ctrl-C.",
        root.display()
    );

    let mut watches: HashMap<String, Watch> = HashMap::new();
    let mut refused: HashMap<String, Instant> = HashMap::new();
    loop {
        let listing = list(&root);
        let round = plan(&known, &listing, &mut watches, &mut refused, Instant::now());
        let mut dirty = false;
        for path in &round.deleted {
            println!("{path} was deleted here; nothing changes in the library.");
            known.remove(path);
            dirty = true;
        }

        for group in batches(round.hash, &listing) {
            let mut outgoing = Vec::new();
            for path in group {
                let file = root.join(&path);
                let bytes = match std::fs::read(&file) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        eprintln!("could not read {}: {error}", file.display());
                        // Watched afresh, so it is read again once it settles.
                        watches.remove(&path);
                        continue;
                    }
                };
                let blake3 = blake3::hash(&bytes).to_hex().to_string();
                let seen = listing[&path];
                let sent = Known {
                    size: seen.size,
                    modified_ms: stamp(&seen),
                    blake3,
                };
                // Touched and not changed: the bytes are the ones already sent.
                if known
                    .get(&path)
                    .is_some_and(|last| last.blake3 == sent.blake3)
                {
                    known.insert(path, sent);
                    dirty = true;
                    continue;
                }
                outgoing.push((path, bytes, sent));
            }

            if !outgoing.is_empty() {
                let result = {
                    let files: Vec<crate::Outgoing> = outgoing
                        .iter()
                        .map(|(path, bytes, sent)| crate::Outgoing {
                            path,
                            bytes,
                            blake3: &sent.blake3,
                        })
                        .collect();
                    crate::send_files(&client, &server, library, &files, None).await
                };
                match result {
                    Ok(done) => {
                        let names: Vec<&str> =
                            outgoing.iter().map(|(path, ..)| path.as_str()).collect();
                        println!("Sent {}.", names.join(", "));
                        if !done.have.is_empty() {
                            println!(
                                "Already in the library as they are: {}.",
                                done.have.join(", ")
                            );
                        }
                        if let Some(batch) = &done.batch {
                            println!(
                                "Ingested {}, skipped {}, revised {}, unkept {}.",
                                batch.ingested, batch.skipped, batch.revised, batch.unkept
                            );
                            for failure in &batch.failed {
                                eprintln!("{} was not kept: {}", failure.path, failure.reason);
                            }
                        }
                        let committed: Vec<String> = outgoing
                            .iter()
                            .map(|(path, ..)| path.clone())
                            .filter(|path| !done.have.contains(path))
                            .collect();
                        let refusals = refusals(&committed, done.batch.as_ref());
                        if !refusals.is_empty() {
                            eprintln!(
                                "Sent again in {} minutes, or sooner if they change: {}.",
                                RETRY.as_secs() / 60,
                                refusals.join(", ")
                            );
                        }
                        let sent = outgoing
                            .into_iter()
                            .map(|(path, _, sent)| (path, sent))
                            .collect();
                        settle(&mut known, &mut refused, sent, &refusals, Instant::now());
                        dirty = true;
                    }
                    Err(error) => {
                        eprintln!("Not sent: {error:#}");
                        // Watched afresh, so they are sent again once they settle.
                        for (path, ..) in &outgoing {
                            watches.remove(path);
                        }
                    }
                }
            }
        }

        if dirty {
            let written = serde_json::to_string_pretty(&known)
                .context("could not describe what was sent")
                .and_then(|text| crate::desktop::write_whole(&state, &text));
            if let Err(error) = written {
                eprintln!(
                    "could not keep what was sent in {}: {error:#}; a restart sends those files again, and the server skips the bytes it holds",
                    state.display()
                );
            }
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::SETTLE;
    use std::time::SystemTime;

    fn seen(size: u64, modified: u64) -> Seen {
        Seen {
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(modified),
        }
    }

    fn listing(files: &[(&str, Seen)]) -> BTreeMap<String, Seen> {
        files
            .iter()
            .map(|(path, seen)| ((*path).to_owned(), *seen))
            .collect()
    }

    #[test]
    fn two_folders_watched_into_one_library_keep_separate_states() {
        let library = "01931b6e-0000-7000-8000-000000000001";
        let flanges = state_name(library, Path::new("/home/jbo/parts/flanges"));
        let brackets = state_name(library, Path::new("/home/jbo/parts/brackets"));
        assert_ne!(flanges, brackets);
        assert_eq!(
            flanges,
            state_name(library, Path::new("/home/jbo/parts/flanges"))
        );
        assert!(flanges.starts_with(&format!("watch-{library}-")) && flanges.ends_with(".json"));
    }

    #[test]
    fn the_ignore_list_is_data_6_2s_whole() {
        for name in [
            ".DS_Store",
            "Thumbs.db",
            "~$flange-dn40-lp-3310-02.stl",
            "flange-dn40-lp-3310-02.stl.bak",
            "fixture-plate.3dm.bak",
            "spur-gear-m2-20t.3mf.tmp",
            "vee-block.stl.lck",
            "bracket.FCStd.autosave",
            ".git",
        ] {
            assert!(ignored(name), "{name} is ignored");
        }
        for name in [
            "flange-dn40-lp-3310-02.stl",
            "spur-gear-m2-20t.3mf",
            "fixture-plate.STEP",
        ] {
            assert!(!ignored(name), "{name} is watched");
        }
    }

    #[test]
    fn a_source_path_is_the_path_under_the_folder_and_never_leaves_it() {
        let root = Path::new("/home/mira/parts");
        assert_eq!(
            source_path(
                root,
                Path::new("/home/mira/parts/Flanges/DN40/flange-dn40-lp-3310-02.stl")
            )
            .as_deref(),
            Some("Flanges/DN40/flange-dn40-lp-3310-02.stl")
        );
        assert_eq!(
            source_path(root, Path::new("/home/mira/fixtures/vee-block.stl")),
            None
        );
        assert_eq!(
            source_path(
                root,
                Path::new("/home/mira/parts/../fixtures/vee-block.stl")
            ),
            None
        );
        assert_eq!(source_path(root, root), None);
    }

    #[test]
    fn a_new_file_is_hashed_once_it_has_settled() {
        let (known, now) = (BTreeMap::new(), Instant::now());
        let files = listing(&[("Flanges/flange-dn40-lp-3310-02.stl", seen(9_684, 10))]);
        let mut watches = HashMap::new();
        assert!(
            plan(&known, &files, &mut watches, &mut HashMap::new(), now)
                .hash
                .is_empty(),
            "still settling"
        );
        assert_eq!(
            plan(
                &known,
                &files,
                &mut watches,
                &mut HashMap::new(),
                now + SETTLE
            )
            .hash,
            ["Flanges/flange-dn40-lp-3310-02.stl"]
        );
        assert!(
            plan(
                &known,
                &files,
                &mut watches,
                &mut HashMap::new(),
                now + SETTLE * 2
            )
            .hash
            .is_empty(),
            "once"
        );
    }

    #[test]
    fn a_file_deleted_here_is_reported_and_nothing_is_sent() {
        let known = BTreeMap::from([(
            "vee-block-lp-3072-02.stl".to_owned(),
            Known {
                size: 5_284,
                modified_ms: 10_000,
                blake3: "a".repeat(64),
            },
        )]);
        let round = plan(
            &known,
            &BTreeMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
            Instant::now(),
        );
        assert_eq!(round.deleted, ["vee-block-lp-3072-02.stl"]);
        assert!(round.hash.is_empty());
    }

    #[test]
    fn after_a_restart_an_unchanged_file_sends_nothing() {
        let look = seen(9_684, 10);
        let known = BTreeMap::from([(
            "flange-dn40-lp-3310-02.stl".to_owned(),
            Known {
                size: look.size,
                modified_ms: stamp(&look),
                blake3: "b".repeat(64),
            },
        )]);
        let files = listing(&[("flange-dn40-lp-3310-02.stl", look)]);
        let (mut watches, now) = (HashMap::new(), Instant::now());
        for later in [Duration::ZERO, SETTLE, SETTLE * 3] {
            let round = plan(
                &known,
                &files,
                &mut watches,
                &mut HashMap::new(),
                now + later,
            );
            assert!(
                round.hash.is_empty() && round.deleted.is_empty(),
                "{round:?}"
            );
        }
    }

    #[test]
    fn files_settled_together_go_up_in_uploads_that_fit_in_memory() {
        let mib = 1024 * 1024;
        let files = listing(&[
            ("planetary-carrier-lp-3480-02.3mf", seen(40 * mib, 10)),
            ("spur-gear-m2-20t.stl", seen(20 * mib, 10)),
            ("vee-block-lp-3072-02.stl", seen(10 * mib, 10)),
            ("housing-lp-7710-01.step", seen(90 * mib, 10)),
            ("flange-dn40-lp-3310-02.stl", seen(mib, 10)),
        ]);
        let order = [
            "planetary-carrier-lp-3480-02.3mf",
            "spur-gear-m2-20t.stl",
            "vee-block-lp-3072-02.stl",
            "housing-lp-7710-01.step",
            "flange-dn40-lp-3310-02.stl",
        ];
        let groups = batches(
            order.iter().map(|path| (*path).to_owned()).collect(),
            &files,
        );
        assert_eq!(
            groups,
            [
                vec!["planetary-carrier-lp-3480-02.3mf", "spur-gear-m2-20t.stl"],
                vec!["vee-block-lp-3072-02.stl"],
                vec!["housing-lp-7710-01.step"],
                vec!["flange-dn40-lp-3310-02.stl"],
            ]
        );
    }

    fn batch(failed_total: u32, failed: &[&str]) -> crate::Batch {
        crate::Batch {
            finished_at: Some("2026-09-15T08:12:40Z".to_owned()),
            ingested: 0,
            skipped: 0,
            revised: 0,
            unkept: 0,
            failed_total,
            failed: failed
                .iter()
                .map(|path| crate::Failure {
                    path: (*path).to_owned(),
                    reason: "LP-3310-02 is checked out to mira; check it in, then save again."
                        .to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_refused_file_is_not_recorded_as_sent_and_is_sent_again_later() {
        let files = listing(&[
            ("flange-dn40-lp-3310-02.stl", seen(10_112, 40)),
            ("idler-pulley-lp-4820-00.stl", seen(5_284, 40)),
        ]);
        let (mut known, mut watches, mut refused) =
            (BTreeMap::new(), HashMap::new(), HashMap::new());
        let start = Instant::now();
        assert!(
            plan(&known, &files, &mut watches, &mut refused, start)
                .hash
                .is_empty()
        );
        let settled = start + SETTLE;
        let round = plan(&known, &files, &mut watches, &mut refused, settled);
        assert_eq!(round.hash.len(), 2);

        let committed = round.hash.clone();
        let refusals = refusals(&committed, Some(&batch(1, &["flange-dn40-lp-3310-02.stl"])));
        assert_eq!(refusals, ["flange-dn40-lp-3310-02.stl"]);
        let sent = committed
            .iter()
            .map(|path| {
                let look = files[path];
                (
                    path.clone(),
                    Known {
                        size: look.size,
                        modified_ms: stamp(&look),
                        blake3: "c".repeat(64),
                    },
                )
            })
            .collect();
        settle(&mut known, &mut refused, sent, &refusals, settled);
        assert_eq!(
            known.keys().collect::<Vec<_>>(),
            ["idler-pulley-lp-4820-00.stl"],
            "only the kept one"
        );

        let before = plan(
            &known,
            &files,
            &mut watches,
            &mut refused,
            settled + RETRY - SETTLE,
        );
        assert!(before.hash.is_empty(), "not before RETRY: {before:?}");
        let due = settled + RETRY;
        assert!(
            plan(&known, &files, &mut watches, &mut refused, due)
                .hash
                .is_empty(),
            "settling again"
        );
        assert_eq!(
            plan(&known, &files, &mut watches, &mut refused, due + SETTLE).hash,
            ["flange-dn40-lp-3310-02.stl"]
        );
    }

    #[test]
    fn failures_that_cannot_all_be_matched_refuse_the_whole_upload() {
        let committed = [
            "flange-dn40-lp-3310-02.stl".to_owned(),
            "vee-block-lp-3072-02.stl".to_owned(),
        ];
        assert!(
            refusals(&committed, None).is_empty(),
            "nothing committed, nothing refused"
        );
        assert!(refusals(&committed, Some(&batch(0, &[]))).is_empty());
        assert_eq!(
            refusals(
                &committed,
                Some(&batch(101, &["flange-dn40-lp-3310-02.stl"]))
            ),
            committed,
            "more failures than the batch lists"
        );
        assert_eq!(
            refusals(&committed, Some(&batch(1, &[""]))),
            committed,
            "a failure naming no path"
        );
    }

    #[test]
    fn a_file_changed_while_the_watch_was_stopped_is_hashed() {
        let known = BTreeMap::from([(
            "flange-dn40-lp-3310-02.stl".to_owned(),
            Known {
                size: 9_684,
                modified_ms: stamp(&seen(9_684, 10)),
                blake3: "b".repeat(64),
            },
        )]);
        let files = listing(&[("flange-dn40-lp-3310-02.stl", seen(10_112, 40))]);
        let (mut watches, now) = (HashMap::new(), Instant::now());
        assert!(
            plan(&known, &files, &mut watches, &mut HashMap::new(), now)
                .hash
                .is_empty()
        );
        assert_eq!(
            plan(
                &known,
                &files,
                &mut watches,
                &mut HashMap::new(),
                now + SETTLE
            )
            .hash,
            ["flange-dn40-lp-3310-02.stl"]
        );
    }
}
