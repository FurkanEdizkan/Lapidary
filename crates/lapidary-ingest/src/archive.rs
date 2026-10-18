//! An uploaded `.zip` or `.7z`, opened (A10, owner 2026-10-18: "we should be able to upload zip and 7z").
//!
//! The archive stays where the upload put it, as it was sent. Its models are unpacked beside it, into a
//! folder named after it (`uploads/<user>/pack.zip` → `uploads/<user>/pack/…`), each through
//! [`InboxWriter::put_or_reuse`]: never over a different file, and a retry reuses what the first run
//! wrote. Each becomes its own `IngestInbox` job in the upload's batch, so it is indexed exactly as an
//! uploaded file is. STEP and IGES files are unpacked too, so their own job refuses them by name, as
//! an uploaded folder's are.
//!
//! Streamed, never read whole: an entry goes to disk through a reader that stops at the limit, and the
//! size an archive claims for an entry is never trusted. A name that would leave the folder, a link, an
//! archive inside the archive and an entry that can't be read (encrypted, an unsupported method) are
//! skipped and named. An uploaded RAR is kept as a pack (A13): the only reader is non-free C++.

use lapidary_core::{CAD_FORMATS, MESH_EXTENSIONS, source_format};
use lapidary_storage::InboxWriter;
use std::io::Read;
use std::path::Path;

/// The archives an upload opens, as `source_format` spells them.
pub(crate) use lapidary_core::ARCHIVE_EXTENSIONS as ARCHIVES;
/// The archives refused by name, with what to do instead.
pub(crate) const REFUSED_ARCHIVES: [&str; 1] = ["rar"];

/// How much one archive may unpack.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// Bytes in one entry.
    pub entry: u64,
    /// Bytes in all of them.
    pub total: u64,
    /// Entries looked at, of any kind.
    pub entries: usize,
}

// ponytail: fixed limits. An upload is already capped at 2 GB, and a pack that unpacks past 8 GB or
// holds 20,000 entries is refused rather than written; make them settings if a real pack hits them.
pub(crate) const LIMITS: Limits = Limits {
    entry: 2 * 1024 * 1024 * 1024,
    total: 8 * 1024 * 1024 * 1024,
    entries: 20_000,
};

/// What one archive gave.
#[derive(Debug, Default)]
pub(crate) struct Unpacked {
    /// `(path inside the archive, where it was written in the inbox)`, in the archive's order.
    pub placed: Vec<(String, String)>,
    /// What was left out, each as `name (why)`.
    pub skipped: Vec<String>,
}

/// Unpack the models in the archive at `archive` (inbox-relative) into the folder beside it.
/// An `Err` is a sentence for the person who uploaded it; nothing it says was written stays half-done,
/// because each entry is placed whole or not at all.
pub(crate) fn unpack(inbox: &Path, archive: &str, limits: Limits) -> Result<Unpacked, String> {
    let shown = archive.rsplit('/').next().unwrap_or(archive);
    let into = without_extension(archive);
    let file = std::fs::File::open(inbox.join(archive))
        .map_err(|e| format!("Could not read the uploaded {shown} back: {e}."))?;
    let writer = InboxWriter::open(inbox);
    let mut out = Unpacked::default();
    let mut budget = Budget {
        limits,
        total: 0,
        seen: 0,
    };
    match source_format(archive).as_str() {
        "zip" => unpack_zip(file, shown, &into, &writer, &mut budget, &mut out)?,
        "7z" => unpack_7z(file, shown, &into, &writer, &mut budget, &mut out)?,
        other => {
            return Err(format!(
                "{shown} is a .{other} file, not an archive Lapidary opens."
            ));
        }
    }
    Ok(out)
}

/// `uploads/a/pack (2).zip` → `uploads/a/pack (2)`.
pub(crate) fn without_extension(path: &str) -> String {
    let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    if dir.is_empty() {
        stem.to_owned()
    } else {
        format!("{dir}/{stem}")
    }
}

struct Budget {
    limits: Limits,
    total: u64,
    seen: usize,
}

