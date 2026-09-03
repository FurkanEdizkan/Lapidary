//! Domain types shared by every Lapidary crate. Depends on no other Lapidary crate.

mod approximate;
mod error;
mod ids;
mod job;
mod measurement;
mod part;

pub use approximate::Approximate;
pub use error::CoreError;
pub use ids::{BatchId, BlobHash, JobId, LibraryId, PartId, RevisionId};
pub use job::{BatchStatus, JobFailure, JobState, Outcome, ScanAccepted};
pub use measurement::{MeshMeasurements, Provenance};
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
    fn blob_hash_rejects_uppercase_hex() {
        let hash = BlobHash::from_bytes([0xab; 32]);
        let upper = hash.to_hex().to_ascii_uppercase();
        assert!(
            BlobHash::parse_hex(&upper).is_err(),
            "uppercase hex must be rejected so a digest has exactly one string form"
        );
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
    fn part_id_round_trips_through_uuid() {
        let id = PartId::new();
        assert_eq!(PartId::from_uuid(id.as_uuid()), id);
    }

    #[test]
    fn ids_round_trip_through_display_and_from_str() {
        let library = LibraryId::new();
        assert_eq!(
            library.to_string().parse::<LibraryId>().expect("valid id"),
            library
        );

        let part = PartId::new();
        assert_eq!(part.to_string().parse::<PartId>().expect("valid id"), part);

        let revision = RevisionId::new();
        assert_eq!(
            revision
                .to_string()
                .parse::<RevisionId>()
                .expect("valid id"),
            revision
        );
    }

    #[test]
    fn id_from_str_rejects_non_uuid() {
        let err = "not-a-uuid"
            .parse::<PartId>()
            .expect_err("not a valid uuid");
        assert!(
            err.to_string().contains("not-a-uuid"),
            "error message must name the rejected input, got: {err}"
        );
    }

    #[test]
    fn from_uuid_is_deterministic() {
        // Two ids built with `from_uuid` on the same uuid are equal: identity comes
        // from the wrapped uuid alone, not from anything tied to the call site.
        let uuid = PartId::new().as_uuid();
        let part = PartId::from_uuid(uuid);
        let other_part = PartId::from_uuid(uuid);
        assert_eq!(part, other_part);
    }

    #[test]
    fn approximate_marks_mesh_derived_values() {
        let from_brep = Approximate::analytic(20.0_f64);
        let from_mesh = Approximate::tessellated(19.987_f64);
        assert!(!from_brep.is_approximate());
        assert!(from_mesh.is_approximate());
        assert_eq!(*from_mesh.value(), 19.987);
    }

    #[test]
    fn an_open_mesh_reports_no_volume_at_all() {
        // Signed-volume integration over a non-watertight mesh returns a number that
        // means nothing. "Measurement must not lie" includes refusing to answer.
        let m = MeshMeasurements {
            bbox_mm: [88.0, 34.0, 12.0],
            triangle_count: 12_940,
            surface_area_mm2: 15_320.5,
            volume_mm3: None,
            is_watertight: false,
        };
        assert!(m.volume_approximate().is_none());
    }

    #[test]
    fn a_closed_mesh_reports_volume_as_tessellated_never_analytic() {
        let m = MeshMeasurements {
            bbox_mm: [61.0, 42.0, 18.5],
            triangle_count: 48_112,
            surface_area_mm2: 9_804.25,
            volume_mm3: Some(21_478.5),
            is_watertight: true,
        };
        let v = m
            .volume_approximate()
            .expect("watertight mesh has a volume");
        assert!(
            v.is_approximate(),
            "a mesh-derived volume is never analytic"
        );
        assert_eq!(*v.value(), 21_478.5);
    }

    #[test]
    fn provenance_round_trips_through_its_wire_form() {
        assert_eq!(Provenance::Tessellated.as_str(), "tessellated");
        assert_eq!(Provenance::Analytic.as_str(), "analytic");
        assert_eq!(
            "tessellated".parse::<Provenance>().expect("parses"),
            Provenance::Tessellated
        );
        assert!("guessed".parse::<Provenance>().is_err());
    }
}
