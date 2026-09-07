# Folder Tree and Cycles

> 54 nodes

## Key Concepts

- **PgFolders** (52 connections) — `crates/lapidary-db/src/folders.rs`
- **folders.rs** (21 connections) — `crates/lapidary-api/tests/folders.rs`
- **folders.rs** (17 connections) — `crates/lapidary-db/tests/folders.rs`
- **send()** (16 connections) — `crates/lapidary-api/tests/folders.rs`
- **moves.rs** (16 connections) — `crates/lapidary-api/tests/moves.rs`
- **move_request()** (16 connections) — `crates/lapidary-api/tests/moves.rs`
- **library()** (16 connections) — `crates/lapidary-db/tests/folders.rs`
- **seed_model()** (15 connections) — `crates/lapidary-api/tests/moves.rs`
- **patch_folder()** (12 connections) — `crates/lapidary-api/tests/folders.rs`
- **json_request()** (10 connections) — `crates/lapidary-api/tests/folders.rs`
- **library()** (10 connections) — `crates/lapidary-api/tests/moves.rs`
- **library()** (9 connections) — `crates/lapidary-api/tests/folders.rs`
- **deep_chain()** (9 connections) — `crates/lapidary-db/tests/folders.rs`
- **seed_part()** (8 connections) — `crates/lapidary-api/tests/folders.rs`
- **the_tree_counts_every_model_under_a_category_and_no_deleted_one()** (7 connections) — `crates/lapidary-api/tests/folders.rs`
- **a_model_the_storage_migration_has_not_reached_cannot_be_moved()** (7 connections) — `crates/lapidary-api/tests/moves.rs`
- **the_history_records_both_ends_of_a_move()** (7 connections) — `crates/lapidary-api/tests/moves.rs`
- **tree()** (6 connections) — `crates/lapidary-api/tests/folders.rs`
- **a_folder_cannot_be_parented_into_another_library()** (6 connections) — `crates/lapidary-api/tests/folders.rs`
- **a_rename_alone_does_not_move_a_category_to_the_root()** (6 connections) — `crates/lapidary-api/tests/folders.rs`
- **deleting_a_folder_hides_its_models_and_touches_no_file()** (6 connections) — `crates/lapidary-api/tests/folders.rs`
- **app()** (6 connections) — `crates/lapidary-api/tests/moves.rs`
- **a_move_changes_where_a_model_is_and_never_what_it_is()** (6 connections) — `crates/lapidary-api/tests/moves.rs`
- **a_failed_rename_leaves_the_database_untouched()** (6 connections) — `crates/lapidary-api/tests/moves.rs`
- **a_store_that_cannot_take_the_directory_says_so_without_naming_the_disk()** (6 connections) — `crates/lapidary-api/tests/moves.rs`
- *... and 29 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (44 shared connections)
- [[Folder and Part Identifiers]] (23 shared connections)
- [[Folder Routes]] (7 shared connections)
- [[Image Upload and Refusals]] (6 shared connections)
- [[Soft Delete Subtree]] (6 shared connections)
- [[Router Roles and Errors]] (5 shared connections)
- [[Part Repository and Derivatives]] (4 shared connections)
- [[Scan Enqueue]] (3 shared connections)
- [[Download Byte Fidelity]] (2 shared connections)
- [[Prototype Image Slot]] (2 shared connections)
- [[Derive Worker Outcomes]] (2 shared connections)
- [[Storage Migration]] (2 shared connections)

## Source Files

- `crates/lapidary-api/tests/folders.rs`
- `crates/lapidary-api/tests/moves.rs`
- `crates/lapidary-db/src/folders.rs`
- `crates/lapidary-db/tests/folders.rs`

## Audit Trail

- EXTRACTED: 345 (83%)
- INFERRED: 73 (17%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*