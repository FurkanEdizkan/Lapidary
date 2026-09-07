# Soft Delete Subtree

> 34 nodes

## Key Concepts

- **LibraryId** (115 connections) — `web/src/bindings/LibraryId.ts`
- **DbError** (100 connections) — `crates/lapidary-db/src/lib.rs`
- **BatchId** (27 connections) — `web/src/bindings/BatchId.ts`
- **PgStorageMigration** (9 connections) — `crates/lapidary-db/src/migrate.rs`
- **JobPayload** (8 connections) — `crates/lapidary-core/src/job.rs`
- **BatchStatus** (8 connections) — `crates/lapidary-core/src/job.rs`
- **.enqueue()** (8 connections) — `crates/lapidary-db/src/jobs.rs`
- **.enqueue_into()** (8 connections) — `crates/lapidary-db/src/jobs.rs`
- **.batch_status()** (8 connections) — `crates/lapidary-db/src/jobs.rs`
- **.scan_directory()** (8 connections) — `crates/lapidary-ingest/src/scan.rs`
- **.enqueue_scan()** (7 connections) — `crates/lapidary-db/src/jobs.rs`
- **.enqueue_migration_if_absent()** (6 connections) — `crates/lapidary-db/src/jobs.rs`
- **.active_migration_batch()** (6 connections) — `crates/lapidary-db/src/jobs.rs`
- **.dequeue()** (6 connections) — `crates/lapidary-db/src/jobs.rs`
- **.pending_sources()** (6 connections) — `crates/lapidary-db/src/migrate.rs`
- **.reenqueue_migration_if_absent()** (5 connections) — `crates/lapidary-db/src/jobs.rs`
- **connect()** (5 connections) — `crates/lapidary-db/src/lib.rs`
- **.libraries_needing_migration()** (5 connections) — `crates/lapidary-db/src/migrate.rs`
- **.auto_thumbnail()** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **.libraries()** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **.create_library()** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **.would_cycle()** (4 connections) — `crates/lapidary-db/src/folders.rs`
- **.soft_delete_subtree()** (4 connections) — `crates/lapidary-db/src/folders.rs`
- **.library_has_history()** (4 connections) — `crates/lapidary-db/src/jobs.rs`
- **.listener()** (4 connections) — `crates/lapidary-db/src/jobs.rs`
- *... and 9 more nodes in this community*

## Relationships

- [[Folder and Part Identifiers]] (51 shared connections)
- [[Job Queue and Cancellation]] (26 shared connections)
- [[Storage Paths and IO]] (24 shared connections)
- [[Grid Row Columns]] (20 shared connections)
- [[Derive Worker Outcomes]] (18 shared connections)
- [[Ingest and Derive Handlers]] (11 shared connections)
- [[Folder Tree Drag Tests]] (11 shared connections)
- [[ts-rs Wire Bindings]] (11 shared connections)
- [[Prototype Image Slot]] (9 shared connections)
- [[Router Roles and Errors]] (9 shared connections)
- [[Scan Enqueue]] (8 shared connections)
- [[Library Settings and Thumbnails]] (7 shared connections)

## Source Files

- `crates/lapidary-core/src/job.rs`
- `crates/lapidary-db/src/folders.rs`
- `crates/lapidary-db/src/jobs.rs`
- `crates/lapidary-db/src/lib.rs`
- `crates/lapidary-db/src/migrate.rs`
- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-ingest/src/scan.rs`
- `web/src/bindings/BatchId.ts`
- `web/src/bindings/LibraryId.ts`

## Audit Trail

- EXTRACTED: 396 (99%)
- INFERRED: 6 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*