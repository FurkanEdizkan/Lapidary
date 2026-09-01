#![deny(clippy::unwrap_used)]
//! Domain types shared by every Lapidary crate. Depends on no other Lapidary crate.

mod approximate;
mod error;
mod ids;
mod part;

pub use approximate::Approximate;
pub use error::CoreError;
pub use ids::{BlobHash, LibraryId, PartId, RevisionId};
pub use part::{LibraryMode, PartSummary};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_hash_round_trips_through_hex() {
        let hash = BlobHash::from_bytes([0xab; 32]);
        let hex = hash.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(BlobHash::parse_hex(&hex).expect("valid hex"), hash);
    }

    #[test]
    fn blob_hash_rejects_wrong_length() {
        assert!(BlobHash::parse_hex("abcd").is_err());
    }

    #[test]
    fn part_summary_serialises_camel_case() {
        let now = jiff::Timestamp::now();
        let summary = PartSummary {
            id: PartId::new(),
            library: LibraryId::new(),
            name: "Bearing block, 608ZZ".to_owned(),
            part_number: Some("LP-1042-03".to_owned()),
            thumbnail: Some(BlobHash::from_bytes([0x11; 32])),
            triangle_count: Some(48_112),
            approximate: true,
            created_at: now,
            updated_at: now,
        };
        let json = serde_json::to_value(&summary).expect("serialises");
        assert!(json.get("partNumber").is_some(), "expected camelCase keys");
        assert!(json.get("part_number").is_none());
        assert!(json.get("createdAt").is_some());
        assert_eq!(
            json["thumbnail"], "1111111111111111111111111111111111111111111111111111111111111111",
            "a blob hash must go over the wire as hex, never as a byte array"
        );
    }

    #[test]
    fn approximate_marks_mesh_derived_values() {
        let from_brep = Approximate::analytic(20.0_f64);
        let from_mesh = Approximate::tessellated(19.987_f64);
        assert!(!from_brep.is_approximate());
        assert!(from_mesh.is_approximate());
        assert_eq!(*from_mesh.value(), 19.987);
    }
}
