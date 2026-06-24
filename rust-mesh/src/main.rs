//! rust-mesh — optional CPU mesh sidecar for Lapidary.
//!
//! Usage:
//!   rust-mesh <file.stl|.obj> [--lod out.stl] [--thumb out.png] [--size N] [--json]
//!   rust-mesh --version
//!
//! It computes the exact bounding box (mm) and triangle count, and — given --lod —
//! writes a decimated binary-STL level-of-detail mesh via vertex clustering. Given
//! --thumb writes a software-rasterized PNG thumbnail. Output (with --json) is a
//! single line: {"bbox":[x,y,z],"triangles":N}.

use std::env;
use std::fs;
use std::io::BufWriter;
use std::process::exit;

type V3 = [f32; 3];

struct Mesh {
    tris: Vec<[V3; 3]>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rust-mesh 0.1.0");
        return;
    }
    if args.len() < 2 {
        eprintln!("usage: rust-mesh <file> [--lod out.stl] [--thumb out.png] [--size N] [--json]");
        exit(2);
    }
    let input = &args[1];
    let lod_out = flag_value(&args, "--lod");
    let thumb_out = flag_value(&args, "--thumb");
    let size: u32 = flag_value(&args, "--size")
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let want_json = args.iter().any(|a| a == "--json");

    let mesh = match load(input) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            exit(1);
        }
    };

    let (min, max) = bounds(&mesh);
    let bbox = [
        round2(max[0] - min[0]),
        round2(max[1] - min[1]),
        round2(max[2] - min[2]),
    ];
    let triangles = mesh.tris.len();

    if let Some(out) = lod_out {
        let lod = decimate(&mesh, 48);
        if let Err(e) = write_binary_stl(&out, &lod) {
            eprintln!("warning: could not write lod: {e}");
        }
    }

    if let Some(out) = thumb_out {
        let rgba = render(&mesh, size);
        if let Err(e) = write_png(&out, size, &rgba) {
            eprintln!("warning: could not write thumb: {e}");
        }
    }

    if want_json {
        println!(
            "{{\"bbox\":[{},{},{}],\"triangles\":{}}}",
            bbox[0], bbox[1], bbox[2], triangles
        );
    } else {
        println!("bbox {}x{}x{} mm, {} triangles", bbox[0], bbox[1], bbox[2], triangles);
    }
}

// ─── PNG thumbnail ──────────────────────────────────────────────────────────

