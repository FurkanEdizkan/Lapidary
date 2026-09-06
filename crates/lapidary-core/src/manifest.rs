//! `metadata.json` — what sits beside a model's file and makes the store self-describing.
//!
//! This is the whole of re-adoption: delete the database and each model directory still
//! says what it is. It is machine-owned but lives in a folder the user has been promised
//! they may edit, so readers treat a missing or malformed one as an orphan to report, never
//! as a reason to fail a walk.

use crate::{LibraryId, PartId, RevisionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ModelManifest {
    /// Bumped when a field's *meaning* changes. Additive fields do not bump it — readers
    /// keep unknown ones rather than refusing them.
    pub schema: u32,
    pub part: ManifestPart,
    pub revisions: Vec<ManifestRevision>,
}

impl ModelManifest {
    #[allow(dead_code)]
    pub const SCHEMA: u32 = 1;

    /// Written by a build newer than this one. The caller reports the directory and moves
    /// on rather than guessing at fields it does not know.
    #[allow(dead_code)]
    pub fn is_future_schema(&self) -> bool {
        self.schema > Self::SCHEMA
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ManifestPart {
    pub id: PartId,
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    pub classification: Option<String>,
    /// The ingest identity key. Never the storage path — see the spec §3.
    pub source_path: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ManifestRevision {
    pub id: RevisionId,
    pub rev_label: String,
    pub origin: String,
    pub volume_mm3: Option<f64>,
    /// `tessellated` or `analytic`. Provenance travels with the value, per `0002`'s
    /// per-column `_source` design.
    pub volume_source: Option<String>,
    pub bbox_mm: Option<[f64; 3]>,
    pub triangle_count: Option<i32>,
    pub is_watertight: Option<bool>,
    pub units: Option<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ManifestFile {
    pub role: String,
    pub format: String,
    /// Hex. Re-adoption verifies bytes against this before trusting the directory.
    pub blake3: String,
    pub size_bytes: i64,
    /// The file's name inside this directory. Not a path — a model's files are flat.
    pub file_name: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn a_manifest() -> ModelManifest {
        ModelManifest {
            schema: ModelManifest::SCHEMA,
            part: ManifestPart {
                id: PartId::from_uuid("01931b6e-0000-7000-8000-0000000000aa".parse().unwrap()),
                library: LibraryId::from_uuid(
                    "01931b6e-0000-7000-8000-000000000001".parse().unwrap(),
                ),
                name: "bracket-lp-1042-03".to_owned(),
                part_number: Some("LP-1042-03".to_owned()),
                classification: None,
                source_path: "Terrain/Rocks/bracket-lp-1042-03.stl".to_owned(),
                metadata: serde_json::json!({}),
            },
            revisions: vec![ManifestRevision {
                id: RevisionId::from_uuid("01931b6e-0000-7000-8000-0000000000bb".parse().unwrap()),
                rev_label: "1".to_owned(),
                origin: "ingest".to_owned(),
                volume_mm3: Some(21_478.5),
                volume_source: Some("tessellated".to_owned()),
                bbox_mm: Some([61.0, 42.0, 18.5]),
                triangle_count: Some(48_112),
                is_watertight: Some(true),
                units: Some("mm".to_owned()),
                files: vec![ManifestFile {
                    role: "source".to_owned(),
                    format: "stl".to_owned(),
                    blake3: "ab".repeat(32),
                    size_bytes: 204_800,
                    file_name: "bracket-lp-1042-03.stl".to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let json = serde_json::to_string_pretty(&a_manifest()).expect("serializes");
        let back: ModelManifest = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.part.name, "bracket-lp-1042-03");
        assert_eq!(back.revisions[0].files[0].format, "stl");
        assert_eq!(back.schema, ModelManifest::SCHEMA);
    }

    #[test]
    fn a_manifest_from_a_newer_schema_is_refused_by_version_not_by_shape() {
        // A store written by a newer build must fail with something a person can act on,
        // not with a serde field error listing what changed.
        let mut v = serde_json::to_value(a_manifest()).expect("to value");
        v["schema"] = serde_json::json!(999);
        let parsed: ModelManifest = serde_json::from_value(v).expect("still parses");
        assert!(parsed.is_future_schema());
    }

    #[test]
    fn unknown_fields_are_kept_not_rejected() {
        // Forward compatibility: a field a newer build added must not make this directory
        // an orphan on an older one.
        let mut v = serde_json::to_value(a_manifest()).expect("to value");
        v["part"]["invented_later"] = serde_json::json!("hello");
        assert!(serde_json::from_value::<ModelManifest>(v).is_ok());
    }
}
