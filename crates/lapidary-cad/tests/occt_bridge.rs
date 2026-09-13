//! `OcctKernel` against the real `occt-bridge` and OCCT.
//!
//! Ignored everywhere else, because it needs both. `cargo xtask verify occt` builds the
//! `occt-test` stage of `deploy/Containerfile`, which sets `LAPIDARY_OCCT_BRIDGE` and runs this
//! file with `--ignored`. The fixtures are the generated ones in `fixtures/step`; their README
//! is `sidecar/occt-bridge/README.md`.
#![cfg(feature = "occt-kernel")]

use lapidary_cad::{CadError, Entity, Kernel, KernelParams, MeasurementProvenance, OcctKernel};
use lapidary_core::DerivativeKind;
use std::time::{Duration, Instant};

fn kernel() -> OcctKernel {
    let bridge = std::env::var("LAPIDARY_OCCT_BRIDGE").unwrap_or_else(|_| {
        panic!("LAPIDARY_OCCT_BRIDGE must name occt-bridge; run cargo xtask verify occt")
    });
    OcctKernel::new(bridge, Duration::from_secs(120)).expect("the bridge answers its version")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/../../fixtures/step/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reads {path}: {e}"))
}

fn params(format: &str, produce: &[DerivativeKind]) -> KernelParams {
    KernelParams {
        linear_deflection_mm: None,
        format: format.to_owned(),
        produce: produce.to_vec(),
    }
}

fn cylinder_radii(entities: &[Entity]) -> Vec<f64> {
    entities
        .iter()
        .filter_map(|entity| match entity {
            Entity::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect()
}

const CYLINDER_VOLUME: f64 = std::f64::consts::PI * 11.0 * 11.0 * 30.0;

#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn a_22_mm_cylinder_reads_as_an_exact_cylinder() {
    let out = kernel()
        .process(
            &fixture("cylinder-d22-lp-9010-00.step"),
            &params("step", &[]),
        )
        .await
        .expect("converts");

    let volume = out
        .measurements
        .volume_mm3
        .expect("a closed solid has a volume");
    assert!(
        (volume - CYLINDER_VOLUME).abs() <= 1e-9 * CYLINDER_VOLUME,
        "volume {volume} is πr²h, not a tessellated approximation of it"
    );
    for (got, want) in out.measurements.bbox_mm.iter().zip([22.0, 22.0, 30.0]) {
        assert!(
            (got - want).abs() <= 1e-6,
            "bounding box {:?}",
            out.measurements.bbox_mm
        );
    }
    assert_eq!(out.provenance, MeasurementProvenance::ANALYTIC);
    // What the file says about itself, as the fixture generator's OCCT writer put it.
    let metadata = out.metadata.as_ref().expect("a STEP file has a header");
    assert!(
        metadata
            .schemas
            .iter()
            .any(|schema| schema.starts_with("AP242")),
        "the schema the file declares: {:?}",
        metadata.schemas
    );
    assert_eq!(
        metadata.originating_system.as_deref(),
        Some("Open CASCADE 8.0")
    );
    let radii = cylinder_radii(&out.entities);
    assert!(
        radii.iter().any(|r| (r - 11.0).abs() <= 1e-9),
        "one cylindrical face of radius 11: {radii:?}"
    );
    assert_eq!(out.structure.as_ref().map(|s| s.parts), Some(1));
}

#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn a_file_written_in_inches_is_read_in_millimetres() {
    let kernel = kernel();
    let millimetres = kernel
        .process(
            &fixture("cylinder-d22-lp-9010-00.step"),
            &params("step", &[]),
        )
        .await
        .expect("converts the mm file");
    let inches = kernel
        .process(
            &fixture("cylinder-d22-inch-units-lp-9011-00.step"),
            &params("step", &[]),
        )
        .await
        .expect("converts the inch file");

    let (mm, inch) = (
        millimetres.measurements.volume_mm3.expect("volume"),
        inches.measurements.volume_mm3.expect("volume"),
    );
    assert!(
        (mm - inch).abs() <= 1e-9 * mm,
        "the same cylinder, whatever unit it was written in: {mm} mm³ against {inch} mm³"
    );
    assert!(
        cylinder_radii(&inches.entities)
            .iter()
            .any(|r| (r - 11.0).abs() <= 1e-6),
        "radius in millimetres, not 0.433 inches"
    );
}

