//! `OcctKernel`: the CAD kernel, which runs `occt-bridge` as a subprocess.
//!
//! A process and not a linked library, so an OCCT crash takes down one job instead of the
//! worker (`sidecar/occt-bridge/README.md`). What crosses the boundary is files: the input
//! bytes go into a scratch directory, and `occt-bridge convert` writes four files back —
//! `mesh.stl`, `parts.json`, `measurements.json`, `entities.json` and `structure.json`.
//!
//! The mesh goes through [`MeshKernel`] unchanged, so a STEP part's LOD rungs and thumbnail
//! come from the same clustering, rasterizer and GLB writer as an STL's. What the B-rep knows
//! better than a mesh then replaces what the mesh said: volume, surface area and bounding box,
//! marked analytic. The triangle count stays the mesh's, because it is one.
//!
//! Three ways out besides success, kept distinct because they are handled differently:
//! a **refusal** (exit 2) is a verdict on the file, and every attempt gets the same one; a
//! **crash** (any other failure, or a signal) says nothing about the file and is worth
//! retrying; a **timeout** kills the bridge — `kill_on_drop` — rather than leaving it to
//! finish a job nobody is waiting for.

use crate::kernel::{
    AssemblyTree, CadError, CadMetadata, Entity, Kernel, KernelOutput, KernelParams, KernelVersion,
    MeasurementProvenance,
};
use crate::{GLB_VERSION, RASTER_VERSION};
use lapidary_core::Provenance;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

/// The bridge's exit code for a file it read and refused. Every other failure is a crash.
const REFUSED_EXIT: i32 = 2;

/// Linear deflection when a job does not ask for one, in millimetres.
const DEFAULT_DEFLECTION_MM: f64 = 0.1;

/// How much of a crashed bridge's stderr is kept for the message a person reads.
const STDERR_TAIL_BYTES: usize = 600;

pub struct OcctKernel {
    bridge: PathBuf,
    timeout: Duration,
    /// `occt-bridge version`, read once: `occt 8.0.1 bridge 1`.
    bridge_version: String,
}

impl OcctKernel {
    /// Asks the bridge its version before accepting any work, so a worker whose image lacks
    /// the bridge or its libraries fails at startup rather than on its first STEP file.
    pub fn new(bridge: impl Into<PathBuf>, timeout: Duration) -> Result<Self, CadError> {
        let bridge = bridge.into();
        let output = std::process::Command::new(&bridge)
            .arg("version")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| CadError::KernelUnavailable {
                detail: format!("could not run {}: {e}", bridge.display()),
            })?;
        let bridge_version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || bridge_version.is_empty() {
            return Err(CadError::KernelUnavailable {
                detail: format!(
                    "{} version answered {} with {:?}",
                    bridge.display(),
                    describe(output.status),
                    tail(&output.stderr)
                ),
            });
        }
        Ok(Self {
            bridge,
            timeout,
            bridge_version,
        })
    }
}

