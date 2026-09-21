//! A part's shape profile, and the rules for calling two parts alike (Phase 6; design in
//! `docs/goals/phase-6.md`).
//!
//! The profile is computed in the worker from a part's stored L0 tessellation — never from the source
//! file, never by the kernel — by `lapidary_cad::shape`. This module is only what every consumer
//! agrees on: the vector's length and version, the distance between two profiles, and the two
//! thresholds. Nothing here is shown to a person as a number, so nothing here needs the
//! "approximate" label: a person sees "near-duplicate" or "similar", never a score.

/// How many floats a profile's descriptor holds: 32 D2-distribution bins, stored as square roots of
/// their probabilities, then λ2/λ1, λ3/λ1 and ln(area / m²).
pub const DESCRIPTOR_LEN: usize = 35;

/// The profile algorithm's version. A change to the sampler, its seed, the bins or the ratios bumps
/// it; a stored profile of another version is ignored by every read and computed again.
pub const SHAPE_VERSION: i16 = 1;

/// The largest [`distance`] at which two parts of about the same size are proposed as
/// near-duplicates. A starting value: goal G2 calibrates it against the STL corpus and records how.
pub const NEAR_DUPLICATE_DISTANCE: f32 = 0.04;

/// How far apart two parts' sizes may be, as `|ln(a / b)|`, and still be near-duplicates: 2%.
/// A 20 mm and a 40 mm spacer have one shape and are not one part.
pub fn size_band() -> f64 {
    1.02_f64.ln()
}

/// One part's shape: its descriptor, and how big it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeProfile {
    /// The mean distance between two points on the surface, in millimetres. Needs no volume, so an
    /// open mesh has one; scale changes it, while it leaves the descriptor alone.
    pub size_mm: f64,
    pub descriptor: [f32; DESCRIPTOR_LEN],
}

/// The distance between two descriptors: Euclidean over the whole vector. Because the D2 block holds
/// square roots of probabilities, that block's part of it is the Hellinger distance between the two
/// distributions, bounded and symmetric.
pub fn distance(a: &[f32; DESCRIPTOR_LEN], b: &[f32; DESCRIPTOR_LEN]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Whether two profiles are near-duplicates: alike in shape **and** in size.
pub fn is_near_duplicate(a: &ShapeProfile, b: &ShapeProfile) -> bool {
    distance(&a.descriptor, &b.descriptor) <= NEAR_DUPLICATE_DISTANCE
        && (a.size_mm / b.size_mm).ln().abs() <= size_band()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(size_mm: f64, fill: f32) -> ShapeProfile {
        ShapeProfile {
            size_mm,
            descriptor: [fill; DESCRIPTOR_LEN],
        }
    }

    #[test]
    fn a_profile_is_no_distance_from_itself_and_distance_is_symmetric() {
        let a = profile(20.0, 0.1).descriptor;
        let mut b = a;
        b[3] = 0.4;
        assert_eq!(distance(&a, &a), 0.0);
        assert_eq!(distance(&a, &b), distance(&b, &a));
        assert!((distance(&a, &b) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn the_size_band_admits_one_point_nine_percent_and_refuses_two_point_one() {
        let base = profile(100.0, 0.2);
        assert!(is_near_duplicate(&base, &profile(101.9, 0.2)));
        assert!(
            is_near_duplicate(&profile(101.9, 0.2), &base),
            "either way round"
        );
        assert!(!is_near_duplicate(&base, &profile(102.1, 0.2)));
        assert!(!is_near_duplicate(&base, &profile(97.9, 0.2)));
    }

    #[test]
    fn the_same_size_with_a_different_shape_is_not_a_near_duplicate() {
        let a = profile(50.0, 0.2);
        let mut b = profile(50.0, 0.2);
        b.descriptor[0] = 0.3; // 0.1 apart, past the threshold
        assert!(!is_near_duplicate(&a, &b));
    }
}
