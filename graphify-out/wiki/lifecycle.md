# lifecycle

> 17 nodes

## Key Concepts

- **main.rs** (20 connections) — `bin/lapidary-server/src/main.rs`
- **worker_router()** (8 connections) — `bin/lapidary-server/src/main.rs`
- **spawn_worker()** (7 connections) — `bin/lapidary-server/src/main.rs`
- **main()** (7 connections) — `bin/lapidary-server/src/main.rs`
- **Config** (6 connections) — `bin/lapidary-server/src/main.rs`
- **enqueue_pending_migrations()** (5 connections) — `bin/lapidary-server/src/main.rs`
- **seed_examples()** (4 connections) — `bin/lapidary-server/src/main.rs`
- **shutdown_signal()** (3 connections) — `bin/lapidary-server/src/main.rs`
- **the_worker_router_serves_both_health_and_scan()** (3 connections) — `bin/lapidary-server/src/main.rs`
- **enqueue_pending_migrations_queues_exactly_one_job_for_a_library_with_un_migrated_files()** (3 connections) — `bin/lapidary-server/src/main.rs`
- **enqueue_pending_migrations_queues_nothing_for_a_library_with_nothing_to_migrate()** (3 connections) — `bin/lapidary-server/src/main.rs`
- **default_bind()** (2 connections) — `bin/lapidary-server/src/main.rs`
- **kernel_description()** (2 connections) — `bin/lapidary-server/src/main.rs`
- **the_api_role_router_serves_its_own_scan_and_not_ingests()** (2 connections) — `bin/lapidary-server/src/main.rs`
- **JoinHandle** (1 connections)
- **an_empty_worker_variable_is_treated_as_unset_rather_than_as_a_bad_value()** (1 connections) — `bin/lapidary-server/src/main.rs`
- **kernel_description_reports_the_mock_implementation_when_the_feature_is_on()** (1 connections) — `bin/lapidary-server/src/main.rs`

## Relationships

- [[Library Scan and Counts]] (8 shared connections)
- [[Storage Paths and IO]] (6 shared connections)
- [[Folder and Part Identifiers]] (5 shared connections)
- [[Stub Crate Shells]] (2 shared connections)
- [[Job Queue and Cancellation]] (2 shared connections)
- [[Image Upload and Refusals]] (1 shared connections)
- [[Core Schema]] (1 shared connections)
- [[Download Byte Fidelity]] (1 shared connections)

## Source Files

- `bin/lapidary-server/src/main.rs`

## Audit Trail

- EXTRACTED: 78 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*