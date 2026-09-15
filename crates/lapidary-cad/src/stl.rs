//! STL parsing. Hand-written rather than pulled from a crate: the format is 84 bytes of
//! header plus 50 per triangle, the project prefers fewer dependencies, and the error
//! text is a product surface — an operator who dropped a bad file needs to be told what
//! to do about it.

use crate::CadError;

/// Triangles only. STL carries per-facet normals, but they are frequently wrong in
/// real files, so we ignore them and recompute from winding.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<[[f32; 3]; 3]>,
    /// How many triangles each part has, in order: the first `parts[0]` triangles are the first
    /// part's, and so on. Empty for a mesh file, which is one part. The CAD bridge fills it from an
    /// assembly's tree, so the viewer can hide one part.
    pub parts: Vec<u32>,
}

const HEADER: usize = 80;
const COUNT: usize = 4;
const TRIANGLE: usize = 50;

/// `mesh` as binary STL, the form every slicer reads: an 80-byte header naming Lapidary, the triangle count,
/// then each triangle with a facet normal worked out from its corners and a zero attribute word.
pub fn write_stl(mesh: &Mesh) -> Result<Vec<u8>, CadError> {
    let count = u32::try_from(mesh.triangles.len()).map_err(|_| CadError::ExportFailed {
        format: "STL".to_owned(),
        detail: format!(
            "{} triangles is more than an STL can count",
            mesh.triangles.len()
        ),
    })?;
    let mut out = Vec::with_capacity(HEADER + COUNT + TRIANGLE * mesh.triangles.len());
    let mut header = [0u8; HEADER];
    // Not `solid`: a binary STL whose header starts with it reads as ASCII to some tools.
    let name = b"Lapidary export, binary STL, millimetres";
    header[..name.len()].copy_from_slice(name);
    out.extend_from_slice(&header);
    out.extend_from_slice(&count.to_le_bytes());
    for [a, b, c] in &mesh.triangles {
        for v in [facet_normal(a, b, c), *a, *b, *c] {
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(out)
}

/// A triangle's unit normal by the right-hand rule, or zero for a triangle with no area.
fn facet_normal(a: &[f32; 3], b: &[f32; 3], c: &[f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if length > 0.0 {
        [n[0] / length, n[1] / length, n[2] / length]
    } else {
        [0.0; 3]
    }
}

pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, CadError> {
    if bytes.is_empty() {
        return Err(CadError::MalformedMesh {
            format: "STL".to_owned(),
            detail: "the file is 0 bytes".to_owned(),
        });
    }

    // Detection is size arithmetic, never a magic-string sniff: plenty of binary STLs
    // begin with the ASCII word "solid", and a parser that trusts the prefix reads the
    // binary body as text and produces nonsense.
    if bytes.len() >= HEADER + COUNT {
        let mut count = [0u8; 4];
        count.copy_from_slice(&bytes[HEADER..HEADER + COUNT]);
        let claimed = u32::from_le_bytes(count) as usize;
        if bytes.len() == HEADER + COUNT + claimed * TRIANGLE {
            return parse_binary(bytes, claimed);
        }
        // The length says binary but does not add up. If it also does not look like
        // text, report the binary shape — that is the more useful diagnosis.
        if !looks_like_ascii(bytes) {
            return Err(CadError::MalformedMesh {
                format: "STL".to_owned(),
                detail: format!(
                    "the header claims {claimed} triangles, which needs {} bytes, but the file is {} — it looks truncated or incomplete",
                    HEADER + COUNT + claimed * TRIANGLE,
                    bytes.len()
                ),
            });
        }
    }

    parse_ascii(bytes)
}

fn looks_like_ascii(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(512)];
    window.starts_with(b"solid") && window.iter().all(|b| b.is_ascii())
}

/// x/y/z for a coordinate index, used only in error text.
fn axis_name(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        _ => "z",
    }
}

fn parse_binary(bytes: &[u8], count: usize) -> Result<Mesh, CadError> {
    let mut triangles = Vec::with_capacity(count);
    let mut at = HEADER + COUNT;
    for i in 0..count {
        at += 12; // per-facet normal, ignored
        let mut tri = [[0.0f32; 3]; 3];
        for (vi, vertex) in tri.iter_mut().enumerate() {
            for (ci, component) in vertex.iter_mut().enumerate() {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&bytes[at..at + 4]);
                let value = f32::from_le_bytes(raw);
                // A NaN or infinite coordinate has no meaningful geometry, and letting
                // it through is worse than rejecting the file: f32::min/max silently
                // drop NaN operands, so it vanishes from the bounding box while area
                // and volume downstream turn into NaN with no visible cause.
                if !value.is_finite() {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "triangle {} has a non-finite {} coordinate on vertex {} — the file is likely corrupt",
                            i + 1,
                            axis_name(ci),
                            vi + 1
                        ),
                    });
                }
                *component = value;
                at += 4;
            }
        }
        at += 2; // attribute byte count
        triangles.push(tri);
    }
    finish("STL", triangles)
}

