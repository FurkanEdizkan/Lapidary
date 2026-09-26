//! A part's shape profile, computed from a mesh (Phase 6; design in `docs/goals/phase-6.md`
//! § Shape profile).
//!
//! The worker reads a part's stored L0 tessellation, decodes it with [`read_triangles`], and
//! hands the triangles here. Never the source file, never the CAD kernel: L0 is 5k triangles
//! whatever the part was, its clustering smooths the difference between an STL and a STEP of one
//! part, and an algorithm change costs a decode rather than a re-tessellation. What comes out is
//! [`ShapeProfile`], whose meaning — the length, the version, the distance, the thresholds — is
//! `lapidary_core::shape`'s, because the api compares profiles this crate never sees.
//!
//! # What the 35 floats are
//!
//! - **32 bins of a D2 shape distribution.** [`PAIRS`] pairs of points are drawn uniformly over
//!   the surface — the triangle by area, the point uniformly inside it — and each pair's distance
//!   is divided by the mean distance `m`. The 32 bins span `[0, 3m)`, anything past it lands in
//!   the last, and each bin is stored as the **square root of its probability**, so the plain
//!   Euclidean distance `lapidary_core::shape::distance` takes over that block is the Hellinger
//!   distance between the two distributions.
//! - **λ2/λ1 and λ3/λ1**, from the area-weighted covariance of the surface about its own
//!   centroid: how slab-like and how rod-like the part is, whatever its size or orientation.
//! - **ln(area / m²)**, the one term that says how much surface a part carries for its size — a
//!   sphere and a fin-covered heatsink have much the same D2 and are not much alike.
//!
//! `size_mm` is `m` itself, in millimetres. It needs no volume, so an open or non-manifold mesh
//! has one, which a bounding box or a volume would not give honestly.
//!
//! # Invariance, and the one place it is deliberately incomplete
//!
//! Translation and rotation hold because the covariance is taken about the centroid and the D2
//! block is built from distances alone. Uniform scale holds because every distance is divided by
//! `m`, and `m` carries the scale instead. Triangle and vertex order hold to within the sampler's
//! own noise: reordering triangles changes which of them the cumulative-area search picks, so two
//! orderings of one mesh differ by about 0.013 in [`distance`](lapidary_core::shape::distance) —
//! the noise floor [`PAIRS`] derives, a third of `NEAR_DUPLICATE_DISTANCE`. Re-tessellation holds
//! the same way, and no threshold can ever be tighter than that floor.
//!
//! **Mirror images look identical, on purpose.** Nothing here can tell a left hand from a right
//! one: every ingredient is a distance, an area or an eigenvalue, and reflection changes none of
//! them. A left and a right bracket are therefore proposed as near-duplicates, and the person
//! says "Not the same" once — which is exactly what `part_link`'s `distinct` is for. Telling them
//! apart needs a signed quantity (an oriented volume, or a chirality term), and a wrong guess at
//! one would split real duplicates, which is the worse failure.
//!
//! [`read_triangles`]: crate::read_triangles

use crate::kernel::CadError;
use lapidary_core::{DESCRIPTOR_LEN, ShapeProfile};

/// How many bins the D2 distribution is counted into. The remaining three floats of
/// [`DESCRIPTOR_LEN`] are the ratios.
const BINS: usize = 32;

const _: () = assert!(
    DESCRIPTOR_LEN == BINS + 3,
    "the descriptor is 32 D2 bins and three ratios"
);

/// How many point pairs the D2 distribution is drawn from.
///
/// This fixes the descriptor's **noise floor**, and the arithmetic is worth writing down because
/// it bounds what any threshold can mean. A bin holding probability `p` is estimated with standard
/// deviation `√(p(1−p)/PAIRS)`, and storing its square root divides that by `2√p`, leaving
/// `√((1−p)/(4·PAIRS))` — very nearly `1/(2√PAIRS)` whatever `p` is. Over 32 independent bins
/// that is `√(BINS/(4·PAIRS))` = **0.011** between two samplings of one shape. Measured at 0.013
/// for a cylinder whose triangles were reordered, which is that floor and no more.
///
/// So the floor is a quarter of `NEAR_DUPLICATE_DISTANCE`, and halving it would cost four times
/// the pairs. 65,536 is the design's figure (`docs/goals/phase-6.md`); raising it is a
/// `SHAPE_VERSION` bump and the lead's decision, not this crate's.
const PAIRS: usize = 65_536;

