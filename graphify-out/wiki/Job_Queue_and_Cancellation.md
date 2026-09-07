# Job Queue and Cancellation

> 93 nodes

## Key Concepts

- **PgJobs** (64 connections) — `crates/lapidary-db/src/jobs.rs`
- **jobs.rs** (32 connections) — `crates/lapidary-db/tests/jobs.rs`
- **seeded()** (28 connections) — `crates/lapidary-db/tests/jobs.rs`
- **jobs.rs** (16 connections) — `crates/lapidary-api/tests/jobs.rs`
- **Duration** (15 connections)
- **worker.rs** (15 connections) — `crates/lapidary-jobs/tests/worker.rs`
- **resumption.rs** (14 connections) — `crates/lapidary-jobs/tests/resumption.rs`
- **get_status()** (13 connections) — `crates/lapidary-api/tests/jobs.rs`
- **jobs.rs** (10 connections) — `crates/lapidary-db/src/jobs.rs`
- **WorkerHandler** (10 connections)
- **policy.rs** (10 connections) — `crates/lapidary-jobs/src/policy.rs`
- **run()** (10 connections) — `crates/lapidary-jobs/src/worker.rs`
- **CancellationToken** (9 connections)
- **next_state()** (9 connections) — `crates/lapidary-jobs/src/policy.rs`
- **seeded()** (8 connections) — `crates/lapidary-api/tests/jobs.rs`
- **events()** (8 connections) — `crates/lapidary-api/tests/jobs.rs`
- **WorkerConfig** (8 connections) — `crates/lapidary-jobs/src/worker.rs`
- **seeded_finished_batch()** (7 connections) — `crates/lapidary-api/tests/jobs.rs`
- **worker.rs** (7 connections) — `crates/lapidary-jobs/src/worker.rs`
- **a_worker_dying_mid_scan_loses_only_what_it_held()** (7 connections) — `crates/lapidary-jobs/tests/resumption.rs`
- **drain()** (7 connections) — `crates/lapidary-jobs/tests/worker.rs`
- **JobId** (7 connections) — `web/src/bindings/JobId.ts`
- **.complete()** (6 connections) — `crates/lapidary-db/src/jobs.rs`
- **.reschedule()** (6 connections) — `crates/lapidary-db/src/jobs.rs`
- **insert_job()** (6 connections) — `crates/lapidary-db/tests/jobs.rs`
- *... and 68 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (54 shared connections)
- [[Soft Delete Subtree]] (26 shared connections)
- [[Derive Worker Outcomes]] (9 shared connections)
- [[Scan Enqueue]] (7 shared connections)
- [[Storage Paths and IO]] (6 shared connections)
- [[Folder and Part Identifiers]] (6 shared connections)
- [[Folder Tree Drag Tests]] (5 shared connections)
- [[Ingest and Derive Handlers]] (4 shared connections)
- [[Core Schema]] (4 shared connections)
- [[Router Roles and Errors]] (3 shared connections)
- [[Prototype Image Slot]] (3 shared connections)
- [[Phase 1 Gaps and Search]] (3 shared connections)

## Source Files

- `crates/lapidary-api/tests/jobs.rs`
- `crates/lapidary-db/src/jobs.rs`
- `crates/lapidary-db/tests/jobs.rs`
- `crates/lapidary-ingest/src/seed.rs`
- `crates/lapidary-jobs/src/policy.rs`
- `crates/lapidary-jobs/src/worker.rs`
- `crates/lapidary-jobs/tests/resumption.rs`
- `crates/lapidary-jobs/tests/worker.rs`
- `web/src/bindings/JobId.ts`

## Audit Trail

- EXTRACTED: 504 (85%)
- INFERRED: 90 (15%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*