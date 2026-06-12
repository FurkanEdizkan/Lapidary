//! rust-mesh — optional CPU mesh sidecar for Lapidary.
//!
//! Usage:
//!   rust-mesh <file.stl|.obj> [--lod out.stl] [--json]
//!   rust-mesh --version
//!
//! It computes the exact bounding box (mm) and triangle count, and — given --lod —
//! writes a decimated binary-STL level-of-detail mesh via vertex clustering. Output
//! (with --json) is a single line: {"bbox":[x,y,z],"triangles":N}.
//!
//! Dependency-free so it compiles offline in the container build.

use std::env;
use std::fs;
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
        eprintln!("usage: rust-mesh <file> [--lod out.stl] [--json]");
        exit(2);
    }
    let input = &args[1];
    let lod_out = flag_value(&args, "--lod");
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

    if want_json {
        println!(
            "{{\"bbox\":[{},{},{}],\"triangles\":{}}}",
            bbox[0], bbox[1], bbox[2], triangles
        );
    } else {
        println!("bbox {}x{}x{} mm, {} triangles", bbox[0], bbox[1], bbox[2], triangles);
    }
}

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