/// How far out the bins reach, as a multiple of the mean distance. Past three means it is
/// counted in the last bin: on a long thin part the tail is what says it is long and thin, and
/// dropping it would make every rod look alike.
const SPAN: f64 = 3.0;

/// The sampler's seed. Arbitrary, and fixed for good: changing it changes every descriptor, which
/// is what `SHAPE_VERSION` exists to say out loud.
const SEED: u64 = 0x_D25E_ED00_0000_0001;

/// splitmix64, from the reference implementation. Written out rather than depended on: it is
/// eight lines, it must produce the same stream on every platform and for every future build of
/// Lapidary, and a crate that changed its stream in a point release would silently invalidate
/// every stored profile.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`, from the top 53 bits — every f64 in the range is reachable and none
    /// is reachable twice, which the `as f64 / u64::MAX as f64` shortcut gets wrong at both ends.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// The shape profile of an indexed mesh.
///
/// Pure and deterministic: the same triangles in the same order give bit-identical output on
/// every run, which is what lets a stored profile be compared with a fresh one.
///
/// Refused, rather than answered with something meaningless, when the mesh has no surface: no
/// triangles, zero total area, every point coincident, or any coordinate that is not finite. A
/// profile of such a mesh would fail `part_shape`'s `size_mm > 0` check at the last moment, and a
/// caller would retry it forever.
pub fn profile(positions: &[[f32; 3]], indices: &[u32]) -> Result<ShapeProfile, CadError> {
    let refused = |detail: String| CadError::Unrenderable {
        detail: format!("its shape could not be measured — {detail}"),
    };

    // Everything below is f64. The stored positions are f32, but a covariance sums squares of
    // coordinates, and on a part 300 mm from the origin f32 loses the small eigenvalue entirely.
    let mut corners: Vec<[[f64; 3]; 3]> = Vec::with_capacity(indices.len() / 3);
    for triangle in indices.chunks_exact(3) {
        let mut out = [[0.0; 3]; 3];
        for (corner, &index) in out.iter_mut().zip(triangle) {
            let vertex = positions
                .get(index as usize)
                .ok_or_else(|| refused(format!("a triangle names vertex {index}")))?;
            *corner = vertex.map(f64::from);
            if !corner.iter().all(|c| c.is_finite()) {
                return Err(refused("a corner is not a finite position".to_owned()));
            }
        }
        corners.push(out);
    }
    if corners.is_empty() {
        return Err(refused("it has no triangles".to_owned()));
    }

    // Area and centroid per triangle, and the running total the sampler picks triangles from.
    let areas: Vec<f64> = corners.iter().map(triangle_area).collect();
    let mut total_area = 0.0;
    let cumulative: Vec<f64> = areas
        .iter()
        .map(|area| {
            total_area += area;
            total_area
        })
        .collect();
    if !total_area.is_finite() || total_area <= 0.0 {
        return Err(refused(
            "its triangles enclose no surface at all".to_owned(),
        ));
    }
    let mut weighted_centre = [0.0; 3];
    for (t, area) in corners.iter().zip(&areas) {
        for axis in 0..3 {
            weighted_centre[axis] += area * (t[0][axis] + t[1][axis] + t[2][axis]) / 3.0;
        }
    }
    let centre = weighted_centre.map(|c| c / total_area);

    // The area-weighted covariance of the surface about its own centroid, exact per triangle: a
    // triangle's own second moment about its centroid is (1/12)·Σ(vᵢ − g)(vᵢ − g)ᵀ, and the
    // centroid's own offset from the part's centre carries the rest. Sampling points and taking
    // their covariance instead would put the sampler's noise into the two ratios as well.
    let mut covariance = [[0.0f64; 3]; 3];
    for (t, area) in corners.iter().zip(&areas) {
        let g = [
            (t[0][0] + t[1][0] + t[2][0]) / 3.0,
            (t[0][1] + t[1][1] + t[2][1]) / 3.0,
            (t[0][2] + t[1][2] + t[2][2]) / 3.0,
        ];
        let offset = [g[0] - centre[0], g[1] - centre[1], g[2] - centre[2]];
        let spread = t.map(|v| [v[0] - g[0], v[1] - g[1], v[2] - g[2]]);
        for row in 0..3 {
            for column in 0..3 {
                let own: f64 = spread.iter().map(|v| v[row] * v[column]).sum();
                covariance[row][column] += area * (own / 12.0 + offset[row] * offset[column]);
            }
        }
    }
    for row in covariance.iter_mut() {
        for cell in row.iter_mut() {
            *cell /= total_area;
        }
    }
    let [l1, l2, l3] = eigenvalues(covariance);
    if !l1.is_finite() || l1 <= 0.0 {
        return Err(refused(
            "every one of its points is in one place".to_owned(),
        ));
    }

    // The D2 distribution. Two independent draws per pair, each a triangle chosen with
    // probability proportional to its area and a point uniform inside it.
    let mut rng = SplitMix64(SEED);
    let mut distances = vec![0.0f64; PAIRS];
    let mut sum = 0.0;
    for distance in distances.iter_mut() {
        let a = surface_point(&corners, &cumulative, total_area, &mut rng);
        let b = surface_point(&corners, &cumulative, total_area, &mut rng);
        *distance = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        sum += *distance;
    }
    let mean = sum / PAIRS as f64;
    if !mean.is_finite() || mean <= 0.0 {
        return Err(refused(
            "two points on it are never any distance apart".to_owned(),
        ));
    }

    let width = SPAN * mean / BINS as f64;
    let mut counts = [0u32; BINS];
    for distance in &distances {
        let bin = ((distance / width) as usize).min(BINS - 1);
        counts[bin] += 1;
    }

    let mut descriptor = [0.0f32; DESCRIPTOR_LEN];
    for (slot, count) in descriptor.iter_mut().zip(&counts) {
        *slot = (f64::from(*count) / PAIRS as f64).sqrt() as f32;
    }
    // Clamped because a covariance that is numerically flat can put a hair below zero into λ3,
    // and a negative ratio is a coordinate no consumer expects.
    descriptor[BINS] = (l2 / l1).clamp(0.0, 1.0) as f32;
    descriptor[BINS + 1] = (l3 / l1).clamp(0.0, 1.0) as f32;
    descriptor[BINS + 2] = (total_area / (mean * mean)).ln() as f32;
    if !descriptor.iter().all(|f| f.is_finite()) {
        return Err(refused(
            "one of its 35 numbers came out infinite".to_owned(),
        ));
    }

    Ok(ShapeProfile {
        size_mm: mean,
        descriptor,
    })
}

