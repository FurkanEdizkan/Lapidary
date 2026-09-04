//! Vertex clustering: one indexing function at three grid sizes.
//!
//! `measure.rs` already quantises vertices and hashes them, to rebuild the adjacency that
//! STL's per-facet vertex duplication destroys. Clustering is that same idea at a coarser
//! grid, and the whole ladder falls out of one function:
//!
//! | Rung | Cell size | Effect |
//! |---|---|---|
//! | `L2` | 1e-4 mm | lossless de-duplication; nothing dropped |
//! | `L1` | bounding box / 96 | lossy |
//! | `L0` | bounding box / 32 | lossy |
//!
//! The input is `Mesh`, an unindexed triangle soup. That is not a limitation here:
//! clustering does not need indexed input, it *produces* it, and an indexed mesh is what
//! glTF wants. Nothing about `Mesh` changes.
//!
//! Cell size is a fraction of the bounding box rather than an absolute length, so a 5 mm
//! screw and a 2 m gantry get comparable triangle counts. `docs/prototype-notes.md` records
//! that the deleted prototype clustered on a 48³ grid and that "the 48 constant was tuned
//! by eye and should become an L0/L1/L2 ladder".

use crate::glb::write_glb;
use crate::kernel::CadError;
use crate::stl::Mesh;
use std::collections::HashMap;

/// The finest cell this module will use, and the grid `L2` always uses. Matches
/// `measure.rs`'s quantisation: finer than any real mesh tolerance, coarse enough to
/// collapse the f32 representation noise at a shared corner.
const FINEST_MM: f64 = 1e-4;

/// How many times the budget retry may halve the grid before accepting an over-budget
/// rung. Two, like `raster.rs`'s two `FALLBACK_PX` steps: a third pass costs another full
/// clustering for a mesh that is already saying it will not fit.
const MAX_RETRIES: u32 = 2;

/// A rung of the ladder. Ordinal rather than a triangle count: the count is an outcome of
/// the grid and the mesh, and `DATA.md` §2.1's figures are targets rather than guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    L0,
    L1,
    L2,
}

impl Lod {
    /// Ascending detail.
    pub const ALL: [Lod; 3] = [Lod::L0, Lod::L1, Lod::L2];

    /// The `derivative.kind` this rung is stored under. `DATA.md` §3.2 fixes the
    /// vocabulary — `tessellation_l0|l1|l2` — so it is not this module's to invent.
    pub fn as_kind(self) -> &'static str {
        match self {
            Lod::L0 => "tessellation_l0",
            Lod::L1 => "tessellation_l1",
            Lod::L2 => "tessellation_l2",
        }
    }

    /// Cells per axis across the bounding box. `None` means the fixed `FINEST_MM` grid,
    /// which is lossless de-duplication rather than decimation.
    fn cells(self) -> Option<u32> {
        match self {
            Lod::L0 => Some(32),
            Lod::L1 => Some(96),
            Lod::L2 => None,
        }
    }

    /// Triangle budget from `DATA.md` §2.1. Approximate by design — the retry in
    /// `index_mesh` is what keeps them approximately true across a corpus, which a fixed
    /// grid does not.
    fn budget(self) -> Option<u32> {
        match self {
            Lod::L0 => Some(5_000),
            Lod::L1 => Some(50_000),
            Lod::L2 => None,
        }
    }
}

/// An indexed mesh: shared positions plus a triangle index buffer.
///
/// Separate from the glTF writer on purpose — clustering is testable without a glTF
/// parser, and the writer is testable without a mesh.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Indexed {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// The grid actually used, after any budget retry, so a caller can record how the
    /// derivative was made. `None` for `L2`, which has no cell count — writing `0` there
    /// would put a lie into `params_json`.
    pub grid: Option<u32>,
}

impl Indexed {
    pub(crate) fn triangle_count(&self) -> u32 {
        // Every triangle contributes exactly three indices, so this cannot be lossy.
        (self.indices.len() / 3) as u32
    }
}

/// A vertex's cell. Signed because a coordinate may sit below the bounding-box minimum by
/// a rounding step.
type Cell = (i64, i64, i64);

fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for triangle in &mesh.triangles {
        for vertex in triangle {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex[axis]);
                max[axis] = max[axis].max(vertex[axis]);
            }
        }
    }
    (min, max)
}

