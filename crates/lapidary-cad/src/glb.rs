//! glTF 2.0 binary output, uncompressed.
//!
//! `DATA.md` §2.2 chose meshopt as the codec and that stands — but its decoder is Phase 3's
//! viewer, and the Rust binding wraps C, which would put a C toolchain into the worker
//! image against the offline-build constraint `docs/prototype-notes.md` calls worth
//! preserving. Derivatives are designed to be evicted and regenerated (`DATA.md` §1.5), so
//! Phase 3 re-encodes and the cost is one pass over disposable data.
//!
//! What is written: one buffer, two bufferViews, two accessors, one mesh with one
//! primitive, one node, one scene. No materials and no normals — the viewer computes
//! normals from winding, exactly as `raster.rs` already does, and a normal buffer would
//! double the file for data the consumer regenerates anyway.

use crate::cluster::Indexed;
use crate::kernel::CadError;

/// Bumped whenever a change alters output bytes, and carried in `kernel_version` beside the
/// parser and the rasterizer. A regenerated rung must be distinguishable from a stale one —
/// the same rule `raster.rs`'s `RASTER_VERSION` exists for.
pub const GLB_VERSION: &str = "glb-1";

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

/// Round up to the next 4-byte boundary. Both chunks and both bufferViews need it: the
/// container requires it of chunks, and an accessor whose byteOffset is not a multiple of
/// its component size is invalid even where a lenient loader accepts it.
fn pad_to_four(n: usize) -> usize {
    n.div_ceil(4) * 4
}

pub(crate) fn write_glb(indexed: &Indexed) -> Result<Vec<u8>, CadError> {
    if indexed.indices.is_empty() {
        return Err(CadError::Unrenderable {
            detail: "the mesh has no triangles left after clustering".to_owned(),
        });
    }

    let positions_len = indexed.positions.len() * 12;
    let indices_offset = pad_to_four(positions_len);
    let indices_len = indexed.indices.len() * 4;
    let buffer_len = indices_offset + indices_len;
    let (min, max) = position_bounds(indexed);

    let document = serde_json::json!({
        "asset": { "version": "2.0", "generator": format!("lapidary-cad {GLB_VERSION}") },
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0 } ],
        "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 }, "indices": 1 } ] } ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": FLOAT,
                "count": indexed.positions.len(),
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
                "count": indexed.indices.len(),
                "type": "SCALAR"
            }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": positions_len,
              "target": ARRAY_BUFFER },
            { "buffer": 0, "byteOffset": indices_offset, "byteLength": indices_len,
              "target": ELEMENT_ARRAY_BUFFER }
        ],
        "buffers": [ { "byteLength": buffer_len } ]
    });

    let mut json_chunk =
        serde_json::to_vec(&document).map_err(|source| CadError::Unrenderable {
            detail: format!("could not encode the glTF document: {source}"),
        })?;
    // JSON pads with spaces, BIN pads with zeros. The specification is explicit, and a
    // loader that reads the JSON chunk as a string chokes on a trailing NUL.
    json_chunk.resize(pad_to_four(json_chunk.len()), b' ');

    let mut bin_chunk = Vec::with_capacity(buffer_len);
    for position in &indexed.positions {
        for axis in position {
            bin_chunk.extend_from_slice(&axis.to_le_bytes());
        }
    }
    bin_chunk.resize(indices_offset, 0);
    for index in &indexed.indices {
        bin_chunk.extend_from_slice(&index.to_le_bytes());
    }
    bin_chunk.resize(pad_to_four(bin_chunk.len()), 0);

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

fn position_bounds(indexed: &Indexed) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in &indexed.positions {
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

    #[test]
    fn the_buffer_views_do_not_overlap_and_fit_the_buffer() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        let views = parsed.json["bufferViews"]
            .as_array()
            .expect("two buffer views");
        let end = |v: &serde_json::Value| {
            v["byteOffset"].as_u64().unwrap_or(0) + v["byteLength"].as_u64().unwrap_or(0)
        };
        assert!(end(&views[0]) <= views[1]["byteOffset"].as_u64().expect("offset"));
        assert!(
            end(&views[1])
                <= parsed.json["buffers"][0]["byteLength"]
                    .as_u64()
                    .expect("buffer length")
        );
    }

    #[test]
    fn a_round_trip_through_the_independent_reader_recovers_the_vertices() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        let offset = parsed.json["bufferViews"][0]["byteOffset"]
            .as_u64()
            .expect("offset") as usize;
        let recovered: Vec<f32> = parsed.bin[offset..offset + 36]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().expect("four bytes")))
            .collect();
        assert_eq!(recovered, vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0]);
    }

    #[test]
    fn an_empty_mesh_is_an_error_rather_than_an_unopenable_file() {
        let empty = Indexed {
            positions: vec![],
            indices: vec![],
            grid: None,
        };
        write_glb(&empty).expect_err("a rung with no triangles is not a glTF file");
    }
}
