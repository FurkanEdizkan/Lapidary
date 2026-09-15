//! Streaming ZIP bundles (`docs/DATA.md` §5.3, Phase 4 slice 2 spec §6): STORE entries written
//! as their bytes are read, and the `manifest.json` that makes a bundle verifiable rather than a
//! folder of mystery files.
//!
//! Written by hand rather than through `zip`, which seeks back to patch each entry's sizes and
//! CRC, where a response body cannot seek. A STORE entry with a data descriptor needs no seek:
//! its local header says the sizes follow the bytes, and the central directory at the end
//! carries them again, which is where readers look.

use serde::{Deserialize, Serialize};
use std::io::{self, Cursor, Read, Write};

/// `manifest.json`'s `format`, so an importer can tell a bundle from any other ZIP.
pub const FORMAT: &str = "lapidary-bundle";
/// `manifest.json`'s `version`. An importer refuses a version it does not know.
pub const VERSION: u32 = 1;
/// Where the manifest sits in a bundle.
pub const MANIFEST: &str = "manifest.json";
/// An archive this long needs ZIP64 offsets, which this writer does not write.
///
/// ponytail: a bundle stops short of 4 GiB. Add ZIP64 records when a real export needs more.
pub const MAX_BYTES: u64 = 0xFFFF_FFFF;
/// More entries than this need ZIP64 too.
pub const MAX_ENTRIES: usize = 0xFFFF;

const LOCAL: u32 = 0x0403_4b50;
const DESCRIPTOR: u32 = 0x0807_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// Bit 3, sizes in a data descriptor after the bytes; bit 11, names in UTF-8.
const FLAGS: u16 = 0x0808;
const VERSION_NEEDED: u16 = 20;
/// 1980-01-01 00:00, DOS time's own start: a bundle's bytes do not depend on when it was made.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;

/// A bundle's manifest: the library it came from and every part in it, each with its revisions
/// oldest first, so an importer replays the history in the order it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub library: ManifestLibrary,
    pub parts: Vec<ManifestPart>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestLibrary {
    pub name: String,
    /// `hobby` or `controlled`.
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestPart {
    pub name: String,
    pub part_number: Option<String>,
    pub source_path: String,
    pub tags: Vec<String>,
    pub sources: Vec<ManifestSource>,
    /// Oldest first.
    pub revisions: Vec<ManifestRevision>,
}

/// Where a part came from and under what licence. Prices stay out: they are one shop's figure on
/// one day, and a bundle travels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSource {
    pub url: Option<String>,
    pub vendor: Option<String>,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestRevision {
    pub rev_label: String,
    pub parent_label: Option<String>,
    pub origin: String,
    pub created_at: String,
    pub blake3: String,
    pub size_bytes: u64,
    pub format: String,
    /// The entry holding this revision's bytes.
    pub path: String,
}

/// Where a revision's file sits in a bundle: the current one at the part's own source path, each
/// earlier one beside it under `revisions/<label>/`, as the library keeps them on disk.
pub fn entry_path(source_path: &str, rev_label: &str, current: bool) -> String {
    if current {
        return source_path.to_owned();
    }
    match source_path.rsplit_once('/') {
        Some((folder, file)) => format!("{folder}/revisions/{rev_label}/{file}"),
        None => format!("revisions/{rev_label}/{source_path}"),
    }
}

/// The archive's exact length before a byte is written, so a response can promise it and a
/// client can tell a whole bundle from one that stopped.
pub fn archive_len<'a>(entries: impl IntoIterator<Item = (&'a str, u64)>) -> u64 {
    let (mut local, mut central) = (0u64, 0u64);
    for (name, size) in entries {
        let name = name.len() as u64;
        local += 30 + name + size + 16;
        central += 46 + name;
    }
    local + central + 22
}

