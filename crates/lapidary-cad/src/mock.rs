use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use std::path::Path;

/// Returns canned output for known fixture names. Phase 0b replaces this in production
/// with OcctKernel; this stays for tests.
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
    fn version(&self) -> KernelVersion {
        KernelVersion {
            implementation: "mock".to_owned(),
            version: "0a".to_owned(),
        }
    }

    async fn process(&self, src: &Path, _params: &KernelParams) -> Result<KernelOutput, CadError> {
        let name = src.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        match name {
            "bearing-block-608zz.step" => Ok(KernelOutput {
                triangle_count: 48_112,
                bbox_mm: [61.0, 42.0, 18.5],
                entities: vec![
                    "CYLINDRICAL_SURFACE:22.000".to_owned(),
                    "PLANE:top".to_owned(),
                    "CYLINDRICAL_SURFACE:8.000".to_owned(),
                ],
            }),
            "bracket-lp-1042-03.stl" => Ok(KernelOutput {
                triangle_count: 12_940,
                bbox_mm: [88.0, 34.0, 12.0],
                entities: Vec::new(), // mesh input: no analytic entities, values are approximate
            }),
            _ => Err(CadError::NoFixture {
                path: src.display().to_string(),
            }),
        }
    }
}
