# Storage Migration

> 33 nodes

## Key Concepts

- **migrate.rs** (35 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **seed_cas_part()** (23 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **seeded()** (16 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **handler_over()** (16 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **migrate_job()** (14 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_second_runner_may_not_reap_a_move_the_first_committed()** (12 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **an_interrupted_migration_loses_no_file()** (10 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **settling_one_shared_blob_is_all_or_nothing()** (10 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_second_runner_leaves_no_duplicate_model_directory()** (9 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **storage_path_of()** (8 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **two_categories_that_slug_alike_get_two_directories()** (8 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **two_libraries_sharing_one_blob_settle_together()** (8 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **one_runner_holds_a_hash_and_the_next_one_is_told_so()** (8 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **second_library()** (7 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **downloads_as()** (7 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **re_running_a_drained_migration_is_a_no_op()** (7 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **another_librarys_backlog_does_not_keep_this_job_running()** (7 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_blob_that_does_not_match_its_hash_is_refused_rather_than_copied()** (7 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **cas_rel()** (6 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_migrated_file_is_really_uncompressed_and_its_row_says_so()** (6 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_back_filled_category_ends_up_at_a_windows_safe_path()** (6 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **a_category_whose_disambiguated_slug_is_taken_too_does_not_stall_the_library()** (6 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **wait_for_lock_waiters()** (6 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **migrate_storage_moves_a_cas_blob_into_a_model_directory()** (5 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- **lock_waiters()** (5 connections) — `crates/lapidary-ingest/tests/migrate.rs`
- *... and 8 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (23 shared connections)
- [[Soft Delete Subtree]] (6 shared connections)
- [[Folder and Part Identifiers]] (5 shared connections)
- [[Scan Enqueue]] (4 shared connections)
- [[Core Schema]] (3 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Grid Row Columns]] (2 shared connections)
- [[Job Queue and Cancellation]] (2 shared connections)
- [[Folder Tree and Cycles]] (2 shared connections)
- [[Part Repository and Derivatives]] (2 shared connections)
- [[detail]] (1 shared connections)
- [[Phase 1 Gaps and Search]] (1 shared connections)

## Source Files

- `crates/lapidary-ingest/tests/migrate.rs`

## Audit Trail

- EXTRACTED: 277 (98%)
- INFERRED: 6 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*