# OBJ Parsing

> 21 nodes

## Key Concepts

- **obj.rs** (19 connections) — `crates/lapidary-cad/src/obj.rs`
- **parse()** (15 connections) — `crates/lapidary-cad/src/obj.rs`
- **parse_obj()** (13 connections) — `crates/lapidary-cad/src/obj.rs`
- **parse()** (8 connections) — `crates/lapidary-cad/src/mesh_kernel.rs`
- **parse_face()** (8 connections) — `crates/lapidary-cad/src/obj.rs`
- **parse_vertex()** (6 connections) — `crates/lapidary-cad/src/obj.rs`
- **resolve()** (4 connections) — `crates/lapidary-cad/src/obj.rs`
- **triangulate()** (3 connections) — `crates/lapidary-cad/src/obj.rs`
- **SplitWhitespace** (2 connections)
- **a_triangle_face_parses()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **a_quad_face_is_triangulated_into_two_triangles()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **negative_indices_count_back_from_the_end()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **a_file_with_no_faces_fails_with_an_actionable_message()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **a_non_finite_coordinate_is_rejected()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **a_zero_index_is_rejected_rather_than_wrapping()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **a_face_with_two_vertices_is_an_error_not_a_skipped_line()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **an_out_of_range_index_names_how_many_vertices_exist()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **the_real_fixture_parses_with_its_real_triangle_count()** (2 connections) — `crates/lapidary-cad/src/obj.rs`
- **axis_name()** (1 connections) — `crates/lapidary-cad/src/obj.rs`
- **texture_and_normal_components_are_ignored()** (1 connections) — `crates/lapidary-cad/src/obj.rs`
- **comments_blank_lines_and_crlf_are_tolerated()** (1 connections) — `crates/lapidary-cad/src/obj.rs`

## Relationships

- [[Pin Everything]] (7 shared connections)
- [[Storage Paths and IO]] (6 shared connections)
- [[Blob CAS and Hash-First]] (3 shared connections)
- [[Mesh Kernel Dispatch]] (2 shared connections)
- [[raster]] (2 shared connections)
- [[Prototype Image Slot]] (2 shared connections)
- [[3MF Parsing and Bombs]] (1 shared connections)
- [[Path Escape Refusals]] (1 shared connections)
- [[CAD Kernel and OCCT Sidecar]] (1 shared connections)

## Source Files

- `crates/lapidary-cad/src/mesh_kernel.rs`
- `crates/lapidary-cad/src/obj.rs`

## Audit Trail

- EXTRACTED: 92 (93%)
- INFERRED: 7 (7%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*