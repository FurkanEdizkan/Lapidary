#![deny(clippy::unwrap_used)]
//! The CAD kernel boundary. One shipped implementation (OCCT, native, in the worker
//! container) plus a test double. The open path never invokes this crate.

mod kernel;
#[cfg(feature = "mock-kernel")]
mod mock;

pub use kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
#[cfg(feature = "mock-kernel")]
pub use mock::MockKernel;

#[cfg(all(test, feature = "mock-kernel"))]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn mock_kernel_reports_a_pinned_version() {
        let kernel = MockKernel::new();
        assert_eq!(kernel.version().implementation, "mock");
    }

    #[tokio::test]
    async fn mock_kernel_returns_fixture_output_for_a_known_part() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(
                Path::new("bearing-block-608zz.step"),
                &KernelParams::default(),
            )
            .await
            .expect("mock kernel processes the known fixture");
        assert_eq!(out.triangle_count, 48_112);
        assert_eq!(out.bbox_mm, [61.0, 42.0, 18.5]);
        assert!(
            !out.entities.is_empty(),
            "STEP input must yield B-rep entities"
        );
    }

    #[tokio::test]
    async fn mock_kernel_reports_an_actionable_error_for_unknown_input() {
        let kernel = MockKernel::new();
        let err = kernel
            .process(Path::new("nonexistent.step"), &KernelParams::default())
            .await
            .expect_err("unknown fixture must fail");
        let msg = err.to_string();
        assert!(msg.contains("nonexistent.step"));
        assert!(msg.contains("fixture"), "error must say what to do");
    }
}
