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

/// Filesystem component limit (ext4, NTFS and APFS all cap a single path component at
/// 255 bytes) minus the widest thing that lands on top of a capped name: the seven-byte
/// `_a1b2c3` disambiguation suffix. That headroom also covers the one-byte `_` a reserved
/// device name gets appended, so one budget serves both call sites below.
///
/// `MAX_CHARS` alone does not bound this: 120 characters of a three-byte CJK script is
/// 360 bytes, and four-byte emoji make it worse. Both limits are enforced together —
/// whichever is reached first stops the cut.
const MAX_BYTES: usize = 255 - 7;

/// A legibility cap for one- and two-byte scripts, so an ASCII or Turkish name is not cut
/// far short of `MAX_BYTES` just because a byte budget alone would allow it to run on.
/// Counts characters, not bytes, so a multi-byte name is not silently halved — but it is
/// `MAX_BYTES`, not this, that actually protects the filesystem's component limit for a
/// wider script; see the loop in `slugify` that enforces both at once.
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

    // Truncate first, bounded on characters and bytes at once, before any trimming.
    // Trimming first and then cutting at a character count can slice a chunk out of the
    // middle of the trimmed string and expose a trailing dot or space that was never at
    // the end of the input — e.g. `"bracket ".repeat(20)`, trimmed then cut at 120 chars,
    // used to end in a literal space that trimming had already passed by once.
    let mut capped = String::new();
    for (chars_taken, c) in replaced.chars().enumerate() {
        if chars_taken >= MAX_CHARS || capped.len() + c.len_utf8() > MAX_BYTES {
            break;
        }
        capped.push(c);
    }

    // Trim what the cut may have exposed, not just what the input carried: an input that
    // is all dots or spaces past the cut point trims to empty here, and the check below
    // catches that the same way it catches a short all-dots input — one fallback, not two.
    let trimmed = capped.trim().trim_end_matches(['.', ' ']).trim();

    if trimmed.is_empty() {
        return "unnamed".to_owned();
    }

    // Windows reserves the *stem*, so `AUX.stl` is refused too. Compare before any
    // extension.
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return format!("{trimmed}_");
    }

    trimmed.to_owned()
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
    fn truncation_does_not_re_expose_a_trailing_space() {
        // Reviewer-reproduced against the previous trim-then-cap order: cutting at a
        // character count after trimming can land the cut on a character that was safely
        // inside the string, exposing a trailing space the earlier trim had already
        // removed once.
        let out = slugify(&"bracket ".repeat(20));
        assert!(
            !out.ends_with(' '),
            "must not end in a space after the cut: {out:?}"
        );
    }

    #[test]
    fn an_all_dots_name_past_the_cap_still_falls_back_to_unnamed() {
        // The cut can turn a name that had one real character into a string of nothing
        // but dots — the same shape as a short all-dots input, and it must land on the
        // same fallback rather than returning "".
        assert_eq!(slugify(&(".".repeat(130) + "x")), "unnamed");
    }

    #[test]
    fn the_cap_bounds_bytes_as_well_as_characters() {
        // 120 three-byte CJK characters would be 360 bytes — well past a 255-byte
        // filesystem component limit. The byte bound must cut this well short of the
        // character bound instead of only counting characters.
        let out = slugify(&"件".repeat(200));
        assert!(
            out.len() <= MAX_BYTES,
            "{} bytes exceeds the {MAX_BYTES}-byte budget",
            out.len()
        );

        // Four-byte characters (emoji) are the same story, tighter still.
        let out = slugify(&"😀".repeat(200));
        assert!(
            out.len() <= MAX_BYTES,
            "{} bytes exceeds the {MAX_BYTES}-byte budget",
            out.len()
        );
    }

    #[test]
    fn a_reserved_name_with_a_long_tail_still_respects_the_cap_after_the_suffix() {
        // `format!("{trimmed}_")` adds one character on top of an already-capped name;
        // the result must still be bounded, not grow with however long the untruncated
        // tail happened to be.
        let out = slugify(&("AUX.".to_owned() + &"x".repeat(300)));
        assert_eq!(
            out.chars().count(),
            MAX_CHARS + 1,
            "the capped name plus its `_` suffix, not the untruncated tail: {out:?}"
        );
        assert!(out.starts_with("AUX."));
        assert!(out.ends_with('_'));
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
