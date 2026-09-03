//! Turning a mesh into numbers. Every figure here is tessellated by construction.

use crate::stl::Mesh;
use lapidary_core::MeshMeasurements;
use std::collections::HashMap;

pub fn measure(mesh: &Mesh) -> MeshMeasurements {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut area = 0.0f64;
    let mut signed_volume = 0.0f64;

    for tri in &mesh.triangles {
        for v in tri {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        let [a, b, c] = [d(tri[0]), d(tri[1]), d(tri[2])];
        area += cross(sub(b, a), sub(c, a))
            .iter()
            .map(|k| k * k)
            .sum::<f64>()
            .sqrt()
            / 2.0;
        // Signed volume of the tetrahedron (origin, a, b, c). Sums to the enclosed
        // volume only when the surface is closed — hence the watertight gate below.
        signed_volume += dot(a, cross(b, c)) / 6.0;
    }

    let is_watertight = is_closed(mesh);

    MeshMeasurements {
        bbox_mm: [
            (max[0] - min[0]) as f64,
            (max[1] - min[1]) as f64,
            (max[2] - min[2]) as f64,
        ],
        triangle_count: mesh.triangles.len() as u32,
        surface_area_mm2: area,
        // A wrong number is worse than no number: only report volume for a closed surface.
        volume_mm3: is_watertight.then_some(signed_volume.abs()),
        is_watertight,
    }
}

/// Closed means every edge is shared by exactly two triangles. Vertices are quantised
/// before comparison because STL stores each vertex independently as f32, so the same
/// corner arrives with different bit patterns from different facets.
fn is_closed(mesh: &Mesh) -> bool {
    let mut edges: HashMap<(u64, u64), i32> = HashMap::new();
    for tri in &mesh.triangles {
        let k = [key(tri[0]), key(tri[1]), key(tri[2])];
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            *edges.entry((lo, hi)).or_insert(0) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|&n| n == 2)
}

fn key(v: [f32; 3]) -> u64 {
    // 1e-4 mm quantisation: finer than any real mesh tolerance, coarse enough to
    // collapse f32 representation noise at a shared corner.
    let q = |x: f32| (x as f64 / 1e-4).round() as i64;
    let (x, y, z) = (q(v[0]), q(v[1]), q(v[2]));
    let mut h = 1469598103934665603u64;
    for part in [x, y, z] {
        for byte in part.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(1099511628211);
        }
    }
    h
}

fn d(v: [f32; 3]) -> [f64; 3] {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stl::Mesh;

    /// A unit cube, closed. Two triangles per face, 12 total.
    fn unit_cube() -> Mesh {
        let v = [
            [0.0f32, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 2, 1],
            [0, 3, 2], // bottom
            [4, 5, 6],
            [4, 6, 7], // top
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        Mesh {
            triangles: faces.iter().map(|f| [v[f[0]], v[f[1]], v[f[2]]]).collect(),
        }
    }

    #[test]
    fn a_closed_cube_measures_its_bbox_and_volume() {
        let m = measure(&unit_cube());
        assert_eq!(m.triangle_count, 12);
        assert_eq!(m.bbox_mm, [1.0, 1.0, 1.0]);
        assert!(m.is_watertight);
        let volume = m.volume_mm3.expect("a closed mesh has a volume");
        assert!((volume - 1.0).abs() < 1e-4, "unit cube volume was {volume}");
    }

    #[test]
    fn an_open_mesh_reports_no_volume() {
        let mut mesh = unit_cube();
        mesh.triangles.pop(); // remove one face: no longer closed
        let m = measure(&mesh);
        assert!(!m.is_watertight);
        assert!(
            m.volume_mm3.is_none(),
            "an open mesh must report no volume rather than a meaningless number"
        );
    }

    #[test]
    fn surface_area_is_always_reported_even_when_open() {
        let mut mesh = unit_cube();
        mesh.triangles.pop();
        let m = measure(&mesh);
        // 11 of 12 unit-cube triangles, each of area 0.5.
        assert!((m.surface_area_mm2 - 5.5).abs() < 1e-4);
    }

    #[test]
    fn a_cube_whose_shared_corners_differ_in_the_last_float_bit_is_still_closed() {
        // STL stores every vertex independently per facet — there is no shared vertex
        // list on disk. So the same physical corner routinely arrives as several
        // different bit patterns depending on which facet last computed it: a real
        // slicer or CAD export can legitimately write one corner as `1.0` from one
        // triangle and `0.99999994` or `1.0000001` (its immediate f32 neighbours) from
        // another. `unit_cube()` above sidesteps this entirely by reading every corner
        // from one shared array, so it cannot tell us whether quantisation is doing
        // anything. This test builds the same cube without that shortcut.
        let one_lo = f32::from_bits(1.0f32.to_bits() - 1); // 0.99999994
        let one_hi = f32::from_bits(1.0f32.to_bits() + 1); // 1.0000001

        // Three encodings of the same 8 corners, each corner's `1.0` components drawn
        // from a different neighbour of 1.0f32 — the "same" corner, three bit patterns.
        let variants: [[[f32; 3]; 8]; 3] = [
            [
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            [
                [0.0, 0.0, 0.0],
                [one_lo, 0.0, 0.0],
                [one_lo, one_lo, 0.0],
                [0.0, one_lo, 0.0],
                [0.0, 0.0, one_lo],
                [one_lo, 0.0, one_lo],
                [one_lo, one_lo, one_lo],
                [0.0, one_lo, one_lo],
            ],
            [
                [0.0, 0.0, 0.0],
                [one_hi, 0.0, 0.0],
                [one_hi, one_hi, 0.0],
                [0.0, one_hi, 0.0],
                [0.0, 0.0, one_hi],
                [one_hi, 0.0, one_hi],
                [one_hi, one_hi, one_hi],
                [0.0, one_hi, one_hi],
            ],
        ];
        let faces = [
            [0, 2, 1],
            [0, 3, 2], // bottom
            [4, 5, 6],
            [4, 6, 7], // top
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        // Each face draws its 3 corners from a different variant array — exactly as
        // each facet in a real STL independently stores its own vertex coordinates —
        // so two triangles sharing a physical corner almost never see the same bits.
        let mesh = Mesh {
            triangles: faces
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let v = &variants[i % variants.len()];
                    [v[f[0]], v[f[1]], v[f[2]]]
                })
                .collect(),
        };

        let m = measure(&mesh);
        assert!(
            m.is_watertight,
            "a cube is closed regardless of which f32 neighbour of each corner a facet happened to store"
        );
        let volume = m.volume_mm3.expect("a closed mesh has a volume");
        assert!((volume - 1.0).abs() < 1e-3, "unit cube volume was {volume}");
    }
}