struct Written {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

/// A ZIP of STORE entries, written straight through to `out` as each entry's bytes are read.
pub struct StoreZip<W: Write> {
    out: W,
    offset: u64,
    written: Vec<Written>,
}

impl<W: Write> StoreZip<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            offset: 0,
            written: Vec::new(),
        }
    }

    /// One entry, its bytes read from `source` to the end and written as they arrive. Returns
    /// how many bytes the entry holds.
    pub fn add(&mut self, name: &str, source: &mut dyn Read) -> io::Result<u64> {
        if self.written.len() >= MAX_ENTRIES {
            return Err(io::Error::other(format!(
                "a bundle holds at most {MAX_ENTRIES} files; export fewer parts at a time"
            )));
        }
        let name_len = u16::try_from(name.len()).map_err(|_| {
            io::Error::other(format!(
                "{name} is too long a path for a ZIP entry; rename the part's file"
            ))
        })?;
        let offset = fits(self.offset)?;

        let mut header = Vec::with_capacity(30 + name.len());
        put32(&mut header, LOCAL);
        put16(&mut header, VERSION_NEEDED);
        put16(&mut header, FLAGS);
        put16(&mut header, 0);
        put16(&mut header, DOS_TIME);
        put16(&mut header, DOS_DATE);
        put32(&mut header, 0);
        put32(&mut header, 0);
        put32(&mut header, 0);
        put16(&mut header, name_len);
        put16(&mut header, 0);
        header.extend_from_slice(name.as_bytes());
        self.emit(&header)?;

        let mut crc = crc32fast::Hasher::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            crc.update(&buffer[..read]);
            self.emit(&buffer[..read])?;
            size += read as u64;
        }
        let crc = crc.finalize();
        let stored = fits(size)?;

        let mut descriptor = Vec::with_capacity(16);
        put32(&mut descriptor, DESCRIPTOR);
        put32(&mut descriptor, crc);
        put32(&mut descriptor, stored);
        put32(&mut descriptor, stored);
        self.emit(&descriptor)?;

        self.written.push(Written {
            name: name.to_owned(),
            crc,
            size: stored,
            offset,
        });
        Ok(size)
    }

    /// The central directory and its end record, then the writer back, flushed.
    pub fn finish(mut self) -> io::Result<W> {
        let start = fits(self.offset)?;
        let count = u16::try_from(self.written.len()).map_err(|_| {
            io::Error::other("a bundle holds at most 65,535 files; export fewer parts at a time")
        })?;
        let mut directory = Vec::new();
        for entry in &self.written {
            put32(&mut directory, CENTRAL);
            put16(&mut directory, VERSION_NEEDED);
            put16(&mut directory, VERSION_NEEDED);
            put16(&mut directory, FLAGS);
            put16(&mut directory, 0);
            put16(&mut directory, DOS_TIME);
            put16(&mut directory, DOS_DATE);
            put32(&mut directory, entry.crc);
            put32(&mut directory, entry.size);
            put32(&mut directory, entry.size);
            put16(
                &mut directory,
                u16::try_from(entry.name.len()).unwrap_or(u16::MAX),
            );
            put16(&mut directory, 0);
            put16(&mut directory, 0);
            put16(&mut directory, 0);
            put16(&mut directory, 0);
            put32(&mut directory, 0);
            put32(&mut directory, entry.offset);
            directory.extend_from_slice(entry.name.as_bytes());
        }
        let directory_len = fits(directory.len() as u64)?;
        self.emit(&directory)?;

        let mut end = Vec::with_capacity(22);
        put32(&mut end, END);
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, count);
        put16(&mut end, count);
        put32(&mut end, directory_len);
        put32(&mut end, start);
        put16(&mut end, 0);
        self.emit(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn emit(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.offset += bytes.len() as u64;
        Ok(())
    }
}

/// An offset or size, refused once it would need ZIP64.
fn fits(value: u64) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| {
        io::Error::other(
            "the bundle reached 4 GiB, which needs ZIP64 this writer does not write; export fewer parts at a time",
        )
    })
}