/// Render `mesh` to an RGBA8 image of `size`×`size` pixels using a simple
/// software rasterizer: isometric-ish rotation, orthographic projection, Lambert.
fn render(mesh: &Mesh, size: u32) -> Vec<u8> {
    let sz = size as usize;
    let bg = [13u8, 13, 14, 255];

    // Isometric-ish rotation: ~30° about Y, ~25° about X.
    let ry = 30.0f32.to_radians();
    let rx = 25.0f32.to_radians();

    // Rotate all vertices and accumulate rotated bounds for scale.
    let rotate = |v: V3| -> V3 {
        // Y rotation
        let vx = v[0] * ry.cos() + v[2] * ry.sin();
        let vy = v[1];
        let vz = -v[0] * ry.sin() + v[2] * ry.cos();
        // X rotation
        let ox = vx;
        let oy = vy * rx.cos() - vz * rx.sin();
        let oz = vy * rx.sin() + vz * rx.cos();
        [ox, oy, oz]
    };

    // Find the rotated bounding box (in 2D screen space x,y) for centering.
    let mut rmin = [f32::INFINITY; 2];
    let mut rmax = [f32::NEG_INFINITY; 2];
    for tri in &mesh.tris {
        for v in tri {
            let r = rotate(*v);
            rmin[0] = rmin[0].min(r[0]);
            rmin[1] = rmin[1].min(r[1]);
            rmax[0] = rmax[0].max(r[0]);
            rmax[1] = rmax[1].max(r[1]);
        }
    }
    // Compute scale to fit rotated footprint into image with a small margin.
    let margin = 0.05f32;
    let span_x = (rmax[0] - rmin[0]).max(1e-6);
    let span_y = (rmax[1] - rmin[1]).max(1e-6);
    let usable = size as f32 * (1.0 - 2.0 * margin);
    let scale = usable / span_x.max(span_y);
    let cx = (rmin[0] + rmax[0]) * 0.5;
    let cy = (rmin[1] + rmax[1]) * 0.5;
    let half = size as f32 * 0.5;

    // Light direction (view-space) normalized: [-0.4, 0.6, 0.8].
    let light_raw = [-0.4f32, 0.6, 0.8];
    let light_len = (light_raw[0] * light_raw[0]
        + light_raw[1] * light_raw[1]
        + light_raw[2] * light_raw[2])
        .sqrt();
    let light = [
        light_raw[0] / light_len,
        light_raw[1] / light_len,
        light_raw[2] / light_len,
    ];
    let ambient = 0.15f32;
    // Base color #bcc0c8 = (188,192,200).
    let base_r = 188.0f32 / 255.0;
    let base_g = 192.0f32 / 255.0;
    let base_b = 200.0f32 / 255.0;

    // RGBA pixel buffer (row-major, y=0 is top).
    let mut rgba = vec![0u8; sz * sz * 4];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = bg[0]; px[1] = bg[1]; px[2] = bg[2]; px[3] = bg[3];
    }
    // Depth buffer: largest z-value wins (nearest to camera).
    let mut zbuf = vec![f32::NEG_INFINITY; sz * sz];

    // Project a world point → (screen_x, screen_y, depth).
    let project = |v: V3| -> (f32, f32, f32) {
        let r = rotate(v);
        let sx = (r[0] - cx) * scale + half;
        // Flip Y so +Y world → up on screen.
        let sy = half - (r[1] - cy) * scale;
        let depth = r[2]; // +z toward viewer
        (sx, sy, depth)
    };

    for tri in &mesh.tris {
        // Project all three verts.
        let (x0, y0, z0) = project(tri[0]);
        let (x1, y1, z1) = project(tri[1]);
        let (x2, y2, z2) = project(tri[2]);

        // Face normal from rotated verts for correct shading.
        let rv0 = rotate(tri[0]);
        let rv1 = rotate(tri[1]);
        let rv2 = rotate(tri[2]);
        let n = normal_v3(&rv0, &rv1, &rv2);
        let diff = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
        let lum = ambient + (1.0 - ambient) * diff;

        let pr = (base_r * lum * 255.0).round().clamp(0.0, 255.0) as u8;
        let pg = (base_g * lum * 255.0).round().clamp(0.0, 255.0) as u8;
        let pb = (base_b * lum * 255.0).round().clamp(0.0, 255.0) as u8;

        // Tight axis-aligned bounding box of this triangle.
        let min_x = x0.min(x1).min(x2).floor() as i32;
        let max_x = x0.max(x1).max(x2).ceil() as i32;
        let min_y = y0.min(y1).min(y2).floor() as i32;
        let max_y = y0.max(y1).max(y2).ceil() as i32;

        // Scanline rasterize.
        for py in min_y..=max_y {
            if py < 0 || py >= sz as i32 { continue; }
            for px_i in min_x..=max_x {
                if px_i < 0 || px_i >= sz as i32 { continue; }

                let pcx = px_i as f32 + 0.5;
                let pcy = py as f32 + 0.5;

                // Barycentric test.
                let (u, v, w) = barycentric(
                    x0, y0, x1, y1, x2, y2,
                    pcx, pcy,
                );
                if u < -1e-5 || v < -1e-5 || w < -1e-5 { continue; }

                // Interpolate depth.
                let depth = u * z0 + v * z1 + w * z2;

                let idx = py as usize * sz + px_i as usize;
                if depth > zbuf[idx] {
                    zbuf[idx] = depth;
                    let base = idx * 4;
                    rgba[base]     = pr;
                    rgba[base + 1] = pg;
                    rgba[base + 2] = pb;
                    rgba[base + 3] = 255;
                }
            }
        }
    }

    rgba
}

