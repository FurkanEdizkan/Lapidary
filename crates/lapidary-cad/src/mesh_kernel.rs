//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::cluster::ladder;
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use crate::{RASTER_VERSION, measure, parse_stl, render_thumbnail};

pub struct MeshKernel;

#[async_trait::async_trait]
impl Kernel for MeshKernel {
    fn version(&self) -> KernelVersion {
        KernelVersion {
            implementation: "mesh".to_owned(),
            version: format!("stl-1+{RASTER_VERSION}"),
        }
    }

    /// Parse once, then measure, rasterize and cluster off the one `Mesh`. Ordered
    /// cheapest-first only incidentally; what matters is that a parse failure costs no
    /// raster and no clustering, and that all four outputs describe the same geometry.
    async fn process(
        &self,
        bytes: &[u8],
        _params: &KernelParams,
    ) -> Result<KernelOutput, CadError> {
        // Task 7 dispatches on `params.format`; until the OBJ parser exists there is one
        // parser to reach for, and the only caller filtered on `.stl` before enqueueing.
        let mesh = parse_stl(bytes)?;
        Ok(KernelOutput {
            measurements: measure(&mesh),
            thumbnail_webp: render_thumbnail(&mesh)?,
            tessellations: ladder(&mesh)?,
            // Uninhabited until Phase 2's STEP ingest gives `Entity` variants. A mesh has
            // no analytic surfaces to recover, so this is the truthful answer, not a stub.
            entities: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::Lod;

    fn stl_params() -> KernelParams {
        KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
        }
    }

    #[tokio::test]
    async fn ingesting_a_real_stl_yields_measurements_and_a_thumbnail() {
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let out = MeshKernel
            .process(bytes, &stl_params())
            .await
            .expect("ingests");
        assert!(out.measurements.triangle_count > 0);
        assert!(!out.thumbnail_webp.is_empty());
    }

    #[tokio::test]
    async fn one_call_produces_the_whole_ladder_in_ascending_detail() {
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let out = MeshKernel
            .process(bytes, &stl_params())
            .await
            .expect("ingests");
        // Every rung is a real glTF file, not a placeholder: the ladder is what the
        // viewer fetches, and an empty rung is a rung that cannot be opened.
        for rung in &out.tessellations {
            assert!(!rung.glb.is_empty(), "{:?} has no bytes", rung.lod);
        }
        assert_eq!(
            out.tessellations.map(|t| t.lod),
            [Lod::L0, Lod::L1, Lod::L2],
            "the array is ordered, and consumers index it rather than search it"
        );
    }

    #[tokio::test]
    async fn the_reported_version_pins_both_the_parser_and_the_rasterizer() {
        // derivative.kernel_version must change when output bytes could change, or a
        // regenerated thumbnail is indistinguishable from a stale one.
        let v = MeshKernel.version();
        assert_eq!(v.implementation, "mesh");
        assert!(v.version.contains(crate::RASTER_VERSION));
    }

    #[tokio::test]
    async fn a_malformed_file_reports_the_parse_error_not_a_render_error() {
        let err = MeshKernel
            .process(b"not an stl at all", &stl_params())
            .await
            .expect_err("must fail");
        assert!(matches!(err, CadError::MalformedMesh { .. }));
    }
}