fn parse_ascii(bytes: &[u8]) -> Result<Mesh, CadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CadError::MalformedMesh {
        format: "STL".to_owned(),
        detail: "the file is neither a valid binary STL nor valid UTF-8 text".to_owned(),
    })?;

    let mut triangles = Vec::new();
    // Facet/loop state, tracked explicitly rather than just grouping every three
    // `vertex` lines in file order: a file whose facet boundaries are corrupted but
    // whose total vertex count still lands on a multiple of three would otherwise
    // parse into a plausible-looking mesh whose triangles mix vertices from unrelated
    // facets — wrong topology, silently.
    let mut facet_open = false;
    let mut loop_open = false;
    let mut current: Vec<[f32; 3]> = Vec::with_capacity(3);

    for (number, line) in text.lines().enumerate() {
        let line_no = number + 1;
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };

        match keyword {
            "facet" => {
                if facet_open {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} opens a facet before the previous one was closed with `endfacet` — the file's facet structure is corrupt"
                        ),
                    });
                }
                facet_open = true;
            }
            "endfacet" => {
                if loop_open {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} closes a facet whose loop was never closed with `endloop` — the file's facet structure is corrupt"
                        ),
                    });
                }
                facet_open = false;
            }
            "outer" => {
                if loop_open {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} opens a loop before the previous one was closed with `endloop` — the file's facet structure is corrupt"
                        ),
                    });
                }
                loop_open = true;
                current.clear();
            }
            "endloop" => {
                if !loop_open {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} closes a loop that was never opened with `outer loop` — the file's facet structure is corrupt"
                        ),
                    });
                }
                if current.len() != 3 {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} closes a facet with {} vertices, expected 3 — the file's facet structure is corrupt",
                            current.len()
                        ),
                    });
                }
                triangles.push([current[0], current[1], current[2]]);
                current.clear();
                loop_open = false;
            }
            "vertex" => {
                if !loop_open {
                    return Err(CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!(
                            "line {line_no} has a `vertex` outside an `outer loop` — the file's facet structure is corrupt"
                        ),
                    });
                }
                let mut vertex = [0.0f32; 3];
                for (i, slot) in vertex.iter_mut().enumerate() {
                    let token = parts.next().ok_or_else(|| CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!("line {line_no} has {i} coordinates, expected 3"),
                    })?;
                    *slot = token.parse().map_err(|_| CadError::MalformedMesh {
                        format: "STL".to_owned(),
                        detail: format!("line {line_no} has `{token}` where a number was expected"),
                    })?;
                    // Same reasoning as the binary path: a NaN or infinite vertex is
                    // not a coordinate anything downstream can use, and Rust's f32
                    // parser accepts "nan" / "inf" / "infinity" without complaint.
                    if !slot.is_finite() {
                        return Err(CadError::MalformedMesh {
                            format: "STL".to_owned(),
                            detail: format!(
                                "line {line_no} has `{token}`, which is not a finite number — the file is likely corrupt"
                            ),
                        });
                    }
                }
                current.push(vertex);
            }
            _ => {} // solid / endsolid / facet-normal-vector text — no structural meaning here
        }
    }

    if facet_open || loop_open {
        return Err(CadError::MalformedMesh {
            format: "STL".to_owned(),
            detail: "the file ends with a facet still open — the file's facet structure is corrupt"
                .to_owned(),
        });
    }

    finish("STL", triangles)
}