/// Cell size per axis, floored at `FINEST_MM`.
///
/// That floor does two jobs, and the second is easy to miss. It stops a cell finer than
/// `measure.rs`'s own quantisation, which would distinguish vertices that module already
/// treats as one corner and risks overflowing the `i64` cell index on a large part. And it
/// covers the **zero-extent** case for free: a flat plate has no span on one axis, and
/// `0.0 / n` is `0.0`, which `max` lifts to `FINEST_MM` rather than leaving a divisor of
/// zero.
///
/// An explicit `if span <= 0.0` branch was written here first and removed: its mutation
/// did not bite, because `max` had already made it unreachable. A guard that guards
/// nothing reads as protection and is not.
fn cell_size(min: [f32; 3], max: [f32; 3], cells: Option<u32>) -> [f64; 3] {
    let Some(n) = cells else {
        return [FINEST_MM; 3];
    };
    std::array::from_fn(|axis| {
        let span = f64::from(max[axis]) - f64::from(min[axis]);
        (span / f64::from(n)).max(FINEST_MM)
    })
}

/// Clamped to `cells - 1` on each axis when there is a cell count, because a vertex
/// exactly at the bounding-box maximum quantises to `cells` rather than `cells - 1` — an
/// off-by-one that gives `cells + 1` cells per axis and leaves every max-boundary vertex
/// alone in a cell of its own. At `cells = 1` it is the difference between one cell and
/// two, so a triangle that should have collapsed survives instead.
///
/// `L2` passes `None` and is deliberately unclamped: its grid is an absolute 1e-4 mm, not
/// a subdivision of anything, so it has no last cell to fold into.
fn cell_of(vertex: [f32; 3], min: [f32; 3], size: [f64; 3], cells: Option<u32>) -> Cell {
    let quantise = |axis: usize| {
        let raw = ((f64::from(vertex[axis]) - f64::from(min[axis])) / size[axis]).floor() as i64;
        match cells {
            Some(n) => raw.clamp(0, i64::from(n) - 1),
            None => raw,
        }
    };
    (quantise(0), quantise(1), quantise(2))
}

/// Index a mesh at a given cell size, dropping degenerate triangles.
///
/// A triangle whose corners fall into fewer than three distinct cells has collapsed to a
/// line or a point. Dropping it is the point of clustering rather than a loss: a zero-area
/// triangle contributes nothing to a render and makes some viewers emit a NaN normal.
///
/// Two passes on purpose. A one-pass version assigns a cell's representative the first
/// time it sees the cell, including for triangles it then drops, leaving positions in the
/// buffer that no index references — legal glTF, wasted bytes in the rung that most needs
/// to be small. Deciding survival first means every position emitted is referenced.
///
/// Deterministic: a cell's representative is the first surviving triangle's vertex in that
/// cell, in `mesh.triangles` order. The `HashMap` is only ever looked up, never iterated,
/// so its ordering cannot leak into the output — which matters because `kernel_version`
/// and `params_json` are supposed to make regeneration reproduce the bytes.
fn index_at(
    mesh: &Mesh,
    min: [f32; 3],
    size: [f64; 3],
    cells: Option<u32>,
) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut surviving: Vec<([Cell; 3], [[f32; 3]; 3])> = Vec::new();
    for triangle in &mesh.triangles {
        let corners = [
            cell_of(triangle[0], min, size, cells),
            cell_of(triangle[1], min, size, cells),
            cell_of(triangle[2], min, size, cells),
        ];
        if corners[0] != corners[1] && corners[1] != corners[2] && corners[0] != corners[2] {
            surviving.push((corners, *triangle));
        }
    }

    let mut index_of: HashMap<Cell, u32> = HashMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(surviving.len() * 3);
    for (corners, vertices) in &surviving {
        for (cell, vertex) in corners.iter().zip(vertices) {
            let index = *index_of.entry(*cell).or_insert_with(|| {
                positions.push(*vertex);
                // Safe for any mesh that fits in memory: one position per cell, and a u32
                // index buffer caps a rung at four billion vertices regardless.
                (positions.len() - 1) as u32
            });
            indices.push(index);
        }
    }

    (positions, indices)
}

/// Index a mesh for one rung, coarsening the grid if the result overshoots the rung's
/// triangle budget.
///
/// The retry mirrors `raster.rs`, which halves the thumbnail's pixel size when the encode
/// exceeds `MAX_THUMB_BYTES` — the same problem, the same shape of answer.
///
/// There is deliberately no retry in the other direction. A mesh that comes in under
/// budget has clustered to itself, which is correct; refining toward the budget would make
/// a twelve-triangle bracket run extra passes to produce twelve triangles.
pub(crate) fn index_mesh(mesh: &Mesh, lod: Lod) -> Indexed {
    let (min, max) = bounds(mesh);
    let mut cells = lod.cells();

    loop {
        let (positions, indices) = index_at(mesh, min, cell_size(min, max, cells), cells);
        let indexed = Indexed {
            positions,
            indices,
            grid: cells,
        };

        let over_budget = lod
            .budget()
            .is_some_and(|budget| indexed.triangle_count() > budget);
        // `cells` is `None` only for L2, which has no budget and so never gets here.
        let Some(current) = cells else {
            return indexed;
        };
        if !over_budget || current <= starting_cells(lod) / (1 << MAX_RETRIES) {
            return indexed;
        }
        // `max(1)` rather than allowing 0: a zero grid divides by zero in `cell_size`, and
        // one cell per axis is the coarsest meaningful clustering.
        cells = Some((current / 2).max(1));
    }
}

