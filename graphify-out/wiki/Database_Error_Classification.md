# Database Error Classification

> 29 nodes

## Key Concepts

- **blob table** (9 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **file.storage_path** (7 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **Purge Recomputes ref_count, Never Decrements** (6 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **Quarantine Is a Timestamp, Not a quarantine/ Tree** (6 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **The 30-Day Reaper Sweep** (6 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **quarantined_file table** (6 connections) — `docs/superpowers/specs/2026-09-07-purge-removes-the-model-directory-design.md`
- **Blob CAS (BLAKE3, two-level hex sharding, ref_count)** (5 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **Path-Addressed, Browsable Store** (5 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **Model Directory Naming and Slug Rules** (5 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **migrate_storage Job** (5 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **A Category Rename Never Moves Bytes** (4 connections) — `docs/superpowers/plans/2026-09-07-after-the-folder-tree-what-is-next.md`
- **file table** (4 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **Source Dedup Is Gone, and ref_count Loses an Implication** (4 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **The Schema Is the Safety Property, Not the WHERE Clause** (4 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **ref_count Is Non-Load-Bearing for Destructive Decisions** (4 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **repath_category Job (proposed, later refused)** (3 connections) — `docs/superpowers/plans/2026-09-07-folder-tree-and-moves-followups.md`
- **Orphan Blob Reap Ordering** (3 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **metadata.json — A Self-Describing Store** (3 connections) — `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- **The api Inserts the blob Row at ref_count 0** (3 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **Unlink Before Commit, and Only the File Is Fatal** (3 connections) — `docs/superpowers/specs/2026-09-07-purge-removes-the-model-directory-design.md`
- **Slice 7 Mutation Ledger (M7-1 … M7-5)** (2 connections) — `docs/superpowers/plans/2026-09-06-phase-1-slice-7-HANDOFF.md`
- **Re-Ingest Un-Quarantines Immediately, Not Within the Hour** (2 connections) — `docs/superpowers/plans/2026-09-06-phase-1-slice-7-HANDOFF.md`
- **file.storage_path Stays Nullable** (2 connections) — `docs/superpowers/plans/2026-09-07-after-the-folder-tree-what-is-next.md`
- **The Clock Belongs to the Bytes** (2 connections) — `docs/superpowers/specs/2026-09-07-purge-removes-the-model-directory-design.md`
- **What the Slice 7 Audit Did Not Prove** (1 connections) — `docs/superpowers/plans/2026-09-06-phase-1-slice-7-HANDOFF.md`
- *... and 4 more nodes in this community*

## Relationships

- [[lapidary-core (L0)]] (6 shared connections)
- [[Phase 1 — The Remaining Slices, Re-Cut]] (5 shared connections)
- [[Job Error Taxonomy]] (4 shared connections)
- [[compilerOptions]] (2 shared connections)
- [[SourceStore]] (2 shared connections)
- [[Pinned Stack and Uploads]] (2 shared connections)
- [[Job Payload Round-Trips]] (1 shared connections)

## Source Files

- `docs/superpowers/plans/2026-09-06-phase-1-slice-7-HANDOFF.md`
- `docs/superpowers/plans/2026-09-07-after-the-folder-tree-what-is-next.md`
- `docs/superpowers/plans/2026-09-07-folder-tree-and-moves-followups.md`
- `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- `docs/superpowers/specs/2026-09-07-purge-removes-the-model-directory-design.md`

## Audit Trail

- EXTRACTED: 104 (96%)
- INFERRED: 4 (4%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*