/// The most files an import reads out of one bundle (`docs/DATA.md` §5.4).
pub const MAX_IMPORT_ENTRIES: usize = 10_000;
/// The most bytes an import reads out of one bundle: the upload route's own cap.
pub const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// A bundle read back and checked whole before anything is written: every name inside it, every
/// file stored as it is, the manifest's format and version, and every revision's file present at
/// the size and BLAKE3 the manifest names, with an origin this build knows.
pub struct Bundle {
    pub manifest: Manifest,
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Bundle {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let refuse = |detail: String| {
            format!(
                "This is not a bundle Lapidary can import: {detail}. Export it again from Lapidary, then import it."
            )
        };
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| refuse(format!("it does not open as a ZIP ({err})")))?;
        if archive.len() > MAX_IMPORT_ENTRIES {
            return Err(refuse(format!(
                "it holds {} files, and an import reads at most {MAX_IMPORT_ENTRIES}",
                archive.len()
            )));
        }
        let mut total = 0u64;
        for index in 0..archive.len() {
            let entry = archive
                .by_index_raw(index)
                .map_err(|err| refuse(format!("file {index} cannot be read ({err})")))?;
            let name = entry.name().to_owned();
            if !safe_name(&name) {
                return Err(refuse(format!("it names a file outside itself, {name}")));
            }
            if entry.compression() != zip::CompressionMethod::Stored {
                return Err(refuse(format!(
                    "{name} is compressed, where a bundle stores its files as they are"
                )));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_IMPORT_BYTES {
                return Err(refuse("its files come to more than 2 GiB".to_owned()));
            }
        }

        let mut text = String::new();
        archive
            .by_name(MANIFEST)
            .map_err(|_| refuse("it has no manifest.json".to_owned()))?
            .read_to_string(&mut text)
            .map_err(|err| refuse(format!("its manifest.json cannot be read ({err})")))?;
        let manifest: Manifest = serde_json::from_str(&text)
            .map_err(|err| refuse(format!("its manifest.json does not parse ({err})")))?;
        if manifest.format != FORMAT || manifest.version != VERSION {
            return Err(refuse(format!(
                "its manifest is {} version {}, and this Lapidary reads {FORMAT} version {VERSION}",
                manifest.format, manifest.version
            )));
        }

        let mut source_paths = std::collections::HashSet::new();
        for part in &manifest.parts {
            if !safe_name(&part.source_path) || !source_paths.insert(part.source_path.as_str()) {
                return Err(refuse(format!(
                    "two parts claim {}, or it names a place outside the library",
                    part.source_path
                )));
            }
            if part.revisions.is_empty() {
                return Err(refuse(format!("{} has no revisions", part.source_path)));
            }
            for revision in &part.revisions {
                if lapidary_core::RevisionOrigin::parse(&revision.origin).is_none() {
                    return Err(refuse(format!(
                        "revision {} of {} came by {:?}, which this Lapidary does not know",
                        revision.rev_label, part.source_path, revision.origin
                    )));
                }
                let bytes = read_entry(&mut archive, &revision.path).map_err(refuse)?;
                if bytes.len() as u64 != revision.size_bytes
                    || blake3::hash(&bytes).to_hex().as_str() != revision.blake3
                {
                    return Err(refuse(format!(
                        "the file at {} is not the one its manifest names: its size or hash differs",
                        revision.path
                    )));
                }
            }
        }
        Ok(Self { manifest, archive })
    }

    /// One revision's bytes, which [`Bundle::open`] has already checked.
    pub fn bytes(&mut self, path: &str) -> Result<Vec<u8>, String> {
        read_entry(&mut self.archive, path)
    }
}

fn read_entry(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    path: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(path)
        .map_err(|_| format!("it has no file at {path}, which its manifest names"))?;
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry
        .read_to_end(&mut bytes)
        .map_err(|err| format!("the file at {path} cannot be read ({err})"))?;
    Ok(bytes)
}

