//! How `NEAR_DUPLICATE_DISTANCE` was calibrated (goal G2, stage 3).
//!
//! `#[ignore]`d, and it has to be: it reads about 6 GB of the owner's own STL corpus, which no CI
//! runner and no other machine has. It is checked in because the number it produces is an owner
//! decision that has to be reproducible — the alternative is a threshold in `lapidary-core` with
//! a comment claiming it was measured and no way to measure it again.
//!
//! Run it in release, outside the compile lock, after building it under the lock:
//!
//! ```sh
//! cargo xtask heavy -- cargo test --release -p lapidary-cad --test calibration --no-run
//! target/release/deps/calibration-<hash> --ignored --nocapture
//! ```
//!
//! Inputs, both overridable by environment variable:
//! - `LAPIDARY_CORPUS_INDEX` — a `size<TAB>path` index, paths relative to the corpus root.
//! - `LAPIDARY_CORPUS_ROOT` — the corpus itself, **read only**; this test opens files and writes
//!   nothing under it.
//!
//! It writes its report to `target/calibration-G2.txt` beside stdout, so the numbers in the goal's
//! Record can be checked against the run that produced them.

use lapidary_cad::{Lod, Mesh, cluster, parse_stl, profile, read_triangles};
use lapidary_core::shape::{DESCRIPTOR_LEN, ShapeProfile, distance, size_band};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Instant;

/// How many bins of the descriptor are the D2 distribution; the rest are the three ratios.
const BINS: usize = 32;

/// Files past this are skipped and counted. Three other lanes share 15.5 GiB of RAM, and a
/// several-hundred-megabyte mesh is a clustering run that takes the machine down with it. The
/// 1,000-file index tops out at 14.4 MB, so nothing in this run is actually skipped — the cap is
/// here for the day somebody points it at the whole 320 GB corpus.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Every nth corpus part gets a set of generated duplicates. 1,000/15 is about 66 parts, which is
/// enough to see a distribution without quadrupling the run.
const EVERY: usize = 15;

fn percentiles(sorted: &[f32], at: &[f64]) -> Vec<f32> {
    at.iter()
        .map(|q| {
            let index = ((sorted.len() - 1) as f64 * q).round() as usize;
            sorted[index]
        })
        .collect()
}

/// A profile's four blocks of distance from another: the 32 D2 bins together (which is the
/// Hellinger part), then each ratio on its own. Squares to the whole distance, so it says which
/// term spends the budget.
fn blocks(a: &[f32; DESCRIPTOR_LEN], b: &[f32; DESCRIPTOR_LEN]) -> [f32; 4] {
    let d2 = a[..BINS]
        .iter()
        .zip(&b[..BINS])
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt();
    [
        d2,
        (a[BINS] - b[BINS]).abs(),
        (a[BINS + 1] - b[BINS + 1]).abs(),
        (a[BINS + 2] - b[BINS + 2]).abs(),
    ]
}