impl Budget {
    fn count(&mut self, shown: &str) -> Result<(), String> {
        self.seen += 1;
        if self.seen > self.limits.entries {
            return Err(format!(
                "{shown} holds more than {} entries, more than one upload unpacks. Extract it and upload the folder.",
                self.limits.entries
            ));
        }
        Ok(())
    }
}

/// What to do with one entry, by its name.
enum Take {
    /// A model, or a STEP/IGES file for its own job to refuse by name.
    Unpack(String),
    Skip(String),
    Ignore,
}

fn judge(name: &str) -> Take {
    let name = name.replace('\\', "/");
    if lapidary_core::slug::reject_escaping_path(&name).is_err()
        || name.starts_with('/')
        || name.contains(':')
    {
        return Take::Skip(format!("{name} (its name would leave the folder)"));
    }
    if name
        .split('/')
        .any(|part| part.starts_with('.') || part == "__MACOSX")
    {
        return Take::Ignore;
    }
    let format = source_format(&name);
    if MESH_EXTENSIONS.contains(&format.as_str()) || CAD_FORMATS.contains(&format.as_str()) {
        Take::Unpack(name)
    } else if ARCHIVES.contains(&format.as_str()) || REFUSED_ARCHIVES.contains(&format.as_str()) {
        Take::Skip(format!(
            "{name} (an archive inside the archive; upload it on its own)"
        ))
    } else {
        Take::Ignore
    }
}

/// A reader that fails once more than `left` bytes pass, whatever the entry claimed.
struct Capped<'a, R> {
    inner: R,
    left: u64,
    total: &'a mut u64,
    limit_total: u64,
}

impl<R: Read> Read for Capped<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        let read_u64 = read as u64;
        if read_u64 > self.left || *self.total + read_u64 > self.limit_total {
            return Err(std::io::Error::other("past the unpack limit"));
        }
        self.left -= read_u64;
        *self.total += read_u64;
        Ok(read)
    }
}

fn place(
    writer: &InboxWriter,
    into: &str,
    name: &str,
    from: &mut dyn Read,
    budget: &mut Budget,
    shown: &str,
) -> Result<String, String> {
    let mut capped = Capped {
        inner: from,
        left: budget.limits.entry,
        total: &mut budget.total,
        limit_total: budget.limits.total,
    };
    writer
        .put_or_reuse(&format!("{into}/{name}"), &mut capped)
        .map(|placed| placed.path)
        .map_err(|e| {
            if e.to_string().contains("past the unpack limit") {
                format!(
                    "{name} in {shown} unpacks to more than one upload takes ({} GB a file, {} GB in all). Extract it and upload the folder.",
                    budget.limits.entry >> 30,
                    budget.limits.total >> 30
                )
            } else {
                format!("Could not unpack {name} from {shown}: {e}")
            }
        })
}

