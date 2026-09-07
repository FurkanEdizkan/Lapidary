# Ingest and Derive Handlers

> 67 nodes

## Key Concepts

- **handler.rs** (76 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **handler_over()** (35 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **job_for()** (23 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **JobRow** (19 connections) — `crates/lapidary-db/src/jobs.rs`
- **seeded()** (17 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **stage()** (13 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **derivatives()** (9 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_derive_job_naming_another_librarys_revision_renders_nothing()** (9 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **scanned_paths()** (9 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_derive_job_fills_the_missing_thumbnail_and_reports_rendered()** (8 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_derived_l0_is_byte_identical_to_the_one_ingest_wrote()** (8 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **derive_job_for()** (7 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **only_revision()** (7 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **all_files()** (7 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **blob_job()** (7 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_library_that_declines_to_render_gets_no_thumbnail_and_still_fills_the_grid()** (7 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **derive_job()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **stop_rendering()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **upload_into()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_known_hash_is_skipped_before_the_kernel_ever_sees_the_bytes()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_failure_after_the_blob_write_leaves_no_orphan_blob_on_disk()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **a_failed_link_to_existing_bytes_leaves_the_first_parts_blobs_alone()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **each_rung_is_valid_gltf_and_l0_is_smaller_than_l2()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **scan_job()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **ingest_paths_in()** (6 connections) — `crates/lapidary-ingest/tests/handler.rs`
- *... and 42 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (60 shared connections)
- [[Soft Delete Subtree]] (11 shared connections)
- [[Derive Worker Outcomes]] (8 shared connections)
- [[Scan Enqueue]] (7 shared connections)
- [[Folder and Part Identifiers]] (6 shared connections)
- [[Prototype Image Slot]] (5 shared connections)
- [[Job Queue and Cancellation]] (4 shared connections)
- [[Part Repository and Derivatives]] (3 shared connections)
- [[detail]] (2 shared connections)
- [[The Open Path Never Touches A Source File…]] (2 shared connections)
- [[Folder Tree Drag Tests]] (2 shared connections)
- [[Router Roles and Errors]] (1 shared connections)

## Source Files

- `crates/lapidary-db/src/jobs.rs`
- `crates/lapidary-ingest/tests/handler.rs`

## Audit Trail

- EXTRACTED: 479 (99%)
- INFERRED: 4 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*