#[async_trait::async_trait]
impl Kernel for OcctKernel {
    /// The OCCT build, the bridge, the deflection and the mesh pipeline's own versions: two
    /// workers that would tessellate the same file differently must not report the same
    /// version, or diffs between revisions show phantom deltas (`docs/ARCHITECTURE.md`).
    fn version(&self, params: &KernelParams) -> KernelVersion {
        KernelVersion {
            implementation: "occt".to_owned(),
            version: format!(
                "{}+deflection-{}+{GLB_VERSION}+{RASTER_VERSION}",
                self.bridge_version.replace(' ', "-"),
                params.linear_deflection_mm.unwrap_or(DEFAULT_DEFLECTION_MM)
            ),
        }
    }

    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError> {
        let format = params.format.to_ascii_lowercase();
        let bridge_format = match format.as_str() {
            "step" | "stp" => "step",
            "iges" | "igs" => "iges",
            _ => return Err(CadError::UnsupportedFormat { format }),
        };
        let named = bridge_format.to_ascii_uppercase();

        let scratch = tempfile::tempdir().map_err(|e| CadError::KernelUnavailable {
            detail: format!("could not create a scratch directory: {e}"),
        })?;
        let input = scratch.path().join(format!("input.{format}"));
        let out_dir = scratch.path().join("out");
        let prepared = async {
            tokio::fs::write(&input, bytes).await?;
            tokio::fs::create_dir(&out_dir).await
        };
        prepared.await.map_err(|e| CadError::KernelUnavailable {
            detail: format!("could not write the input into the scratch directory: {e}"),
        })?;

        let deflection = params.linear_deflection_mm.unwrap_or(DEFAULT_DEFLECTION_MM);
        let run = tokio::process::Command::new(&self.bridge)
            .arg("convert")
            .arg("--in")
            .arg(&input)
            .args(["--format", bridge_format, "--out"])
            .arg(&out_dir)
            .arg("--deflection")
            .arg(deflection.to_string())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(self.timeout, run).await {
            Err(_) => {
                return Err(CadError::Timeout {
                    path: format!("this {named} file"),
                    seconds: self.timeout.as_secs(),
                });
            }
            Ok(Err(e)) => {
                return Err(CadError::KernelUnavailable {
                    detail: format!("could not start {}: {e}", self.bridge.display()),
                });
            }
            Ok(Ok(output)) => output,
        };
        if output.status.code() == Some(REFUSED_EXIT) {
            return Err(CadError::CadRefused {
                format: named,
                detail: refusal_detail(&output.stderr),
            });
        }
        if !output.status.success() {
            return Err(CadError::KernelCrashed {
                format: named,
                status: describe(output.status),
                stderr_tail: tail(&output.stderr),
            });
        }

        let stl = tokio::fs::read(out_dir.join("mesh.stl"))
            .await
            .map_err(|e| unreadable("mesh.stl", e))?;
        let mut mesh = crate::parse_stl(&stl)?;
        // Which triangles are whose. The viewer hides and isolates parts by these runs, so counts
        // that do not add up to the mesh are refused rather than trusted to be close.
        let parts: Vec<u32> = read_json(&out_dir, "parts.json").await?;
        let counted: u64 = parts.iter().map(|&count| u64::from(count)).sum();
        if counted != mesh.triangles.len() as u64 {
            return Err(unreadable(
                "parts.json",
                format!(
                    "it counts {counted} triangles and mesh.stl holds {}",
                    mesh.triangles.len()
                ),
            ));
        }
        mesh.parts = parts;
        let mut out = crate::mesh_kernel::produce(
            &mesh,
            &KernelParams {
                linear_deflection_mm: params.linear_deflection_mm,
                format: "stl".to_owned(),
                produce: params.produce.clone(),
            },
        );

        let measured: BridgeMeasurements = read_json(&out_dir, "measurements.json").await?;
        out.measurements.surface_area_mm2 = measured.surface_area_mm2;
        out.measurements.volume_mm3 = measured.volume_mm3;
        // A mesh made face by face is never welded shut, so the mesh's own answer is always
        // "open"; the B-rep says whether there is a closed solid.
        out.measurements.is_watertight = measured.solids > 0;
        out.provenance = MeasurementProvenance {
            bbox: Provenance::Tessellated,
            ..MeasurementProvenance::ANALYTIC
        };
        if let Some(bbox) = measured.bbox_mm {
            out.measurements.bbox_mm = bbox;
            out.provenance.bbox = Provenance::Analytic;
        }

        let entities: BridgeEntities = read_json(&out_dir, "entities.json").await?;
        out.entities = entities.into_entities();
        out.structure = Some(read_json::<AssemblyTree>(&out_dir, "structure.json").await?);
        out.metadata = Some(read_json::<CadMetadata>(&out_dir, "header.json").await?);
        Ok(out)
    }
}

// ---- the bridge's output files ------------------------------------------------------------

#[derive(Deserialize)]
struct BridgeMeasurements {
    solids: u32,
    volume_mm3: Option<f64>,
    surface_area_mm2: f64,
    bbox_mm: Option<[f64; 3]>,
}

#[derive(Deserialize)]
struct BridgeEntities {
    prototypes: Vec<BridgePrototype>,
}

