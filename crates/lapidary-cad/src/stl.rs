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
}

const HEADER: usize = 80;
const COUNT: usize = 4;
const TRIANGLE: usize = 50;

pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, CadError> {
    if bytes.is_empty() {
        return Err(CadError::MalformedStl {
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
            return Err(CadError::MalformedStl {
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

fn parse_binary(bytes: &[u8], count: usize) -> Result<Mesh, CadError> {
    let mut triangles = Vec::with_capacity(count);
    let mut at = HEADER + COUNT;
    for _ in 0..count {
        at += 12; // per-facet normal, ignored
        let mut tri = [[0.0f32; 3]; 3];
        for vertex in &mut tri {
            for component in vertex.iter_mut() {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&bytes[at..at + 4]);
                *component = f32::from_le_bytes(raw);
                at += 4;
            }
        }
        at += 2; // attribute byte count
        triangles.push(tri);
    }
    finish(triangles)
}

fn parse_ascii(bytes: &[u8]) -> Result<Mesh, CadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CadError::MalformedStl {
        detail: "the file is neither a valid binary STL nor valid UTF-8 text".to_owned(),
    })?;

    let mut triangles = Vec::new();
    let mut current: Vec<[f32; 3]> = Vec::with_capacity(3);
    for (number, line) in text.lines().enumerate() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("vertex") {
            continue;
        }
        let mut vertex = [0.0f32; 3];
        for (i, slot) in vertex.iter_mut().enumerate() {
            let token = parts.next().ok_or_else(|| CadError::MalformedStl {
                detail: format!("line {} has {i} coordinates, expected 3", number + 1),
            })?;
            *slot = token.parse().map_err(|_| CadError::MalformedStl {
                detail: format!(
                    "line {} has `{token}` where a number was expected",
                    number + 1
                ),
            })?;
        }
        current.push(vertex);
        if current.len() == 3 {
            triangles.push([current[0], current[1], current[2]]);
            current.clear();
        }
    }
    if !current.is_empty() {
        return Err(CadError::MalformedStl {
            detail: format!(
                "the last facet has {} vertices, expected 3 — the file ends mid-triangle",
                current.len()
            ),
        });
    }
    finish(triangles)
}

fn finish(triangles: Vec<[[f32; 3]; 3]>) -> Result<Mesh, CadError> {
    if triangles.is_empty() {
        return Err(CadError::MalformedStl {
            detail: "the file parsed but contains no triangles".to_owned(),
        });
    }
    Ok(Mesh { triangles })
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
}
