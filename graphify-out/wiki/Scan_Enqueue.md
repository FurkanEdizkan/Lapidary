# Scan Enqueue

> 31 nodes

## Key Concepts

- **Path** (101 connections)
- **AppState** (68 connections)
- **State** (37 connections)
- **batch_status()** (11 connections) — `crates/lapidary-api/src/jobs.rs`
- **lifecycle.rs** (10 connections) — `crates/lapidary-api/src/lifecycle.rs`
- **batch_events()** (9 connections) — `crates/lapidary-api/src/jobs.rs`
- **create()** (9 connections) — `crates/lapidary-api/src/sources.rs`
- **health.rs** (8 connections) — `crates/lapidary-api/src/health.rs`
- **list()** (8 connections) — `crates/lapidary-api/src/images.rs`
- **jobs.rs** (8 connections) — `crates/lapidary-api/src/jobs.rs`
- **scan()** (8 connections) — `crates/lapidary-api/src/scan.rs`
- **sources.rs** (8 connections) — `crates/lapidary-api/src/sources.rs`
- **list()** (8 connections) — `crates/lapidary-api/src/sources.rs`
- **scan()** (8 connections) — `crates/lapidary-ingest/src/scan.rs`
- **remove()** (7 connections) — `crates/lapidary-api/src/lifecycle.rs`
- **restore()** (7 connections) — `crates/lapidary-api/src/lifecycle.rs`
- **purge()** (7 connections) — `crates/lapidary-api/src/lifecycle.rs`
- **scan.rs** (7 connections) — `crates/lapidary-ingest/tests/scan.rs`
- **state()** (5 connections) — `crates/lapidary-ingest/tests/scan.rs`
- **scan()** (5 connections) — `crates/lapidary-ingest/tests/scan.rs`
- **healthz()** (4 connections) — `crates/lapidary-api/src/health.rs`
- **internal_error()** (4 connections) — `crates/lapidary-api/src/jobs.rs`
- **enqueue_failed()** (4 connections) — `crates/lapidary-ingest/src/scan.rs`
- **scanning_enqueues_one_scan_directory_job_and_walks_nothing()** (4 connections) — `crates/lapidary-ingest/tests/scan.rs`
- **no_such_batch()** (3 connections) — `crates/lapidary-api/src/jobs.rs`
- *... and 6 more nodes in this community*

## Relationships

- [[part table]] (29 shared connections)
- [[Library Settings and Thumbnails]] (24 shared connections)
- [[Folder Routes]] (18 shared connections)
- [[Image Decode Limits]] (15 shared connections)
- [[Storage Paths and IO]] (15 shared connections)
- [[Directory Scan Cells]] (12 shared connections)
- [[xtask Verify Gates]] (11 shared connections)
- [[ModelManifest]] (10 shared connections)
- [[Folder and Part Identifiers]] (10 shared connections)
- [[Soft Delete Subtree]] (8 shared connections)
- [[Image Upload and Refusals]] (7 shared connections)
- [[Job Queue and Cancellation]] (7 shared connections)

## Source Files

- `crates/lapidary-api/src/health.rs`
- `crates/lapidary-api/src/images.rs`
- `crates/lapidary-api/src/jobs.rs`
- `crates/lapidary-api/src/lifecycle.rs`
- `crates/lapidary-api/src/scan.rs`
- `crates/lapidary-api/src/sources.rs`
- `crates/lapidary-ingest/src/scan.rs`
- `crates/lapidary-ingest/tests/scan.rs`

## Audit Trail

- EXTRACTED: 359 (97%)
- INFERRED: 11 (3%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*