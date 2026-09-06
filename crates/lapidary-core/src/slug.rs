//! Turning a human name into something every filesystem this ships on can hold.
//!
//! The store is browsable by design, so these names are read by people in a file manager
//! — but they also have to survive Windows, which the agent binary and the Tauri shell
//! both target. Nothing parses them: `metadata.json` inside each directory carries
//! identity, so these optimise for legibility rather than round-tripping.

use crate::{BlobHash, CoreError};

/// Windows reserves these exactly, case-insensitively, with or without an extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters no path component may carry: separators, the set Windows reserves, and
/// control characters.
const HOSTILE: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];

/// 120 chars, not 255 bytes: leaves room for `_a1b2c3` inside the smallest component limit
/// we ship against, and counts characters so a multi-byte name is not silently halved.
const MAX_CHARS: usize = 120;

pub fn slugify(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if HOSTILE.contains(&c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();

    let trimmed = replaced.trim().trim_end_matches(['.', ' ']).trim();

    let capped: String = trimmed.chars().take(MAX_CHARS).collect();

    if capped.is_empty() {
        return "unnamed".to_owned();
    }

    // Windows reserves the *stem*, so `AUX.stl` is refused too. Compare before any
    // extension.
    let stem = capped.split('.').next().unwrap_or(&capped);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return format!("{capped}_");
    }

    capped
}

/// The suffix that resolves a directory collision. Deterministic, so the same model
/// re-ingested lands on the same name.
pub fn disambiguate(slug: &str, hash: &BlobHash) -> String {
    format!("{slug}_{}", &hash.to_hex()[..6])
}

/// Refuse a relative path that would leave the directory it is joined to.
///
/// `Path::join` resolves nothing and refuses nothing: `root.join("/etc/passwd")` *is*
/// `/etc/passwd`. `DATA.md` §5.4 states this rule for archive entries; it belongs on every
/// path that reaches a filesystem from data.
pub fn reject_escaping_path(path: &str) -> Result<(), CoreError> {
    use std::path::{Component, Path};
    let p = Path::new(path);
    let escapes = path.is_empty()
        || p.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        });
    if escapes {
        return Err(CoreError::PathEscapes {
            got: path.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_pass_through() {
        assert_eq!(slugify("bracket-lp-1042-03"), "bracket-lp-1042-03");
        assert_eq!(slugify("Terrain"), "Terrain");
    }

    #[test]
    fn turkish_characters_survive() {
        // DATA.md §5.1 already carries these through Content-Disposition. Stripping them
        // would make the directory unreadable to whoever named the part.
        assert_eq!(slugify("köşebent-ğ-ışık"), "köşebent-ğ-ışık");
    }

    #[test]
    fn windows_hostile_characters_become_dashes() {
        assert_eq!(slugify("Rocks?"), "Rocks-");
        assert_eq!(slugify("Rocks*"), "Rocks-");
        assert_eq!(slugify("A1234:56"), "A1234-56");
        assert_eq!(slugify("a/b\\c"), "a-b-c");
    }

    #[test]
    fn trailing_dots_and_spaces_are_trimmed() {
        // Windows silently drops them, so "bracket." and "bracket" would collide after a
        // round trip through a Windows client.
        assert_eq!(slugify("bracket."), "bracket");
        assert_eq!(slugify("bracket   "), "bracket");
        assert_eq!(slugify("  bracket  ."), "bracket");
    }

    #[test]
    fn reserved_device_names_get_a_suffix() {
        // A model legitimately called AUX is not hypothetical in a parts library.
        assert_eq!(slugify("AUX"), "AUX_");
        assert_eq!(slugify("con"), "con_");
        assert_eq!(slugify("COM4"), "COM4_");
        assert_eq!(slugify("LPT9"), "LPT9_");
        // Not reserved: only the exact names are.
        assert_eq!(slugify("COMET"), "COMET");
        assert_eq!(slugify("COM10"), "COM10");
    }

    #[test]
    fn an_empty_result_still_names_something() {
        assert_eq!(slugify(""), "unnamed");
        assert_eq!(slugify("???"), "---");
        assert_eq!(slugify("   ..."), "unnamed");
    }

    #[test]
    fn long_names_are_capped_on_a_char_boundary() {
        // Model-pack filenames run long. 120 chars leaves room inside a 255-byte
        // component for the disambiguation suffix.
        let long = "ş".repeat(200);
        let out = slugify(&long);
        assert_eq!(out.chars().count(), 120);
        // 120 two-byte chars — proves chars were counted, not bytes. A byte-truncating
        // implementation would land on a different length (and likely split a UTF-8
        // sequence besides).
        assert_eq!(
            out.len(),
            240,
            "120 two-byte chars — proves chars were counted, not bytes"
        );
    }

    #[test]
    fn disambiguate_appends_six_hex_of_the_hash() {
        let hash = crate::BlobHash::from_bytes([
            0xa1, 0xb2, 0xc3, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
            0xdd, 0xee, 0xff, 0x00,
        ]);
        assert_eq!(disambiguate("cliff", &hash), "cliff_a1b2c3");
    }

    #[test]
    fn a_relative_path_that_escapes_is_refused() {
        assert!(reject_escaping_path("Terrain/rock.stl").is_ok());
        assert!(reject_escaping_path("../../etc/passwd").is_err());
        assert!(reject_escaping_path("/etc/passwd").is_err());
        assert!(reject_escaping_path("Terrain/../../etc/passwd").is_err());
        assert!(reject_escaping_path("").is_err());
    }
}
