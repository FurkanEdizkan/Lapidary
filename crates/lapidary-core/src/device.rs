//! How one Lapidary installation names another.
//!
//! A device id is the BLAKE3 of an installation's public key, written in Crockford base32. It is
//! what two people paste to each other to pair (`docs/ROADMAP.md`, "Shared libraries"), and it is
//! the whole of the credential: the peer listener accepts a connection only from a key whose digest
//! is one the owner added by hand. So it is an identifier rather than a secret — knowing one grants
//! nothing, exactly as knowing a blob hash grants nothing.
//!
//! Crockford base32, not hex and not standard base32: it is read off one screen and typed into
//! another, so `I`, `L` and `O` fold onto `1` and `0`, case does not matter, and the hyphens are
//! there to be read rather than to be stored.

use crate::CoreError;
use serde::{Deserialize, Serialize};

/// Crockford's alphabet: the digits, then the letters with `I`, `L`, `O` and `U` left out.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 32 bytes at 5 bits a character, rounded up: 51 full characters and one holding the last bit.
const CHARS: usize = 52;

/// How many characters between hyphens when the id is shown.
const GROUP: usize = 5;

/// Identifies one Lapidary installation to the people it shares with.
///
/// The digest of that installation's public key, so two installations can carry the same name and
/// still never be confused for one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct DeviceId([u8; 32]);

impl DeviceId {
    /// The id of the installation holding this public key.
    pub fn from_public_key(key: &[u8]) -> Self {
        Self(*blake3::hash(key).as_bytes())
    }

    /// Rebuild an id from the digest itself, as it was stored.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest, for comparing against a key presented during a handshake.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut in_group = 0;
        for index in 0..CHARS {
            // Five bits starting here, read big-endian across the byte boundary. The last
            // character holds the one bit left over, padded with zeros.
            let bit = index * 5;
            let byte = bit / 8;
            let offset = bit % 8;
            let mut window = u16::from(self.0[byte]) << 8;
            if byte + 1 < self.0.len() {
                window |= u16::from(self.0[byte + 1]);
            }
            let value = usize::from((window >> (11 - offset)) & 0b1_1111);
            if in_group == GROUP {
                f.write_str("-")?;
                in_group = 0;
            }
            write!(f, "{}", char::from(ALPHABET[value]))?;
            in_group += 1;
        }
        Ok(())
    }
}