/// Barycentric coordinates of (px,py) in triangle (x0,y0)(x1,y1)(x2,y2).
fn barycentric(
    x0: f32, y0: f32,
    x1: f32, y1: f32,
    x2: f32, y2: f32,
    px: f32, py: f32,
) -> (f32, f32, f32) {
    let denom = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
    if denom.abs() < 1e-10 {
        return (-1.0, -1.0, -1.0);
    }
    let u = ((y1 - y2) * (px - x2) + (x2 - x1) * (py - y2)) / denom;
    let v = ((y2 - y0) * (px - x2) + (x0 - x2) * (py - y2)) / denom;
    let w = 1.0 - u - v;
    (u, v, w)
}

/// Surface normal from three 3-D points (as references to arrays).
fn normal_v3(a: &V3, b: &V3, c: &V3) -> V3 {
    let u = sub(*b, *a);
    let v = sub(*c, *a);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 { [n[0] / len, n[1] / len, n[2] / len] } else { [0.0, 0.0, 1.0] }
}

/// Encode RGBA8 pixel data to a PNG file using the `png` crate.
fn write_png(path: &str, size: u32, rgba: &[u8]) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|e| e.to_string())?;
    let ref mut bw = BufWriter::new(file);
    let mut encoder = png::Encoder::new(bw, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── existing helpers ────────────────────────────────────────────────────────

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn load(path: &str) -> Result<Mesh, String> {
    let lower = path.to_lowercase();
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if lower.ends_with(".obj") {
        parse_obj(&String::from_utf8_lossy(&bytes))
    } else if is_ascii_stl(&bytes) {
        parse_ascii_stl(&String::from_utf8_lossy(&bytes))
    } else {
        parse_binary_stl(&bytes)
    }
}

fn is_ascii_stl(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let s = String::from_utf8_lossy(head);
    s.trim_start().starts_with("solid") && s.contains("facet")
}

fn parse_ascii_stl(text: &str) -> Result<Mesh, String> {
    let mut tris = Vec::new();
    let mut verts: Vec<V3> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("vertex") {
            let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            if nums.len() == 3 {
                verts.push([nums[0], nums[1], nums[2]]);
            }
            if verts.len() == 3 {
                tris.push([verts[0], verts[1], verts[2]]);
                verts.clear();
            }
        }
    }
    Ok(Mesh { tris })
}

fn parse_binary_stl(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.len() < 84 {
        return Err("file too short for binary STL".into());
    }
    let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let mut tris = Vec::with_capacity(count);
    let mut off = 84;
    for _ in 0..count {
        if off + 50 > bytes.len() {
            break;
        }
        let mut tri = [[0f32; 3]; 3];
        // skip normal (12 bytes), read 3 vertices
        for v in 0..3 {
            let base = off + 12 + v * 12;
            tri[v] = [
                f32::from_le_bytes([bytes[base], bytes[base + 1], bytes[base + 2], bytes[base + 3]]),
                f32::from_le_bytes([bytes[base + 4], bytes[base + 5], bytes[base + 6], bytes[base + 7]]),
                f32::from_le_bytes([bytes[base + 8], bytes[base + 9], bytes[base + 10], bytes[base + 11]]),
            ];
        }
        tris.push(tri);
        off += 50;
    }
    Ok(Mesh { tris })
}