fn unpack_zip(
    file: std::fs::File,
    shown: &str,
    into: &str,
    writer: &InboxWriter,
    budget: &mut Budget,
    out: &mut Unpacked,
) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        format!(
            "{shown} is not a zip archive Lapidary can read ({e}). Re-pack it and upload it again."
        )
    })?;
    for index in 0..zip.len() {
        budget.count(shown)?;
        let listed = zip
            .name_for_index(index)
            .and_then(Result::ok)
            .map_or_else(|| format!("entry {index}"), |name| name.into_owned());
        let mut entry = match zip.by_index(index) {
            Ok(entry) => entry,
            Err(e) => {
                out.skipped.push(format!("{listed} (can't be read: {e})"));
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let Ok(raw) = entry.name().map(|name| name.into_owned()) else {
            out.skipped
                .push(format!("entry {index} (its name can't be read)"));
            continue;
        };
        if entry.is_symlink() {
            out.skipped.push(format!("{raw} (a link)"));
            continue;
        }
        if entry.enclosed_name().is_none() {
            out.skipped
                .push(format!("{raw} (its name would leave the folder)"));
            continue;
        }
        match judge(&raw) {
            Take::Unpack(name) => {
                let at = place(writer, into, &name, &mut entry, budget, shown)?;
                out.placed.push((name, at));
            }
            Take::Skip(why) => out.skipped.push(why),
            Take::Ignore => {}
        }
    }
    Ok(())
}

fn unpack_7z(
    file: std::fs::File,
    shown: &str,
    into: &str,
    writer: &InboxWriter,
    budget: &mut Budget,
    out: &mut Unpacked,
) -> Result<(), String> {
    use sevenz_rust2::{ArchiveReader, Error, Password};
    let refused = |e: Error| match e {
        Error::PasswordRequired | Error::MaybeBadPassword(_) => {
            format!("{shown} is encrypted. Extract it with its password and upload the folder.")
        }
        Error::UnsupportedCompressionMethod(method) => format!(
            "{shown} is packed with {method}, which Lapidary doesn't unpack. Re-pack it as .zip or with 7-Zip's default LZMA2."
        ),
        other => format!(
            "{shown} is not a 7z archive Lapidary can read ({other}). Re-pack it and upload it again."
        ),
    };
    let mut reader =
        ArchiveReader::new(std::io::BufReader::new(file), Password::empty()).map_err(refused)?;
    // The closure can't return our sentence through the crate's error, so it parks it here.
    let mut stopped: Option<String> = None;
    let walked = reader.for_each_entries(|entry, data| {
        if let Err(sentence) = budget.count(shown) {
            stopped = Some(sentence);
            return Ok(false);
        }
        if entry.is_directory() || entry.is_anti_item() {
            return Ok(true);
        }
        // FILE_ATTRIBUTE_REPARSE_POINT, or a unix mode in the high half that says link.
        let attributes = entry.windows_attributes();
        if entry.has_windows_attributes
            && (attributes & 0x400 != 0 || (attributes >> 16) & 0o170000 == 0o120000)
        {
            out.skipped.push(format!("{} (a link)", entry.name()));
            std::io::copy(data, &mut std::io::sink())?;
            return Ok(true);
        }
        match judge(entry.name()) {
            Take::Unpack(name) => match place(writer, into, &name, data, budget, shown) {
                Ok(at) => out.placed.push((name, at)),
                Err(sentence) => {
                    stopped = Some(sentence);
                    return Ok(false);
                }
            },
            Take::Skip(why) => {
                out.skipped.push(why);
                std::io::copy(data, &mut std::io::sink())?;
            }
            // A solid archive is one stream: what isn't kept is still read through, or the next
            // entry starts in the middle of this one and fails its checksum.
            Take::Ignore => {
                std::io::copy(data, &mut std::io::sink())?;
            }
        }
        Ok(true)
    });
    if let Some(sentence) = stopped {
        return Err(sentence);
    }
    walked.map_err(refused)
}

/// The most entries a pack's page lists (A13). `total` still counts them all.
pub(crate) const MAX_LISTED: usize = 5_000;

/// What a pack holds (A13), from the archive's own index and nothing unpacked:
/// `{"entries": [{"path", "size"}], "total"}`, or `{"unread": "<a sentence>"}`. **Never an error**: a pack
/// whose index can't be read is still a pack, and its page says why there is no list.
pub(crate) fn contents(file: &Path, format: &str) -> serde_json::Value {
    let listed = match format {
        "zip" => zip_index(file),
        "7z" => sevenz_index(file),
        "rar" => Err(
            "RAR archives aren't opened, so what is inside isn't listed: the only RAR reader is not free software."
                .to_owned(),
        ),
        other => Err(format!("A .{other} file isn't an archive Lapidary lists.")),
    };
    match listed {
        Ok(mut entries) => {
            let total = entries.len();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            entries.truncate(MAX_LISTED);
            serde_json::json!({
                "entries": entries.into_iter().map(|(path, size)| serde_json::json!({ "path": path, "size": size })).collect::<Vec<_>>(),
                "total": total,
            })
        }
        Err(unread) => serde_json::json!({ "unread": unread }),
    }
}

fn zip_index(file: &Path) -> Result<Vec<(String, u64)>, String> {
    let open =
        std::fs::File::open(file).map_err(|e| format!("The archive couldn't be read: {e}."))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(open)).map_err(|e| {
        format!("Its index couldn't be read ({e}), so what is inside isn't listed.")
    })?;
    let mut out = Vec::with_capacity(zip.len().min(MAX_LISTED));
    for index in 0..zip.len() {
        // Raw: the index alone, so an encrypted entry is still listed by name.
        let Ok(entry) = zip.by_index_raw(index) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let Ok(name) = entry.name().map(|name| name.into_owned()) else {
            continue;
        };
        out.push((name, entry.size()));
    }
    Ok(out)
}

