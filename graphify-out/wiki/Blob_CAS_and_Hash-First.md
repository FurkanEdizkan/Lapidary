# Blob CAS and Hash-First

> 25 nodes

## Key Concepts

- **Mesh** (26 connections) — `crates/lapidary-cad/src/stl.rs`
- **cluster.rs** (25 connections) — `crates/lapidary-cad/src/cluster.rs`
- **index_mesh()** (19 connections) — `crates/lapidary-cad/src/cluster.rs`
- **cluster()** (10 connections) — `crates/lapidary-cad/src/cluster.rs`
- **Lod** (9 connections) — `crates/lapidary-cad/src/cluster.rs`
- **cell_size()** (5 connections) — `crates/lapidary-cad/src/cluster.rs`
- **.cells()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **bounds()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **starting_cells()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **cube()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **bracket()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **dense_sphere()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **a_triangle_whose_corners_share_a_cell_is_dropped()** (4 connections) — `crates/lapidary-cad/src/cluster.rs`
- **.budget()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **.triangle_count()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **l2_keeps_every_triangle_and_deduplicates_to_eight_corners()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **l0_of_a_dense_part_has_strictly_fewer_triangles_than_l2()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **a_mesh_under_the_budget_still_produces_every_rung()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **every_index_points_at_a_vertex_that_exists()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **a_vertex_at_the_bounding_box_maximum_falls_in_the_last_cell_not_past_it()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **exceeding_the_budget_coarsens_the_grid_and_records_the_grid_it_used()** (3 connections) — `crates/lapidary-cad/src/cluster.rs`
- **no_position_is_left_unreferenced()** (2 connections) — `crates/lapidary-cad/src/cluster.rs`
- **a_flat_part_does_not_divide_by_zero()** (2 connections) — `crates/lapidary-cad/src/cluster.rs`
- **clustering_is_deterministic_for_the_same_input()** (2 connections) — `crates/lapidary-cad/src/cluster.rs`
- **.as_kind()** (1 connections) — `crates/lapidary-cad/src/cluster.rs`

## Relationships

- [[Prototype Image Slot]] (7 shared connections)
- [[raster]] (5 shared connections)
- [[glb]] (4 shared connections)
- [[The Open Path Never Touches A Source File…]] (4 shared connections)
- [[measure]] (4 shared connections)
- [[Folder and Part Identifiers]] (3 shared connections)
- [[OBJ Parsing]] (3 shared connections)
- [[no-bare-strings.test]] (3 shared connections)
- [[Pin Everything]] (2 shared connections)
- [[Mesh Kernel Dispatch]] (2 shared connections)
- [[Storage Paths and IO]] (1 shared connections)
- [[3MF Parsing and Bombs]] (1 shared connections)

## Source Files

- `crates/lapidary-cad/src/cluster.rs`
- `crates/lapidary-cad/src/stl.rs`

## Audit Trail

- EXTRACTED: 151 (99%)
- INFERRED: 2 (1%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*