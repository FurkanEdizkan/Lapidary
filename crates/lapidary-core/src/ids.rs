use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

macro_rules! uuid_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a fresh, time-ordered id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }

            /// Rebuild an id from a uuid read back from storage. The inverse of
            /// `as_uuid`. Deliberately not `From<Uuid>` — three id types converting
            /// implicitly from one `Uuid` is how a `PartId` ends up where a
            /// `LibraryId` belongs.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::CoreError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|_| crate::CoreError::IdParse { got: s.to_owned() })
            }
        }
    };
}

uuid_newtype!(
    LibraryId,
    "Identifies a library. Governance is opt-in per library."
);
uuid_newtype!(PartId, "Identifies a part across all of its revisions.");
uuid_newtype!(RevisionId, "Identifies one immutable revision of a part.");
uuid_newtype!(JobId, "Identifies one unit of queued work.");
uuid_newtype!(
    BatchId,
    "Groups the jobs one scan created. A grouping column, not an entity: nothing is \
     stored under this id, so nothing under it can go stale."
);

/// A BLAKE3 content hash. Content addressing is not authorization — holding one of
/// these never implies the right to read the blob it names.
///
/// Serialises as a 64-character hex string, not as a byte array, so the wire format
/// and the generated TypeScript type agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TS)]
#[ts(export, as = "String")]
pub struct BlobHash([u8; 32]);

impl Serialize for BlobHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for BlobHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        Self::parse_hex(&hex).map_err(serde::de::Error::custom)
    }
}

impl BlobHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parse a 64-character lowercase hex digest.
    pub fn parse_hex(hex: &str) -> Result<Self, crate::CoreError> {
        if hex.len() != 64 {
            return Err(crate::CoreError::BlobHashLength { got: hex.len() });
        }
        // `u8::from_str_radix` below accepts `A-F` as well as `a-f`. Reject uppercase
        // explicitly so a digest has exactly one string form — this string keys URLs
        // and caches in a content-addressed store, so two spellings of the same blob
        // is a real bug, not a cosmetic one.
        if hex.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(crate::CoreError::BlobHashHex);
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let pair = hex
                .get(i * 2..i * 2 + 2)
                .ok_or(crate::CoreError::BlobHashLength { got: hex.len() })?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| crate::CoreError::BlobHashHex)?;
        }
        Ok(Self(bytes))
    }
}
