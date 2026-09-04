use crate::cluster::Tessellation;
use lapidary_core::MeshMeasurements;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Pinned across the fleet — a worker running a different kernel version must not
/// produce derivatives that are cached as equivalent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelVersion {
    pub implementation: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelParams {
    /// Linear deflection in mm for tessellation. None means the kernel's default.
    pub linear_deflection_mm: Option<f64>,
    /// The source format, lowercase and without a dot: `stl`, `obj`, later `step`.
    ///
    /// Carried in params rather than sniffed from the bytes. OBJ is plain text with no
    /// magic number, so sniffing reduces to guessing from the first non-comment line —
    /// and the extension is what the directory walk already filtered on, so it is what
    /// the kernel should be told. It also names the format in the error a person reads
    /// when a file will not parse.
    pub format: String,
}

/// Analytic B-rep entities — axes, radii, normals — that measurement snaps to.
///
/// Deliberately a type with no variants rather than the `Vec<String>` it was. Phase 2's
/// STEP ingest fills it; until then the only thing that can be said about a mesh is that
/// it has none, and that emptiness is load-bearing: it is what stops tessellated numbers
/// being presented as exact. Measurement cannot snap to `"CYLINDRICAL_SURFACE:22.000"`
/// without parsing it back out of a string, which is why the strings are gone.
#[derive(Debug, Clone, PartialEq)]
pub enum Entity {}

/// Everything one kernel call produces for one file.
///
/// Replaces both the placeholder `{ triangle_count, bbox_mm, entities: Vec<String> }` that
/// only `MockKernel` ever built and `mesh_kernel.rs`'s parallel `MeshOutput` that
/// production actually used. `docs/ARCHITECTURE.md` specified this shape and
/// `sidecar/occt-bridge/README.md` recorded the old one as a placeholder; Phase 0a
/// follow-up item 2 closes here.
///
/// Not `Serialize`: the sidecar's wire format is Phase 0b's problem and will not want
/// three `Vec<u8>` blobs inline in JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelOutput {
    pub measurements: MeshMeasurements,
    pub thumbnail_webp: Vec<u8>,
    /// L0, L1, L2 in ascending detail. Always three — a small mesh clusters to itself at
    /// every grid and is written anyway, so consumers never branch on how many there are.
    pub tessellations: [Tessellation; 3],
    pub entities: Vec<Entity>,
}

#[derive(Debug, Error)]
pub enum CadError {
    #[error(
        "Could not read {path} — it may use an unsupported AP schema. Re-export from your CAD tool as AP214 or AP242 and retry."
    )]
    UnsupportedSchema { path: String },

    #[error(
        "No fixture is registered for the {format} format. MockKernel answers only for the formats matched in crates/lapidary-cad/src/mock.rs; add an arm there, or run against the real kernel."
    )]
    NoFixture { format: String },

    #[error(
        "The CAD kernel did not respond within {seconds}s while processing {path}. The file may be unusually large; raise LAPIDARY_KERNEL_TIMEOUT or split the assembly."
    )]
    Timeout { path: String, seconds: u64 },

    #[error(
        "Could not read this {format} — {detail}. Re-export it from your CAD or slicing tool and retry; if it came from a download, the transfer may have been cut short."
    )]
    MalformedMesh { format: String, detail: String },

    #[error(
        "Could not render a thumbnail — {detail}. The file parsed, so the geometry itself may be degenerate; open it in your CAD tool to check."
    )]
    Unrenderable { detail: String },

    #[error(
        "This build has no parser for the {format} format. The mesh kernel reads STL and OBJ; 3MF and STEP are not yet ingested."
    )]
    UnsupportedFormat { format: String },

    #[error(
        "Refused this {format} — {detail}. The file may be corrupt or deliberately crafted; re-export it from a trusted tool and retry."
    )]
    ArchiveRefused { format: String, detail: String },
}

/// One shipped implementation. The trait exists so tests have a double.
#[async_trait::async_trait]
pub trait Kernel: Send + Sync {
    /// Takes the params for the same reason `process` does: the version identifies what
    /// produced a given derivative, and for the mesh kernel that includes which parser
    /// ran. An `obj`-derived thumbnail labelled `stl-1` is indistinguishable from a stale
    /// one, which is the thing `kernel_version` exists to prevent.
    fn version(&self, params: &KernelParams) -> KernelVersion;

    /// Bytes, not a path. Ingest has already read and hashed the file, and reading it a
    /// second time is a second chance to read something different — the hash is committed
    /// before the parse, so a kernel that re-opens the path can disagree with what was
    /// recorded against it.
    ///
    /// Slice 1's plan said `MeshKernel` would implement this trait "by reading the path
    /// and delegating"; the shipped code refused, and was right to. Phase 0b's OCCT kernel
    /// writes the bytes to a scratch file inside the sidecar, which is where that concern
    /// belongs — the sidecar already marshals across a process boundary.
    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError>;
}
