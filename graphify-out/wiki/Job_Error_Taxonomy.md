# Job Error Taxonomy

> 22 nodes

## Key Concepts

- **lapidary-api (L3)** (12 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **lapidary-db (L1)** (6 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **job table** (6 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **JobHandler trait** (6 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **The Worker Loop** (5 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **Probe / Chunk / Commit Upload Routes** (5 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **bin/lapidary-server** (4 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **LAPIDARY_ROLE Route Split** (4 connections) — `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- **FOR UPDATE SKIP LOCKED Lease Dequeue** (4 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **BatchStatus** (4 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **SSE Batch Events Stream** (4 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6b-design.md`
- **DELETE /api/parts/{id} — Soft Delete** (4 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`
- **LISTEN/NOTIFY as an Optimization Over a Polling Floor** (3 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **lapidary-jobs (L2)** (2 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **sqlx::migrate! Embedded Migrator** (2 connections) — `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- **batch_id Is a Grouping Column, Not a Row** (2 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **Library-Scoped Batch Status Route** (2 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **The Staging File's Length Is the Session State** (2 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- **Lease Reclamation Folded Into the Dequeue** (1 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **Graceful Shutdown Releases Leases** (1 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **Lease Heartbeats Deferred** (1 connections) — `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- **POST /api/parts/{id}/restore** (1 connections) — `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`

## Relationships

- [[Job Payload Round-Trips]] (4 shared connections)
- [[Pinned Stack and Uploads]] (4 shared connections)
- [[SourceStore]] (4 shared connections)
- [[Database Error Classification]] (4 shared connections)
- [[compilerOptions]] (3 shared connections)
- [[CAD Kernel and OCCT Sidecar]] (3 shared connections)
- [[detail (lapidary-api)]] (1 shared connections)

## Source Files

- `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`
- `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`
- `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6a-corpus-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-6b-design.md`
- `docs/superpowers/specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`

## Audit Trail

- EXTRACTED: 77 (95%)
- INFERRED: 4 (5%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*