/// The grid a rung starts at, used to bound the retry. Separate from `Lod::cells` so the
/// loop's stopping condition reads as "no more than `MAX_RETRIES` halvings from the start"
/// rather than as an opaque number.
fn starting_cells(lod: Lod) -> u32 {
    lod.cells().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube as a triangle soup: 12 triangles, 36 vertices, 8 distinct corners.
    /// Written out rather than generated so the expected counts below are readable.
    fn cube() -> Mesh {
        let c = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [1, 5, 6],
            [1, 6, 2],
            [2, 6, 7],
            [2, 7, 3],
            [3, 7, 4],
            [3, 4, 0],
        ];
        Mesh {
            triangles: faces.iter().map(|f| [c[f[0]], c[f[1]], c[f[2]]]).collect(),
        }
    }

    fn bracket() -> Mesh {
        crate::parse_stl(include_bytes!("../../../fixtures/bracket-lp-1042-03.stl"))
            .expect("the fixture parses")
    }

    /// A roughly `target`-triangle sphere, so the budget-retry test states its own density
    /// rather than depending on a fixture whose triangle count could change.
    fn dense_sphere(target: u32) -> Mesh {
        let rings = ((target as f64 / 2.0).sqrt().round() as usize).max(4);
        let segments = rings * 2;
        let point = |ring: usize, segment: usize| -> [f32; 3] {
            let phi = std::f64::consts::PI * ring as f64 / rings as f64;
            let theta = 2.0 * std::f64::consts::PI * segment as f64 / segments as f64;
            [
                (phi.sin() * theta.cos() * 10.0) as f32,
                (phi.sin() * theta.sin() * 10.0) as f32,
                (phi.cos() * 10.0) as f32,
            ]
        };
        let mut triangles = Vec::new();
        for ring in 0..rings {
            for segment in 0..segments {
                let (a, b) = (point(ring, segment), point(ring, segment + 1));
                let (c, d) = (point(ring + 1, segment), point(ring + 1, segment + 1));
                triangles.push([a, c, b]);
                triangles.push([b, c, d]);
            }
        }
        Mesh { triangles }
    }

    #[test]
    fn l2_keeps_every_triangle_and_deduplicates_to_eight_corners() {
        let indexed = index_mesh(&cube(), Lod::L2);
        assert_eq!(
            indexed.triangle_count(),
            12,
            "L2 is lossless — no triangle may be dropped"
        );
        assert_eq!(
            indexed.positions.len(),
            8,
            "36 soup vertices are the cube's 8 real corners"
        );
        assert_eq!(
            indexed.grid, None,
            "L2 has no cell count and must not claim one"
        );
    }

    #[test]
    fn l0_of_a_dense_part_has_strictly_fewer_triangles_than_l2() {
        // Deliberately not the bracket fixture: it is 20 triangles, coarser than L0's grid
        // on its own bounding box, so it clusters to itself and L0 == L2. That is correct
        // behaviour (spec 3.6), which makes it the wrong mesh for proving decimation.
        let mesh = dense_sphere(20_000);
        let l0 = index_mesh(&mesh, Lod::L0);
        let l2 = index_mesh(&mesh, Lod::L2);
        assert!(
            l0.triangle_count() < l2.triangle_count(),
            "L0 {} should be coarser than L2 {}",
            l0.triangle_count(),
            l2.triangle_count()
        );
    }

    #[test]
    fn a_mesh_under_the_budget_still_produces_every_rung() {
        // Spec §3.6: a small part clusters to itself at every grid and is written anyway.
        // Content addressing makes three identical rungs one blob with ref_count 3.
        for lod in Lod::ALL {
            let indexed = index_mesh(&cube(), lod);
            assert!(
                indexed.triangle_count() > 0,
                "{lod:?} came out empty for a cube"
            );
        }
    }

    #[test]
    fn every_index_points_at_a_vertex_that_exists() {
        let mesh = bracket();
        for lod in Lod::ALL {
            let indexed = index_mesh(&mesh, lod);
            assert!(
                indexed
                    .indices
                    .iter()
                    .all(|i| (*i as usize) < indexed.positions.len()),
                "{lod:?} emitted an index past the end of the position buffer"
            );
        }
    }

    #[test]
    fn no_position_is_left_unreferenced() {
        // The reason `index_at` is two passes, and it needs a deliberately built mesh.
        //
        // A one-pass version assigns a cell's representative the first time it sees the
        // cell, including for triangles it then drops. That only strands a position when
        // the cell is touched by *nothing else* — and on a smooth dense mesh a collapsed
        // triangle's cell almost always also holds a surviving neighbour's vertex, so no
        // orphan appears. Both a real fixture and a sphere were tried first and neither
        // caught the mutation.
        //
        // What does: one large triangle spanning the box, plus a tiny isolated triangle
        // in a cell the large one never touches. That is a real shape — a small detached
        // boss or rib on a part, entirely inside one L0 cell.
        let mesh = Mesh {
            triangles: vec![
                [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0]],
                // Far from all three corners above, and smaller than one cell.
                [[50.0, 50.0, 0.0], [50.01, 50.0, 0.0], [50.0, 50.01, 0.0]],
            ],
        };
        let indexed = index_mesh(&mesh, Lod::L0);
        assert_eq!(
            indexed.triangle_count(),
            1,
            "the tiny triangle must collapse, or this test proves nothing"
        );
        let referenced: std::collections::HashSet<u32> = indexed.indices.iter().copied().collect();
        assert_eq!(
            referenced.len(),
            indexed.positions.len(),
            "the collapsed triangle's cell must not leave a position nothing indexes"
        );
    }

    #[test]
    fn a_triangle_whose_corners_share_a_cell_is_dropped() {
        // Three vertices well inside one coarse cell: the triangle has collapsed, and must
        // not reach a viewer where a zero-area face yields a NaN normal.
        let tiny = Mesh {
            triangles: vec![[[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.001, 0.0]]],
        };
        let (min, max) = bounds(&tiny);
        let (_, indices) = index_at(&tiny, min, cell_size(min, max, Some(1)), Some(1));
        assert!(indices.is_empty());
    }

    #[test]
    fn a_vertex_at_the_bounding_box_maximum_falls_in_the_last_cell_not_past_it() {
        // Found while writing this module, not by the plan. floor((max - min) / size) is
        // exactly `cells`, one past the last index, so an unclamped grid yields cells + 1
        // buckets and every max-boundary vertex sits alone. At cells = 1 that is the
        // difference between one bucket and two, and a triangle that should collapse does
        // not -- which is how it was caught.
        let size = cell_size([0.0; 3], [10.0; 3], Some(4));
        let at_max = cell_of([10.0, 10.0, 10.0], [0.0; 3], size, Some(4));
        assert_eq!(
            at_max,
            (3, 3, 3),
            "the maximum belongs to the last cell, not cell 4"
        );

        let unclamped = cell_of([10.0, 10.0, 10.0], [0.0; 3], size, None);
        assert_eq!(unclamped, (4, 4, 4), "L2 is deliberately unclamped");
    }

    #[test]
    fn a_flat_part_does_not_divide_by_zero() {
        // Every vertex at z = 0: the z extent is zero, and an unguarded cell size would be
        // 0.0, making every z cell index garbage.
        let flat = Mesh {
            triangles: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]],
        };
        let indexed = index_mesh(&flat, Lod::L0);
        assert_eq!(indexed.triangle_count(), 1);
    }

    #[test]
    fn exceeding_the_budget_coarsens_the_grid_and_records_the_grid_it_used() {
        let indexed = index_mesh(&dense_sphere(20_000), Lod::L0);
        assert!(
            indexed.grid.is_some_and(|g| g < 32),
            "the retry should have coarsened below the starting 32, got {:?}",
            indexed.grid
        );
    }

    #[test]
    fn clustering_is_deterministic_for_the_same_input() {
        // `kernel_version` + `params_json` are supposed to make regeneration reproduce the
        // bytes. A HashMap iteration order leaking into the representative choice would
        // break that silently, and only for some meshes.
        let mesh = bracket();
        assert_eq!(index_mesh(&mesh, Lod::L0), index_mesh(&mesh, Lod::L0));
    }
}

/// One rung, indexed and written as glTF.
#[derive(Debug, Clone, PartialEq)]
pub struct Tessellation {
    pub lod: Lod,
    /// glTF 2.0 binary, uncompressed — see the design doc, section 3.2.
    pub glb: Vec<u8>,
    pub triangle_count: u32,
    /// The grid actually used, after any budget retry, so `params_json` can say how the
    /// derivative was made. `None` for `L2`, which has no cell count.
    pub grid: Option<u32>,
}

/// Cluster a mesh into one rung.
pub fn cluster(mesh: &Mesh, lod: Lod) -> Result<Tessellation, CadError> {
    let indexed = index_mesh(mesh, lod);
    Ok(Tessellation {
        lod,
        triangle_count: indexed.triangle_count(),
        grid: indexed.grid,
        glb: write_glb(&indexed)?,
    })
}
