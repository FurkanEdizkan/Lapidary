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
        // Assert the remedy clause, not just the word "fixture" — deleting the advice and
        // leaving "No fixture is registered for {path}." must fail this test.
        assert!(
            msg.contains("add an arm"),
            "error must say what to do, not just what broke"
        );
    }

    /// The measurement invariant, locked. `CLAUDE.md` requires that mesh-derived values
    /// are labelled approximate, always — which downstream code decides by asking whether
    /// the kernel returned any analytic entities. If this ever returns a non-empty vec for
    /// mesh input, tessellated numbers start being presented as exact.
    #[tokio::test]
    async fn mesh_input_yields_no_analytic_entities() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(
                Path::new("bracket-lp-1042-03.stl"),
                &KernelParams::default(),
            )
            .await
            .expect("mock kernel processes the known mesh fixture");
        assert_eq!(out.triangle_count, 12_940);
        assert_eq!(out.bbox_mm, [88.0, 34.0, 12.0]);
        assert!(
            out.entities.is_empty(),
            "mesh input must yield no analytic entities — every measurement taken from it \
             is approximate, and an entity list is what tells callers otherwise"
        );
    }
}