fn triangle_area(t: &[[f64; 3]; 3]) -> f64 {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() / 2.0
}

/// One point drawn uniformly over the whole surface: the triangle by a binary search on
/// cumulative area, then a point uniform inside it by folding the unit square onto the triangle.
fn surface_point(
    corners: &[[[f64; 3]; 3]],
    cumulative: &[f64],
    total_area: f64,
    rng: &mut SplitMix64,
) -> [f64; 3] {
    // One draw, then the search — a `partition_point` whose predicate draws is a predicate that
    // answers differently every time it is asked, which is not a binary search at all.
    let target = rng.next_f64() * total_area;
    let at = cumulative
        .partition_point(|&upto| upto <= target)
        .min(corners.len() - 1);
    let t = &corners[at];
    let (mut u, mut v) = (rng.next_f64(), rng.next_f64());
    if u + v > 1.0 {
        u = 1.0 - u;
        v = 1.0 - v;
    }
    let mut point = [0.0; 3];
    for axis in 0..3 {
        point[axis] = t[0][axis] + u * (t[1][axis] - t[0][axis]) + v * (t[2][axis] - t[0][axis]);
    }
    point
}

/// The eigenvalues of a symmetric 3×3 matrix, descending, in closed form (Smith's method).
///
/// Closed form rather than an iterative solver so the answer is a fixed number of float
/// operations, and so the same matrix always gives the same bits.
fn eigenvalues(m: [[f64; 3]; 3]) -> [f64; 3] {
    let off = m[0][1] * m[0][1] + m[0][2] * m[0][2] + m[1][2] * m[1][2];
    let trace = m[0][0] + m[1][1] + m[2][2];
    if off == 0.0 {
        // Already diagonal — the cube's covariance, among others, and the case where the
        // general branch below divides by a zero `p`.
        let mut diagonal = [m[0][0], m[1][1], m[2][2]];
        diagonal.sort_by(|a, b| b.total_cmp(a));
        return diagonal;
    }
    let q = trace / 3.0;
    let p2 = (m[0][0] - q).powi(2) + (m[1][1] - q).powi(2) + (m[2][2] - q).powi(2) + 2.0 * off;
    let p = (p2 / 6.0).sqrt();
    if !p.is_finite() || p <= 0.0 {
        return [q, q, q];
    }
    // B = (A - qI)/p, whose determinant gives the angle below.
    let mut b = m;
    for (axis, row) in b.iter_mut().enumerate() {
        row[axis] -= q;
        for cell in row.iter_mut() {
            *cell /= p;
        }
    }
    // Clamped: det(B)/2 is ±1 for a matrix with a repeated eigenvalue and drifts a hair past it
    // in float, where `acos` would answer NaN.
    let r = (determinant(b) / 2.0).clamp(-1.0, 1.0);
    let phi = r.acos() / 3.0;
    let first = q + 2.0 * p * phi.cos();
    let third = q + 2.0 * p * (phi + std::f64::consts::TAU / 3.0).cos();
    [first, trace - first - third, third]
}

