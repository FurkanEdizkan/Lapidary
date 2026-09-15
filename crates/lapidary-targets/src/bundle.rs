//! Streaming ZIP bundles (`docs/DATA.md` §5.3, Phase 4 slice 2 spec §6): STORE entries written
//! as their bytes are read, and the `manifest.json` that makes a bundle verifiable rather than a
//! folder of mystery files.
//!
//! Written by hand rather than through `zip`, which seeks back to patch each entry's sizes and
//! CRC, where a response body cannot seek. A STORE entry with a data descriptor needs no seek:
//! its local header says the sizes follow the bytes, and the central directory at the end
//! carries them again, which is where readers look.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

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
