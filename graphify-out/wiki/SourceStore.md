# SourceStore

> 15 nodes

## Key Concepts

- **SourceStore** (9 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **GET /api/revisions/{id}/download?variant=original** (8 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **SourceReader** (7 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **SourceRelocator** (5 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **SourceWriter** (5 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **Compression::{Zstd, AsIs} Source Policy** (4 connections) — `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- **check_open_path_boundary Grep Gate** (4 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **lapidary-storage (L1)** (3 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **Re-Hash the Bytes Before Serving Them** (3 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **blob.zstd_level Decides Decompression** (3 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **Streaming Instead of Buffering** (3 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **The Verification Promise Changes Shape** (2 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **Download Is Not Open** (1 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **RFC 5987 Content-Disposition Filename** (1 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- **Forward Constraint — Row First, Then Bytes** (1 connections) — `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`

## Relationships

- [[Storage Paths and IO]] (4 shared connections)
- [[Job Error Taxonomy]] (4 shared connections)
- [[Database Error Classification]] (2 shared connections)
- [[lapidary-core (L0)]] (1 shared connections)
- [[Pinned Stack and Uploads]] (1 shared connections)
- [[compilerOptions]] (1 shared connections)

## Source Files

- `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md`
- `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md`
- `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`

## Audit Trail

- EXTRACTED: 55 (93%)
- INFERRED: 4 (7%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*