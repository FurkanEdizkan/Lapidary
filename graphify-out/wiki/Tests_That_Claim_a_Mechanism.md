# Tests That Claim a Mechanism

> 28 nodes

## Key Concepts

- **lib.rs** (32 connections) — `crates/lapidary-db/src/lib.rs`
- **classify_connect_error()** (15 connections) — `crates/lapidary-db/src/lib.rs`
- **redact_credentials()** (13 connections) — `crates/lapidary-db/src/lib.rs`
- **database_error()** (6 connections) — `crates/lapidary-db/src/lib.rs`
- **.code()** (4 connections) — `crates/lapidary-db/src/lib.rs`
- **quoted_name()** (3 connections) — `crates/lapidary-db/src/lib.rs`
- **Cow** (3 connections)
- **invalid_password_classifies_as_authentication_failed()** (3 connections) — `crates/lapidary-db/src/lib.rs`
- **invalid_authorization_specification_also_classifies_as_authentication_failed()** (3 connections) — `crates/lapidary-db/src/lib.rs`
- **invalid_catalog_name_classifies_as_database_missing_and_names_the_database()** (3 connections) — `crates/lapidary-db/src/lib.rs`
- **an_unrecognised_sqlstate_falls_back_to_unreachable()** (3 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_removes_the_password()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_handles_a_password_containing_an_at_sign()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_is_scoped_to_the_authority_not_the_query_string()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_drops_a_password_carried_in_the_query_string()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_fails_closed_on_an_unencoded_slash_in_the_password()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_fails_closed_on_an_unencoded_question_mark_or_hash_in_the_password()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_fails_closed_on_an_unencoded_slash_in_the_username()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_drops_a_fragment_on_a_well_formed_url()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redaction_drops_a_fragment_when_there_are_no_credentials()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **a_non_database_error_falls_back_to_unreachable()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **no_classified_variant_ever_renders_the_password()** (2 connections) — `crates/lapidary-db/src/lib.rs`
- **redact_credentials** (2 connections) — `docs/superpowers/plans/2026-09-02-phase-0a-followups-execution.md`
- **client_message_passes_through_a_curated_variants_own_text_verbatim()** (1 connections) — `crates/lapidary-db/src/lib.rs`
- **client_message_scrubs_an_opaque_query_error_instead_of_the_wrapped_sqlx_text()** (1 connections) — `crates/lapidary-db/src/lib.rs`
- *... and 3 more nodes in this community*

## Relationships

- [[Soft Delete Subtree]] (7 shared connections)
- [[Stub Crate Shells]] (3 shared connections)
- [[Phase 1 Gaps and Search]] (3 shared connections)
- [[Folder and Part Identifiers]] (3 shared connections)
- [[Library Scan and Counts]] (1 shared connections)
- [[Job Queue and Cancellation]] (1 shared connections)
- [[Storage Paths and IO]] (1 shared connections)

## Source Files

- `crates/lapidary-db/src/lib.rs`
- `docs/superpowers/plans/2026-09-02-phase-0a-followups-execution.md`

## Audit Trail

- EXTRACTED: 115 (98%)
- INFERRED: 2 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*