# Download Byte Fidelity

> 32 nodes

## Key Concepts

- **download.rs** (31 connections) — `crates/lapidary-api/tests/download.rs`
- **seed()** (20 connections) — `crates/lapidary-api/tests/download.rs`
- **download_uri()** (19 connections) — `crates/lapidary-api/tests/download.rs`
- **get()** (18 connections) — `crates/lapidary-api/tests/download.rs`
- **ascii_stl()** (16 connections) — `crates/lapidary-api/tests/download.rs`
- **seed_at_path()** (11 connections) — `crates/lapidary-api/tests/download.rs`
- **router** (10 connections) — `web/src/main.tsx`
- **get_streaming()** (9 connections) — `crates/lapidary-api/tests/download.rs`
- **a_nonzero_recorded_level_at_a_folder_path_still_decodes()** (9 connections) — `crates/lapidary-api/tests/download.rs`
- **a_source_blob_missing_from_disk_is_its_own_500()** (8 connections) — `crates/lapidary-api/tests/download.rs`
- **message()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **an_unknown_variant_and_a_missing_one_are_refused_differently()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **a_blob_with_no_recorded_compression_level_is_refused_by_name()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **bytes_that_do_not_hash_to_their_digest_truncate_the_download()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **a_repeated_variant_is_refused_rather_than_resolved()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **a_path_addressed_source_file_missing_from_disk_says_where_to_look()** (7 connections) — `crates/lapidary-api/tests/download.rs`
- **Seeded** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **blob_file()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **a_compressed_source_comes_back_byte_identical()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **a_part_whose_bytes_have_migrated_downloads_from_its_folder_path()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **bytes_at_a_folder_path_that_do_not_hash_truncate_the_download()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **a_turkish_name_is_percent_encoded_in_one_half_and_degraded_in_the_other()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **a_variant_with_nothing_after_the_equals_reads_as_absent()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **a_soft_deleted_part_is_not_found_and_its_blob_stays_cold()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- **the_worker_role_does_not_serve_downloads()** (6 connections) — `crates/lapidary-api/tests/download.rs`
- *... and 7 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (20 shared connections)
- [[Folder and Part Identifiers]] (6 shared connections)
- [[Part Repository and Derivatives]] (6 shared connections)
- [[Scan Enqueue]] (4 shared connections)
- [[Storage Paths and IO]] (4 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Prototype Image Slot]] (3 shared connections)
- [[Derive Worker Outcomes]] (2 shared connections)
- [[Folder Routes]] (2 shared connections)
- [[Folder Tree and Cycles]] (2 shared connections)
- [[Image Upload and Refusals]] (1 shared connections)
- [[Soft Delete Subtree]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/download.rs`
- `crates/lapidary-ingest/src/lib.rs`
- `web/src/main.tsx`

## Audit Trail

- EXTRACTED: 273 (98%)
- INFERRED: 6 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*