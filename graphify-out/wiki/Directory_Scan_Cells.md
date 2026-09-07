# Directory Scan Cells

> 27 nodes

## Key Concepts

- **images.rs** (29 connections) — `crates/lapidary-api/src/images.rs`
- **normalize()** (13 connections) — `crates/lapidary-api/src/images.rs`
- **store()** (13 connections) — `crates/lapidary-api/src/images.rs`
- **from_url()** (12 connections) — `crates/lapidary-api/src/images.rs`
- **upload()** (10 connections) — `crates/lapidary-api/src/images.rs`
- **set_framing()** (10 connections) — `crates/lapidary-api/src/images.rs`
- **encoded()** (9 connections) — `crates/lapidary-api/src/images.rs`
- **refused()** (7 connections) — `crates/lapidary-api/src/images.rs`
- **encode_webp()** (6 connections) — `crates/lapidary-api/src/images.rs`
- **not_fetched()** (5 connections) — `crates/lapidary-api/src/images.rs`
- **ImageError** (4 connections) — `crates/lapidary-api/src/images.rs`
- **NormalizedImage** (4 connections) — `crates/lapidary-api/src/images.rs`
- **a_png_comes_back_as_webp_at_its_own_size()** (3 connections) — `crates/lapidary-api/src/images.rs`
- **a_jpeg_comes_back_as_webp_too()** (3 connections) — `crates/lapidary-api/src/images.rs`
- **an_image_over_the_long_edge_is_resized_and_keeps_its_shape()** (3 connections) — `crates/lapidary-api/src/images.rs`
- **a_favicon_sized_image_is_refused_and_says_what_is_wrong_with_it()** (3 connections) — `crates/lapidary-api/src/images.rs`
- **SetFraming** (3 connections) — `crates/lapidary-api/src/images.rs`
- **bytes_that_are_not_an_image_are_refused_however_they_are_labelled()** (2 connections) — `crates/lapidary-api/src/images.rs`
- **a_truncated_image_is_refused_as_unreadable_rather_than_as_not_an_image()** (2 connections) — `crates/lapidary-api/src/images.rs`
- **an_oversized_file_is_refused_without_being_decoded()** (2 connections) — `crates/lapidary-api/src/images.rs`
- **an_image_that_would_allocate_past_the_decode_limit_is_refused()** (2 connections) — `crates/lapidary-api/src/images.rs`
- **StoredImage** (2 connections) — `crates/lapidary-api/src/images.rs`
- **FetchImageRequest** (2 connections) — `crates/lapidary-api/src/images.rs`
- **DynamicImage** (1 connections)
- **ImageFormat** (1 connections)
- *... and 2 more nodes in this community*

## Relationships

- [[Scan Enqueue]] (12 shared connections)
- [[Folder and Part Identifiers]] (8 shared connections)
- [[part table]] (7 shared connections)
- [[Library Settings and Thumbnails]] (6 shared connections)
- [[Grid Row Columns]] (3 shared connections)
- [[Prototype Image Slot]] (3 shared connections)
- [[Storage Paths and IO]] (2 shared connections)
- [[Part Repository and Derivatives]] (2 shared connections)
- [[Folder Routes]] (1 shared connections)
- [[Image Decode Limits]] (1 shared connections)
- [[Folder Tree Drag Tests]] (1 shared connections)
- [[SSRF Fetch Guard]] (1 shared connections)

## Source Files

- `crates/lapidary-api/src/images.rs`

## Audit Trail

- EXTRACTED: 150 (98%)
- INFERRED: 3 (2%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*