//! Wavefront OBJ parsing. Hand-written for the same reasons `stl.rs` is: the subset that
//! matters here is two keywords, the project prefers fewer dependencies, and the error
//! text is a product surface.
//!
//! Only `v` and `f` carry geometry. `vt`, `vn`, `o`, `g`, `s`, `usemtl` and `mtllib` are
//! read and discarded rather than rejected — a file that names a material library this
//! project will never open is still a perfectly good mesh, and refusing it would reject
//! most of what real exporters emit.

use crate::CadError;
use crate::stl::{Mesh, finish};

const FORMAT: &str = "OBJ";

pub fn parse_obj(bytes: &[u8]) -> Result<Mesh, CadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CadError::MalformedMesh {
        format: FORMAT.to_owned(),
        detail: "the file is not valid UTF-8 text".to_owned(),
    })?;

    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut triangles: Vec<[[f32; 3]; 3]> = Vec::new();

    for (number, line) in text.lines().enumerate() {
        let line_no = number + 1;
        // A `#` runs to end of line wherever it appears, so it is cut before tokenising
        // rather than matched as a keyword — `v 1 2 3 # origin` is a valid vertex.
        let line = line.split('#').next().unwrap_or("");
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };

        match keyword {
            "v" => vertices.push(parse_vertex(&mut parts, line_no)?),
            "f" => triangulate(&parse_face(&mut parts, &vertices, line_no)?, &mut triangles),
            // Everything else is either metadata this project does not use or geometry it
            // does not support (`l` polylines, `p` points, free-form surfaces). Both are
            // silently skipped; `finish` is what refuses a file that yielded no triangles.
            _ => {}
        }
    }

    finish(FORMAT, triangles)
}

fn parse_vertex(
    parts: &mut std::str::SplitWhitespace<'_>,
    line_no: usize,
) -> Result<[f32; 3], CadError> {
    let mut position = [0f32; 3];
    for (axis, slot) in position.iter_mut().enumerate() {
        let token = parts.next().ok_or_else(|| CadError::MalformedMesh {
            format: FORMAT.to_owned(),
            detail: format!("line {line_no}: a vertex needs three coordinates"),
        })?;
        *slot = token.parse().map_err(|_| CadError::MalformedMesh {
            format: FORMAT.to_owned(),
            detail: format!("line {line_no}: {token:?} is not a number"),
        })?;
        // `"nan".parse::<f32>()` succeeds, so this is a separate gate rather than a
        // consequence of the parse. A NaN coordinate propagates into the bounding box and
        // from there into every LOD grid, so it is refused at the door.
        if !slot.is_finite() {
            return Err(CadError::MalformedMesh {
                format: FORMAT.to_owned(),
                detail: format!(
                    "line {line_no}: the {} coordinate is not a finite number",
                    axis_name(axis)
                ),
            });
        }
    }
    // A fourth `w` component is legal and is ignored: it is a rational weight for
    // free-form geometry, and this parser reads polygons.
    Ok(position)
}

fn axis_name(index: usize) -> &'static str {
    match index {
        0 => "x",
        1 => "y",
        _ => "z",
    }
}

/// Resolves one face's vertex references to positions, in file order.
fn parse_face(
    parts: &mut std::str::SplitWhitespace<'_>,
    vertices: &[[f32; 3]],
    line_no: usize,
) -> Result<Vec<[f32; 3]>, CadError> {
    let mut corners = Vec::new();
    for token in parts {
        // `v`, `v/vt`, `v//vn` and `v/vt/vn` all begin with the position index, and only
        // the position is kept — normals are recomputed from winding, as `stl.rs` says,
        // and nothing here textures anything.
        let first = token.split('/').next().unwrap_or("");
        let index: i64 = first.parse().map_err(|_| CadError::MalformedMesh {
            format: FORMAT.to_owned(),
            detail: format!("line {line_no}: {token:?} is not a vertex reference"),
        })?;
        corners.push(resolve(index, vertices, line_no)?);
    }

    if corners.len() < 3 {
        return Err(CadError::MalformedMesh {
            format: FORMAT.to_owned(),
            detail: format!(
                "line {line_no}: a face needs at least three vertices, but this one has {}",
                corners.len()
            ),
        });
    }
    Ok(corners)
}

/// OBJ indices are 1-based, and a negative index counts back from the end of the vertex
/// list seen *so far* — `-1` is the most recent vertex. Real exporters emit them, and a
/// parser that treats them as positive produces a mesh that is wrong rather than one that
/// fails, which is why this is its own function with its own test.
fn resolve(index: i64, vertices: &[[f32; 3]], line_no: usize) -> Result<[f32; 3], CadError> {
    let count = vertices.len() as i64;
    let resolved = match index {
        0 => {
            return Err(CadError::MalformedMesh {
                format: FORMAT.to_owned(),
                detail: format!("line {line_no}: 0 is not a vertex index — OBJ counts from 1"),
            });
        }
        n if n < 0 => count + n,
        n => n - 1,
    };
    if resolved < 0 || resolved >= count {
        return Err(CadError::MalformedMesh {
            format: FORMAT.to_owned(),
            detail: format!(
                "line {line_no}: vertex {index} does not exist — the file has defined {count} so far"
            ),
        });
    }
    Ok(vertices[resolved as usize])
}

