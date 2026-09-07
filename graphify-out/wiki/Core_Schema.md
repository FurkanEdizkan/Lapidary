# Core Schema

> 9 nodes

## Key Concepts

- **Uuid** (17 connections)
- **PendingSource** (15 connections) — `crates/lapidary-db/src/migrate.rs`
- **HashClaim** (9 connections) — `crates/lapidary-db/src/migrate.rs`
- **migrate.rs** (7 connections) — `crates/lapidary-db/src/migrate.rs`
- **PendingRow** (6 connections) — `crates/lapidary-db/src/migrate.rs`
- **.settle()** (5 connections) — `crates/lapidary-db/src/migrate.rs`
- **DetailColumns** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **.into_source()** (4 connections) — `crates/lapidary-db/src/migrate.rs`
- **.rows()** (2 connections) — `crates/lapidary-db/src/migrate.rs`

## Relationships

- [[Folder and Part Identifiers]] (10 shared connections)
- [[Soft Delete Subtree]] (6 shared connections)
- [[Job Queue and Cancellation]] (4 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Storage Migration]] (3 shared connections)
- [[Router Roles and Errors]] (2 shared connections)
- [[Derive Worker Outcomes]] (2 shared connections)
- [[Grid Row Columns]] (2 shared connections)
- [[Library Scan and Counts]] (2 shared connections)
- [[Storage Paths and IO]] (2 shared connections)
- [[Prototype Image Slot]] (2 shared connections)
- [[detail]] (1 shared connections)

## Source Files

- `crates/lapidary-db/src/migrate.rs`
- `crates/lapidary-db/src/repo.rs`

## Audit Trail

- EXTRACTED: 70 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*