#[derive(Deserialize)]
struct BridgePrototype {
    prototype: String,
    faces: Vec<BridgeFace>,
    circles: Vec<BridgeCircle>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum BridgeFace {
    Plane {
        face: u32,
        origin: [f64; 3],
        normal: [f64; 3],
    },
    Cylinder {
        face: u32,
        radius: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
    Cone {
        face: u32,
        ref_radius: f64,
        semi_angle_rad: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
    Sphere {
        face: u32,
        radius: f64,
        center: [f64; 3],
    },
    Torus {
        face: u32,
        major_radius: f64,
        minor_radius: f64,
        origin: [f64; 3],
        axis: [f64; 3],
    },
}

#[derive(Deserialize)]
struct BridgeCircle {
    edge: u32,
    radius: f64,
    center: [f64; 3],
    normal: [f64; 3],
}

impl BridgeEntities {
    fn into_entities(self) -> Vec<Entity> {
        let mut entities = Vec::new();
        for BridgePrototype {
            prototype,
            faces,
            circles,
        } in self.prototypes
        {
            for face in faces {
                let prototype = prototype.clone();
                entities.push(match face {
                    BridgeFace::Plane {
                        face,
                        origin,
                        normal,
                    } => Entity::Plane {
                        prototype,
                        face,
                        origin,
                        normal,
                    },
                    BridgeFace::Cylinder {
                        face,
                        radius,
                        origin,
                        axis,
                    } => Entity::Cylinder {
                        prototype,
                        face,
                        radius,
                        origin,
                        axis,
                    },
                    BridgeFace::Cone {
                        face,
                        ref_radius,
                        semi_angle_rad,
                        origin,
                        axis,
                    } => Entity::Cone {
                        prototype,
                        face,
                        ref_radius,
                        semi_angle_rad,
                        origin,
                        axis,
                    },
                    BridgeFace::Sphere {
                        face,
                        radius,
                        center,
                    } => Entity::Sphere {
                        prototype,
                        face,
                        radius,
                        center,
                    },
                    BridgeFace::Torus {
                        face,
                        major_radius,
                        minor_radius,
                        origin,
                        axis,
                    } => Entity::Torus {
                        prototype,
                        face,
                        major_radius,
                        minor_radius,
                        origin,
                        axis,
                    },
                });
            }
            for BridgeCircle {
                edge,
                radius,
                center,
                normal,
            } in circles
            {
                entities.push(Entity::Circle {
                    prototype: prototype.clone(),
                    edge,
                    radius,
                    center,
                    normal,
                });
            }
        }
        entities
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(dir: &Path, file: &str) -> Result<T, CadError> {
    let bytes = tokio::fs::read(dir.join(file))
        .await
        .map_err(|e| unreadable(file, e))?;
    serde_json::from_slice(&bytes).map_err(|e| unreadable(file, e))
}

fn unreadable(file: &str, error: impl std::fmt::Display) -> CadError {
    CadError::KernelOutputUnreadable {
        file: file.to_owned(),
        detail: error.to_string(),
    }
}

/// The bridge's own sentence from `{"kind":"refused","detail":...}`, or its raw stderr if it
/// said something else.
fn refusal_detail(stderr: &[u8]) -> String {
    #[derive(Deserialize)]
    struct Refusal {
        detail: String,
    }
    serde_json::from_slice::<Refusal>(stderr)
        .map(|r| r.detail)
        .unwrap_or_else(|_| tail(stderr))
}

fn describe(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit code {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("killed by signal {signal}");
        }
    }
    "an unknown exit status".to_owned()
}

fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(STDERR_TAIL_BYTES);
    String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    //! Driven by a fake bridge — a shell script that answers the way `occt-bridge` does — so
    //! the driver is tested everywhere `sh` runs. The real bridge against real OCCT is
    //! `cargo xtask verify occt`, inside the worker image.
    //!
    //! One script for every test, written once, with the behaviour chosen by the input bytes
    //! each test sends. A script per test raced: writing an executable while another test's
    //! freshly forked child still held that file's descriptor made exec fail with "Text file
    //! busy", now and then, and only when the tests ran in parallel.
    use super::*;
    use lapidary_core::DerivativeKind;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::OnceLock;

    const BRACKET_STL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/bracket-lp-1042-03.stl"
    );

    const SCRIPT: &str = r#"#!/bin/sh
case "$1" in
  version) echo 'occt 8.0.1 bridge 1' ;;
  convert)
    out="$7"
    read -r behaviour arg < "$3"
    case "$behaviour" in
      REFUSE) echo '{"kind":"refused","detail":"OCCT could not parse this file as STEP"}' >&2; exit 2 ;;
      CRASH) echo 'Segmentation fault in BRepMesh' >&2; kill -9 $$ ;;
      SLEEP) sleep 3; touch "$arg" ;;
      TOUCH) touch "$arg" ;;
      *)
        cp "BRACKET" "$out/mesh.stl"
        echo '[20]' > "$out/parts.json"
        echo '{"units":"mm","solids":1,"volume_mm3":11403.98133253095,"surface_area_mm2":2833.53958,"bbox_min":[-11,-11,0],"bbox_max":[11,11,30],"bbox_mm":[22,22,30]}' > "$out/measurements.json"
        echo '{"units":"mm","prototypes":[{"prototype":"0:1:1:1","faces":[{"face":1,"type":"cylinder","radius":11,"origin":[0,0,0],"axis":[0,0,1]},{"face":2,"type":"plane","origin":[0,0,30],"normal":[0,0,1]}],"circles":[{"edge":1,"radius":11,"center":[0,0,30],"normal":[0,0,1]}]}]}' > "$out/entities.json"
        echo '{"units":"mm","roots":[{"name":"cylinder-d22-lp-9010-00","prototype":"0:1:1:1","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":1,"prototypes":1}' > "$out/structure.json"
        echo '{"file_name":"cylinder-d22-lp-9010-00.step","time_stamp":"2026-09-13T09:00:00","authors":["J. Okafor"],"organizations":["Lapidary fixtures"],"originating_system":"SOLIDWORKS 2025","preprocessor":"Open CASCADE 8.0.1","descriptions":[],"schemas":["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"],"materials":["AISI 1045 steel"]}' > "$out/header.json"
        ;;
    esac
    ;;
