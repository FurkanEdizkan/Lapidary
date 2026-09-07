# Resumable Chunked Upload

> 22 nodes

## Key Concepts

- **Server** (24 connections) — `crates/lapidary-api/tests/upload.rs`
- **upload.rs** (16 connections) — `crates/lapidary-api/tests/upload.rs`
- **.commit()** (10 connections) — `crates/lapidary-api/tests/upload.rs`
- **.chunk()** (10 connections) — `crates/lapidary-api/tests/upload.rs`
- **.send()** (8 connections) — `crates/lapidary-api/tests/upload.rs`
- **.post()** (7 connections) — `crates/lapidary-api/tests/upload.rs`
- **.probe()** (6 connections) — `crates/lapidary-api/tests/upload.rs`
- **.upload()** (6 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_probe_answers_need_rows_for_bytes_the_store_already_holds()** (6 connections) — `crates/lapidary-api/tests/upload.rs`
- **committing_stores_the_bytes_records_the_blob_and_queues_the_mesh()** (6 connections) — `crates/lapidary-api/tests/upload.rs`
- **committing_bytes_that_do_not_match_their_claim_stores_nothing()** (5 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_path_that_escapes_is_refused_before_anything_is_written()** (5 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_probe_of_an_empty_library_needs_every_file_and_its_bytes()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_chunk_at_the_wrong_offset_is_refused_and_says_where_to_resume()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **committing_without_transferring_says_to_send_the_bytes()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_hash_that_is_not_a_hash_never_reaches_the_filesystem()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **uploading_into_a_library_that_does_not_exist_is_a_404()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_realistic_chunk_is_not_refused_as_too_large()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **a_chunk_past_the_limit_is_refused_in_words_rather_than_by_the_framework()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **committing_nothing_is_a_success_with_no_batch_to_watch()** (4 connections) — `crates/lapidary-api/tests/upload.rs`
- **library()** (3 connections) — `crates/lapidary-api/tests/upload.rs`
- **the_worker_role_serves_no_upload_route()** (3 connections) — `crates/lapidary-api/tests/upload.rs`

## Relationships

- [[Library Scan and Counts]] (14 shared connections)
- [[Folder Routes]] (5 shared connections)
- [[Router Roles and Errors]] (5 shared connections)
- [[Image Upload and Refusals]] (3 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Scan Enqueue]] (1 shared connections)
- [[Storage Paths and IO]] (1 shared connections)
- [[Soft Delete Subtree]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/upload.rs`

## Audit Trail

- EXTRACTED: 121 (82%)
- INFERRED: 26 (18%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*