#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn iges_reads_and_claims_no_volume_it_does_not_have() {
    let out = kernel()
        .process(
            &fixture("angle-bracket-60x60x40-lp-9004-00.igs"),
            &params("iges", &[]),
        )
        .await
        .expect("converts");

    assert_eq!(
        out.measurements.volume_mm3, None,
        "trimmed faces that were never sewn into a solid have no volume to report"
    );
    assert_eq!(
        out.metadata
            .as_ref()
            .and_then(|metadata| metadata.originating_system.as_deref()),
        Some("Open CASCADE 8.0"),
        "the IGES global section is read as the STEP header is"
    );
    for (got, want) in out.measurements.bbox_mm.iter().zip([60.0, 40.0, 60.0]) {
        assert!(
            (got - want).abs() <= 1e-6,
            "bounding box {:?}",
            out.measurements.bbox_mm
        );
    }
    assert_eq!(out.structure.as_ref().map(|s| s.parts), Some(1));
}

#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn a_truncated_step_is_refused_not_crashed() {
    let whole = fixture("cylinder-d22-lp-9010-00.step");
    let err = kernel()
        .process(&whole[..whole.len() / 2], &params("step", &[]))
        .await
        .expect_err("half a STEP file is not a part");

    assert!(
        matches!(err, CadError::CadRefused { .. }),
        "a verdict on the file, not a kernel crash: {err:?}"
    );
}

/// `ROADMAP.md`, Phase 0: "`occt-bridge` converts a 200-part STEP assembly to glTF + tree +
/// entities in under 30 s". Timed from the bytes to the finished output — the bridge process,
/// the mesh pipeline's clustering into a GLB rung, and the thumbnail.
#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn the_phase_0_exit_converts_a_200_part_assembly_to_gltf_tree_and_entities_in_under_30_s() {
    let kernel = kernel();
    let bytes = fixture("fixture-plate-assembly-lp-9000-00.step");
    let asked = params(
        "step",
        &[DerivativeKind::Thumbnail, DerivativeKind::TessellationL0],
    );

    let started = Instant::now();
    let out = kernel.process(&bytes, &asked).await.expect("converts");
    let elapsed = started.elapsed();

    let structure = out.structure.as_ref().expect("an assembly has a tree");
    assert_eq!(structure.parts, 200, "every placed part");
    assert_eq!(structure.prototypes, 8);
    // Ingest stores the tree by serializing this value, and the page reads it back as the same
    // type. What `FakeCad` cannot show is that a real bridge tree survives the trip.
    let stored = serde_json::to_vec(structure).expect("the tree serializes");
    let read_back: lapidary_cad::AssemblyTree =
        serde_json::from_slice(&stored).expect("and reads back");
    assert_eq!(
        &read_back, structure,
        "the stored tree is the tree the bridge read"
    );
    let entities = serde_json::to_value(&out.entities).expect("the entities serialize");
    assert!(
        entities
            .as_array()
            .is_some_and(|all| all.iter().all(|entity| entity["type"].is_string())),
        "every stored entity names its kind"
    );
    let glb_bytes: usize = out.tessellations.iter().map(|rung| rung.glb.len()).sum();
    assert!(glb_bytes > 0, "glTF: the L0 rung");
    assert!(!out.entities.is_empty(), "entities");
    assert!(
        out.thumbnail_webp.is_some(),
        "and the thumbnail ingest asks for"
    );
    println!(
        "PHASE-0-EXIT elapsed_ms={} input_bytes={} parts={} prototypes={} triangles={} glb_bytes={} entities={} kernel=\"{}\"",
        elapsed.as_millis(),
        bytes.len(),
        structure.parts,
        structure.prototypes,
        out.measurements.triangle_count,
        glb_bytes,
        out.entities.len(),
        kernel.version(&asked).version,
    );
    assert!(elapsed < Duration::from_secs(30), "took {elapsed:?}");
}