esac
"#;

    fn fake_bridge() -> &'static Path {
        static BRIDGE: OnceLock<PathBuf> = OnceLock::new();
        BRIDGE.get_or_init(|| {
            let dir = tempfile::tempdir().expect("temp dir").keep();
            let path = dir.join("occt-bridge");
            std::fs::write(&path, SCRIPT.replace("BRACKET", BRACKET_STL))
                .expect("writes the fake bridge");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            // Wait out "Text file busy": a child another test forked while this file was open
            // for writing holds the descriptor until it execs. After this, nothing writes here.
            for _ in 0..200 {
                if let Ok(output) = std::process::Command::new(&path).arg("version").output()
                    && output.status.success()
                {
                    return path;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the fake bridge never became runnable");
        })
    }

    fn step_params() -> KernelParams {
        KernelParams {
            linear_deflection_mm: None,
            format: "step".to_owned(),
            produce: vec![DerivativeKind::Thumbnail, DerivativeKind::TessellationL0],
        }
    }

    fn kernel(timeout: Duration) -> OcctKernel {
        OcctKernel::new(fake_bridge(), timeout).expect("the fake bridge answers its version")
    }

    #[tokio::test]
    async fn a_conversion_carries_the_bridges_exact_figures_and_marks_them_analytic() {
        let out = kernel(Duration::from_secs(10))
            .process(b"ISO-10303-21;", &step_params())
            .await
            .expect("converts");

        assert_eq!(out.measurements.volume_mm3, Some(11403.98133253095));
        assert_eq!(out.measurements.bbox_mm, [22.0, 22.0, 30.0]);
        assert!(
            out.measurements.is_watertight,
            "one closed solid, whatever the mesh says"
        );
        assert_eq!(out.provenance, MeasurementProvenance::ANALYTIC);
        assert!(
            out.entities.contains(&Entity::Cylinder {
                prototype: "0:1:1:1".to_owned(),
                face: 1,
                radius: 11.0,
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
            }),
            "the cylinder face a measurement snaps to: {:?}",
            out.entities
        );
        assert_eq!(
            out.entities.len(),
            3,
            "two faces and a circle: {:?}",
            out.entities
        );
        assert_eq!(out.structure.as_ref().map(|s| s.parts), Some(1));
        let metadata = out
            .metadata
            .as_ref()
            .expect("what the file says about itself");
        assert_eq!(
            metadata.originating_system.as_deref(),
            Some("SOLIDWORKS 2025")
        );
        assert_eq!(metadata.materials, ["AISI 1045 steel"]);
        assert!(
            out.thumbnail_webp.is_some(),
            "the mesh pipeline drew the thumbnail"
        );
        assert_eq!(
            out.tessellations.len(),
            1,
            "and clustered the L0 rung it was asked for"
        );
        assert!(
            out.measurements.triangle_count > 0,
            "the triangle count stays the mesh's"
        );
    }

    #[tokio::test]
    async fn a_file_the_bridge_refuses_is_a_refusal_with_its_reason() {
        let err = kernel(Duration::from_secs(10))
            .process(b"REFUSE", &step_params())
            .await
            .expect_err("refused");
        match err {
            CadError::CadRefused { format, detail } => {
                assert_eq!(format, "STEP");
                assert_eq!(detail, "OCCT could not parse this file as STEP");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_bridge_that_dies_is_a_crash_not_a_refusal() {
        let err = kernel(Duration::from_secs(10))
            .process(b"CRASH", &step_params())
            .await
            .expect_err("crashed");
        match err {
            CadError::KernelCrashed {
                status,
                stderr_tail,
                ..
            } => {
                assert_eq!(status, "killed by signal 9");
                assert!(
                    stderr_tail.contains("BRepMesh"),
                    "keeps what the bridge said: {stderr_tail}"
                );
            }
            other => panic!("expected a crash, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_bridge_that_does_not_answer_in_time_is_a_timeout_and_is_killed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let marker = dir.path().join("still-running");
        let input = format!("SLEEP {}", marker.display());

        let err = kernel(Duration::from_millis(300))
            .process(input.as_bytes(), &step_params())
            .await
            .expect_err("timed out");
        assert!(matches!(err, CadError::Timeout { .. }), "got {err:?}");
        std::thread::sleep(Duration::from_secs(4));
        assert!(
            !marker.exists(),
            "kill_on_drop must stop the bridge, not leave it to finish"
        );
    }

    #[test]
    fn the_kernel_version_names_the_occt_build_and_the_deflection() {
        let kernel = kernel(Duration::from_secs(1));
        let coarse = kernel.version(&KernelParams {
            linear_deflection_mm: Some(0.5),
            ..step_params()
        });
        let fine = kernel.version(&step_params());

        assert_eq!(coarse.implementation, "occt");
        assert!(
            fine.version
                .starts_with("occt-8.0.1-bridge-1+deflection-0.1+"),
            "{}",
            fine.version
        );
        assert_ne!(
            coarse.version, fine.version,
            "a different deflection tessellates differently"
        );
    }

    #[test]
    fn a_missing_bridge_is_unavailable_before_any_work_is_accepted() {
        let err = OcctKernel::new("/nonexistent/occt-bridge", Duration::from_secs(1))
            .err()
            .expect("no bridge to ask");
        assert!(
            matches!(err, CadError::KernelUnavailable { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_format_the_bridge_does_not_read_is_refused_before_it_runs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let marker = dir.path().join("ran");
        let input = format!("TOUCH {}", marker.display());

        let err = kernel(Duration::from_secs(1))
            .process(
                input.as_bytes(),
                &KernelParams {
                    format: "stl".to_owned(),
                    ..step_params()
                },
            )
            .await
            .expect_err("STL is the mesh kernel's");
        assert!(
            matches!(err, CadError::UnsupportedFormat { .. }),
            "got {err:?}"
        );
        assert!(!marker.exists(), "the bridge never ran");
    }
}