fn determinant(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{Lod, cluster};
    use crate::glb::read_triangles;
    use crate::stl::{Mesh, parse_stl};
    use lapidary_core::shape::{NEAR_DUPLICATE_DISTANCE, distance, is_near_duplicate};

    /// How far two profiles of one shape may sit apart before a test calls it a failure. Twice the
    /// sampler's noise floor of 0.011 (the arithmetic is on [`PAIRS`]), so anything under this is
    /// "the same shape, sampled twice" and anything over it is a real difference. Half of
    /// `NEAR_DUPLICATE_DISTANCE` and not the whole of it: a tolerance as wide as the threshold
    /// would pass a descriptor that had stopped being invariant at all.
    const NOISE: f32 = NEAR_DUPLICATE_DISTANCE / 2.0;

    /// A soup of triangles as `profile` wants it: one vertex per corner, no sharing. Clustering
    /// is what shares vertices, and these meshes are the input to clustering.
    fn soup(mesh: &Mesh) -> (Vec<[f32; 3]>, Vec<u32>) {
        let positions: Vec<[f32; 3]> = mesh.triangles.iter().flatten().copied().collect();
        let indices = (0..positions.len() as u32).collect();
        (positions, indices)
    }

    fn profile_of(mesh: &Mesh) -> ShapeProfile {
        let (positions, indices) = soup(mesh);
        profile(&positions, &indices).expect("this mesh has a shape")
    }

    /// The profile of what the worker really reads: the mesh clustered to L0 and written as our
    /// GLB, then decoded again.
    fn profile_of_l0(mesh: &Mesh) -> ShapeProfile {
        let rung = cluster(mesh, Lod::L0).expect("clusters");
        let (positions, indices) = read_triangles(&rung.glb).expect("our own rung decodes");
        profile(&positions, &indices).expect("an L0 rung has a shape")
    }

    fn mapped(mesh: &Mesh, f: impl Fn([f32; 3]) -> [f32; 3]) -> Mesh {
        Mesh {
            triangles: mesh
                .triangles
                .iter()
                .map(|t| [f(t[0]), f(t[1]), f(t[2])])
                .collect(),
            parts: vec![],
        }
    }

    /// A rotation about (1, 2, 3)/|(1, 2, 3)| by 0.7 rad — an axis and an angle that line up with
    /// no coordinate plane, so a descriptor that quietly depended on one fails.
    fn rotated(mesh: &Mesh) -> Mesh {
        let (x, y, z) = (1.0f64, 2.0, 3.0);
        let n = (x * x + y * y + z * z).sqrt();
        let (a, b, c) = (x / n, y / n, z / n);
        let (s, k) = (0.7f64.sin(), 1.0 - 0.7f64.cos());
        let t = 0.7f64.cos();
        let m = [
            [t + a * a * k, a * b * k - c * s, a * c * k + b * s],
            [b * a * k + c * s, t + b * b * k, b * c * k - a * s],
            [c * a * k - b * s, c * b * k + a * s, t + c * c * k],
        ];
        mapped(mesh, |v| {
            let v = v.map(f64::from);
            [0, 1, 2].map(|row| (m[row][0] * v[0] + m[row][1] * v[1] + m[row][2] * v[2]) as f32)
        })
    }

    /// A 60 mm cube, filed the way an STL files one: two triangles a face, wound outward.
    fn cube(side: f32) -> Mesh {
        let h = side / 2.0;
        let corner = |x: i32, y: i32, z: i32| {
            [
                if x > 0 { h } else { -h },
                if y > 0 { h } else { -h },
                if z > 0 { h } else { -h },
            ]
        };
        let mut triangles = Vec::with_capacity(12);
        for (axis, sign) in [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)] {
            let mut quad = [[0i32; 3]; 4];
            for (at, uv) in [(0, (-1, -1)), (1, (1, -1)), (2, (1, 1)), (3, (-1, 1))] {
                let mut point = [0; 3];
                point[axis] = sign;
                point[(axis + 1) % 3] = uv.0 * sign;
                point[(axis + 2) % 3] = uv.1;
                quad[at] = point;
            }
            let p = quad.map(|q| corner(q[0], q[1], q[2]));
            triangles.push([p[0], p[1], p[2]]);
            triangles.push([p[0], p[2], p[3]]);
        }
        Mesh {
            triangles,
            parts: vec![],
        }
    }

    /// A closed cylinder standing on the z axis: sides, and a fan on each end.
    fn cylinder(radius: f32, height: f32, segments: usize) -> Mesh {
        let at = |i: usize, z: f32| {
            let angle = std::f64::consts::TAU * i as f64 / segments as f64;
            [radius * angle.cos() as f32, radius * angle.sin() as f32, z]
        };
        let (bottom, top) = (-height / 2.0, height / 2.0);
        let mut triangles = Vec::with_capacity(segments * 4);
        for i in 0..segments {
            let (a, b) = (at(i, bottom), at(i + 1, bottom));
            let (c, d) = (at(i, top), at(i + 1, top));
            triangles.push([a, b, d]);
            triangles.push([a, d, c]);
            triangles.push([[0.0, 0.0, bottom], b, a]);
            triangles.push([[0.0, 0.0, top], c, d]);
        }
        Mesh {
            triangles,
            parts: vec![],
        }
    }

    fn bracket() -> Mesh {
        parse_stl(include_bytes!("../../../fixtures/bracket-lp-1042-03.stl"))
            .expect("the bracket fixture parses")
    }

    #[test]
    fn the_same_mesh_twice_gives_bit_identical_output() {
        let mesh = cylinder(11.0, 48.0, 64);
        let (positions, indices) = soup(&mesh);
        let once = profile(&positions, &indices).expect("profiles");
        let twice = profile(&positions, &indices).expect("profiles");
        assert_eq!(
            once.descriptor.map(f32::to_bits),
            twice.descriptor.map(f32::to_bits),
            "the profile must be pure: a stored one is compared against a fresh one"
        );
        assert_eq!(once.size_mm.to_bits(), twice.size_mm.to_bits());
    }

    #[test]
    fn moving_a_part_does_not_change_its_shape() {
        let mesh = bracket();
        let moved = mapped(&mesh, |v| [v[0] + 250.0, v[1] - 80.0, v[2] + 1200.0]);
        let (here, there) = (profile_of(&mesh), profile_of(&moved));
        assert!(
            distance(&here.descriptor, &there.descriptor) < NOISE,
            "translated: {}",
            distance(&here.descriptor, &there.descriptor)
        );
        assert!((here.size_mm - there.size_mm).abs() / here.size_mm < 1e-4);
    }

    #[test]
    fn turning_a_part_does_not_change_its_shape() {
        let mesh = bracket();
        let (upright, turned) = (profile_of(&mesh), profile_of(&rotated(&mesh)));
        assert!(
            distance(&upright.descriptor, &turned.descriptor) < NOISE,
            "rotated: {}",
            distance(&upright.descriptor, &turned.descriptor)
        );
        assert!((upright.size_mm - turned.size_mm).abs() / upright.size_mm < 1e-4);
    }

    #[test]
    fn filing_the_same_triangles_in_another_order_does_not_change_the_shape() {
        let mesh = cylinder(11.0, 48.0, 64);
        // Reversed, and with each triangle's corners rotated: a mesh written by another
        // exporter, holding the same surface.
        let shuffled = Mesh {
            triangles: mesh
                .triangles
                .iter()
                .rev()
                .map(|t| [t[2], t[0], t[1]])
                .collect(),
            parts: vec![],
        };
        let apart = distance(
            &profile_of(&mesh).descriptor,
            &profile_of(&shuffled).descriptor,
        );
        assert!(apart < NOISE, "reordered: {apart}");
    }

    #[test]
    fn a_finer_tessellation_of_one_cylinder_is_the_same_shape() {
        let coarse = profile_of(&cylinder(11.0, 48.0, 48));
        let fine = profile_of(&cylinder(11.0, 48.0, 192));
        let apart = distance(&coarse.descriptor, &fine.descriptor);
        assert!(apart < NOISE, "re-tessellated: {apart}");
        assert!(is_near_duplicate(&coarse, &fine), "and a near-duplicate");
    }

    /// The whole production path: cluster to L0, write the GLB, decode it, profile. L0's grid is
    /// aligned to the part's bounding box, so a turned part clusters to genuinely different
    /// triangles — this is the invariance that has to survive a real rung, not just a mesh.
    #[test]
    fn a_turned_part_is_still_a_near_duplicate_through_its_l0_rung() {
        let mesh = cylinder(11.0, 48.0, 96);
        let upright = profile_of_l0(&mesh);
        let turned = profile_of_l0(&rotated(&mesh));
        assert!(
            is_near_duplicate(&upright, &turned),
            "turned through L0: {} apart, sizes {} and {}",
            distance(&upright.descriptor, &turned.descriptor),
            upright.size_mm,
            turned.size_mm
        );
    }

    #[test]
    fn scaling_a_part_keeps_its_descriptor_and_scales_its_size() {
        let mesh = bracket();
        let small = profile_of(&mesh);
        let large = profile_of(&mapped(&mesh, |v| v.map(|c| c * 2.5)));
        let apart = distance(&small.descriptor, &large.descriptor);
        assert!(apart < 1e-3, "scaled 2.5x: the descriptor moved {apart}");
        assert!(
            (large.size_mm / small.size_mm - 2.5).abs() < 1e-3,
            "size_mm went {} -> {}",
            small.size_mm,
            large.size_mm
        );
        assert!(
            !is_near_duplicate(&small, &large),
            "one shape at two sizes is two parts"
        );
    }

    /// Documented, not a bug: see this module's header. A left and a right bracket are proposed
    /// as near-duplicates and a person answers "Not the same" once.
    #[test]
    fn a_mirror_image_looks_identical_and_is_meant_to() {
        let mesh = bracket();
        let mirrored = mapped(&mesh, |v| [-v[0], v[1], v[2]]);
        let apart = distance(
            &profile_of(&mesh).descriptor,
            &profile_of(&mirrored).descriptor,
        );
        assert!(
            apart < NOISE,
            "mirroring must be invisible to the descriptor, saw {apart}"
        );
    }

    #[test]
    fn a_cube_a_cylinder_and_a_bracket_are_farther_apart_than_the_threshold() {
        let shapes = [
            ("cube", profile_of(&cube(60.0))),
            ("cylinder", profile_of(&cylinder(11.0, 48.0, 96))),
            ("bracket", profile_of(&bracket())),
        ];
        for (i, (one, first)) in shapes.iter().enumerate() {
            for (other, second) in &shapes[i + 1..] {
                let apart = distance(&first.descriptor, &second.descriptor);
                assert!(
                    apart > NEAR_DUPLICATE_DISTANCE,
                    "{one} and {other} are only {apart} apart"
                );
            }
        }
    }

    #[test]
    fn a_mesh_with_no_surface_is_refused_rather_than_profiled() {
        let flat = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]];
        let message = profile(&flat, &[0, 1, 2])
            .expect_err("three points on a line have no surface")
            .to_string();
        assert!(
            message.contains("no surface"),
            "the error must say what is wrong with the mesh, got: {message}"
        );
        profile(&[], &[]).expect_err("no triangles at all");
        profile(&flat, &[0, 1, 9]).expect_err("an index past the positions");
        let nowhere = [[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        profile(&nowhere, &[0, 1, 2]).expect_err("a corner that is not a position");
    }
}
