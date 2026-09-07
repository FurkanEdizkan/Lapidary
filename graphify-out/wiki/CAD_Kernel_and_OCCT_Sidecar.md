# CAD Kernel and OCCT Sidecar

> 29 nodes

## Key Concepts

- **parse_3mf** (10 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- **MeshKernel** (9 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **lapidary-cad (L2)** (8 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **lapidary-ingest (L3)** (8 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **Kernel trait** (6 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **Vertex-Clustering LOD Ladder (L0/L1/L2)** (6 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- **occt-bridge Sidecar** (5 connections) — `sidecar/occt-bridge/README.md`
- **MockKernel** (4 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **Caps — Decompressed Size, Entries, Ratio, Triangles** (4 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- **Recursive Ingest Walk (depth 16)** (4 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **HandlerError (Permanent | Transient)** (3 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **Hand-Written glTF 2.0 Binary Writer** (3 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- **parse_obj** (3 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- **CadError::ArchiveRefused** (3 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- **path_escapes Predicate in lapidary-core** (3 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **OcctKernel** (3 connections) — `sidecar/occt-bridge/README.md`
- **the_model_part_is_found_through_the_relationships()** (2 connections) — `crates/lapidary-cad/src/tmf.rs`
- **Read-Only /ingest Mount** (2 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **KernelOutput (reconciled)** (2 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- **Format Dispatch on Extension, Not Byte Sniffing** (2 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- **The Model Part Is Found Through the Relationships** (2 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- **content-visibility: auto Instead of a Virtualizer** (2 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6b-design.md`
- **CPU-Rasterized 512px WebP Thumbnail** (1 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **A Scan Is Not Transactional Across Files** (1 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **A Parse Failure Is Terminal on the First Attempt** (1 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- *... and 4 more nodes in this community*

## Relationships

- [[Job Error Taxonomy]] (3 shared connections)
- [[Pinned Stack and Uploads]] (3 shared connections)
- [[Job Payload Round-Trips]] (3 shared connections)
- [[3MF Parsing and Bombs]] (2 shared connections)
- [[compilerOptions]] (2 shared connections)
- [[Phase 1 — The Remaining Slices, Re-Cut]] (2 shared connections)
- [[The Open Path Never Touches A Source File…]] (1 shared connections)
- [[lapidary-core (L0)]] (1 shared connections)
- [[Mesh Kernel Dispatch]] (1 shared connections)
- [[OBJ Parsing]] (1 shared connections)
- [[detail (lapidary-api)]] (1 shared connections)
- [[Phase 1 Slice 1 — Local Ingest to a Visible…]] (1 shared connections)

## Source Files

- `crates/lapidary-cad/src/tmf.rs`
- `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md`
- `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6b-design.md`
- `sidecar/occt-bridge/README.md`

## Audit Trail

- EXTRACTED: 89 (88%)
- INFERRED: 12 (12%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*