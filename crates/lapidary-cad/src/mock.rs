use crate::cluster::{Lod, Tessellation};
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use lapidary_core::{DerivativeKind, MeshMeasurements};

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

/// One canned rung, deliberately distinguishable from the other two so a test cannot pass
/// by returning the same rung three times.
///
/// The bytes are markers, not glTF. This is a double; the only thing that produces a real
/// `.glb` is `MeshKernel`, and a mock that returned plausible-looking glTF would invite
/// someone to trust it.
fn canned_rung(lod: Lod) -> Tessellation {
    Tessellation {
        lod,
        glb: format!("mock-{}", lod.as_kind()).into_bytes(),
        triangle_count: match lod {
            Lod::L0 => 500,
            Lod::L1 => 5_000,
            Lod::L2 => 12_940,
        },
        grid: match lod {
            Lod::L0 => Some(32),
            Lod::L1 => Some(96),
            Lod::L2 => None,
        },
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
    /// unconditionally, then one entry per rung `params.produce` asked for, in the order
    /// asked for. A double that handed back the full ladder regardless of `produce` would
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
        let mut tessellations = Vec::new();
        let mut thumbnail_webp = None;
        for want in &params.produce {
            match want {
                DerivativeKind::Thumbnail => thumbnail_webp = Some(b"mock-thumbnail".to_vec()),
                DerivativeKind::TessellationL0 => tessellations.push(canned_rung(Lod::L0)),
                DerivativeKind::TessellationL1 => tessellations.push(canned_rung(Lod::L1)),
                DerivativeKind::TessellationL2 => tessellations.push(canned_rung(Lod::L2)),
            }
        }
        Ok(KernelOutput {
            measurements,
            thumbnail_webp,
            tessellations,
            // Empty, and not because this is a mock: `Entity` is uninhabited until
            // Phase 2's STEP ingest gives it variants, so no kernel can return one.
            entities: Vec::new(),
            // The mock's fixtures are chosen to be renderable, so nothing here refuses.
            // A test that wants a refusal wants the real kernel and a degenerate mesh —
            // `mesh_kernel.rs` has that fixture, because inventing one here would prove
            // only that the mock can be told to lie.
            unproduced: Vec::new(),
        })
    }
}
