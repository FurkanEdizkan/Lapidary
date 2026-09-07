# Prototype Image Slot

> 26 nodes

## Key Concepts

- **Vec** (101 connections)
- **scan.rs** (18 connections) — `crates/lapidary-ingest/src/scan.rs`
- **walk()** (12 connections) — `crates/lapidary-ingest/src/scan.rs`
- **candidates()** (8 connections) — `crates/lapidary-ingest/src/scan.rs`
- **index_at()** (7 connections) — `crates/lapidary-cad/src/cluster.rs`
- **PartImageRow** (7 connections) — `crates/lapidary-db/src/repo.rs`
- **.tree()** (6 connections) — `crates/lapidary-db/src/folders.rs`
- **.moves()** (6 connections) — `crates/lapidary-db/src/repo.rs`
- **.part_images()** (6 connections) — `crates/lapidary-db/src/repo.rs`
- **FsPath** (6 connections)
- **ingest_dir_unreadable()** (6 connections) — `crates/lapidary-ingest/src/scan.rs`
- **entry_read_failure()** (6 connections) — `crates/lapidary-ingest/src/scan.rs`
- **cell_of()** (5 connections) — `crates/lapidary-cad/src/cluster.rs`
- **relative_to()** (5 connections) — `crates/lapidary-ingest/src/scan.rs`
- **queued()** (5 connections) — `crates/lapidary-ingest/tests/scan.rs`
- **Counted** (4 connections) — `crates/lapidary-cad/src/tmf.rs`
- **is_mesh_candidate()** (4 connections) — `crates/lapidary-ingest/src/scan.rs`
- **quarantined_paths()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **logical_lines()** (4 connections) — `xtask/src/deploy.rs`
- **UploadPlan** (3 connections) — `crates/lapidary-api/src/upload.rs`
- **EntryReadFailure** (3 connections) — `crates/lapidary-ingest/src/scan.rs`
- **Cell** (2 connections)
- **.read()** (2 connections) — `crates/lapidary-cad/src/tmf.rs`
- **entry_read_failure_names_the_directory_and_does_not_invent_a_filename()** (2 connections) — `crates/lapidary-ingest/src/scan.rs`
- **an_unreadable_ingest_directory_is_permanent_so_the_browser_sees_it_at_once()** (2 connections) — `crates/lapidary-ingest/src/scan.rs`
- *... and 1 more nodes in this community*

## Relationships

- [[Folder and Part Identifiers]] (22 shared connections)
- [[Storage Paths and IO]] (14 shared connections)
- [[xtask Verify Gates]] (10 shared connections)
- [[Soft Delete Subtree]] (9 shared connections)
- [[Deploy Compose Checks]] (7 shared connections)
- [[Blob CAS and Hash-First]] (7 shared connections)
- [[3MF Parsing and Bombs]] (7 shared connections)
- [[Derive Worker Outcomes]] (6 shared connections)
- [[Ingest and Derive Handlers]] (5 shared connections)
- [[SSRF Fetch Guard]] (4 shared connections)
- [[Approximate Measurement Rules]] (4 shared connections)
- [[Scan Enqueue]] (4 shared connections)

## Source Files

- `crates/lapidary-api/src/upload.rs`
- `crates/lapidary-cad/src/cluster.rs`
- `crates/lapidary-cad/src/tmf.rs`
- `crates/lapidary-db/src/folders.rs`
- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-ingest/src/scan.rs`
- `crates/lapidary-ingest/tests/reap.rs`
- `crates/lapidary-ingest/tests/scan.rs`
- `xtask/src/deploy.rs`

## Audit Trail

- EXTRACTED: 235 (100%)
- INFERRED: 1 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*