# Quarantine Reap

> 22 nodes

## Key Concepts

- **reap.rs** (24 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **seed_part()** (14 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **retire()** (14 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **seed_part_at()** (11 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **hash_of()** (7 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **stage_bytes()** (5 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_blob_past_its_cutoff_loses_its_row_and_its_bytes()** (5 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **the_sweep_ignores_ref_count_entirely()** (5 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **one_wrongly_quarantined_blob_does_not_stop_the_sweep()** (5 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_directory_holding_something_of_the_owners_survives_and_does_not_stop_the_sweep()** (5 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **library()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **quarantined()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_blob_inside_its_thirty_days_is_not_touched()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_blob_something_points_at_again_is_un_quarantined_rather_than_removed()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_live_parts_bytes_survive_a_counter_that_says_nothing_points_at_them()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **re_ingesting_quarantined_bytes_clears_the_flag_without_waiting_for_a_sweep()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **stage_bytes_at()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_purged_model_directory_is_removed_once_its_thirty_days_are_up()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_model_file_inside_its_thirty_days_is_not_touched()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **a_path_a_live_part_has_claimed_again_is_declined_rather_than_unlinked()** (4 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **bytes_exist()** (3 connections) — `crates/lapidary-ingest/tests/reap.rs`
- **model_dir()** (2 connections) — `crates/lapidary-ingest/tests/reap.rs`

## Relationships

- [[Library Scan and Counts]] (15 shared connections)
- [[Scan Enqueue]] (6 shared connections)
- [[Folder and Part Identifiers]] (4 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Part Repository and Derivatives]] (3 shared connections)
- [[Job Queue and Cancellation]] (1 shared connections)
- [[Prototype Image Slot]] (1 shared connections)
- [[Soft Delete Subtree]] (1 shared connections)

## Source Files

- `crates/lapidary-ingest/tests/reap.rs`

## Audit Trail

- EXTRACTED: 137 (98%)
- INFERRED: 3 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*