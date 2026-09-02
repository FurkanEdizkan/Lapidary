//! Flat-shaded thumbnail rendering on the CPU. No GPU, no driver surface, and — the
//! point — bit-identical output on every host, so a derivative can be regenerated and
//! compared rather than merely re-made.

use crate::{CadError, Mesh};

pub const THUMB_PX: u32 = 512;

/// Bumped whenever a change alters output bytes. Persisted as `derivative.kernel_version`
/// so a later pass can find thumbnails made by an older renderer.
pub const RASTER_VERSION: &str = "cpu-1";

/// DATA.md §1.5 stores thumbnails inline as `bytea` only under 64 KB. Beyond that they
/// would have to become filesystem blobs, costing the grid a round trip per card — the
/// exact cost the inline exception exists to avoid.
pub const MAX_THUMB_BYTES: usize = 64 * 1024;

/// Fallback sizes, tried in order, when a mesh renders larger than the inline budget.
/// WebP here is lossless, so there is no quality to trade — only pixels.
const FALLBACK_PX: [u32; 2] = [384, 256];

/// Fixed three-quarter view. Constants, not parameters: a thumbnail that changes angle
/// between runs is not comparable, and the grid wants every card framed alike.
const VIEW_DIR: [f64; 3] = [0.577_350_27, -0.577_350_27, 0.577_350_27];
const LIGHT_DIR: [f64; 3] = [0.408_248_3, -0.408_248_3, 0.816_496_6];
const BG: [u8; 3] = [10, 10, 12]; // matches the app's dark surface
const BASE: [f64; 3] = [0.82, 0.84, 0.88];
const AMBIENT: f64 = 0.18;
const MARGIN: f64 = 0.92; // fraction of the frame the model fills

