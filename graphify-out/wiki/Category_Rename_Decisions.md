# Category Rename Decisions

> 23 nodes

## Key Concepts

- **Hash First, Always** (7 connections) — `CLAUDE.md`
- **Sources Are Path-Addressed, Not Content-Addressed** (5 connections) — `docs/DATA.md`
- **External Round-Trip Watcher Rules** (5 connections) — `docs/DATA.md`
- **Per-Role Compression With Trained Zstd Dictionaries** (4 connections) — `docs/DATA.md`
- **Nightly Tiering Job** (4 connections) — `docs/DATA.md`
- **Two Quarantines, One Timer** (4 connections) — `docs/DATA.md`
- **folder.slug Is The Address, folder.name Is The Label** (3 connections) — `docs/DATA.md`
- **Deletion Is Three Steps** (3 connections) — `docs/DATA.md`
- **Phase 4 — Versioning, Agent, Round-Trip** (3 connections) — `docs/ROADMAP.md`
- **Uploads Volume Outside The Storage Root** (2 connections) — `deploy/compose.yaml`
- **Pinned Tech Stack** (2 connections) — `docs/ARCHITECTURE.md`
- **Three Storage Classes, Three Lifecycles** (2 connections) — `docs/DATA.md`
- **Batched Access Tracking** (2 connections) — `docs/DATA.md`
- **Hash First, Client-Side, Then Probe** (2 connections) — `docs/DATA.md`
- **Two Payment Rails, Both Required** (2 connections) — `docs/ROADMAP.md`
- **Roadmap Open Items** (2 connections) — `docs/ROADMAP.md`
- **The Prototype Never Hashed Anything** (2 connections) — `docs/prototype-notes.md`
- **Library Scan And The Debounce That Never Existed** (2 connections) — `docs/prototype-notes.md`
- **Scanning Is Idempotent** (1 connections) — `README.md`
- **Content Addressing Survives For blobs/ Only** (1 connections) — `docs/DATA.md`
- **Move History: Route Only, Deliberately Unread** (1 connections) — `docs/FEATURES.md`
- **Law 4691 Technology Development Zone Exemption** (1 connections) — `docs/ROADMAP.md`
- **ingestMesh Ordering And Two Callers** (1 connections) — `docs/prototype-notes.md`

## Relationships

- [[LOD Ladder — L0 At Ingest, L1 And L2 On…]] (5 shared connections)
- [[Product Rules and Their Reasons]] (1 shared connections)
- [[Documentation Map]] (1 shared connections)
- [[lib]] (1 shared connections)
- [[Things That Look Like Bugs But Are Decisions]] (1 shared connections)

## Source Files

- `CLAUDE.md`
- `README.md`
- `deploy/compose.yaml`
- `docs/ARCHITECTURE.md`
- `docs/DATA.md`
- `docs/FEATURES.md`
- `docs/ROADMAP.md`
- `docs/prototype-notes.md`

## Audit Trail

- EXTRACTED: 54 (89%)
- INFERRED: 7 (11%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*