fn sevenz_index(file: &Path) -> Result<Vec<(String, u64)>, String> {
    use sevenz_rust2::{Archive, Error, Password};
    let mut open = std::io::BufReader::new(
        std::fs::File::open(file).map_err(|e| format!("The archive couldn't be read: {e}."))?,
    );
    let archive = Archive::read(&mut open, &Password::empty()).map_err(|e| match e {
        Error::PasswordRequired | Error::MaybeBadPassword(_) => {
            "Its index is encrypted, so what is inside isn't listed.".to_owned()
        }
        other => format!("Its index couldn't be read ({other}), so what is inside isn't listed."),
    })?;
    Ok(archive
        .files
        .iter()
        .filter(|entry| !entry.is_directory() && !entry.is_anti_item())
        .map(|entry| (entry.name().to_owned(), entry.size()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).expect("zip"));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            zip.write_all(bytes).expect("bytes");
        }
        zip.finish().expect("finished");
    }

    const SMALL: Limits = Limits {
        entry: 64,
        total: 100,
        entries: 10,
    };

    #[test]
    fn a_zip_unpacks_its_models_beside_it_and_a_retry_writes_nothing_new() {
        let root = tempfile::tempdir().expect("a folder");
        std::fs::create_dir_all(root.path().join("uploads/ayse")).expect("uploads");
        zip_of(
            &root.path().join("uploads/ayse/voron spares (2).zip"),
            &[
                ("stl/z-endstop-mount.stl", b"solid z"),
                ("plate.3mf", b"PK not really"),
                ("printhead.step", b"ISO-10303-21;"),
                ("README.md", b"# Voron"),
                ("__MACOSX/._plate.3mf", b"junk"),
                ("../../escape.stl", b"solid x"),
                ("extras.zip", b"PK"),
            ],
        );
        let first =
            unpack(root.path(), "uploads/ayse/voron spares (2).zip", SMALL).expect("unpacked");
        let names: Vec<&str> = first.placed.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["stl/z-endstop-mount.stl", "plate.3mf", "printhead.step"]
        );
        assert_eq!(
            first.placed[0].1,
            "uploads/ayse/voron spares (2)/stl/z-endstop-mount.stl"
        );
        assert_eq!(first.skipped.len(), 2, "{:?}", first.skipped);
        assert!(
            !root.path().join("escape.stl").exists()
                && !root.path().join("uploads/escape.stl").exists()
        );
        // The archive is kept as it was uploaded.
        assert!(
            root.path()
                .join("uploads/ayse/voron spares (2).zip")
                .exists()
        );
        let again = unpack(root.path(), "uploads/ayse/voron spares (2).zip", SMALL).expect("again");
        assert_eq!(again.placed, first.placed);
    }

    #[test]
    fn a_folder_already_there_is_never_written_over() {
        let root = tempfile::tempdir().expect("a folder");
        std::fs::create_dir_all(root.path().join("pack")).expect("pack");
        std::fs::write(root.path().join("pack/bracket.stl"), b"solid mine").expect("mine");
        zip_of(
            &root.path().join("pack.zip"),
            &[("bracket.stl", b"solid theirs")],
        );
        let unpacked = unpack(root.path(), "pack.zip", SMALL).expect("unpacked");
        assert_eq!(unpacked.placed[0].1, "pack/bracket (2).stl");
        assert_eq!(
            std::fs::read(root.path().join("pack/bracket.stl")).expect("read"),
            b"solid mine"
        );
    }

    #[test]
    fn limits_hold_whatever_the_archive_claims() {
        let root = tempfile::tempdir().expect("a folder");
        zip_of(&root.path().join("big.zip"), &[("big.stl", &[b'x'; 65])]);
        let said = unpack(root.path(), "big.zip", SMALL).expect_err("too big");
        assert!(said.contains("more than one upload takes"), "{said}");
        assert!(!root.path().join("big/big.stl").exists());
        let many: Vec<(String, Vec<u8>)> = (0..11).map(|i| (format!("{i}.txt"), vec![])).collect();
        let many: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        zip_of(&root.path().join("many.zip"), &many);
        assert!(
            unpack(root.path(), "many.zip", SMALL)
                .expect_err("too many")
                .contains("more than 10 entries")
        );
    }

    #[test]
    fn what_is_not_a_readable_archive_says_so() {
        let root = tempfile::tempdir().expect("a folder");
        std::fs::write(root.path().join("broken.zip"), b"not a zip").expect("broken");
        std::fs::write(root.path().join("broken.7z"), b"not a 7z").expect("broken");
        assert!(
            unpack(root.path(), "broken.zip", SMALL)
                .expect_err("zip")
                .contains("not a zip archive")
        );
        assert!(
            unpack(root.path(), "broken.7z", SMALL)
                .expect_err("7z")
                .contains("not a 7z archive")
        );
        assert_eq!(
            without_extension("uploads/a/pack (2).zip"),
            "uploads/a/pack (2)"
        );
        assert_eq!(without_extension("pack.tar.7z"), "pack.tar");
    }

    #[test]
    fn a_packs_contents_come_from_its_index_and_never_fail() {
        let root = tempfile::tempdir().expect("a folder");
        let zip = root.path().join("castigator.zip");
        zip_of(
            &zip,
            &[
                ("Supported/castigator-body.stl", b"solid body"),
                ("readme.txt", b"hi"),
            ],
        );
        let listed = contents(&zip, "zip");
        assert_eq!(listed["total"], 2);
        assert_eq!(
            listed["entries"][0]["path"],
            "Supported/castigator-body.stl"
        );
        assert_eq!(listed["entries"][0]["size"], 10);
        // A RAR, a broken archive and a missing file are packs with a sentence, not failures.
        std::fs::write(root.path().join("nuns.rar"), b"Rar!").expect("rar");
        assert!(
            contents(&root.path().join("nuns.rar"), "rar")["unread"]
                .as_str()
                .is_some_and(|s| s.contains("RAR"))
        );
        std::fs::write(root.path().join("broken.zip"), b"nope").expect("broken");
        assert!(contents(&root.path().join("broken.zip"), "zip")["unread"].is_string());
        assert!(contents(&root.path().join("gone.7z"), "7z")["unread"].is_string());
    }

    #[test]
    fn a_7z_unpacks_like_a_zip() {
        let root = tempfile::tempdir().expect("a folder");
        let source = root.path().join("source");
        std::fs::create_dir_all(source.join("parts")).expect("source");
        std::fs::write(source.join("parts/hinge-pin.stl"), b"solid hinge").expect("stl");
        // Ahead of the model in the one solid stream, as a pack's photos are.
        std::fs::write(source.join("a-photo.jpg"), vec![7u8; 50_000]).expect("jpg");
        std::fs::write(source.join("notes.txt"), b"print at 0.2").expect("txt");
        sevenz_rust2::compress_to_path(&source, root.path().join("hinge.7z")).expect("7z");
        let listed = contents(&root.path().join("hinge.7z"), "7z");
        assert_eq!(listed["total"], 3, "{listed}");
        let unpacked = unpack(root.path(), "hinge.7z", SMALL).expect("unpacked");
        assert_eq!(
            unpacked.placed,
            [(
                "parts/hinge-pin.stl".to_owned(),
                "hinge/parts/hinge-pin.stl".to_owned()
            )]
        );
        assert_eq!(
            std::fs::read(root.path().join("hinge/parts/hinge-pin.stl")).expect("read"),
            b"solid hinge"
        );
    }
}
