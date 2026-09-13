//! glTF 2.0 binary output, compressed with `EXT_meshopt_compression`.
//!
//! `DATA.md` §2.2 chose meshopt: it decodes an order of magnitude faster than Draco, and decode
//! is what a person waits on. Positions are encoded in `ATTRIBUTES` mode and indices in
//! `TRIANGLES` mode. Nothing about what is drawn changes.
//!
//! **Lossless, on purpose.** `KHR_mesh_quantization` would shrink positions further by rounding
//! them to a 16-bit grid over the bounding box — on the 315 mm fixture assembly, a step of
//! 0.005 mm. Measurement snaps to an analytic entity only when a triangle's corners lie within
//! 0.001 mm of its surface (`web/src/lib/measure.ts`), so a quantized L2 would stop measuring
//! exactly. The vertex codec round-trips float32 bit for bit, which the tests below check.
//!
//! Before encoding, triangles are put in vertex-cache order and vertices in the order they are
//! first used, which is what the two codecs compress best. The mesh is the same set of
//! triangles, each wound the same way.
//!
//! The `meshopt` crate's build script compiles meshoptimizer's C++ with `cc` in the build
//! stage, which already runs `cc` for `blake3`'s C. The runtime image gains nothing.
//!
//! What is written: one buffer holding the compressed bytes, a fallback buffer holding none,
//! two bufferViews, two accessors, one mesh with one primitive, one node, one scene. A mesh read
//! from an assembly also carries `extras.parts`: how many triangles each placed part has, in the
//! tree's depth-first order, each part's triangles one contiguous run of the index buffer. That is
//! what the viewer hides and isolates parts by. The
//! extension is required, so a loader without it refuses the file rather than reading the empty
//! fallback. No materials and no normals: the viewer shades each face flat, as `raster.rs` does,
//! and a normal buffer would carry what the consumer derives anyway.

use crate::cluster::Indexed;
use crate::kernel::CadError;

/// Bumped whenever a change alters output bytes, and carried in `kernel_version` beside the
/// parser and the rasterizer. A regenerated rung must be distinguishable from a stale one —
/// the same rule `raster.rs`'s `RASTER_VERSION` exists for.
pub const GLB_VERSION: &str = "glb-3";

const MAGIC: u32 = 0x4654_6C67; // "glTF"
const CONTAINER_VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

/// glTF component types.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// glTF bufferView targets.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

const MESHOPT: &str = "EXT_meshopt_compression";

/// Round up to the next 4-byte boundary. The container requires it of chunks, and the
/// uncompressed layout keeps the index view where an accessor may start.
fn pad_to_four(n: usize) -> usize {
    n.div_ceil(4) * 4
}

