# occt-bridge

Empty until Phase 0b.

This directory will hold the C++ sidecar wrapping Open CASCADE: STEP and IGES reading,
tessellation, analytic B-rep entity extraction for measurement, and format conversion. It
is a separate process rather than a linked library, so an OCCT crash takes down one job
instead of the worker.

In Phase 0a the `Kernel` trait in `crates/lapidary-cad` has exactly one implementation,
`MockKernel`, returning canned output for named parts. Phase 0b adds `OcctKernel` behind
the same trait and builds OCCT from source in the worker image. The trait does not change.

Nothing in Phase 0a depends on this directory.
