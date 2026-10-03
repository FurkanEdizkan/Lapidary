use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use lapidary_core::MeshMeasurements;

/// Returns canned output for known formats. Phase 0b replaces this in production with
/// `OcctKernel`; this stays for tests.
///
/// Keyed on `params.format` rather than a filename, because slice 3 changed the trait to
/// take bytes — see `Kernel::process`. A double that dispatched on a name would need the
/// production type to carry a name for its benefit alone, which is the wrong direction.
pub struct MockKernel;

impl MockKernel {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Kernel for MockKernel {
    /// The same for every format: a double has one set of canned bytes, so nothing
    /// about its output varies with the parser that would have run.
    fn version(&self, _params: &KernelParams) -> KernelVersion {
        KernelVersion {
            implementation: "mock".to_owned(),
            version: "0a".to_owned(),
        }
    }

    /// Selective, for the same reason `MeshKernel::process` is: measurements
    /// unconditionally, then one entry per export `params.produce` asked for, in the order
    /// asked for. A double that handed back every export regardless of `produce` would
    /// let a test of selectivity pass without proving anything — the canned bytes are
    /// allowed to be fake, the contract is not.
    async fn process(
        &self,
        _bytes: &[u8],
        params: &KernelParams,
    ) -> Result<KernelOutput, CadError> {
        let measurements = match params.format.as_str() {
            "step" => MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            "stl" | "obj" => MeshMeasurements {
                bbox_mm: [88.0, 34.0, 12.0],
                triangle_count: 12_940,
                surface_area_mm2: 15_320.5,
                volume_mm3: None,
                is_watertight: false,
            },
            other => {
                return Err(CadError::NoFixture {
                    format: other.to_owned(),
                });
            }
        };
        let exports = params
            .produce
            .iter()
            .map(|want| (*want, format!("mock-{}", want.as_str()).into_bytes()))
            .collect();
        Ok(KernelOutput {
            centre_of_mass_mm: None,
            exports,
            measurements,
            // The mock's fixtures are chosen to be writable, so nothing here refuses.
            // A test that wants a refusal wants the real kernel and a degenerate mesh —
            // `mesh_kernel.rs` has that fixture, because inventing one here would prove
            // only that the mock can be told to lie.
            unproduced: Vec::new(),
        })
    }
}
