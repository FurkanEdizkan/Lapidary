use serde::{Deserialize, Serialize};
use std::path::Path;
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelOutput {
    pub triangle_count: u32,
    pub bbox_mm: [f64; 3],
    /// Analytic B-rep entities, empty for mesh input. Measurement snaps to these.
    pub entities: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CadError {
    #[error(
        "Could not read {path} — it may use an unsupported AP schema. Re-export from your CAD tool as AP214 or AP242 and retry."
    )]
    UnsupportedSchema { path: String },

    #[error(
        "No fixture is registered for {path}. MockKernel answers only for the part names matched in crates/lapidary-cad/src/mock.rs; add an arm there, or run against the real kernel."
    )]
    NoFixture { path: String },

    #[error(
        "The CAD kernel did not respond within {seconds}s while processing {path}. The file may be unusually large; raise LAPIDARY_KERNEL_TIMEOUT or split the assembly."
    )]
    Timeout { path: String, seconds: u64 },

    #[error(
        "Could not read this STL — {detail}. Re-export it from your CAD or slicing tool and retry; if it came from a download, the transfer may have been cut short."
    )]
    MalformedStl { detail: String },
}

/// One shipped implementation. The trait exists so tests have a double.
#[async_trait::async_trait]
pub trait Kernel: Send + Sync {
    fn version(&self) -> KernelVersion;

    async fn process(&self, src: &Path, params: &KernelParams) -> Result<KernelOutput, CadError>;
}