pub(crate) fn write_glb(indexed: &Indexed) -> Result<Vec<u8>, CadError> {
    if indexed.indices.is_empty() {
        return Err(CadError::Unrenderable {
            detail: "the mesh has no triangles left after clustering".to_owned(),
        });
    }
    let uncompressible = |what: &str, source: meshopt::Error| CadError::Unrenderable {
        detail: format!("could not compress the {what} of the glTF rung: {source}"),
    };

    // Cache order within each part and never across two, so a part's triangles stay one run the
    // viewer can leave out. Fetch order below renumbers vertices and moves no triangle.
    let whole = [indexed.triangle_count()];
    let runs: &[u32] = if indexed.parts.is_empty() {
        &whole
    } else {
        &indexed.parts
    };
    let mut indices = Vec::with_capacity(indexed.indices.len());
    let mut start = 0;
    for &triangles in runs {
        let end = start + triangles as usize * 3;
        if end > start {
            indices.extend(meshopt::optimize_vertex_cache(
                &indexed.indices[start..end],
                indexed.positions.len(),
            ));
        }
        start = end;
    }
    let positions = meshopt::optimize_vertex_fetch(&mut indices, &indexed.positions);
    let vertices = meshopt::encode_vertex_buffer(&positions)
        .map_err(|source| uncompressible("positions", source))?;
    let triangles = meshopt::encode_index_buffer(&indices, positions.len())
        .map_err(|source| uncompressible("triangles", source))?;

    let triangles_offset = pad_to_four(vertices.len());
    let bin_len = pad_to_four(triangles_offset + triangles.len());
    // What the two views decode to, laid out as an uncompressed file would lay them out.
    let positions_len = positions.len() * 12;
    let indices_offset = pad_to_four(positions_len);
    let indices_len = indices.len() * 4;
    let (min, max) = position_bounds(&positions);

    let mut mesh = serde_json::json!({
        "primitives": [ { "attributes": { "POSITION": 0 }, "indices": 1 } ]
    });
    if !indexed.parts.is_empty() {
        mesh["extras"] = serde_json::json!({ "parts": indexed.parts });
    }

    let document = serde_json::json!({
        "asset": { "version": "2.0", "generator": format!("lapidary-cad {GLB_VERSION}") },
        "extensionsUsed": [MESHOPT],
        "extensionsRequired": [MESHOPT],
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0 } ],
        "meshes": [mesh],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": FLOAT,
                "count": positions.len(),
                "type": "VEC3",
                // Required by the specification on POSITION, and not decoration: a viewer
                // frames the part from these, so absent or stale bounds put the camera in
                // the wrong place.
                "min": min,
                "max": max
            },
            {
                "bufferView": 1,
                "componentType": UNSIGNED_INT,
                "count": indices.len(),
                "type": "SCALAR"
            }
        ],
        "bufferViews": [
            {
                "buffer": 1, "byteOffset": 0, "byteLength": positions_len, "byteStride": 12,
                "target": ARRAY_BUFFER,
                "extensions": { MESHOPT: {
                    "buffer": 0, "byteOffset": 0, "byteLength": vertices.len(),
                    "byteStride": 12, "count": positions.len(), "mode": "ATTRIBUTES"
                } }
            },
            {
                "buffer": 1, "byteOffset": indices_offset, "byteLength": indices_len,
                "target": ELEMENT_ARRAY_BUFFER,
                "extensions": { MESHOPT: {
                    "buffer": 0, "byteOffset": triangles_offset, "byteLength": triangles.len(),
                    "byteStride": 4, "count": indices.len(), "mode": "TRIANGLES"
                } }
            }
        ],
        "buffers": [
            { "byteLength": bin_len },
            { "byteLength": indices_offset + indices_len,
              "extensions": { MESHOPT: { "fallback": true } } }
        ]
    });

    let mut json_chunk =
        serde_json::to_vec(&document).map_err(|source| CadError::Unrenderable {
            detail: format!("could not encode the glTF document: {source}"),
        })?;
    // JSON pads with spaces, BIN pads with zeros. The specification is explicit, and a
    // loader that reads the JSON chunk as a string chokes on a trailing NUL.
    json_chunk.resize(pad_to_four(json_chunk.len()), b' ');

    let mut bin_chunk = Vec::with_capacity(bin_len);
    bin_chunk.extend_from_slice(&vertices);
    bin_chunk.resize(triangles_offset, 0);
    bin_chunk.extend_from_slice(&triangles);
    bin_chunk.resize(bin_len, 0);

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&bin_chunk);
    Ok(out)
}

