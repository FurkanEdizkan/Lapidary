# Derive Worker Outcomes

> 39 nodes

## Key Concepts

- **HandlerError** (32 connections) — `crates/lapidary-jobs/src/handler.rs`
- **RevisionId** (27 connections) — `web/src/bindings/RevisionId.ts`
- **.index()** (18 connections) — `crates/lapidary-ingest/src/handler.rs`
- **Outcome** (15 connections)
- **handler.rs** (13 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.model_dir_for()** (13 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.derive_one()** (12 connections) — `crates/lapidary-ingest/src/derive.rs`
- **.migrate_storage()** (11 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **WorkerHandler** (9 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.ingest_blob()** (9 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.migrate_one_hash()** (9 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **.copy_into_model_dir()** (9 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **classify_write()** (8 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.reslug_back_filled_categories()** (8 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **.revisions_missing()** (7 connections) — `crates/lapidary-db/src/repo.rs`
- **.handle()** (7 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.ingest_one()** (7 connections) — `crates/lapidary-ingest/src/handler.rs`
- **reap_copy()** (7 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **.latest_revision()** (6 connections) — `crates/lapidary-db/src/repo.rs`
- **classify_db()** (5 connections) — `crates/lapidary-ingest/src/handler.rs`
- **.handle()** (5 connections) — `crates/lapidary-jobs/tests/resumption.rs`
- **.handle()** (5 connections) — `crates/lapidary-jobs/tests/worker.rs`
- **.handle()** (5 connections) — `crates/lapidary-jobs/tests/worker.rs`
- **missing()** (4 connections) — `crates/lapidary-ingest/src/derive.rs`
- **reap_source()** (4 connections) — `crates/lapidary-ingest/src/handler.rs`
- *... and 14 more nodes in this community*

## Relationships

- [[Storage Paths and IO]] (23 shared connections)
- [[Soft Delete Subtree]] (18 shared connections)
- [[Folder and Part Identifiers]] (13 shared connections)
- [[Job Queue and Cancellation]] (9 shared connections)
- [[Part Repository and Derivatives]] (9 shared connections)
- [[Folder Tree Drag Tests]] (9 shared connections)
- [[Ingest and Derive Handlers]] (8 shared connections)
- [[Prototype Image Slot]] (6 shared connections)
- [[Phase 1 Gaps and Search]] (6 shared connections)
- [[migrate]] (6 shared connections)
- [[ts-rs Wire Bindings]] (5 shared connections)
- [[The Open Path Never Touches A Source File…]] (3 shared connections)

## Source Files

- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-ingest/src/derive.rs`
- `crates/lapidary-ingest/src/handler.rs`
- `crates/lapidary-ingest/src/migrate.rs`
- `crates/lapidary-jobs/src/handler.rs`
- `crates/lapidary-jobs/tests/resumption.rs`
- `crates/lapidary-jobs/tests/worker.rs`
- `web/src/bindings/RevisionId.ts`

## Audit Trail

- EXTRACTED: 271 (93%)
- INFERRED: 20 (7%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*