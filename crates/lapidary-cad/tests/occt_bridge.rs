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

/// The ball knob's curved faces come back as the entities measurement snaps to: its ball as a sphere of
/// radius 10, its tip as a cone opening at 45° to its axis, and its groove as a torus of 1.5 mm section.
#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn a_ball_knob_reads_as_a_sphere_a_cone_and_a_torus() {
    let out = kernel()
        .process(
            &fixture("ball-knob-d20-lp-9020-00.step"),
            &params("step", &[]),
        )
        .await
        .expect("converts");
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9;
    assert!(
        out.entities.iter().any(|entity| matches!(entity,
            lapidary_core::Entity::Sphere { radius, .. } if near(*radius, 10.0))),
        "a sphere of radius 10: {:?}",
        out.entities
    );
    assert!(
        out.entities.iter().any(|entity| matches!(entity,
            lapidary_core::Entity::Cone { semi_angle_rad, .. }
                if near(semi_angle_rad.abs(), std::f64::consts::FRAC_PI_4))),
        "a cone at 45° to its axis: {:?}",
        out.entities
    );
    assert!(
        out.entities.iter().any(|entity| matches!(entity,
            lapidary_core::Entity::Torus { minor_radius, major_radius, .. }
                if near(*minor_radius, 1.5) && near(*major_radius, 6.0))),
        "a torus of 1.5 mm section on a 6 mm ring: {:?}",
        out.entities
    );
}

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
    assert_eq!(
        metadata.materials,
        ["Stainless steel 1.4301"],
        "the material the fixture generator attached, read back by name"
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
async fn iges_faces_are_sewn_into_the_solid_they_bound() {
    let out = kernel()
        .process(
            &fixture("angle-bracket-60x60x40-lp-9004-00.igs"),
            &params("iges", &[]),
        )
        .await
        .expect("converts");

    // Two plates of 8 mm, 60 by 40, meeting in an 8 by 8 by 40 corner counted once.
    let bracket = 60.0 * 40.0 * 8.0 + 8.0 * 40.0 * 60.0 - 8.0 * 40.0 * 8.0;
    let volume = out
        .measurements
        .volume_mm3
        .expect("the bracket's IGES faces close into a solid once sewn");
    assert!(
        (volume - bracket).abs() <= 1e-6 * bracket,
        "volume {volume} is the bracket's {bracket} mm³, integrated over the sewn B-rep"
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
    // Every node is named for a part. OCCT names an instance the file left unnamed after the
    // label it points at (`=>[0:1:1:9]`), and a tree of those is navigable only in the sense
    // that it opens: the live stack showed one on every placed part until the bridge fell back
    // to the prototype's name.
    fn unnamed(node: &lapidary_cad::AssemblyNode) -> Option<&str> {
        if node.name.is_empty() || node.name.starts_with("=>") {
            return Some(&node.name);
        }
        node.children.iter().find_map(unnamed)
    }
    assert_eq!(
        structure.roots.iter().find_map(unnamed),
        None,
        "every node is named for a part, not an OCCT label"
    );
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

/// The assembly's full-detail rung says which triangles are whose: one count per placed part, in
/// the tree's depth-first order, together every triangle of the rung. The viewer hides and
/// isolates parts by these runs, so a walk that skipped or reordered a part would hide the wrong
/// one.
#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn the_assembly_rung_counts_every_placed_parts_triangles() {
    let out = kernel()
        .process(
            &fixture("fixture-plate-assembly-lp-9000-00.step"),
            &params("step", &[DerivativeKind::TessellationL2]),
        )
        .await
        .expect("converts");
    let structure = out.structure.as_ref().expect("an assembly has a tree");
    let rung = out.tessellations.first().expect("the L2 rung");
    let json_len = u32::from_le_bytes(rung.glb[12..16].try_into().expect("four bytes")) as usize;
    let document: serde_json::Value =
        serde_json::from_slice(&rung.glb[20..20 + json_len]).expect("the JSON chunk parses");
    let parts: Vec<u32> = serde_json::from_value(document["meshes"][0]["extras"]["parts"].clone())
        .expect("the rung counts triangles per part");
    assert_eq!(
        parts.len(),
        structure.parts as usize,
        "one count per placed part"
    );
    assert_eq!(
        parts.iter().sum::<u32>(),
        rung.triangle_count,
        "and together they are the whole rung"
    );
    assert!(
        parts.iter().all(|&count| count > 0),
        "every placed part has triangles"
    );
    assert_eq!(
        out.measurements.triangle_count, 28_576,
        "walking the tree meshes exactly what the whole shape did"
    );
}

/// The PMI fixture's annotations come back as they were written, each on the face it was put on:
/// the diameter with its bounds, flatness on the top face, and perpendicularity to datum A, which
/// is the only way OCCT reads a datum at all. A file with no PMI reports none.
#[tokio::test]
#[ignore = "needs occt-bridge and OCCT: run cargo xtask verify occt"]
async fn an_ap242_files_dimensions_tolerances_and_datums_are_read_onto_their_faces() {
    let kernel = kernel();
    let asked = params("step", &[DerivativeKind::TessellationL0]);
    let out = kernel
        .process(&fixture("cylinder-d22-pmi-lp-9012-00.step"), &asked)
        .await
        .expect("converts");
    let pmi = out.pmi.as_ref().expect("the file specifies PMI");
    // What measurement reads on the face an annotation names: the kind of surface, and for a
    // plane the height it sits at.
    let surface = |named: &lapidary_core::PmiFace| {
        out.entities.iter().find_map(|entity| match entity {
            Entity::Cylinder {
                prototype, face, ..
            } if named.face == Some(*face) && *prototype == named.prototype => {
                Some(("cylinder", 0.0))
            }
            Entity::Plane {
                prototype,
                face,
                origin,
                ..
            } if named.face == Some(*face) && *prototype == named.prototype => {
                Some(("plane", origin[2]))
            }
            _ => None,
        })
    };

    let [diameter] = pmi.dimensions.as_slice() else {
        panic!("one dimension, got {:?}", pmi.dimensions)
    };
    assert_eq!(
        (
            diameter.kind.as_str(),
            diameter.value,
            diameter.upper,
            diameter.lower
        ),
        ("diameter", 22.0, Some(0.05), Some(0.0))
    );
    assert_eq!(surface(&diameter.faces[0]), Some(("cylinder", 0.0)));

    let tolerances: Vec<(&str, f64, &[String])> = pmi
        .tolerances
        .iter()
        .map(|t| (t.kind.as_str(), t.value, t.datums.as_slice()))
        .collect();
    assert_eq!(
        tolerances,
        [
            ("flatness", 0.02, &[][..]),
            ("perpendicularity", 0.05, &["A".to_owned()][..])
        ]
    );
    assert_eq!(
        surface(&pmi.tolerances[0].faces[0]),
        Some(("plane", 30.0)),
        "flatness is on the top face"
    );
    assert_eq!(
        surface(&pmi.tolerances[1].faces[0]),
        Some(("cylinder", 0.0)),
        "and perpendicularity on the cylinder"
    );

    let [datum] = pmi.datums.as_slice() else {
        panic!("one datum, got {:?}", pmi.datums)
    };
    assert_eq!(datum.name, "A");
    assert_eq!(
        surface(&datum.faces[0]),
        Some(("plane", 0.0)),
        "datum A is the base"
    );

    let plain = kernel
        .process(&fixture("cylinder-d22-lp-9010-00.step"), &asked)
        .await
        .expect("converts");
    assert_eq!(plain.pmi, None, "a file with no PMI reports none");
}
