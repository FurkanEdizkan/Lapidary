# Part Repository and Derivatives

> 71 nodes

## Key Concepts

- **PgParts** (105 connections) — `crates/lapidary-db/src/repo.rs`
- **PgIngest** (70 connections) — `crates/lapidary-db/src/repo.rs`
- **repo.rs** (67 connections) — `crates/lapidary-db/tests/repo.rs`
- **library()** (51 connections) — `crates/lapidary-db/tests/repo.rs`
- **blob_row()** (34 connections) — `crates/lapidary-db/tests/repo.rs`
- **watertight()** (29 connections) — `crates/lapidary-db/tests/repo.rs`
- **seed_part()** (19 connections) — `crates/lapidary-db/tests/repo.rs`
- **only_revision()** (16 connections) — `crates/lapidary-db/tests/repo.rs`
- **seed_named()** (13 connections) — `crates/lapidary-db/tests/repo.rs`
- **insert_derivative()** (12 connections) — `crates/lapidary-db/tests/repo.rs`
- **seeded_part()** (11 connections) — `crates/lapidary-db/tests/repo.rs`
- **linking_onto_a_rungs_blob_records_the_level_of_the_file_it_wrote()** (10 connections) — `crates/lapidary-db/tests/repo.rs`
- **second_library()** (9 connections) — `crates/lapidary-db/tests/repo.rs`
- **revisions_missing_returns_only_the_revisions_lacking_that_generated_kind()** (9 connections) — `crates/lapidary-db/tests/repo.rs`
- **revision_source_returns_the_source_files_hash_and_format()** (9 connections) — `crates/lapidary-db/tests/repo.rs`
- **the_library_total_shares_derivative_bytes_and_counts_inline_previews_at_all()** (9 connections) — `crates/lapidary-db/tests/repo.rs`
- **the_instance_total_counts_a_shared_derivative_once_where_two_libraries_each_count_it()** (9 connections) — `crates/lapidary-db/tests/repo.rs`
- **a_hash_another_library_holds_is_not_held_by_this_one()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **a_duplicate_ingested_mid_migration_describes_its_own_file_not_the_legacy_blobs()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **upserting_a_thumbnail_twice_leaves_one_row_holding_the_second_bytes()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **upserting_over_the_other_storage_shape_moves_the_reference()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **a_deleted_part_has_nothing_to_download_and_a_live_one_answers_in_full()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **a_hash_addressed_thumbnail_is_refused_rather_than_written()** (8 connections) — `crates/lapidary-db/tests/repo.rs`
- **a_known_hash_is_reported_as_existing()** (7 connections) — `crates/lapidary-db/tests/repo.rs`
- **the_grid_page_returns_newest_first_with_a_thumbnail_hash()** (7 connections) — `crates/lapidary-db/tests/repo.rs`
- *... and 46 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (60 shared connections)
- [[Folder and Part Identifiers]] (24 shared connections)
- [[Grid Row Columns]] (11 shared connections)
- [[Derive Worker Outcomes]] (9 shared connections)
- [[Folder Tree Drag Tests]] (8 shared connections)
- [[Scan Enqueue]] (7 shared connections)
- [[Soft Delete Subtree]] (7 shared connections)
- [[Download Byte Fidelity]] (6 shared connections)
- [[Library Settings and Thumbnails]] (6 shared connections)
- [[Router Roles and Errors]] (4 shared connections)
- [[Folder Tree and Cycles]] (4 shared connections)
- [[stl]] (4 shared connections)

## Source Files

- `crates/lapidary-db/src/repo.rs`
- `crates/lapidary-db/tests/repo.rs`

## Audit Trail

- EXTRACTED: 666 (83%)
- INFERRED: 137 (17%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*