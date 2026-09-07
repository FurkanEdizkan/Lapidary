# Deploy Compose Checks

> 46 nodes

## Key Concepts

- **deploy.rs** (46 connections) — `xtask/src/deploy.rs`
- **check_compose()** (17 connections) — `xtask/src/deploy.rs`
- **check_containerfile()** (12 connections) — `xtask/src/deploy.rs`
- **Violation** (9 connections) — `xtask/src/deploy.rs`
- **check_open_path_boundary()** (9 connections) — `xtask/src/deploy.rs`
- **parse_services()** (7 connections) — `xtask/src/deploy.rs`
- **check()** (5 connections) — `xtask/src/deploy.rs`
- **ServiceBlock** (4 connections) — `xtask/src/deploy.rs`
- **scalar()** (3 connections) — `xtask/src/deploy.rs`
- **is_module()** (2 connections) — `xtask/src/deploy.rs`
- **server_features_on_a_non_listed_service_fails_and_names_it()** (2 connections) — `xtask/src/deploy.rs`
- **worker_missing_server_features_fails()** (2 connections) — `xtask/src/deploy.rs`
- **column_zero_comment_banner_does_not_hide_services_below_it()** (2 connections) — `xtask/src/deploy.rs`
- **kernel_linked_service_absent_from_the_file_entirely_fails_and_names_it()** (2 connections) — `xtask/src/deploy.rs`
- **worker_missing_lapidary_role_fails_and_names_it()** (2 connections) — `xtask/src/deploy.rs`
- **api_missing_lapidary_role_fails_and_names_it()** (2 connections) — `xtask/src/deploy.rs`
- **the_worker_running_the_api_role_fails_even_though_the_key_is_present()** (2 connections) — `xtask/src/deploy.rs`
- **the_build_short_form_is_reported_as_unreadable_not_silently_skipped()** (2 connections) — `xtask/src/deploy.rs`
- **services_not_running_lapidary_server_need_no_lapidary_role()** (2 connections) — `xtask/src/deploy.rs`
- **hardcoded_features_flag_in_build_line_fails()** (2 connections) — `xtask/src/deploy.rs`
- **arg_before_first_from_fails()** (2 connections) — `xtask/src/deploy.rs`
- **arg_after_second_from_fails_even_though_it_is_after_the_first()** (2 connections) — `xtask/src/deploy.rs`
- **lowercase_from_between_arg_and_build_line_hides_the_stage_boundary()** (2 connections) — `xtask/src/deploy.rs`
- **compose_with_no_services_key_fails_as_parse_stale_not_as_clean()** (2 connections) — `xtask/src/deploy.rs`
- **containerfile_with_no_cargo_build_line_fails_as_parse_stale()** (2 connections) — `xtask/src/deploy.rs`
- *... and 21 more nodes in this community*

## Relationships

- [[Prototype Image Slot]] (7 shared connections)
- [[Folder and Part Identifiers]] (4 shared connections)
- [[Storage Paths and IO]] (2 shared connections)
- [[Phase 1 Gaps and Search]] (1 shared connections)
- [[cargo xtask check-deploy]] (1 shared connections)

## Source Files

- `xtask/src/deploy.rs`

## Audit Trail

- EXTRACTED: 172 (99%)
- INFERRED: 1 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*