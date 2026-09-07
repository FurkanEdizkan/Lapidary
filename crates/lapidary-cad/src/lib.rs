//! The CAD kernel boundary. One shipped implementation (OCCT, native, in the worker
//! container) plus a test double. The open path never invokes this crate.

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
pub use glb::GLB_VERSION;
pub use kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion, Unproduced};
pub use measure::measure;
pub use mesh_kernel::MeshKernel;
#[cfg(feature = "mock-kernel")]
pub use mock::MockKernel;
pub use obj::parse_obj;
pub use raster::{MAX_THUMB_BYTES, RASTER_VERSION, THUMB_PX, render_thumbnail};
pub use stl::{Mesh, parse_stl};
pub use tmf::parse_3mf;

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

    /// The measurement invariant, now held by the type system rather than by this test.
    ///
    /// `CLAUDE.md` requires that mesh-derived values are labelled approximate, always —
    /// which downstream code decides by asking whether the kernel returned any analytic
    /// entities. Slice 3 made `Entity` an uninhabited enum, so `Vec<Entity>` is provably
    /// empty for every input, not just for mesh input. That is a stronger guarantee than
    /// this test was: it cannot be broken by a careless kernel, only by giving `Entity` a
    /// variant.
    ///
    /// **Phase 2 must restore the contrast.** The moment STEP ingest adds a variant, this
    /// test stops proving anything on its own and needs its old shape back: a B-rep input
    /// yielding entities beside a mesh input yielding none.
    #[tokio::test]
    async fn mesh_input_yields_no_analytic_entities() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(b"ignored by the double", &params("stl"))
            .await
            .expect("mock kernel processes a mesh format");
        assert!(
            out.entities.is_empty(),
            "mesh input must yield no analytic entities — every measurement taken from it \
             is approximate, and an entity list is what tells callers otherwise"
        );
    }

    /// The ladder is always three rungs, and they are not the same rung three times.
    #[tokio::test]
    async fn the_ladder_is_three_distinguishable_rungs() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(b"ignored by the double", &params("stl"))
            .await
            .expect("processes");
        let counts: Vec<u32> = out.tessellations.iter().map(|t| t.triangle_count).collect();
        assert_eq!(counts.len(), 3);
        assert!(
            counts[0] < counts[1] && counts[1] < counts[2],
            "rungs must ascend in detail, got {counts:?}"
        );
    }

    /// The double honours `produce` too. `MockKernel` is unreachable from the ingest path
    /// today — `handler.rs` names `MeshKernel` directly — so this is not a live bug, but a
    /// double whose contract has drifted from the trait's is how a future test of
    /// selectivity passes without proving anything.
    #[tokio::test]
    async fn the_mock_produces_only_the_rungs_it_was_asked_for() {
        let kernel = MockKernel::new();
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: vec![lapidary_core::DerivativeKind::TessellationL1],
        };
        let out = kernel
            .process(b"ignored by the double", &params)
            .await
            .expect("processes");
        assert_eq!(out.tessellations.len(), 1);
        assert_eq!(out.tessellations[0].lod, Lod::L1);
        assert!(out.thumbnail_webp.is_none());
        // Measurements are unconditional, here as in `MeshKernel`.
        assert_eq!(out.measurements.triangle_count, 12_940);
    }

    /// Request order, not ladder order — `KernelOutput.tessellations` promises "in the
    /// order requested", and a double that sorted its output would quietly disagree with
    /// `MeshKernel` for any caller that asks out of order.
    #[tokio::test]
    async fn the_mock_returns_the_rungs_in_the_order_they_were_asked_for() {
        let kernel = MockKernel::new();
        let params = KernelParams {
            linear_deflection_mm: None,
            format: "stl".to_owned(),
            produce: vec![
                lapidary_core::DerivativeKind::TessellationL2,
                lapidary_core::DerivativeKind::TessellationL0,
            ],
        };
        let out = kernel
            .process(b"ignored by the double", &params)
            .await
            .expect("processes");
        assert_eq!(
            out.tessellations.iter().map(|t| t.lod).collect::<Vec<_>>(),
            vec![Lod::L2, Lod::L0]
        );
    }

    /// Empty `produce` is legal and means measurements only — the same contract
    /// `mesh_kernel.rs` pins for the real thing.
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
        assert!(out.tessellations.is_empty());
        assert!(out.thumbnail_webp.is_none());
        assert_eq!(out.measurements.triangle_count, 12_940);
    }
}