fn parse_obj(text: &str) -> Result<Mesh, String> {
    let mut verts: Vec<V3> = Vec::new();
    let mut tris: Vec<[V3; 3]> = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let nums: Vec<f32> = it.filter_map(|n| n.parse().ok()).collect();
                if nums.len() >= 3 {
                    verts.push([nums[0], nums[1], nums[2]]);
                }
            }
            Some("f") => {
                let idx: Vec<usize> = it
                    .filter_map(|tok| tok.split('/').next().and_then(|n| n.parse::<i64>().ok()))
                    .map(|i| if i < 0 { (verts.len() as i64 + i) as usize } else { (i - 1) as usize })
                    .collect();
                // fan-triangulate polygons
                for k in 1..idx.len().saturating_sub(1) {
                    if let (Some(&a), Some(&b), Some(&c)) = (idx.first(), idx.get(k), idx.get(k + 1)) {
                        if a < verts.len() && b < verts.len() && c < verts.len() {
                            tris.push([verts[a], verts[b], verts[c]]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(Mesh { tris })
}

fn bounds(m: &Mesh) -> (V3, V3) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for t in &m.tris {
        for v in t {
            for k in 0..3 {
                if v[k] < min[k] {
                    min[k] = v[k];
                }
                if v[k] > max[k] {
                    max[k] = v[k];
                }
            }
        }
    }
    if !min[0].is_finite() {
        (min, max) = ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

/// Vertex-clustering decimation: snap vertices to a `grid`^3 lattice over the bbox,
/// then drop triangles whose snapped corners collapse together.
fn decimate(m: &Mesh, grid: usize) -> Mesh {
    let (min, max) = bounds(m);
    let span = [
        (max[0] - min[0]).max(1e-6),
        (max[1] - min[1]).max(1e-6),
        (max[2] - min[2]).max(1e-6),
    ];
    let snap = |p: V3| -> V3 {
        let g = grid as f32;
        [
            min[0] + ((p[0] - min[0]) / span[0] * g).round() / g * span[0],
            min[1] + ((p[1] - min[1]) / span[1] * g).round() / g * span[1],
            min[2] + ((p[2] - min[2]) / span[2] * g).round() / g * span[2],
        ]
    };
    let key = |p: V3| -> (i64, i64, i64) {
        let g = grid as f32;
        (
            ((p[0] - min[0]) / span[0] * g).round() as i64,
            ((p[1] - min[1]) / span[1] * g).round() as i64,
            ((p[2] - min[2]) / span[2] * g).round() as i64,
        )
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in &m.tris {
        let (ka, kb, kc) = (key(t[0]), key(t[1]), key(t[2]));
        if ka == kb || kb == kc || ka == kc {
            continue; // collapsed — skip
        }
        // dedupe identical snapped triangles
        let mut tri_key = [ka, kb, kc];
        tri_key.sort();
        if !seen.insert(tri_key) {
            continue;
        }
        out.push([snap(t[0]), snap(t[1]), snap(t[2])]);
    }
    // If decimation removed everything (tiny mesh), keep the original.
    if out.is_empty() {
        return Mesh { tris: m.tris.clone() };
    }
    Mesh { tris: out }
}

fn write_binary_stl(path: &str, m: &Mesh) -> Result<(), String> {
    let mut buf: Vec<u8> = Vec::with_capacity(84 + m.tris.len() * 50);
    buf.extend_from_slice(&[0u8; 80]); // header
    buf.extend_from_slice(&(m.tris.len() as u32).to_le_bytes());
    for t in &m.tris {
        let n = normal(t);
        for comp in n {
            buf.extend_from_slice(&comp.to_le_bytes());
        }
        for v in t {
            for comp in v {
                buf.extend_from_slice(&comp.to_le_bytes());
            }
        }
        buf.extend_from_slice(&[0u8; 2]); // attribute byte count
    }
    fs::write(path, buf).map_err(|e| e.to_string())
}

fn normal(t: &[V3; 3]) -> V3 {
    let u = sub(t[1], t[0]);
    let v = sub(t[2], t[0]);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn round2(n: f32) -> f32 {
    (n * 100.0).round() / 100.0
}
