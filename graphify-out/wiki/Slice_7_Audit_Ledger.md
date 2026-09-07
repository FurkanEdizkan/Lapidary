# Slice 7 Audit Ledger

> 24 nodes

## Key Concepts

- **Blob CAS** (10 connections) — `docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md`
- **Path-Addressed Source Storage** (7 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **GET /api/revisions/{id}/download** (6 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-5-browser.md`
- **Role-Aware Router** (4 connections) — `docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md`
- **GET /api/blob/{blake3}** (4 connections) — `docs/superpowers/plans/2026-09-04-phase-1-slice-3-lod.md`
- **blob.last_accessed_at** (4 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-4-derivatives.md`
- **SourceReader** (4 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-5-browser.md`
- **slugify** (4 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **Hash-First Short-Circuit** (3 connections) — `docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md`
- **JobHandler** (3 connections) — `docs/superpowers/plans/2026-09-03-phase-1-slice-2-jobs.md`
- **Compression** (3 connections) — `docs/superpowers/plans/2026-09-04-phase-1-slice-3b-3mf.md`
- **Phase 1 Slice 5 Handoff** (3 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-5-HANDOFF.md`
- **Storage Layout, Folder Tree and Moves** (3 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **PgFolders** (3 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **JobPayload::MigrateStorage** (3 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **Deliberate StoredBlob Duplication** (2 connections) — `docs/superpowers/plans/2026-09-02-phase-1-slice-1-HANDOFF.md`
- **Stale Progress on a Hidden Tab** (2 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-4-HANDOFF.md`
- **RFC 5987 Content-Disposition** (2 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-5-browser.md`
- **ModelManifest** (2 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **Dual-Layout Reads** (2 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`
- **Reap Keyed on This Job's Bytes** (1 connections) — `docs/superpowers/plans/2026-09-04-phase-1-slice-3-HANDOFF.md`
- **zstd Cold Tier Deferred** (1 connections) — `docs/superpowers/plans/2026-09-05-phase-1-remaining-slices.md`
- **HEAD Warms a Blob** (1 connections) — `docs/superpowers/plans/2026-09-05-phase-1-slice-5-HANDOFF.md`
- **part_move** (1 connections) — `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`

## Relationships

- [[cargo xtask check-deploy]] (5 shared connections)
- [[KernelParams]] (5 shared connections)
- [[Verification Bar and Slice Handoffs]] (3 shared connections)
- [[parts]] (2 shared connections)
- [[Storage Paths and IO]] (2 shared connections)
- [[Phase 1 Gaps and Search]] (1 shared connections)
- [[Path Escape Refusals]] (1 shared connections)
- [[detail]] (1 shared connections)
- [[Folder Tree and Cycles]] (1 shared connections)
- [[slug]] (1 shared connections)

## Source Files

- `docs/superpowers/plans/2026-09-02-phase-1-slice-1-HANDOFF.md`
- `docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md`
- `docs/superpowers/plans/2026-09-03-phase-1-slice-2-jobs.md`
- `docs/superpowers/plans/2026-09-04-phase-1-slice-3-HANDOFF.md`
- `docs/superpowers/plans/2026-09-04-phase-1-slice-3-lod.md`
- `docs/superpowers/plans/2026-09-04-phase-1-slice-3b-3mf.md`
- `docs/superpowers/plans/2026-09-05-phase-1-remaining-slices.md`
- `docs/superpowers/plans/2026-09-05-phase-1-slice-4-HANDOFF.md`
- `docs/superpowers/plans/2026-09-05-phase-1-slice-4-derivatives.md`
- `docs/superpowers/plans/2026-09-05-phase-1-slice-5-HANDOFF.md`
- `docs/superpowers/plans/2026-09-05-phase-1-slice-5-browser.md`
- `docs/superpowers/plans/2026-09-06-folder-tree-and-moves.md`

## Audit Trail

- EXTRACTED: 63 (81%)
- INFERRED: 15 (19%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*