# blob

> 15 nodes

## Key Concepts

- **blob.rs** (17 connections) — `crates/lapidary-api/tests/blob.rs`
- **seed_reachable_rung()** (14 connections) — `crates/lapidary-api/tests/blob.rs`
- **get()** (12 connections) — `crates/lapidary-api/tests/blob.rs`
- **last_read_us()** (6 connections) — `crates/lapidary-api/tests/blob.rs`
- **serving_a_blob_records_when_it_was_last_read()** (5 connections) — `crates/lapidary-api/tests/blob.rs`
- **reading_a_blob_twice_moves_the_timestamp_forward()** (5 connections) — `crates/lapidary-api/tests/blob.rs`
- **a_blob_that_is_not_served_is_not_recorded_as_read()** (5 connections) — `crates/lapidary-api/tests/blob.rs`
- **source_hash()** (4 connections) — `crates/lapidary-api/tests/blob.rs`
- **a_referenced_blob_is_served_with_immutable_caching_and_an_etag()** (4 connections) — `crates/lapidary-api/tests/blob.rs`
- **the_worker_role_does_not_serve_blobs()** (4 connections) — `crates/lapidary-api/tests/blob.rs`
- **library()** (3 connections) — `crates/lapidary-api/tests/blob.rs`
- **measurements()** (3 connections) — `crates/lapidary-api/tests/blob.rs`
- **a_blob_on_disk_that_no_derivative_references_is_not_found()** (3 connections) — `crates/lapidary-api/tests/blob.rs`
- **an_unknown_hash_is_not_found_with_the_same_body()** (3 connections) — `crates/lapidary-api/tests/blob.rs`
- **a_head_request_warms_last_accessed_at_the_way_a_get_does()** (3 connections) — `crates/lapidary-api/tests/blob.rs`

## Relationships

- [[Library Scan and Counts]] (10 shared connections)
- [[Folder and Part Identifiers]] (3 shared connections)
- [[Folder Tree Drag Tests]] (3 shared connections)
- [[Image Upload and Refusals]] (1 shared connections)
- [[Storage Paths and IO]] (1 shared connections)
- [[Soft Delete Subtree]] (1 shared connections)
- [[measure]] (1 shared connections)
- [[Scan Enqueue]] (1 shared connections)
- [[Part Repository and Derivatives]] (1 shared connections)
- [[Folder Routes]] (1 shared connections)
- [[Prototype Image Slot]] (1 shared connections)
- [[Download Byte Fidelity]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/blob.rs`

## Audit Trail

- EXTRACTED: 90 (99%)
- INFERRED: 1 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*