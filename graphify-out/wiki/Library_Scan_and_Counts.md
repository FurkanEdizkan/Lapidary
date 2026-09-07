# Library Scan and Counts

> 49 nodes

## Key Concepts

- **PgPool** (487 connections)
- **migrations.rs** (19 connections) — `crates/lapidary-db/tests/migrations.rs`
- **scan()** (7 connections) — `crates/lapidary-api/tests/scan.rs`
- **seed.rs** (7 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **schema.rs** (6 connections) — `crates/lapidary-db/tests/schema.rs`
- **migrate_route.rs** (6 connections) — `crates/lapidary-ingest/tests/migrate_route.rs`
- **migrate()** (6 connections) — `crates/lapidary-ingest/tests/migrate_route.rs`
- **examples()** (6 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **scan.rs** (5 connections) — `crates/lapidary-api/tests/scan.rs`
- **state()** (4 connections) — `crates/lapidary-api/tests/scan.rs`
- **state()** (4 connections) — `crates/lapidary-ingest/tests/migrate_route.rs`
- **scanning_from_the_browser_enqueues_a_scan_directory_job()** (3 connections) — `crates/lapidary-api/tests/scan.rs`
- **scanning_a_library_that_does_not_exist_is_a_404_not_an_accepted_scan()** (3 connections) — `crates/lapidary-api/tests/scan.rs`
- **parts_in()** (3 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **still_deleted()** (3 connections) — `crates/lapidary-ingest/tests/handler.rs`
- **ScanAccepted** (3 connections)
- **the_route_queues_one_job_and_a_second_call_queues_nothing()** (3 connections) — `crates/lapidary-ingest/tests/migrate_route.rs`
- **a_first_run_lands_the_bundled_parts_with_real_measurements()** (3 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **a_second_start_seeds_nothing()** (3 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **a_library_with_history_but_no_parts_is_not_seeded()** (3 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **deleting_every_seeded_part_does_not_bring_them_back()** (3 connections) — `crates/lapidary-ingest/tests/seed.rs`
- **two_parts_at_one_source_path_in_one_library_are_refused()** (2 connections) — `crates/lapidary-db/tests/migrations.rs`
- **two_parts_with_one_name_at_different_paths_are_allowed()** (2 connections) — `crates/lapidary-db/tests/migrations.rs`
- **a_job_that_claims_done_without_an_outcome_is_refused()** (2 connections) — `crates/lapidary-db/tests/migrations.rs`
- **a_job_that_claims_failed_without_a_reason_is_refused()** (2 connections) — `crates/lapidary-db/tests/migrations.rs`
- *... and 24 more nodes in this community*

## Relationships

- [[Ingest and Derive Handlers]] (60 shared connections)
- [[Part Repository and Derivatives]] (60 shared connections)
- [[Job Queue and Cancellation]] (54 shared connections)
- [[Folder Tree and Cycles]] (44 shared connections)
- [[Parts Grid Route]] (34 shared connections)
- [[Image Upload and Refusals]] (33 shared connections)
- [[Router Roles and Errors]] (27 shared connections)
- [[Storage Migration]] (23 shared connections)
- [[Download Byte Fidelity]] (20 shared connections)
- [[stl]] (16 shared connections)
- [[Quarantine Reap]] (15 shared connections)
- [[Resumable Chunked Upload]] (14 shared connections)

## Source Files

- `crates/lapidary-api/tests/scan.rs`
- `crates/lapidary-db/tests/migrations.rs`
- `crates/lapidary-db/tests/schema.rs`
- `crates/lapidary-ingest/tests/handler.rs`
- `crates/lapidary-ingest/tests/migrate_route.rs`
- `crates/lapidary-ingest/tests/seed.rs`

## Audit Trail

- EXTRACTED: 642 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*