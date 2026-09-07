# Crate Layers and Role Split

> 20 nodes

## Key Concepts

- **job.rs** (23 connections) — `crates/lapidary-core/src/job.rs`
- **.from_row()** (9 connections) — `crates/lapidary-core/src/job.rs`
- **sample_status()** (4 connections) — `crates/lapidary-core/src/job.rs`
- **.to_json()** (3 connections) — `crates/lapidary-core/src/job.rs`
- **ScanAccepted** (2 connections) — `crates/lapidary-core/src/job.rs`
- **a_batch_status_round_trips()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **batch_status_serialises_camel_case_keys_so_the_generated_type_matches()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **an_ingest_blob_payload_round_trips_through_its_row()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **an_ingest_blob_row_missing_its_hash_names_the_kind_and_the_problem()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **an_existing_ingest_row_still_deserialises()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **an_unknown_kind_names_itself()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **a_malformed_payload_does_not_assert_who_wrote_the_row()** (2 connections) — `crates/lapidary-core/src/job.rs`
- **JobState** (1 connections) — `crates/lapidary-core/src/job.rs`
- **Outcome** (1 connections) — `crates/lapidary-core/src/job.rs`
- **job_state_serialises_camel_case_so_the_wire_matches_the_generated_type()** (1 connections) — `crates/lapidary-core/src/job.rs`
- **scan_accepted_serialises_camel_case_keys_so_the_generated_type_matches()** (1 connections) — `crates/lapidary-core/src/job.rs`
- **an_ingest_payload_round_trips_byte_identically()** (1 connections) — `crates/lapidary-core/src/job.rs`
- **a_derive_payload_carries_a_revision_and_one_kind()** (1 connections) — `crates/lapidary-core/src/job.rs`
- **a_scan_directory_payload_is_empty_and_round_trips()** (1 connections) — `crates/lapidary-core/src/job.rs`
- **a_migrate_storage_payload_is_empty_and_round_trips()** (1 connections) — `crates/lapidary-core/src/job.rs`

## Relationships

- [[Soft Delete Subtree]] (6 shared connections)
- [[Folder and Part Identifiers]] (2 shared connections)
- [[Router Roles and Errors]] (2 shared connections)
- [[The Open Path Never Touches A Source File…]] (1 shared connections)
- [[Folder Tree Drag Tests]] (1 shared connections)
- [[Storage Paths and IO]] (1 shared connections)
- [[Self]] (1 shared connections)
- [[Stub Crate Shells]] (1 shared connections)

## Source Files

- `crates/lapidary-core/src/job.rs`

## Audit Trail

- EXTRACTED: 63 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*