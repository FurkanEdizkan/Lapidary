//! The mesh kernel boundary. One shipped implementation (`MeshKernel`, which reads STL, OBJ
//! and 3MF) plus a test double. The open path never invokes this crate.

mod cluster;
mod glb;
mod kernel;
mod measure;
mod mesh_kernel;
#[cfg(feature = "mock-kernel")]
mod mock;
mod obj;
mod raster;
mod stl;
mod tmf;

pub use cluster::{Lod, Tessellation, cluster};
pub use glb::{GLB_VERSION, read_triangles};
pub use kernel::{
    CadError, Kernel, KernelOutput, KernelParams, KernelVersion, MeasurementProvenance, Unproduced,
};
pub use measure::measure;
pub use mesh_kernel::MeshKernel;
#[cfg(feature = "mock-kernel")]
pub use mock::MockKernel;
pub use obj::parse_obj;
pub use raster::{MAX_THUMB_BYTES, RASTER_VERSION, THUMB_PX, render_thumbnail};
pub use stl::{Mesh, parse_stl, write_stl};
pub use tmf::{parse_3mf, write_3mf};

#[cfg(all(test, feature = "mock-kernel"))]
mod tests {
    use super::*;

    fn params(format: &str) -> KernelParams {
        KernelParams {
            linear_deflection_mm: None,
            format: format.to_owned(),
            produce: lapidary_core::DerivativeKind::ALL.to_vec(),
        }
    }

    #[tokio::test]
    async fn mock_kernel_reports_a_pinned_version() {
        let kernel = MockKernel::new();
        assert_eq!(kernel.version(&params("stl")).implementation, "mock");
    }

    #[tokio::test]
    async fn mock_kernel_returns_fixture_output_for_a_known_format() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(b"ignored by the double", &params("step"))
            .await
            .expect("mock kernel processes a known format");
        assert_eq!(out.measurements.triangle_count, 48_112);
        assert_eq!(out.measurements.bbox_mm, [61.0, 42.0, 18.5]);
    }

    #[tokio::test]
    async fn mock_kernel_reports_an_actionable_error_for_an_unknown_format() {
        let kernel = MockKernel::new();
        let err = kernel
            .process(b"anything", &params("iges"))
            .await
            .expect_err("an unregistered format must fail");
        let msg = err.to_string();
        assert!(msg.contains("iges"));
        // Assert the remedy clause, not just the word "fixture" — deleting the advice and
        // leaving "No fixture is registered for the {format} format." must fail this test.
        assert!(
            msg.contains("add an arm"),
            "error must say what to do, not just what broke"
        );
    }

    /// The double honours `produce` too. `MockKernel` is unreachable from the ingest path
    /// today — `handler.rs` names `MeshKernel` directly — so this is not a live bug, but a
    /// double whose contract has drifted from the trait's is how a future test of
    /// selectivity passes without proving anything.
    #[tokio::test]
    async fn the_mock_produces_only_the_exports_it_was_asked_for() {
        let kernel = MockKernel::new();
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: vec![lapidary_core::DerivativeKind::Export3mf],
        };
        let out = kernel
            .process(b"ignored by the double", &params)
            .await
            .expect("processes");
        assert_eq!(
            out.exports
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>(),
            vec![lapidary_core::DerivativeKind::Export3mf]
        );
        // Measurements are unconditional, here as in `MeshKernel`.
        assert_eq!(out.measurements.triangle_count, 12_940);
    }

    /// Request order, not declaration order — `KernelOutput.exports` promises "one for each
    /// export `params.produce` asked for", and a double that sorted its output would quietly
    /// disagree with `MeshKernel` for any caller that asks out of order.
    #[tokio::test]
    async fn the_mock_returns_the_exports_in_the_order_they_were_asked_for() {
        let kernel = MockKernel::new();
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: vec![
                lapidary_core::DerivativeKind::Export3mf,
                lapidary_core::DerivativeKind::ExportStl,
            ],
        };
        let out = kernel
            .process(b"ignored by the double", &params)
            .await
            .expect("processes");
        assert_eq!(
            out.exports
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>(),
            vec![
                lapidary_core::DerivativeKind::Export3mf,
                lapidary_core::DerivativeKind::ExportStl
            ]
        );
    }

    /// Empty `produce` is legal and means measurements only — the same contract
    /// `mesh_kernel.rs` pins for the real thing, and what every ingest now asks for.
    #[tokio::test]
    async fn the_mock_asked_for_nothing_still_measures() {
        let kernel = MockKernel::new();
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: Vec::new(),
        };
        let out = kernel
            .process(b"ignored by the double", &params)
            .await
            .expect("processes");
        assert!(out.exports.is_empty());
        assert_eq!(out.measurements.triangle_count, 12_940);
    }
}
