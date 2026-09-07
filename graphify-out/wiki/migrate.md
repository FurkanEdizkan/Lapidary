# migrate

> 8 nodes

## Key Concepts

- **migrate.rs** (17 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **migrate()** (9 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **accepted()** (4 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **enqueue_failed()** (4 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **manifest_naming()** (4 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **a_failed_move_reaps_its_own_manifest_and_leaves_somebody_elses()** (4 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **a_part()** (3 connections) — `crates/lapidary-ingest/src/migrate.rs`
- **a_permanent_refusal_displaces_a_transient_one_but_not_the_other_way()** (1 connections) — `crates/lapidary-ingest/src/migrate.rs`

## Relationships

- [[Derive Worker Outcomes]] (6 shared connections)
- [[Scan Enqueue]] (4 shared connections)
- [[part table]] (4 shared connections)
- [[Soft Delete Subtree]] (3 shared connections)
- [[detail]] (2 shared connections)
- [[Folder and Part Identifiers]] (2 shared connections)
- [[Library Settings and Thumbnails]] (1 shared connections)
- [[Folder Routes]] (1 shared connections)
- [[Job Queue and Cancellation]] (1 shared connections)

## Source Files

- `crates/lapidary-ingest/src/migrate.rs`

## Audit Trail

- EXTRACTED: 45 (98%)
- INFERRED: 1 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*