/// A name that stays inside the archive, and inside a library once a part is filed under it: no
/// absolute path, no drive, no backslash, no empty, `.` or `..` segment.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':'])
        && name
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_bundle_reads_back_as_stored_entries_holding_their_bytes_at_the_promised_length() {
        let flange = b"solid flange-dn40-lp-3310-02\nendsolid flange\n".repeat(700);
        let vee = b"solid vee-block-lp-3072-02\nendsolid vee\n".to_vec();
        let entries = [
            ("flanges/flange-dn40-lp-3310-02.stl", &flange),
            ("vee-block-lp-3072-02.stl", &vee),
        ];
        let mut zip = StoreZip::new(Vec::new());
        for (name, bytes) in entries {
            assert_eq!(
                zip.add(name, &mut bytes.as_slice()).expect("adds"),
                bytes.len() as u64
            );
        }
        let archive = zip.finish().expect("finishes");
        assert_eq!(
            archive.len() as u64,
            archive_len(entries.map(|(name, bytes)| (name, bytes.len() as u64)))
        );

        let mut read = zip::ZipArchive::new(Cursor::new(archive)).expect("an ordinary ZIP");
        assert_eq!(read.len(), 2);
        for (name, bytes) in entries {
            let mut entry = read.by_name(name).expect("the entry");
            assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
            let mut back = Vec::new();
            entry
                .read_to_end(&mut back)
                .expect("reads, its CRC checked at the end");
            assert_eq!(&back, bytes);
        }
    }

    fn manifest_for(parts: &[(&str, &[u8])]) -> Manifest {
        Manifest {
            format: FORMAT.to_owned(),
            version: VERSION,
            library: ManifestLibrary {
                name: "Workshop".to_owned(),
                mode: "controlled".to_owned(),
            },
            parts: parts
                .iter()
                .map(|(path, bytes)| ManifestPart {
                    name: (*path).to_owned(),
                    part_number: None,
                    source_path: (*path).to_owned(),
                    tags: vec![],
                    sources: vec![],
                    revisions: vec![ManifestRevision {
                        rev_label: "1".to_owned(),
                        parent_label: None,
                        origin: "ingest".to_owned(),
                        created_at: "2026-09-15T08:00:00Z".to_owned(),
                        blake3: blake3::hash(bytes).to_hex().to_string(),
                        size_bytes: bytes.len() as u64,
                        format: "stl".to_owned(),
                        path: (*path).to_owned(),
                    }],
                })
                .collect(),
        }
    }

    fn archive(files: &[(&str, &[u8])], manifest: Option<&Manifest>) -> Vec<u8> {
        let mut zip = StoreZip::new(Vec::new());
        for (name, bytes) in files {
            zip.add(name, &mut &bytes[..]).expect("adds");
        }
        if let Some(manifest) = manifest {
            let json = serde_json::to_vec(manifest).expect("serialises");
            zip.add(MANIFEST, &mut json.as_slice()).expect("adds");
        }
        zip.finish().expect("finishes")
    }

    #[test]
    fn a_bundle_opens_whole_and_hands_back_each_revisions_bytes() {
        let flange: &[u8] = b"solid flange-dn40-lp-3310-02\nendsolid\n";
        let manifest = manifest_for(&[("flanges/flange-dn40-lp-3310-02.stl", flange)]);
        let mut bundle = Bundle::open(archive(
            &[("flanges/flange-dn40-lp-3310-02.stl", flange)],
            Some(&manifest),
        ))
        .expect("opens");
        assert_eq!(bundle.manifest, manifest);
        assert_eq!(
            bundle
                .bytes("flanges/flange-dn40-lp-3310-02.stl")
                .expect("bytes"),
            flange
        );
    }

    #[test]
    fn a_bundle_that_is_not_whole_is_refused_before_anything_is_read_into_a_library() {
        let flange: &[u8] = b"solid flange-dn40-lp-3310-02\nendsolid\n";
        let path = "flanges/flange-dn40-lp-3310-02.stl";
        let good = manifest_for(&[(path, flange)]);

        let escaping = Bundle::open(archive(
            &[(path, flange), ("../../.ssh/id_ed25519", b"key")],
            Some(&good),
        ));
        assert!(
            escaping
                .err()
                .is_some_and(|message| message.contains("outside"))
        );

        assert!(
            Bundle::open(archive(&[(path, flange)], None))
                .err()
                .is_some_and(|message| message.contains("manifest.json"))
        );

        let mut wrong_hash = good.clone();
        wrong_hash.parts[0].revisions[0].blake3 = "00".repeat(32);
        assert!(
            Bundle::open(archive(&[(path, flange)], Some(&wrong_hash)))
                .err()
                .is_some_and(|message| message.contains("size or hash"))
        );

        let mut newer = good.clone();
        newer.version = VERSION + 1;
        assert!(Bundle::open(archive(&[(path, flange)], Some(&newer))).is_err());

        let mut unknown_origin = good;
        unknown_origin.parts[0].revisions[0].origin = "teleport".to_owned();
        assert!(Bundle::open(archive(&[(path, flange)], Some(&unknown_origin))).is_err());

        assert!(Bundle::open(b"not a zip at all".to_vec()).is_err());
    }

    #[test]
    fn an_earlier_revision_sits_under_revisions_beside_the_current_file() {
        assert_eq!(
            entry_path("flanges/flange-dn40-lp-3310-02.stl", "2", true),
            "flanges/flange-dn40-lp-3310-02.stl"
        );
        assert_eq!(
            entry_path("flanges/flange-dn40-lp-3310-02.stl", "1", false),
            "flanges/revisions/1/flange-dn40-lp-3310-02.stl"
        );
        assert_eq!(
            entry_path("vee-block-lp-3072-02.stl", "1", false),
            "revisions/1/vee-block-lp-3072-02.stl"
        );
    }
}