/// The distance with `ln(area / m²)` left out: the other 34 floats only.
///
/// Not a candidate descriptor — it is a measurement. The calibration below shows that term is
/// what a re-clustered rung moves most, and the lead needs the figure with and without it to
/// decide whether the design's third ratio earns its place at L0's grid.
fn distance_without_area(a: &[f32; DESCRIPTOR_LEN], b: &[f32; DESCRIPTOR_LEN]) -> f32 {
    a[..DESCRIPTOR_LEN - 1]
        .iter()
        .zip(&b[..DESCRIPTOR_LEN - 1])
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

fn rotate(mesh: &Mesh, radians: f64) -> Mesh {
    // About (1, 2, 3)/|(1, 2, 3)|: an axis that lines up with no coordinate plane, so the L0 grid
    // really is rebuilt around a different bounding box.
    let n = 14.0f64.sqrt();
    let (a, b, c) = (1.0 / n, 2.0 / n, 3.0 / n);
    let (s, t) = (radians.sin(), radians.cos());
    let k = 1.0 - t;
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

fn mapped(mesh: &Mesh, f: impl Fn([f32; 3]) -> [f32; 3]) -> Mesh {
    Mesh {
        triangles: mesh
            .triangles
            .iter()
            .map(|t| [f(t[0]), f(t[1]), f(t[2])])
            .collect(),
        parts: mesh.parts.clone(),
    }
}

/// The profile of a mesh as the worker computes it: clustered to a rung, written as our GLB,
/// decoded again, profiled.
fn through_rung(mesh: &Mesh, lod: Lod) -> Option<(ShapeProfile, f64, u32)> {
    let rung = cluster(mesh, lod).ok()?;
    let (positions, indices) = read_triangles(&rung.glb).ok()?;
    let started = Instant::now();
    let profile = profile(&positions, &indices).ok()?;
    Some((
        profile,
        started.elapsed().as_secs_f64() * 1000.0,
        rung.triangle_count,
    ))
}

#[test]
#[ignore = "reads the owner's 6 GB STL corpus; see this file's header"]
fn calibrate_the_near_duplicate_threshold() {
    let root = std::env::var("LAPIDARY_CORPUS_ROOT")
        .unwrap_or_else(|_| "/mnt/Storage2/All/STL Files".to_owned());
    let index = std::env::var("LAPIDARY_CORPUS_INDEX").unwrap_or_else(|_| {
        format!(
            "{}/../../target/calibration/corpus-1000.tsv",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let listing = std::fs::read_to_string(&index)
        .unwrap_or_else(|e| panic!("could not read the corpus index {index}: {e}"));

    let mut report = String::new();
    let mut profiles: Vec<(String, ShapeProfile)> = Vec::new();
    let mut identical: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut timings: Vec<f64> = Vec::new();
    let mut triangles_l0: Vec<u32> = Vec::new();
    let (mut too_big, mut unparsed, mut unprofiled) = (0usize, 0usize, 0usize);
    /// One generated duplicate, measured every way the Record needs it.
    struct Copy {
        what: &'static str,
        apart: f32,
        /// The same distance with `ln(area / m²)` left out.
        apart_without_area: f32,
        /// `|ln(size_a / size_b)|`, which the ±2% band admits up to `size_band()` = 0.0198.
        size_shift: f64,
        blocks: [f32; 4],
    }
    let mut generated: Vec<Copy> = Vec::new();

    let started = Instant::now();
    for (at, line) in listing.lines().enumerate() {
        let Some((size, path)) = line.split_once('\t') else {
            continue;
        };
        if size.parse::<u64>().unwrap_or(0) > MAX_FILE_BYTES {
            too_big += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(format!("{root}/{path}")) else {
            unparsed += 1;
            continue;
        };
        let Ok(mesh) = parse_stl(&bytes) else {
            unparsed += 1;
            continue;
        };
        let Some((shape, took_ms, l0_triangles)) = through_rung(&mesh, Lod::L0) else {
            unprofiled += 1;
            continue;
        };
        timings.push(took_ms);
        triangles_l0.push(l0_triangles);
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        identical.entry(hasher.finish()).or_default().push(at);

        // The generated duplicates of every nth part, each through the same L0 path a re-ingest
        // would take: turned, moved, scaled 1% (inside the size band), and re-tessellated at L1's
        // budget, which is what a kernel-version bump does to a stored rung.
        if at % EVERY == 0 {
            let cases: [(&str, Mesh, Lod); 4] = [
                ("rotated 37°", rotate(&mesh, 0.6458), Lod::L0),
                (
                    "moved 250 mm",
                    mapped(&mesh, |v| [v[0] + 250.0, v[1] - 80.0, v[2] + 40.0]),
                    Lod::L0,
                ),
                ("scaled 1%", mapped(&mesh, |v| v.map(|c| c * 1.01)), Lod::L0),
                ("re-tessellated (L1)", mesh.clone(), Lod::L1),
            ];
            for (what, copy, lod) in cases {
                let Some((other, _, _)) = through_rung(&copy, lod) else {
                    continue;
                };
                generated.push(Copy {
                    what,
                    apart: distance(&shape.descriptor, &other.descriptor),
                    apart_without_area: distance_without_area(&shape.descriptor, &other.descriptor),
                    // Whether the copy is still a candidate at all: rotation and re-tessellation
                    // move `size_mm` as well as the descriptor, and a part outside the band is
                    // never compared, whatever the threshold is.
                    size_shift: (shape.size_mm / other.size_mm).ln().abs(),
                    blocks: blocks(&shape.descriptor, &other.descriptor),
                });
            }
        }
        profiles.push((path.to_owned(), shape));
    }
    let walked = started.elapsed().as_secs_f64();

    writeln!(
        report,
        "corpus: {} profiled of {} indexed ({} unreadable or unparsed, {} unprofilable, {} over \
         {} MB), in {walked:.0}s",
        profiles.len(),
        listing.lines().count(),
        unparsed,
        unprofiled,
        too_big,
        MAX_FILE_BYTES / 1024 / 1024,
    )
    .expect("writes");

    timings.sort_by(f64::total_cmp);
    triangles_l0.sort_unstable();
    writeln!(
        report,
        "one profile of one L0: median {:.2} ms, p95 {:.2} ms, worst {:.2} ms (L0 triangles: \
         median {}, worst {})",
        timings[timings.len() / 2],
        timings[timings.len() * 95 / 100],
        timings[timings.len() - 1],
        triangles_l0[triangles_l0.len() / 2],
        triangles_l0[triangles_l0.len() - 1],
    )
    .expect("writes");

    let repeats: usize = identical.values().filter(|of| of.len() > 1).count();
    writeln!(
        report,
        "byte-identical groups in the index: {repeats} (those pairs are the \"Identical\" rule's, \
         not the threshold's)",
    )
    .expect("writes");

    // Nearest neighbour of every part, twice: over the whole set, and over the parts inside its
    // ±2% size band, which is the only comparison the near-duplicate rule ever makes.
    let mut nearest: Vec<f32> = Vec::with_capacity(profiles.len());
    let mut nearest_in_band: Vec<f32> = Vec::new();
    let mut nearest_in_band_without_area: Vec<f32> = Vec::new();
    let mut in_band_pairs = 0u64;
    let mut closest: Vec<(f32, usize, usize)> = Vec::new();
    for (i, (_, a)) in profiles.iter().enumerate() {
        let (mut best, mut best_band) = (f32::INFINITY, f32::INFINITY);
        let mut best_band_without_area = f32::INFINITY;
        let mut best_at = i;
        for (j, (_, b)) in profiles.iter().enumerate() {
            if i == j {
                continue;
            }
            let apart = distance(&a.descriptor, &b.descriptor);
            if apart < best {
                best = apart;
                best_at = j;
            }
            if (a.size_mm / b.size_mm).ln().abs() <= size_band() {
                in_band_pairs += 1;
                best_band = best_band.min(apart);
                best_band_without_area =
                    best_band_without_area.min(distance_without_area(&a.descriptor, &b.descriptor));
            }
        }
        nearest.push(best);
        if best_band.is_finite() {
            nearest_in_band.push(best_band);
            nearest_in_band_without_area.push(best_band_without_area);
        }
        if i < best_at {
            closest.push((best, i, best_at));
        }
    }
    nearest.sort_by(f32::total_cmp);
    nearest_in_band.sort_by(f32::total_cmp);
    nearest_in_band_without_area.sort_by(f32::total_cmp);
    let at = [0.0, 0.001, 0.005, 0.01, 0.05, 0.1, 0.25, 0.5, 0.9, 1.0];
    writeln!(
        report,
        "\nnearest-neighbour distance over all {} parts, percentiles {at:?}:\n  {:?}",
        profiles.len(),
        percentiles(&nearest, &at),
    )
    .expect("writes");
    writeln!(
        report,
        "nearest neighbour inside the ±2% size band ({} parts have one, {in_band_pairs} ordered \
         pairs are in band):\n  {:?}",
        nearest_in_band.len(),
        percentiles(&nearest_in_band, &at),
    )
    .expect("writes");

    writeln!(
        report,
        "the same, with ln(area/m²) left out of the distance (34 floats):\n  {:?}",
        percentiles(&nearest_in_band_without_area, &at),
    )
    .expect("writes");

    // How many parts have a neighbour under each candidate threshold. The threshold has to sit
    // where this stops growing with it.
    writeln!(
        report,
        "\nparts with an in-band neighbour under d (of {}):",
        nearest_in_band.len()
    )
    .expect("writes");
    for candidate in [0.005f32, 0.01, 0.02, 0.03, 0.04, 0.06, 0.08, 0.12, 0.2] {
        let under = nearest_in_band.iter().filter(|&&d| d <= candidate).count();
        let not_identical = nearest_in_band
            .iter()
            .filter(|&&d| d <= candidate && d > 1e-6)
            .count();
        let without_area = nearest_in_band_without_area
            .iter()
            .filter(|&&d| d <= candidate)
            .count();
        writeln!(
            report,
            "  d <= {candidate:<6} {under:>4} parts ({not_identical} of them not at distance 0); \
             {without_area:>4} without the area term",
        )
        .expect("writes");
    }

    // The generated duplicates, per kind: what a re-ingest of the same part really costs.
    writeln!(
        report,
        "\ngenerated duplicates, distance from the original:"
    )
    .expect("writes");
    for what in [
        "rotated 37°",
        "moved 250 mm",
        "scaled 1%",
        "re-tessellated (L1)",
    ] {
        let theirs: Vec<&Copy> = generated.iter().filter(|c| c.what == what).collect();
        if theirs.is_empty() {
            continue;
        }
        let median = |mut of: Vec<f32>| {
            of.sort_by(f32::total_cmp);
            (of[of.len() / 2], of[of.len() * 9 / 10], of[of.len() - 1])
        };
        let (mid, p90, worst) = median(theirs.iter().map(|c| c.apart).collect());
        let (mid_flat, p90_flat, _) = median(theirs.iter().map(|c| c.apart_without_area).collect());
        let out_of_band = theirs.iter().filter(|c| c.size_shift > size_band()).count();
        let mut shifts: Vec<f64> = theirs.iter().map(|c| c.size_shift).collect();
        shifts.sort_by(f64::total_cmp);
        let mut mean_blocks = [0.0f32; 4];
        for copy in &theirs {
            for (slot, value) in mean_blocks.iter_mut().zip(&copy.blocks) {
                *slot += value / theirs.len() as f32;
            }
        }
        writeln!(
            report,
            "  {what:<20} n={:<4} median {mid:.4}  p90 {p90:.4}  worst {worst:.4}\n\
             {:22}without the area term: median {mid_flat:.4}, p90 {p90_flat:.4}\n\
             {:22}size_mm moved: median {:.4}, p90 {:.4} (band admits {:.4}); {out_of_band} of \
             {} fell out of the band\n\
             {:22}mean blocks: D2 {:.4}, λ2/λ1 {:.4}, λ3/λ1 {:.4}, ln(A/m²) {:.4}",
            theirs.len(),
            "",
            "",
            shifts[shifts.len() / 2],
            shifts[shifts.len() * 9 / 10],
            size_band(),
            theirs.len(),
            "",
            mean_blocks[0],
            mean_blocks[1],
            mean_blocks[2],
            mean_blocks[3],
        )
        .expect("writes");
    }

    // The closest genuine pairs, named, so the gap can be read rather than trusted.
    closest.sort_by(|a, b| a.0.total_cmp(&b.0));
    writeln!(report, "\nthe 25 closest pairs in the corpus:").expect("writes");
    for (apart, i, j) in closest.iter().take(25) {
        writeln!(
            report,
            "  {apart:.5}\n  a {}\n  b {}",
            profiles[*i].0, profiles[*j].0
        )
        .expect("writes");
    }

    print!("{report}");
    let out = format!(
        "{}/../../target/calibration-G2.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(&out, &report).unwrap_or_else(|e| panic!("could not write {out}: {e}"));
    println!("\nwritten to {out}");
}
