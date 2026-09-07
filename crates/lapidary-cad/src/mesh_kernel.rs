//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::cluster::{Lod, cluster};
use crate::glb::GLB_VERSION;
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion, Unproduced};
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
        // **Parsing is fatal; producing a picture of what parsed is not.** A file we cannot
        // read has no measurements and no part to hang them on, so this `?` stays. Below it,
        // a derivative that cannot be made is recorded as not made — because a mesh that
        // parses is a model, and losing the model because its preview could not be drawn is
        // the wrong way round. `ROADMAP.md`'s exit criterion says so in as many words, and a
        // real corpus produced exactly one such file in 1,095.
        let mesh = parse(bytes, &params.format)?;
        let mut tessellations = Vec::new();
        let mut thumbnail_webp = None;
        let mut unproduced = Vec::new();
        for want in &params.produce {
            let made = match want {
                DerivativeKind::Thumbnail => render_thumbnail(&mesh).map(|webp| {
                    thumbnail_webp = Some(webp);
                }),
                DerivativeKind::TessellationL0 => cluster(&mesh, Lod::L0).map(|rung| {
                    tessellations.push(rung);
                }),
                DerivativeKind::TessellationL1 => cluster(&mesh, Lod::L1).map(|rung| {
                    tessellations.push(rung);
                }),
                DerivativeKind::TessellationL2 => cluster(&mesh, Lod::L2).map(|rung| {
                    tessellations.push(rung);
                }),
            };
            if let Err(reason) = made {
                unproduced.push(Unproduced {
                    kind: *want,
                    reason: reason.to_string(),
                });
            }
        }
        Ok(KernelOutput {
            measurements: measure(&mesh),
            thumbnail_webp,
            tessellations,
            unproduced,
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

    /// A binary STL of one degenerate triangle: three vertices at the same point.
    ///
    /// Parses perfectly — the header, the count and the record are all well formed — and has
    /// no extent, so nothing can be drawn of it. That is the shape of the file that failed
    /// in the owner's corpus, built here rather than committed so it can be read.
    fn zero_extent_stl() -> Vec<u8> {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // Normal, then three identical vertices, then the attribute byte count.
        for _ in 0..12 {
            bytes.extend_from_slice(&0f32.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    /// **A mesh that parses is a model, even when no picture of it can be drawn.**
    ///
    /// This used to be an error, and the caller's only option was to fail the ingest — so a
    /// file like this was absent from the library rather than present with no preview.
    /// `ROADMAP.md`'s Phase 1 exit criterion asks for the opposite in as many words, and a
    /// measured run over 1,095 real files found exactly one of these.
    #[tokio::test]
    async fn a_mesh_no_picture_can_be_made_of_still_yields_its_measurements() {
        let out = MeshKernel
            .process(&zero_extent_stl(), &stl_params())
            .await
            .expect("a file that parses is not an error, whatever can be drawn of it");

        assert_eq!(
            out.measurements.triangle_count, 1,
            "the geometry is real and measured, which is what makes it a part"
        );
        assert!(out.thumbnail_webp.is_none(), "and no picture was made");
        assert!(out.tessellations.is_empty(), "nor any rung");
    }

    /// What could not be made is reported rather than dropped, with the kind and the reason —
    /// so an operator wondering why one card has no picture can find out.
    #[tokio::test]
    async fn what_could_not_be_produced_is_named_along_with_why() {
        let out = MeshKernel
            .process(&zero_extent_stl(), &stl_params())
            .await
            .expect("parses");

        let kinds: Vec<_> = out.unproduced.iter().map(|u| u.kind).collect();
        assert!(
            kinds.contains(&DerivativeKind::Thumbnail),
            "the thumbnail is named: {kinds:?}"
        );
        assert!(
            out.unproduced.iter().all(|u| !u.reason.is_empty()),
            "and each carries the kernel's own sentence about it"
        );
        // The message no longer claims "thumbnail" for a rung that could not be written:
        // `Unrenderable` is raised by the glTF writer too, and said so for years.
        assert!(
            out.unproduced
                .iter()
                .all(|u| !u.reason.contains("Could not render a thumbnail")),
            "the wording is about a view of the mesh, and the kind says which: {:?}",
            out.unproduced
        );
    }

    /// A readable file is still fatal when it is not readable. The `?` on `parse` stays.
    #[tokio::test]
    async fn a_file_that_does_not_parse_is_still_an_error() {
        let out = MeshKernel
            .process(b"this is not an STL at all", &stl_params())
            .await;
        assert!(
            out.is_err(),
            "no mesh means no measurements and no part to hang them on"
        );
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
