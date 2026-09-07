# PgJobs

> God node · 64 connections · `crates/lapidary-db/src/jobs.rs`

**Community:** [[Job Queue and Cancellation]]

## Connections by Relation

### calls
- [[accept()]] `INFERRED`
- [[batch_status()]] `INFERRED`
- [[.migrate_storage()]] `INFERRED`
- [[batch_events()]] `INFERRED`
- [[migrate()]] `INFERRED`
- [[scan()]] `INFERRED`
- [[.scan_directory()]] `INFERRED`
- [[seeded_finished_batch()]] `INFERRED`
- [[a_worker_dying_mid_scan_loses_only_what_it_held()]] `INFERRED`
- [[drain()]] `INFERRED`
- [[a_running_batch_reports_its_counts()]] `INFERRED`
- [[a_batch_belonging_to_another_library_is_not_found()]] `INFERRED`
- [[a_batch_that_queued_nothing_has_no_status_resource()]] `INFERRED`
- [[the_worker_role_does_not_serve_batch_status()]] `INFERRED`
- [[seed_examples()]] `INFERRED`
- [[releasing_a_workers_leases_moves_every_kind_it_holds_migrate_storage_included()]] `INFERRED`
- [[a_job_past_its_attempt_cap_is_abandoned_without_running_the_handler()]] `INFERRED`
- [[polling_discovers_work_enqueued_while_the_worker_sleeps()]] `INFERRED`
- [[shutting_down_hands_back_what_the_worker_still_holds()]] `INFERRED`
- [[a_batch_id_from_another_library_does_not_resolve()]] `INFERRED`

### contains
- [[jobs.rs]] `EXTRACTED`

### imports_from
- [[jobs.rs]] `EXTRACTED`

### method
- [[.batch_status()]] `EXTRACTED`
- [[.enqueue()]] `EXTRACTED`
- [[.enqueue_into()]] `EXTRACTED`
- [[.enqueue_scan()]] `EXTRACTED`
- [[.active_migration_batch()]] `EXTRACTED`
- [[.complete()]] `EXTRACTED`
- [[.dequeue()]] `EXTRACTED`
- [[.enqueue_migration_if_absent()]] `EXTRACTED`
- [[.reschedule()]] `EXTRACTED`
- [[.fail()]] `EXTRACTED`
- [[.reenqueue_migration_if_absent()]] `EXTRACTED`
- [[.library_has_history()]] `EXTRACTED`
- [[.listener()]] `EXTRACTED`
- [[.release_leases()]] `EXTRACTED`

### references
- [[run()]] `EXTRACTED`
- [[PgJobs]] `INFERRED`

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*