impl std::str::FromStr for DeviceId {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // The hyphens and any spaces are for reading; what was typed is what is left.
        let typed: Vec<char> = s
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .collect();
        if typed.len() != CHARS {
            return Err(CoreError::DeviceIdLength {
                got: typed.len(),
                expected: CHARS,
            });
        }
        let mut digest = [0u8; 32];
        for (index, raw) in typed.into_iter().enumerate() {
            // Crockford reads the lookalikes as the digits they resemble. `U` is left out of the
            // alphabet altogether and folds onto nothing, so it is refused by name below.
            let folded = match raw.to_ascii_uppercase() {
                'I' | 'L' => '1',
                'O' => '0',
                other => other,
            };
            let value = ALPHABET
                .iter()
                .position(|candidate| char::from(*candidate) == folded)
                .ok_or(CoreError::DeviceIdCharacter { got: raw })?;
            // The last character carries one bit of the digest and four that only pad it out to
            // five. Those four must be zero: ignoring them instead would let `…ZZZZZ` and
            // `…ZZZZG` name one machine, and an id is what a person compares by eye before
            // pairing. Refusing them also keeps every bit below in range.
            if index == CHARS - 1 && value & 0b0_1111 != 0 {
                return Err(CoreError::DeviceIdPadding { got: raw });
            }
            // The same five bits `Display` wrote, put back where they came from.
            for step in 0..5 {
                if value & (0b1_0000 >> step) != 0 {
                    let at = index * 5 + step;
                    digest[at / 8] |= 0x80 >> (at % 8);
                }
            }
        }
        Ok(Self(digest))
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for DeviceId {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// The public key of a Lapidary install, as `ring` hands one over: 32 bytes of Ed25519.
    const KEY: &[u8] = b"\x3b\x6a\x27\xbc\xce\xb6\xa4\x2d\x62\xa3\xa8\xd0\x2a\x6f\x0d\x73\
\x65\x32\x15\x77\x1d\xe2\x43\xa6\x3a\xc0\x48\xa1\x8b\x59\xda\x29";

    #[test]
    fn the_id_is_the_blake3_of_the_public_key() {
        assert_eq!(
            DeviceId::from_public_key(KEY).as_bytes(),
            blake3::hash(KEY).as_bytes(),
            "the id is the digest of the key and nothing else"
        );
    }

    /// Hand-checkable: every bit zero is every character `0`.
    #[test]
    fn all_zero_bytes_are_all_zero_characters() {
        let shown = DeviceId::from_bytes([0; 32]).to_string();
        assert_eq!(shown.replace('-', "").len(), CHARS);
        assert_eq!(shown.replace('-', ""), "0".repeat(CHARS));
        assert_eq!(&shown[..6], "00000-", "shown in groups of five");
    }

    /// Hand-checkable the other way: 256 bits of ones is 51 characters of `Z` (five ones each),
    /// and one bit left over, padded with four zeros to `0b10000` — Crockford's `G`.
    #[test]
    fn all_one_bits_end_in_the_padded_character() {
        let shown = DeviceId::from_bytes([0xFF; 32])
            .to_string()
            .replace('-', "");
        assert_eq!(shown, format!("{}G", "Z".repeat(CHARS - 1)));
    }

    #[test]
    fn a_device_id_round_trips_through_its_text() {
        let id = DeviceId::from_public_key(KEY);
        assert_eq!(DeviceId::from_str(&id.to_string()), Ok(id));
    }

    /// It is read off one screen and typed into another, so the characters that look alike are
    /// read as each other, case is ignored, and the hyphens are optional.
    #[test]
    fn reading_one_back_forgives_what_a_person_mistypes() {
        let id = DeviceId::from_bytes([0; 32]);
        let typed = id.to_string().to_lowercase().replace('0', "o");
        assert_eq!(DeviceId::from_str(&typed), Ok(id), "O reads as zero");
        assert_eq!(
            DeviceId::from_str(&id.to_string().replace('-', "")),
            Ok(id),
            "the hyphens are for reading, not for storing"
        );
        let ones = DeviceId::from_bytes([0xFF; 32]);
        let typed = ones.to_string().replace('1', "I");
        assert_eq!(DeviceId::from_str(&typed), Ok(ones), "I reads as one");
    }

    #[test]
    fn a_device_id_of_the_wrong_length_says_so() {
        let err = DeviceId::from_str("00000-00000").expect_err("too short");
        assert!(
            err.to_string().contains("52"),
            "the refusal says how long one is: {err}"
        );
    }

    /// The final character holds one bit of the digest and four that pad it out. An id whose
    /// padding bits are set is not one any Lapidary printed, and accepting it would let two
    /// different-looking ids name one machine.
    #[test]
    fn a_last_character_carrying_padding_bits_is_refused() {
        let printed = DeviceId::from_bytes([0xFF; 32]).to_string();
        let mut bent = printed.clone();
        bent.pop();
        bent.push('Z');
        let err = DeviceId::from_str(&bent).expect_err("no machine prints this");
        assert!(
            err.to_string().contains('Z'),
            "the refusal names the character: {err}"
        );
        assert_eq!(
            DeviceId::from_str(&printed),
            Ok(DeviceId::from_bytes([0xFF; 32])),
            "the one it does print still reads back"
        );
    }

    #[test]
    fn a_character_crockford_leaves_out_is_refused() {
        // `U` is not in the alphabet at all, and folds onto nothing.
        let bad = "U".repeat(CHARS);
        let err = DeviceId::from_str(&bad).expect_err("not Crockford base32");
        assert!(
            err.to_string().contains('U'),
            "the refusal names what it could not read: {err}"
        );
    }
}
