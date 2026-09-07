# Folder Tree Drag Tests

> 25 nodes

## Key Concepts

- **BlobHash** (68 connections)
- **PgBlobs** (21 connections) — `crates/lapidary-db/src/repo.rs`
- **StoredBlobRow** (11 connections) — `crates/lapidary-db/src/repo.rs`
- **sweep()** (10 connections) — `crates/lapidary-ingest/src/reap.rs`
- **.reap()** (9 connections) — `crates/lapidary-db/src/repo.rs`
- **.claim_hash()** (7 connections) — `crates/lapidary-db/src/migrate.rs`
- **unreadable()** (6 connections) — `crates/lapidary-api/src/download.rs`
- **.blob()** (6 connections) — `crates/lapidary-db/src/repo.rs`
- **ReapReport** (6 connections) — `crates/lapidary-db/src/repo.rs`
- **reap.rs** (6 connections) — `crates/lapidary-ingest/src/reap.rs`
- **disambiguate()** (5 connections) — `crates/lapidary-core/src/slug.rs`
- **TessellationRow** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **.library_holds()** (5 connections) — `crates/lapidary-db/src/repo.rs`
- **remove_model_file()** (5 connections) — `crates/lapidary-ingest/src/reap.rs`
- **run()** (5 connections) — `crates/lapidary-ingest/src/reap.rs`
- **last_read_us()** (4 connections) — `crates/lapidary-api/tests/download.rs`
- **.exists()** (4 connections) — `crates/lapidary-db/src/repo.rs`
- **.record_unreferenced()** (4 connections) — `crates/lapidary-db/src/repo.rs`
- **.derivative_is_reachable()** (4 connections) — `crates/lapidary-db/src/repo.rs`
- **.image_is_reachable()** (4 connections) — `crates/lapidary-db/src/repo.rs`
- **.staged()** (3 connections) — `crates/lapidary-api/tests/upload.rs`
- **IngestBlobPayload** (3 connections) — `crates/lapidary-core/src/job.rs`
- **hash_lock_key()** (3 connections) — `crates/lapidary-db/src/migrate.rs`
- **.touch_blob()** (2 connections) — `crates/lapidary-db/src/repo.rs`
- **FnMut** (1 connections)

## Relationships

- [[Storage Paths and IO]] (30 shared connections)
- [[Folder and Part Identifiers]] (17 shared connections)
- [[Soft Delete Subtree]] (11 shared connections)
- [[Derive Worker Outcomes]] (9 shared connections)
- [[Grid Row Columns]] (9 shared connections)
- [[Part Repository and Derivatives]] (8 shared connections)
- [[download]] (5 shared connections)
- [[Image Decode Limits]] (5 shared connections)
- [[Job Queue and Cancellation]] (5 shared connections)
- [[part table]] (3 shared connections)
- [[Core Schema]] (3 shared connections)
- [[blob]] (3 shared connections)

## Source Files

- `crates/lapidary-api/src/download.rs`
- `crates/lapidary-api/tests/download.rs`
- `crates/lapidary-api/tests/upload.rs`
- `crates/lapidary-core/src/job.rs`
- `crates/lapidary-core/src/slug.rs`
- `crates/lapidary-db/src/migrate.rs`
- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-ingest/src/reap.rs`

## Audit Trail

- EXTRACTED: 195 (94%)
- INFERRED: 12 (6%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*