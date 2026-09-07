# Image Upload and Refusals

> 48 nodes

## Key Concepts

- **Body** (34 connections)
- **images.rs** (22 connections) — `crates/lapidary-api/tests/images.rs`
- **send()** (18 connections) — `crates/lapidary-api/tests/images.rs`
- **state()** (16 connections) — `crates/lapidary-api/tests/images.rs`
- **seed_part()** (16 connections) — `crates/lapidary-api/tests/images.rs`
- **Request** (14 connections)
- **upload()** (14 connections) — `crates/lapidary-api/tests/images.rs`
- **send()** (13 connections) — `crates/lapidary-api/tests/libraries.rs`
- **libraries.rs** (12 connections) — `crates/lapidary-api/tests/libraries.rs`
- **sources.rs** (12 connections) — `crates/lapidary-api/tests/sources.rs`
- **send()** (12 connections) — `crates/lapidary-api/tests/sources.rs`
- **post()** (11 connections) — `crates/lapidary-api/tests/sources.rs`
- **png()** (10 connections) — `crates/lapidary-api/tests/images.rs`
- **re_framing_changes_the_framing_and_not_the_bytes()** (10 connections) — `crates/lapidary-api/tests/images.rs`
- **an_image_cannot_be_re_framed_through_a_part_it_does_not_belong_to()** (10 connections) — `crates/lapidary-api/tests/images.rs`
- **a_framing_the_columns_would_refuse_is_refused_first()** (10 connections) — `crates/lapidary-api/tests/images.rs`
- **create()** (10 connections) — `crates/lapidary-api/tests/libraries.rs`
- **seed_part()** (10 connections) — `crates/lapidary-api/tests/sources.rs`
- **state()** (9 connections) — `crates/lapidary-api/tests/sources.rs`
- **gallery()** (8 connections) — `crates/lapidary-api/tests/images.rs`
- **a_small_image_is_stored_inline_and_read_back_as_a_data_url()** (8 connections) — `crates/lapidary-api/tests/images.rs`
- **a_large_image_becomes_a_blob_that_the_blob_route_will_serve()** (8 connections) — `crates/lapidary-api/tests/images.rs`
- **reframe()** (8 connections) — `crates/lapidary-api/tests/images.rs`
- **json()** (7 connections) — `crates/lapidary-api/tests/images.rs`
- **from_url()** (7 connections) — `crates/lapidary-api/tests/images.rs`
- *... and 23 more nodes in this community*

## Relationships

- [[Library Scan and Counts]] (33 shared connections)
- [[Folder and Part Identifiers]] (9 shared connections)
- [[Scan Enqueue]] (7 shared connections)
- [[Folder Tree and Cycles]] (6 shared connections)
- [[Router Roles and Errors]] (6 shared connections)
- [[Resumable Chunked Upload]] (3 shared connections)
- [[Prototype Image Slot]] (3 shared connections)
- [[Folder Routes]] (3 shared connections)
- [[download]] (2 shared connections)
- [[Part Repository and Derivatives]] (2 shared connections)
- [[lifecycle]] (1 shared connections)
- [[Job Queue and Cancellation]] (1 shared connections)

## Source Files

- `crates/lapidary-api/tests/images.rs`
- `crates/lapidary-api/tests/libraries.rs`
- `crates/lapidary-api/tests/sources.rs`

## Audit Trail

- EXTRACTED: 426 (100%)
- INFERRED: 2 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*