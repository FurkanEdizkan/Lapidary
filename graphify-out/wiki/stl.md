# stl

> 18 nodes

## Key Concepts

- **lifecycle.rs** (19 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **call()** (14 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **seed()** (13 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **deleting_a_part_hides_it_from_every_read_path_and_restoring_brings_it_all_back()** (7 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **seed_sharing()** (7 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **grid_names()** (6 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **purge_corrects_a_reference_count_that_had_drifted()** (6 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **a_second_delete_is_not_a_success_and_a_live_part_cannot_be_restored()** (5 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **purging_one_of_two_parts_over_the_same_blob_quarantines_nothing()** (5 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **library()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **state()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **keeps_ids_stable()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **purge_refuses_to_be_the_first_step()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **the_removed_list_is_the_only_route_back_to_a_deleted_part()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **the_storage_panel_does_not_report_a_removal_as_a_saving()** (4 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **source_bytes()** (3 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **a_purged_part_leaves_no_rows_behind_it()** (3 connections) — `crates/lapidary-api/tests/lifecycle.rs`
- **blob_state()** (2 connections) — `crates/lapidary-api/tests/lifecycle.rs`

## Relationships

- [[Library Scan and Counts]] (16 shared connections)
- [[Folder and Part Identifiers]] (6 shared connections)
- [[Part Repository and Derivatives]] (4 shared connections)
- [[Image Upload and Refusals]] (1 shared connections)
- [[Soft Delete Subtree]] (1 shared connections)
- [[Scan Enqueue]] (1 shared connections)
- [[Folder Routes]] (1 shared connections)
- [[Router Roles and Errors]] (1 shared connections)
- [[Prototype Image Slot]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/lifecycle.rs`

## Audit Trail

- EXTRACTED: 110 (96%)
- INFERRED: 4 (4%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*