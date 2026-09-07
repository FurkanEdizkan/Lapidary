# SSRF Fetch Guard

> 29 nodes

## Key Concepts

- **fetch.rs** (28 connections) — `crates/lapidary-api/src/fetch.rs`
- **serve()** (10 connections) — `crates/lapidary-api/src/fetch.rs`
- **follow()** (9 connections) — `crates/lapidary-api/src/fetch.rs`
- **one_hop()** (8 connections) — `crates/lapidary-api/src/fetch.rs`
- **FetchError** (7 connections) — `crates/lapidary-api/src/fetch.rs`
- **allowed()** (7 connections) — `crates/lapidary-api/src/fetch.rs`
- **read_capped()** (6 connections) — `crates/lapidary-api/src/fetch.rs`
- **fetch()** (6 connections) — `crates/lapidary-api/src/fetch.rs`
- **fetch_image()** (5 connections) — `crates/lapidary-api/src/fetch.rs`
- **Policy** (4 connections) — `crates/lapidary-api/src/fetch.rs`
- **v4_allowed()** (4 connections) — `crates/lapidary-api/src/fetch.rs`
- **v6_allowed()** (4 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_page_rather_than_an_image_says_so()** (3 connections) — `crates/lapidary-api/src/fetch.rs`
- **Url** (2 connections)
- **Ipv4Addr** (2 connections)
- **public()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **an_image_at_a_reachable_address_comes_back()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_redirect_into_the_metadata_endpoint_is_refused()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_redirect_into_private_space_is_refused_and_a_relative_one_is_resolved()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_redirect_loop_stops_at_the_limit()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_body_larger_than_the_cap_is_stopped_while_it_arrives()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **a_status_that_is_not_success_is_reported_with_its_status()** (2 connections) — `crates/lapidary-api/src/fetch.rs`
- **IpAddr** (1 connections)
- **Ipv6Addr** (1 connections)
- **the_metadata_endpoint_is_refused_in_every_spelling_of_it()** (1 connections) — `crates/lapidary-api/src/fetch.rs`
- *... and 4 more nodes in this community*

## Relationships

- [[Storage Paths and IO]] (5 shared connections)
- [[Prototype Image Slot]] (4 shared connections)
- [[part table]] (2 shared connections)
- [[Job Queue and Cancellation]] (1 shared connections)
- [[Image Decode Limits]] (1 shared connections)
- [[Directory Scan Cells]] (1 shared connections)
- [[Folder and Part Identifiers]] (1 shared connections)
- [[Download Byte Fidelity]] (1 shared connections)

## Source Files

- `crates/lapidary-api/src/fetch.rs`

## Audit Trail

- EXTRACTED: 126 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*