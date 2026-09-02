# occt-bridge

Empty until Phase 0b.

This directory will hold the C++ sidecar wrapping Open CASCADE: STEP and IGES reading,
tessellation, analytic B-rep entity extraction for measurement, and format conversion. It
is a separate process rather than a linked library, so an OCCT crash takes down one job
instead of the worker.

In Phase 0a the `Kernel` trait in `crates/lapidary-cad` has exactly one implementation,
`MockKernel`, returning canned output for named parts. Phase 0b adds `OcctKernel` behind
the same trait and builds OCCT from source in the worker image. What's stable is the
trait's *shape* — one async `process` call taking a source path and params, returning
derivatives, plus a synchronous `version` — and the invariant that mesh input yields no
analytic entities, which is what stops tessellated numbers from being presented as exact.
`KernelOutput`'s fields are not stable: Phase 0a's `{ triangle_count, bbox_mm, entities:
Vec<String> }` is a placeholder, and Phase 0b will replace it with the richer shape
`docs/ARCHITECTURE.md` already specifies — `{ tessellation_l0/l1/l2.glb, structure.json,
entities.json }` — carrying blob references for the LOD ladder and analytic entities with
axes, radii and normals instead of opaque strings like `"CYLINDRICAL_SURFACE:22.000"`.

Nothing in Phase 0a depends on this directory.