fn position_bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal GLB reader written from the specification, deliberately sharing no helper
    /// with the writer above. It re-derives every offset from the bytes rather than
    /// recomputing them the way `write_glb` did — a self-consistent writer passes a reader
    /// built from its own arithmetic every time, which is the failure this exists to catch.
    struct Parsed {
        json: serde_json::Value,
        bin: Vec<u8>,
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        let mut four = [0u8; 4];
        four.copy_from_slice(&bytes[offset..offset + 4]);
        u32::from_le_bytes(four)
    }

    fn read_glb(bytes: &[u8]) -> Parsed {
        assert_eq!(u32_at(bytes, 0), MAGIC, "magic");
        assert_eq!(u32_at(bytes, 4), 2, "container version");
        assert_eq!(
            u32_at(bytes, 8) as usize,
            bytes.len(),
            "the declared length must be the real length"
        );

        let json_len = u32_at(bytes, 12) as usize;
        assert_eq!(u32_at(bytes, 16), CHUNK_JSON);
        let json_start = 20;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[json_start..json_start + json_len])
                .expect("the JSON chunk parses");

        let bin_header = json_start + json_len;
        let bin_len = u32_at(bytes, bin_header) as usize;
        assert_eq!(u32_at(bytes, bin_header + 4), CHUNK_BIN);
        let bin_start = bin_header + 8;
        Parsed {
            json,
            bin: bytes[bin_start..bin_start + bin_len].to_vec(),
        }
    }

    fn a_triangle() -> Indexed {
        Indexed {
            positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
            indices: vec![0, 1, 2],
            grid: None,
            parts: vec![],
        }
    }

    #[test]
    fn the_header_declares_gltf_two_and_the_real_length() {
        let bytes = write_glb(&a_triangle()).expect("writes");
        let parsed = read_glb(&bytes);
        assert_eq!(parsed.json["asset"]["version"], "2.0");
    }

    #[test]
    fn both_chunks_are_four_byte_aligned_and_padded_with_the_right_filler() {
        let bytes = write_glb(&a_triangle()).expect("writes");
        let json_len = u32_at(&bytes, 12) as usize;
        assert_eq!(json_len % 4, 0, "the JSON chunk must be 4-byte aligned");
        // JSON pads with spaces. A NUL here breaks loaders that read the chunk as a string.
        assert_eq!(bytes[20 + json_len - 1], b' ');
        let bin_len = u32_at(&bytes, 20 + json_len) as usize;
        assert_eq!(bin_len % 4, 0, "the BIN chunk must be 4-byte aligned");
    }

    #[test]
    fn the_position_accessor_carries_the_meshs_real_bounds() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        let accessor = &parsed.json["accessors"][0];
        assert_eq!(accessor["min"], serde_json::json!([0.0, 0.0, 0.0]));
        assert_eq!(accessor["max"], serde_json::json!([2.0, 3.0, 0.0]));
    }

    #[test]
    fn the_index_accessor_counts_three_per_triangle() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        assert_eq!(parsed.json["accessors"][1]["count"], 3);
        assert_eq!(parsed.json["accessors"][1]["componentType"], UNSIGNED_INT);
    }

    /// A mesh big enough for the codecs to reorder, with coordinates no decimal rounds cleanly.
    fn a_grid() -> Indexed {
        let side = 12u32;
        let positions = (0..side * side)
            .map(|at| {
                let (x, y) = ((at % side) as f32, (at / side) as f32);
                [x * 0.1 + 1e-4, y * 0.37 - 5.0, (x * y) * 0.013]
            })
            .collect();
        let indices = (0..side - 1)
            .flat_map(|y| (0..side - 1).map(move |x| y * side + x))
            .flat_map(|at| [at, at + 1, at + side, at + 1, at + side + 1, at + side])
            .collect();
        Indexed {
            positions,
            indices,
            grid: None,
            parts: vec![],
        }
    }

    /// Each triangle as its corners' bit patterns, turned to start at its lowest corner so the
    /// winding is kept, then sorted: equal exactly when two meshes hold the same triangles.
    fn triangles(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[[u32; 3]; 3]> {
        let mut out: Vec<[[u32; 3]; 3]> = indices
            .chunks_exact(3)
            .map(|t| {
                let corner = |i: usize| positions[t[i] as usize].map(f32::to_bits);
                let turned = [corner(0), corner(1), corner(2)];
                let first = (0..3).min_by_key(|&i| turned[i]).expect("three corners");
                [
                    turned[first],
                    turned[(first + 1) % 3],
                    turned[(first + 2) % 3],
                ]
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn the_compressed_views_do_not_overlap_and_fit_the_stored_buffer() {
        let parsed = read_glb(&write_glb(&a_grid()).expect("writes"));
        let range = |v: &serde_json::Value| {
            let ext = &v["extensions"][MESHOPT];
            assert_eq!(
                ext["buffer"], 0,
                "compressed bytes live in the GLB's own buffer"
            );
            let start = ext["byteOffset"].as_u64().expect("offset");
            (start, start + ext["byteLength"].as_u64().expect("length"))
        };
        let views = parsed.json["bufferViews"]
            .as_array()
            .expect("two buffer views");
        let (vertices, triangles) = (range(&views[0]), range(&views[1]));
        assert!(vertices.1 <= triangles.0, "the views overlap");
        assert!(
            triangles.1 <= parsed.bin.len() as u64,
            "a view runs past the BIN chunk"
        );
    }

    #[test]
    fn the_extension_is_required_so_no_loader_reads_the_empty_fallback() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        assert_eq!(
            parsed.json["extensionsRequired"],
            serde_json::json!([MESHOPT])
        );
        let fallback = &parsed.json["buffers"][1];
        assert_eq!(fallback["extensions"][MESHOPT]["fallback"], true);
        assert!(
            fallback.get("uri").is_none(),
            "the fallback buffer holds no bytes"
        );
    }

    /// The positions and triangles the two compressed views decode to.
    fn decode(parsed: &Parsed) -> (Vec<[f32; 3]>, Vec<u32>) {
        let view = |i: usize| {
            let ext = &parsed.json["bufferViews"][i]["extensions"][MESHOPT];
            let start = ext["byteOffset"].as_u64().expect("offset") as usize;
            let end = start + ext["byteLength"].as_u64().expect("length") as usize;
            (
                &parsed.bin[start..end],
                ext["count"].as_u64().expect("count") as usize,
            )
        };
        let (vertices, vertex_count) = view(0);
        let (indices, index_count) = view(1);
        (
            meshopt::decode_vertex_buffer(vertices, vertex_count).expect("positions decode"),
            meshopt::decode_index_buffer(indices, index_count).expect("triangles decode"),
        )
    }

    #[test]
    fn decoding_gives_back_every_triangle_bit_for_bit() {
        let mesh = a_grid();
        let (positions, decoded) = decode(&read_glb(&write_glb(&mesh).expect("writes")));
        assert_eq!(
            triangles(&positions, &decoded),
            triangles(&mesh.positions, &mesh.indices)
        );
    }

    /// Two parts, one beside the other: after cache ordering the first part's count of triangles
    /// decodes to the first part's triangles and no others, and `extras.parts` says so.
    #[test]
    fn each_part_decodes_to_its_own_run_of_triangles() {
        let mut two = a_grid();
        let per_part = two.triangle_count();
        let offset = two.positions.len() as u32;
        let beside: Vec<[f32; 3]> = two
            .positions
            .iter()
            .map(|p| [p[0] + 100.0, p[1], p[2]])
            .collect();
        let indices: Vec<u32> = two.indices.iter().map(|i| i + offset).collect();
        two.positions.extend(beside);
        two.indices.extend(indices);
        two.parts = vec![per_part, per_part];

        let parsed = read_glb(&write_glb(&two).expect("writes"));
        assert_eq!(
            parsed.json["meshes"][0]["extras"]["parts"],
            serde_json::json!([per_part, per_part])
        );
        let (positions, decoded) = decode(&parsed);
        let split = per_part as usize * 3;
        assert!(
            decoded[..split]
                .iter()
                .all(|&i| positions[i as usize][0] < 50.0),
            "the first run holds only the first part"
        );
        assert!(
            decoded[split..]
                .iter()
                .all(|&i| positions[i as usize][0] > 50.0),
            "and the second only the second"
        );
        assert!(
            read_glb(&write_glb(&a_grid()).expect("writes")).json["meshes"][0]
                .get("extras")
                .is_none(),
            "a mesh of one part says nothing about parts"
        );
    }

    #[test]
    fn an_empty_mesh_is_an_error_rather_than_an_unopenable_file() {
        let empty = Indexed {
            positions: vec![],
            indices: vec![],
            grid: None,
            parts: vec![],
        };
        write_glb(&empty).expect_err("a rung with no triangles is not a glTF file");
    }
}
