# Phase 1 Gaps and Search

> 22 nodes

## Key Concepts

- **Send** (14 connections)
- **FakeDbError** (13 connections) — `crates/lapidary-db/src/lib.rs`
- **ViolatedConstraint** (12 connections) — `crates/lapidary-ingest/src/handler.rs`
- **Sync** (9 connections)
- **JobHandler** (9 connections) — `crates/lapidary-jobs/src/handler.rs`
- **Kernel** (8 connections) — `crates/lapidary-cad/src/kernel.rs`
- **Display** (8 connections)
- **.into_error()** (6 connections) — `crates/lapidary-db/src/lib.rs`
- **.into_error()** (6 connections) — `crates/lapidary-ingest/src/handler.rs`
- **StdError** (4 connections)
- **.as_error()** (4 connections) — `crates/lapidary-db/src/lib.rs`
- **.as_error_mut()** (4 connections) — `crates/lapidary-db/src/lib.rs`
- **PartRepository** (4 connections) — `crates/lapidary-db/src/repo.rs`
- **.as_error()** (4 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.as_error_mut()** (4 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.constraint()** (3 connections) — `crates/lapidary-ingest/src/handler.rs`
- **DatabaseError** (2 connections)
- **.message()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **.kind()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **ErrorKind** (2 connections)
- **.kind()** (2 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.message()** (1 connections) — `crates/lapidary-ingest/src/handler.rs`

## Relationships

- [[Storage Paths and IO]] (8 shared connections)
- [[Derive Worker Outcomes]] (6 shared connections)
- [[Stub Crate Shells]] (4 shared connections)
- [[The Open Path Never Touches A Source File…]] (3 shared connections)
- [[Folder and Part Identifiers]] (3 shared connections)
- [[Tests That Claim a Mechanism]] (3 shared connections)
- [[Job Queue and Cancellation]] (3 shared connections)
- [[Mesh Kernel Dispatch]] (2 shared connections)
- [[Self]] (2 shared connections)
- [[download]] (1 shared connections)
- [[Phase 1 Slice 1 — Local Ingest to a Visible…]] (1 shared connections)
- [[Commit Message Gate]] (1 shared connections)

## Source Files

- `crates/lapidary-cad/src/kernel.rs`
- `crates/lapidary-db/src/lib.rs`
- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-ingest/src/handler.rs`
- `crates/lapidary-jobs/src/handler.rs`

## Audit Trail

- EXTRACTED: 122 (99%)
- INFERRED: 1 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*