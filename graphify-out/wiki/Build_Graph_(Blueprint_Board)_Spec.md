# Build Graph (Blueprint Board) Spec

> 10 nodes

## Key Concepts

- **health.rs** (10 connections) — `crates/lapidary-api/tests/health.rs`
- **blob_root()** (9 connections) — `crates/lapidary-api/tests/health.rs`
- **healthz_reports_ok_and_the_postgres_major_version()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **healthz_says_what_broke_and_what_to_do_when_the_database_is_gone()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **unknown_routes_are_not_found()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **health_is_served_in_both_roles()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **the_scan_trigger_is_on_the_api_role_and_only_there()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **the_worker_role_does_not_serve_the_grid()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **the_api_role_serves_the_grid()** (3 connections) — `crates/lapidary-api/tests/health.rs`
- **an_unknown_role_is_rejected_with_the_valid_values()** (1 connections) — `crates/lapidary-api/tests/health.rs`

## Relationships

- [[Library Scan and Counts]] (7 shared connections)
- [[Image Upload and Refusals]] (1 shared connections)
- [[Storage Paths and IO]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/health.rs`

## Audit Trail

- EXTRACTED: 41 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*