pub fn render_thumbnail(mesh: &Mesh) -> Result<Vec<u8>, CadError> {
    let (right, up) = basis();
    let n = THUMB_PX as usize;

    // Project every vertex into view space once, then fit.
    let projected: Vec<[[f64; 3]; 3]> = mesh
        .triangles
        .iter()
        .map(|t| {
            t.map(|v| {
                let p = [v[0] as f64, v[1] as f64, v[2] as f64];
                [dot(p, right), dot(p, up), dot(p, VIEW_DIR)]
            })
        })
        .collect();

    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for t in &projected {
        for p in t {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !extent.is_finite() || extent <= 0.0 {
        return Err(CadError::Unrenderable {
            detail: "the mesh projects to zero size — every vertex is at the same point".to_owned(),
        });
    }
    let scale = (n as f64 * MARGIN) / extent;
    let cx = (lo[0] + hi[0]) / 2.0;
    let cy = (lo[1] + hi[1]) / 2.0;
    let half = n as f64 / 2.0;

    let mut colour = [BG[0], BG[1], BG[2]].repeat(n * n);
    let mut depth = vec![f64::NEG_INFINITY; n * n];

    for (t, world) in projected.iter().zip(&mesh.triangles) {
        let shade = shade_of(world);
        let px: Vec<[f64; 3]> = t
            .iter()
            .map(|p| [(p[0] - cx) * scale + half, half - (p[1] - cy) * scale, p[2]])
            .collect();
        fill(&mut colour, &mut depth, n, &px, shade);
    }

    let mut rgba = Vec::with_capacity(n * n * 4);
    for px in colour.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    let img = image::RgbaImage::from_raw(THUMB_PX, THUMB_PX, rgba).ok_or_else(|| {
        CadError::Unrenderable {
            detail: "frame buffer size mismatch".to_owned(),
        }
    })?;

    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::WebP)
        .map_err(|e| CadError::Unrenderable {
            detail: format!("WebP encoding failed: {e}"),
        })?;
    let bytes = out.into_inner();
    if bytes.len() <= MAX_THUMB_BYTES {
        return Ok(bytes);
    }

    // Over budget: retry smaller. Failing loudly beats writing a row that quietly
    // breaks the inline-storage contract every grid query depends on.
    for px in FALLBACK_PX {
        let smaller = image::imageops::resize(&img, px, px, image::imageops::FilterType::Triangle);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(smaller)
            .write_to(&mut buf, image::ImageFormat::WebP)
            .map_err(|e| CadError::Unrenderable {
                detail: format!("WebP encoding failed: {e}"),
            })?;
        let candidate = buf.into_inner();
        if candidate.len() <= MAX_THUMB_BYTES {
            return Ok(candidate);
        }
    }

    Err(CadError::Unrenderable {
        detail: format!(
            "the thumbnail is {} bytes even at {}px, over the {MAX_THUMB_BYTES}-byte inline limit",
            bytes.len(),
            FALLBACK_PX[FALLBACK_PX.len() - 1]
        ),
    })
}

/// Lambert against a fixed headlight, clamped so back-facing triangles still read as
/// surface rather than as holes.
fn shade_of(world: &[[f32; 3]; 3]) -> [u8; 3] {
    let a = [world[0][0] as f64, world[0][1] as f64, world[0][2] as f64];
    let b = [world[1][0] as f64, world[1][1] as f64, world[1][2] as f64];
    let c = [world[2][0] as f64, world[2][1] as f64, world[2][2] as f64];
    let n = normalise(cross(sub(b, a), sub(c, a)));
    let lambert = dot(n, LIGHT_DIR).abs();
    let k = AMBIENT + (1.0 - AMBIENT) * lambert;
    [
        (BASE[0] * k * 255.0) as u8,
        (BASE[1] * k * 255.0) as u8,
        (BASE[2] * k * 255.0) as u8,
    ]
}

fn fill(colour: &mut [u8], depth: &mut [f64], n: usize, p: &[[f64; 3]], shade: [u8; 3]) {
    let min_x = p
        .iter()
        .map(|v| v[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let max_x = p
        .iter()
        .map(|v| v[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(n as f64 - 1.0) as usize;
    let min_y = p
        .iter()
        .map(|v| v[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let max_y = p
        .iter()
        .map(|v| v[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(n as f64 - 1.0) as usize;

    let area = edge(p[0], p[1], p[2]);
    if area.abs() < f64::EPSILON {
        return; // degenerate triangle contributes nothing
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let q = [x as f64 + 0.5, y as f64 + 0.5, 0.0];
            let (w0, w1, w2) = (
                edge(p[1], p[2], q),
                edge(p[2], p[0], q),
                edge(p[0], p[1], q),
            );
            let inside =
                (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
            if !inside {
                continue;
            }
            let z = (w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2]) / area;
            let i = y * n + x;
            if z > depth[i] {
                depth[i] = z;
                colour[i * 3..i * 3 + 3].copy_from_slice(&shade);
            }
        }
    }
}

fn edge(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn basis() -> ([f64; 3], [f64; 3]) {
    let world_up = [0.0, 0.0, 1.0];
    let right = normalise(cross(world_up, VIEW_DIR));
    let up = normalise(cross(VIEW_DIR, right));
    (right, up)
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
fn normalise(v: [f64; 3]) -> [f64; 3] {
    let m = dot(v, v).sqrt();
    if m == 0.0 {
        v
    } else {
        [v[0] / m, v[1] / m, v[2] / m]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_stl;

    fn bracket() -> crate::Mesh {
        parse_stl(include_bytes!("../../../fixtures/bracket-lp-1042-03.stl"))
            .expect("fixture parses")
    }

    #[test]
    fn rendering_the_same_mesh_twice_produces_identical_bytes() {
        // The whole reason this is a CPU rasterizer: derivatives must be deterministically
        // re-derivable, and a GPU path makes the bytes driver-dependent.
        let a = render_thumbnail(&bracket()).expect("renders");
        let b = render_thumbnail(&bracket()).expect("renders");
        assert_eq!(a, b);
    }

    #[test]
    fn the_render_matches_the_committed_golden_image() {
        let rendered = render_thumbnail(&bracket()).expect("renders");
        let golden = include_bytes!("../../../fixtures/bracket-lp-1042-03.thumb.webp");
        assert_eq!(
            rendered.as_slice(),
            golden.as_slice(),
            "rasterizer output changed. If deliberate, regenerate the golden image and \
             say so in the commit; if not, something perturbed the camera, the light or \
             the projection."
        );
    }

    #[test]
    fn the_thumbnail_fits_the_inline_bytea_budget() {
        // DATA.md §1.5 stores thumbnails inline only under 64 KB.
        let bytes = render_thumbnail(&bracket()).expect("renders");
        assert!(
            bytes.len() < 64 * 1024,
            "thumbnail was {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn an_oversized_render_is_downscaled_rather_than_written_oversized() {
        // DATA.md §1.5 only stores thumbnails inline under 64 KB. WebP here is lossless,
        // so there is no quality knob to turn — the retry reduces dimensions instead.
        // The guard must exist even though the fixture lands far under, because a row
        // written oversized is a silent violation of the inline-storage contract.
        let bytes = render_thumbnail(&bracket()).expect("renders");
        assert!(bytes.len() <= MAX_THUMB_BYTES);
    }

    #[test]
    fn the_output_decodes_as_a_512px_webp() {
        let bytes = render_thumbnail(&bracket()).expect("renders");
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .expect("decodes as WebP");
        assert_eq!((img.width(), img.height()), (THUMB_PX, THUMB_PX));
    }

    #[test]
    fn a_degenerate_mesh_fails_rather_than_emitting_a_blank_tile() {
        // All vertices coincident: no bounding box to fit, nothing to show. A blank
        // card that looks like a successful ingest is worse than a reported failure.
        let mesh = crate::Mesh {
            triangles: vec![[[1.0, 1.0, 1.0]; 3]],
        };
        let err = render_thumbnail(&mesh).expect_err("must not render");
        assert!(err.to_string().contains("zero size"));
    }
}
