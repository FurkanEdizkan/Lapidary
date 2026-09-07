# Router Roles and Errors

> 37 nodes

## Key Concepts

- **Value** (48 connections)
- **derive.rs** (30 connections) — `crates/lapidary-api/tests/derive.rs`
- **library()** (14 connections) — `crates/lapidary-api/tests/derive.rs`
- **seed_part()** (14 connections) — `crates/lapidary-api/tests/derive.rs`
- **send()** (14 connections) — `crates/lapidary-api/tests/derive.rs`
- **patch_library()** (12 connections) — `crates/lapidary-api/tests/derive.rs`
- **post_part_thumbnail()** (12 connections) — `crates/lapidary-api/tests/derive.rs`
- **post_sweep()** (12 connections) — `crates/lapidary-api/tests/derive.rs`
- **get_batch()** (12 connections) — `crates/lapidary-api/tests/derive.rs`
- **get_library()** (11 connections) — `crates/lapidary-api/tests/derive.rs`
- **Role** (10 connections) — `crates/lapidary-api/src/lib.rs`
- **a_batch_is_enqueued_under_the_parts_own_library_and_no_other()** (9 connections) — `crates/lapidary-api/tests/derive.rs`
- **accepted_batch()** (8 connections) — `crates/lapidary-api/tests/derive.rs`
- **a_sweep_counts_only_the_revisions_of_the_library_it_names()** (8 connections) — `crates/lapidary-api/tests/derive.rs`
- **the_worker_role_serves_none_of_these_routes()** (8 connections) — `crates/lapidary-api/tests/derive.rs`
- **one_parts_thumbnail_is_a_batch_of_one_the_existing_poll_can_read()** (7 connections) — `crates/lapidary-api/tests/derive.rs`
- **a_sweep_queues_one_job_for_each_revision_with_no_thumbnail()** (7 connections) — `crates/lapidary-api/tests/derive.rs`
- **a_library_with_nothing_missing_queues_nothing_and_that_is_a_success()** (7 connections) — `crates/lapidary-api/tests/derive.rs`
- **lib.rs** (6 connections) — `crates/lapidary-api/src/lib.rs`
- **other_library()** (6 connections) — `crates/lapidary-api/tests/derive.rs`
- **patching_one_library_leaves_another_tenants_setting_alone()** (6 connections) — `crates/lapidary-api/tests/derive.rs`
- **a_library_that_renders_nothing_reads_back_as_off()** (5 connections) — `crates/lapidary-api/tests/derive.rs`
- **a_deleted_part_is_not_worth_rendering_and_answers_the_same_404()** (5 connections) — `crates/lapidary-api/tests/derive.rs`
- **.from_env_str()** (4 connections) — `crates/lapidary-api/src/lib.rs`
- **turning_the_ingest_thumbnail_off_answers_with_the_value_that_landed()** (4 connections) — `crates/lapidary-api/tests/derive.rs`
- *... and 12 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (27 shared connections)
- [[Soft Delete Subtree]] (9 shared connections)
- [[Folder and Part Identifiers]] (7 shared connections)
- [[Parts Grid Route]] (6 shared connections)
- [[Image Upload and Refusals]] (6 shared connections)
- [[Folder Routes]] (6 shared connections)
- [[xtask Verify Gates]] (5 shared connections)
- [[Folder Tree and Cycles]] (5 shared connections)
- [[Resumable Chunked Upload]] (5 shared connections)
- [[Part Repository and Derivatives]] (4 shared connections)
- [[Job Queue and Cancellation]] (3 shared connections)
- [[Storage Paths and IO]] (2 shared connections)

## Source Files

- `crates/lapidary-api/src/error.rs`
- `crates/lapidary-api/src/lib.rs`
- `crates/lapidary-api/tests/derive.rs`

## Audit Trail

- EXTRACTED: 311 (99%)
- INFERRED: 4 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*