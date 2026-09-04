//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::cluster::ladder;
use crate::glb::GLB_VERSION;
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use crate::stl::Mesh;
use crate::{RASTER_VERSION, measure, parse_obj, parse_stl, render_thumbnail};

pub struct MeshKernel;

/// Dispatch is on the format the scan already selected the file by, never on a byte
/// sniff. OBJ has no magic number, so sniffing reduces to guessing from the first
/// non-comment line — and the extension is what the walk filtered on, so it is what the
/// parser is told. A file whose extension lies fails to parse, which is the same
/// treatment a corrupt STL already gets.
fn parse(bytes: &[u8], format: &str) -> Result<Mesh, CadError> {
    match format.to_ascii_lowercase().as_str() {
        "stl" => parse_stl(bytes),
        "obj" => parse_obj(bytes),
        // Unreachable through the scan, which admits only the two above. It is an error
        // rather than a panic because the alternative to a per-file `Permanent` failure
        // is a worker that dies on one misrouted job.
        other => Err(CadError::UnsupportedFormat {
            format: other.to_owned(),
        }),
    }
}

#[async_trait::async_trait]
impl Kernel for MeshKernel {
    /// `{parser}-1+{writer}+{rasterizer}` — three things that can change output bytes
    /// independently, each named. The parser is in there because an OBJ-derived
    /// derivative and an STL-derived one are different bytes from different code, and a
    /// version that says `stl-1` for both makes a regenerated derivative
    /// indistinguishable from a stale one.
    fn version(&self, params: &KernelParams) -> KernelVersion {
        KernelVersion {
            implementation: "mesh".to_owned(),
            version: format!(
                "{}-1+{GLB_VERSION}+{RASTER_VERSION}",
                params.format.to_ascii_lowercase()
            ),
        }
    }

    /// Parse once, then measure, rasterize and cluster off the one `Mesh`. Ordered
    /// cheapest-first only incidentally; what matters is that a parse failure costs no
    /// raster and no clustering, and that all four outputs describe the same geometry.
    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        let mesh = parse(bytes, &params.format)?;
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

    fn params(format: &str) -> KernelParams {
        KernelParams {
            linear_deflection_mm: None,
            format: format.to_owned(),
        }
    }

    fn stl_params() -> KernelParams {
        params("stl")
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
    async fn the_reported_version_pins_the_parser_the_writer_and_the_rasterizer() {
        // derivative.kernel_version must change when output bytes could change, or a
        // regenerated thumbnail is indistinguishable from a stale one. Three things can
        // change them independently, so all three are named.
        let v = MeshKernel.version(&stl_params());
        assert_eq!(v.implementation, "mesh");
        assert_eq!(
            v.version,
            format!("stl-1+{GLB_VERSION}+{}", crate::RASTER_VERSION)
        );
    }

    #[tokio::test]
    async fn two_formats_report_two_versions() {
        // The correctness case: the same part ingested as STL and as OBJ produces
        // different derivative bytes from different code, so the two must be tellable
        // apart by the column that exists to tell them apart.
        assert_ne!(
            MeshKernel.version(&params("obj")).version,
            MeshKernel.version(&stl_params()).version
        );
    }

    #[tokio::test]
    async fn an_obj_file_is_parsed_by_the_obj_parser() {
        let bytes = include_bytes!("../../../fixtures/idler-bracket-lp-2210-01.obj");
        let out = MeshKernel
            .process(bytes, &params("obj"))
            .await
            .expect("ingests");
        assert_eq!(out.measurements.triangle_count, 20);
    }

    #[tokio::test]
    async fn a_format_with_no_parser_is_an_error_rather_than_a_panic() {
        let err = MeshKernel
            .process(b"PK\x03\x04", &params("3mf"))
            .await
            .expect_err("must fail");
        assert!(matches!(err, CadError::UnsupportedFormat { .. }));
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
