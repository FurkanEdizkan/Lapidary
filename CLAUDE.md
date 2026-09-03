# Lapidary — working rules

Lapidary is a **visual index for 3D part libraries with a return path**. Users scan
hundreds of parts, decide if one fits, hand it to the right external tool, and the result
comes back versioned. Hobbyist STLs and industrial STEP assemblies, same architecture.

Read `docs/README.md` for the doc map. Read the relevant doc before writing code in an
area — these encode decisions that are expensive to reverse.

---

## Non-negotiable product rules

- **We never edit geometry.** Editing happens in Rhino, Fusion, FreeCAD, Blender, Orca.
  We visualize, measure, route, version.
- **We never delete user data implicitly.** Delete is soft. Purge is separate and
  explicit. Blobs quarantine 30 days before removal. Derivative cache eviction is a
  different action with different wording and must never read as data loss.
- **Format negotiation is automatic.** Slicers get 3MF/STL, CAD gets STEP, viewer gets
  glTF. Never hand a mesh to someone who needed B-rep.
- **Measurement must not lie.** Analytic values from B-rep entities where available.
  Mesh-derived measurements are labelled "approximate" in the UI, always.
- **Versioning is Perforce-shaped, not Git-shaped.** Immutable content-addressed
  snapshots, lineage DAG, pessimistic locks. No merge, no branches, no textual diff.
  Geometric diff replaces textual diff.
- **Governance is opt-in per library.** Hobby libraries have no revisions, states or
  approvals. Flipping a library to `controlled` turns that machinery on.
- **The application is free and complete.** No gated features in the app. Revenue is the
  server, fleet, support and cloud.
- **Downloads are never silently converted.** `variant=original` returns byte-identical
  ingested bytes. Anything we produced is named `*.lapidary.*`.

## Non-negotiable technical rules

- **Container-first.** Podman and Docker. The Tauri app is a later phase and bundles
  **only our own binaries** — never Postgres, never OCCT.
- **No SQL outside `lapidary-db`.** Everything goes through repository traits.
- **The open path never touches a source file and never invokes the CAD kernel.** Opening
  a part reads metadata + derivatives only. If you find yourself parsing STEP on open,
  stop.
- **Hash first, always.** BLAKE3 before anything else in ingest. A known hash
  short-circuits the whole pipeline.
- **Generated columns are explicitly `STORED`.** PG 18 defaults to virtual, and virtual
  columns cannot be indexed. Our `tsvector` search column must be STORED.
- **Content addressing is not authorization.** Knowing a blob hash must never grant
  access. Always check tenant + part reachability.
- **Pin everything.** Exact image digests, `Cargo.lock` committed, GitHub Actions pinned
  to commit SHAs.
- **`lapidary-api` is a library that builds a Router.** Never a binary. Never fork it per
  distribution.

## Style

- Rust: `thiserror` in libraries, `anyhow` at binary edges. No `unwrap()` outside tests.
- Errors say what broke and what to do. "Could not read this STEP file — it may use an
  unsupported AP schema. Re-export from your CAD tool and retry." Not "parse failed (3)."
- Frontend: dark only, no light mode. Motion is mechanical — 120/180/280ms,
  `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity only. Respect
  `prefers-reduced-motion`.
- No bare user-facing strings in components. English only, but every string goes through
  `src/lib/strings.ts`. Turkish is the planned second locale.
- Real content in all examples and fixtures. Plausible part numbers, real dimensions.
  Never "Part 1 / Part 2".

## When unsure

Prefer the boring option. This is a solo-maintained project that will run in air-gapped
industrial environments. Fewer dependencies, fewer moving parts, more explicit failure
modes.