pub(crate) fn finish(format: &str, triangles: Vec<[[f32; 3]; 3]>) -> Result<Mesh, CadError> {
    if triangles.is_empty() {
        return Err(CadError::MalformedMesh {
            format: format.to_owned(),
            detail: "the file parsed but contains no triangles".to_owned(),
        });
    }
    Ok(Mesh {
        triangles,
        parts: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A binary STL whose header starts with "solid" — the exact file that breaks a
    /// parser which sniffs the magic string instead of checking the length.
    fn binary_stl_that_looks_ascii(triangles: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let mut header = [0u8; 80];
        header[..5].copy_from_slice(b"solid");
        v.extend_from_slice(&header);
        v.extend_from_slice(&triangles.to_le_bytes());
        for _ in 0..triangles {
            v.extend_from_slice(&[0u8; 12]); // normal, ignored — we recompute
            for xyz in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                for c in xyz {
                    v.extend_from_slice(&c.to_le_bytes());
                }
            }
            v.extend_from_slice(&[0u8; 2]); // attribute byte count
        }
        v
    }

    #[test]
    fn a_binary_stl_beginning_with_solid_is_not_mistaken_for_ascii() {
        let mesh = parse_stl(&binary_stl_that_looks_ascii(2)).expect("parses as binary");
        assert_eq!(mesh.triangles.len(), 2);
    }

    #[test]
    fn an_ascii_stl_parses() {
        let src = b"solid spacer
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex 1 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid spacer
";
        let mesh = parse_stl(src).expect("parses as ascii");
        assert_eq!(mesh.triangles.len(), 1);
        assert_eq!(mesh.triangles[0][1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_truncated_binary_stl_says_what_broke_and_what_to_do() {
        let mut bytes = binary_stl_that_looks_ascii(10);
        bytes.truncate(200); // claims 10 triangles, carries far fewer
        let err = parse_stl(&bytes).expect_err("must not parse");
        let msg = err.to_string();
        assert!(msg.contains("10"), "message names the claimed count: {msg}");
        assert!(
            msg.contains("truncated") || msg.contains("incomplete"),
            "message must suggest a cause: {msg}"
        );
    }

    #[test]
    fn an_empty_file_is_rejected_rather_than_read_as_zero_triangles() {
        let err = parse_stl(&[]).expect_err("must not parse");
        assert!(err.to_string().contains("0 bytes"));
    }

    #[test]
    fn a_mesh_with_no_triangles_is_rejected() {
        // 84 bytes is a structurally valid binary STL claiming zero triangles. It is
        // still not a part, and ingesting it would create a card for nothing.
        let err = parse_stl(&binary_stl_that_looks_ascii(0)).expect_err("must not parse");
        assert!(err.to_string().contains("no triangles"));
    }

    /// A one-triangle binary STL with a caller-supplied second vertex, everything else a
    /// normal, well-formed triangle.
    fn binary_stl_with_vertex(second_vertex: [f32; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 80]);
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&[0u8; 12]); // normal, ignored
        for xyz in [[0.0f32, 0.0, 0.0], second_vertex, [0.0, 1.0, 0.0]] {
            for c in xyz {
                v.extend_from_slice(&c.to_le_bytes());
            }
        }
        v.extend_from_slice(&[0u8; 2]); // attribute byte count
        v
    }

    #[test]
    fn a_binary_stl_with_a_non_finite_coordinate_is_rejected() {
        // f32::min/max silently drop a NaN operand, so a NaN vertex does not fail
        // loudly downstream — it just vanishes from the bounding box while area and
        // volume quietly turn into NaN. Must be caught here, at the boundary.
        let err = parse_stl(&binary_stl_with_vertex([f32::NAN, 0.0, 0.0]))
            .expect_err("a NaN coordinate must not parse");
        let msg = err.to_string();
        assert!(
            msg.contains("non-finite"),
            "message names the defect: {msg}"
        );
        assert!(
            msg.contains("triangle 1"),
            "message names which triangle: {msg}"
        );
    }

    #[test]
    fn an_ascii_stl_with_a_non_finite_coordinate_is_rejected() {
        // Rust's f32 FromStr accepts "inf" without error — nothing about the token
        // itself looks malformed, so this can only be caught by checking the value.
        let src = b"solid spacer
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex inf 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid spacer
";
        let err = parse_stl(src).expect_err("an infinite coordinate must not parse");
        let msg = err.to_string();
        assert!(
            msg.contains("not a finite number"),
            "message names the defect: {msg}"
        );
        assert!(msg.contains("line 5"), "message names the line: {msg}");
    }

    #[test]
    fn an_ascii_vertex_outside_a_loop_is_rejected() {
        // A `vertex` line the parser reaches before any `outer loop` has no facet to
        // belong to — grouping it anyway is how corrupted facet boundaries turn into a
        // mesh whose triangles mix vertices from unrelated facets.
        let src = b"solid corrupt
facet normal 0 0 1
    vertex 0 0 0
  outer loop
    vertex 1 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid corrupt
";
        let err = parse_stl(src).expect_err("a vertex outside a loop must not parse");
        let msg = err.to_string();
        assert!(
            msg.contains("vertex") && msg.contains("outer loop"),
            "message names the structural defect: {msg}"
        );
        assert!(msg.contains("line 3"), "message names the line: {msg}");
    }

    #[test]
    fn a_facet_with_two_vertices_followed_by_another_facet_is_rejected() {
        // The first facet's loop never closes — a second `facet` starts while it is
        // still open. Counting every three `vertex` lines in file order (ignoring
        // facet boundaries entirely) would silently stitch this facet's two vertices
        // to the next facet's first vertex and produce a plausible-looking triangle
        // that belongs to no real facet.
        let src = b"solid corrupt
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex 1 0 0
facet normal 0 1 0
  outer loop
    vertex 0 0 0
    vertex 0 1 0
    vertex 1 1 0
  endloop
endfacet
endsolid corrupt
";
        let err = parse_stl(src).expect_err("an unclosed facet must not parse");
        let msg = err.to_string();
        assert!(
            msg.contains("facet") && msg.contains("endfacet"),
            "message names the structural defect: {msg}"
        );
        assert!(msg.contains("line 6"), "message names the line: {msg}");
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    /// A written STL reads back as the triangles it was written from, each with a unit normal.
    #[test]
    fn a_written_stl_reads_back_as_the_same_triangles() {
        let mesh = Mesh {
            triangles: vec![
                [[0.0, 0.0, 0.0], [22.5, 0.0, 0.0], [0.0, 30.25, 0.0]],
                [[0.0, 0.0, 5.0], [1.0, 1.0, 5.0], [2.0, 2.0, 5.0]],
            ],
            parts: Vec::new(),
        };
        let bytes = write_stl(&mesh).expect("writes");
        assert_eq!(bytes.len(), 84 + 50 * 2);
        assert!(!bytes.starts_with(b"solid"));
        assert_eq!(
            parse_stl(&bytes).expect("reads back").triangles,
            mesh.triangles
        );
        let normal: Vec<f32> = bytes[84..96]
            .chunks(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(normal, [0.0, 0.0, 1.0], "the first triangle faces up");
        let flat: Vec<f32> = bytes[134..146]
            .chunks(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(
            flat,
            [0.0, 0.0, 0.0],
            "a triangle with no area has no normal"
        );
    }
}
