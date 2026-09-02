//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::{CadError, KernelVersion, RASTER_VERSION, measure, parse_stl, render_thumbnail};
use lapidary_core::MeshMeasurements;

#[derive(Debug)]
pub struct MeshOutput {
    pub measurements: MeshMeasurements,
    pub thumbnail_webp: Vec<u8>,
}

pub struct MeshKernel;

impl MeshKernel {
    /// Bytes rather than a path: ingest has already read and hashed the file, and
    /// reading it twice would be a second chance to read something different.
    pub fn ingest(&self, bytes: &[u8]) -> Result<MeshOutput, CadError> {
        let mesh = parse_stl(bytes)?;
        Ok(MeshOutput {
            measurements: measure(&mesh),
            thumbnail_webp: render_thumbnail(&mesh)?,
        })
    }

    pub fn version(&self) -> KernelVersion {
        KernelVersion {
            implementation: "mesh".to_owned(),
            version: format!("stl-1+{RASTER_VERSION}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingesting_a_real_stl_yields_measurements_and_a_thumbnail() {
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let out = MeshKernel.ingest(bytes).expect("ingests");
        assert!(out.measurements.triangle_count > 0);
        assert!(!out.thumbnail_webp.is_empty());
    }

    #[test]
    fn the_reported_version_pins_both_the_parser_and_the_rasterizer() {
        // derivative.kernel_version must change when output bytes could change, or a
        // regenerated thumbnail is indistinguishable from a stale one.
        let v = MeshKernel.version();
        assert_eq!(v.implementation, "mesh");
        assert!(v.version.contains(crate::RASTER_VERSION));
    }

    #[test]
    fn a_malformed_file_reports_the_parse_error_not_a_render_error() {
        let err = MeshKernel
            .ingest(b"not an stl at all")
            .expect_err("must fail");
        assert!(matches!(err, CadError::MalformedStl { .. }));
    }
}
