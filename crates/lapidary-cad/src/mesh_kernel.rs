//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::cluster::{Lod, cluster};
use crate::glb::GLB_VERSION;
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use crate::stl::Mesh;
use crate::{RASTER_VERSION, measure, parse_3mf, parse_obj, parse_stl, render_thumbnail};
use lapidary_core::DerivativeKind;

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
        "3mf" => parse_3mf(bytes),
        // Unreachable through the scan, which admits only the three above. It is an error
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
    ///
    /// `RASTER_VERSION` is always in this string, even for a call whose `params.produce`
    /// asks for no thumbnail. This names the build, not the run: a worker running a
    /// different kernel version must not produce derivatives that are cached as
    /// equivalent, whether or not this particular call happened to render one.
    fn version(&self, params: &KernelParams) -> KernelVersion {
        KernelVersion {
            implementation: "mesh".to_owned(),
            version: format!(
                "{}-1+{GLB_VERSION}+{RASTER_VERSION}",
                params.format.to_ascii_lowercase()
            ),
        }
    }

    /// Parse once, then measure unconditionally and produce only what `params.produce`
    /// asks for off the one `Mesh`. Measurement always runs — it is what ingest needs to
    /// decide anything at all — but a thumbnail render or a tessellation rung the caller
    /// did not ask for is one that never runs.
    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        let mesh = parse(bytes, &params.format)?;
        let mut tessellations = Vec::new();
        let mut thumbnail_webp = None;
        for want in &params.produce {
            match want {
                DerivativeKind::Thumbnail => thumbnail_webp = Some(render_thumbnail(&mesh)?),
                DerivativeKind::TessellationL0 => tessellations.push(cluster(&mesh, Lod::L0)?),
                DerivativeKind::TessellationL1 => tessellations.push(cluster(&mesh, Lod::L1)?),
                DerivativeKind::TessellationL2 => tessellations.push(cluster(&mesh, Lod::L2)?),
            }
        }
        Ok(KernelOutput {
            measurements: measure(&mesh),
            thumbnail_webp,
            tessellations,
            // Uninhabited until Phase 2's STEP ingest gives `Entity` variants. A mesh has
            // no analytic surfaces to recover, so this is the truthful answer, not a stub.
            entities: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(format: &str) -> KernelParams {
        KernelParams {
            linear_deflection_mm: None,
            format: format.to_owned(),
            produce: DerivativeKind::ALL.to_vec(),
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
        assert!(out.thumbnail_webp.is_some_and(|w| !w.is_empty()));
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
        let lods: Vec<Lod> = out.tessellations.iter().map(|t| t.lod).collect();
        assert_eq!(
            lods,
            vec![Lod::L0, Lod::L1, Lod::L2],
            "produce asked for all three in ascending order, and the output preserves it"
        );
    }

    #[tokio::test]
    async fn asking_for_one_rung_produces_exactly_that_rung_and_no_thumbnail() {
        // The bug this whole slice removes: a kernel that built everything regardless of
        // `produce`. Asking for one rung must return one rung, and no thumbnail at all.
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: vec![DerivativeKind::TessellationL1],
        };
        let out = MeshKernel.process(bytes, &params).await.expect("ingests");
        assert_eq!(out.tessellations.len(), 1);
        assert_eq!(out.tessellations[0].lod, Lod::L1);
        assert!(out.thumbnail_webp.is_none());
    }

    #[tokio::test]
    async fn asking_for_nothing_still_measures() {
        // `KernelParams::produce` promises that empty is legal and means measurements
        // only. The measurement assertions are the point of this test, not decoration:
        // without them it would pass equally against a `process` that saw an empty
        // `produce`, returned early and measured nothing — and measurement is the one
        // thing ingest needs in order to decide anything at all.
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: Vec::new(),
        };
        let out = MeshKernel.process(bytes, &params).await.expect("ingests");
        assert!(out.tessellations.is_empty());
        assert!(out.thumbnail_webp.is_none());
        assert!(out.measurements.triangle_count > 0);
        assert!(out.measurements.surface_area_mm2 > 0.0);
        assert!(
            out.measurements.bbox_mm.iter().all(|mm| *mm > 0.0),
            "a real bracket has extent on all three axes, got {:?}",
            out.measurements.bbox_mm
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
    async fn a_3mf_file_is_parsed_by_the_3mf_parser() {
        let bytes = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");
        let out = MeshKernel
            .process(bytes, &params("3mf"))
            .await
            .expect("ingests");
        assert!(out.measurements.triangle_count > 0);
        assert!(out.thumbnail_webp.is_some_and(|w| !w.is_empty()));
    }

    #[tokio::test]
    async fn the_three_formats_report_three_versions() {
        // slice 3 §3.7's correctness rule, now with a third parser: a derivative's
        // kernel_version must say which parser produced it.
        let versions = [
            MeshKernel.version(&params("stl")).version,
            MeshKernel.version(&params("obj")).version,
            MeshKernel.version(&params("3mf")).version,
        ];
        assert_eq!(
            versions[2],
            format!("3mf-1+{GLB_VERSION}+{}", crate::RASTER_VERSION)
        );
        let unique: std::collections::BTreeSet<&String> = versions.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "each parser needs its own version: {versions:?}"
        );
    }

    #[tokio::test]
    async fn a_format_with_no_parser_is_an_error_rather_than_a_panic() {
        // "step" stands in for any format the scan cannot yet route here: 3MF gained a
        // parser this task, so it can no longer serve as the unsupported case.
        let err = MeshKernel
            .process(b"anything", &params("step"))
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