/// Fan triangulation from the first corner. Correct for the convex faces exporters emit;
/// a concave quad would fan into a triangle outside the polygon, which is a tessellation
/// artefact rather than a parse failure and is not worth an ear-clipping pass here.
fn triangulate(corners: &[[f32; 3]], triangles: &mut Vec<[[f32; 3]; 3]>) {
    for corner in 1..corners.len() - 1 {
        triangles.push([corners[0], corners[corner], corners[corner + 1]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture text is written flush left because that is how OBJ files are written; an
    /// indented heredoc would put the indentation inside the string.
    fn parse(text: &str) -> Result<Mesh, CadError> {
        parse_obj(text.as_bytes())
    }

    const A_QUAD: &str = "v 0 0 0
v 2 0 0
v 2 3 0
v 0 3 0
f 1 2 3 4
";

    #[test]
    fn a_triangle_face_parses() {
        let mesh = parse("v 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 3\n").expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]]]
        );
    }

    #[test]
    fn a_quad_face_is_triangulated_into_two_triangles() {
        let mesh = parse(A_QUAD).expect("parses");
        assert_eq!(mesh.triangles.len(), 2, "a quad fans into two triangles");
        // Fanning from the first corner: (1,2,3) then (1,3,4). Both keep the quad's
        // winding, so the recomputed normals agree with each other.
        assert_eq!(
            mesh.triangles,
            vec![
                [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 3.0, 0.0]],
                [[0.0, 0.0, 0.0], [2.0, 3.0, 0.0], [0.0, 3.0, 0.0]],
            ]
        );
    }

    #[test]
    fn negative_indices_count_back_from_the_end() {
        // Four vertices, so 1..=3 are all in range: reading -1/-2/-3 as positive would
        // succeed and silently build a triangle from the wrong three corners. That is the
        // failure this test exists for, which is why it asserts coordinates rather than
        // that the parse returned Ok.
        let mesh = parse("v 0 0 0\nv 1 1 1\nv 2 2 2\nv 9 9 9\nf -1 -2 -3\n").expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[9.0, 9.0, 9.0], [2.0, 2.0, 2.0], [1.0, 1.0, 1.0]]]
        );
    }

    #[test]
    fn texture_and_normal_components_are_ignored() {
        let with_components = "v 0 0 0
v 2 0 0
v 0 3 0
vt 0 0
vn 0 0 1
f 1/1/1 2//1 3/1
";
        assert_eq!(
            parse(with_components).expect("parses").triangles,
            parse("v 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 3\n")
                .expect("parses")
                .triangles,
            "only the position component of a reference is read"
        );
    }

    #[test]
    fn comments_blank_lines_and_crlf_are_tolerated() {
        let messy = "# an exporter banner\r\nmtllib bracket.mtl\r\n\r\nv 0 0 0 # the origin\r\nv 2 0 0\r\nv 0 3 0\r\no part\r\ns off\r\nusemtl steel\r\nf 1 2 3\r\n";
        assert_eq!(parse(messy).expect("parses").triangles.len(), 1);
    }

    #[test]
    fn a_file_with_no_faces_fails_with_an_actionable_message() {
        let err = parse("v 0 0 0\nv 2 0 0\nv 0 3 0\n").expect_err("must fail");
        // The same gate `parse_stl` uses, so both parsers answer a geometry-free file the
        // same way rather than one erroring and the other returning an empty mesh.
        let CadError::MalformedMesh { format, detail } = err else {
            panic!("expected a malformed-mesh error");
        };
        assert_eq!(format, "OBJ");
        assert!(detail.contains("no triangles"), "{detail}");
    }

    #[test]
    fn a_non_finite_coordinate_is_rejected() {
        // `"nan".parse::<f32>()` succeeds, so nothing but an explicit check catches this.
        let err = parse("v 0 0 0\nv nan 0 0\nv 0 3 0\nf 1 2 3\n").expect_err("must fail");
        assert!(err.to_string().contains("finite"), "{err}");
    }

    #[test]
    fn a_zero_index_is_rejected_rather_than_wrapping() {
        // 0 is not a legal OBJ index, and `0 - 1` on the unsigned path would wrap to the
        // end of the vertex list and quietly pick the wrong corner.
        let err = parse("v 0 0 0\nv 2 0 0\nv 0 3 0\nf 0 1 2\n").expect_err("must fail");
        assert!(err.to_string().contains("counts from 1"), "{err}");
    }

    #[test]
    fn a_face_with_two_vertices_is_an_error_not_a_skipped_line() {
        let err = parse("v 0 0 0\nv 2 0 0\nf 1 2\n").expect_err("must fail");
        assert!(err.to_string().contains("at least three"), "{err}");
    }

    #[test]
    fn an_out_of_range_index_names_how_many_vertices_exist() {
        let err = parse("v 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 7\n").expect_err("must fail");
        assert!(err.to_string().contains("defined 3 so far"), "{err}");
    }

    #[test]
    fn the_real_fixture_parses_with_its_real_triangle_count() {
        let bytes = include_bytes!("../../../fixtures/idler-bracket-lp-2210-01.obj");
        let mesh = parse_obj(bytes).expect("the fixture parses");
        // 14 faces: 6 quad side walls fanning into 12, plus 8 triangulated cap faces.
        // Counted after triangulation, which is the number that would be wrong if the
        // fan were dropped.
        assert_eq!(mesh.triangles.len(), 20);
    }
}
