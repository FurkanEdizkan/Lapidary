//! Parse a mesh file, say how big it is, write one back out for another tool.
//!
//! Three parsers (`stl`, `obj`, `tmf`), one measurer, two export writers. Free functions, no
//! trait: `Kernel` had one shipped implementation and one test double, `handler.rs` named the
//! implementation directly, and its last live purpose was to be an await point two race tests
//! could suspend a job at. That await point is `WorkerHandler::pause` now, which is two `Notify`
//! handles; an interface with one implementation, for one caller, is not worth keeping to get
//! one. Do not add it back to make a second parser fit — a second `match` arm in `parse` is
//! what that costs.
//!
//! The open path never calls this crate. `lapidary-api` may not even link it
//! (`cargo xtask check-layers`), because the `api` image must not carry a parser it never runs.

mod error;
mod measure;
mod obj;
mod stl;
mod tmf;

pub use error::CadError;
pub use measure::measure;
pub use stl::{Mesh, write_stl};
pub use tmf::write_3mf;

// The three parsers are reached through `parse` and nowhere else. Exported one by one they were
// three more ways for a caller to pick the wrong one from an extension it had not lowercased.
use obj::parse_obj;
use stl::parse_stl;
use tmf::parse_3mf;

/// What `derivative.kernel_version`'s first word says. One parser set, so one name.
pub const IMPLEMENTATION: &str = "mesh";

/// The string `derivative.kernel_version` stores for anything this crate writes.
///
/// **Frozen, and the dead tokens in it are the reason.** `glb-3` named the glTF writer and
/// `cpu-1` the thumbnail rasteriser; this goal deleted both. The string stayed exactly as it
/// was because it is a cache key, not a description: `PgJobs::enqueue_stale_derivatives`
/// compares it against every stored export row, so a worker reporting a different string
/// queues a rebuild of every export in the corpus at its next start. Nothing R5 removed can
/// change an export's bytes — `write_stl` and `write_3mf` are untouched — so a bump here would
/// buy a corpus-wide rebuild that produced byte-identical files.
///
/// It changes when the parsers or the export writers change, and not before. The parser is
/// named per format because an OBJ-derived export and an STL-derived one are different bytes
/// from different code, and a version that said `stl-1` for both would make a regenerated
/// export indistinguishable from a stale one.
pub fn kernel_version(format: &str) -> String {
    format!(
        "{IMPLEMENTATION} {}-1+glb-3+cpu-1",
        format.to_ascii_lowercase()
    )
}

/// The mesh in `bytes`, read by the parser `format` names.
///
/// Dispatch is on the format the scan already selected the file by, never on a byte sniff. OBJ
/// is plain text with no magic number, so sniffing reduces to guessing from the first
/// non-comment line — and the extension is what the walk filtered on, so it is what the parser
/// is told. A file whose extension lies fails to parse, which is the same treatment a corrupt
/// STL already gets.
pub fn parse(bytes: &[u8], format: &str) -> Result<Mesh, CadError> {
    match format.to_ascii_lowercase().as_str() {
        "stl" => parse_stl(bytes),
        "obj" => parse_obj(bytes),
        "3mf" => parse_3mf(bytes),
        // Unreachable through the scan, which admits only the three above, and through
        // ingest, which refuses STEP and IGES by name before reaching here. It is an error
        // rather than a panic because the alternative to a per-file `Permanent` failure is a
        // worker that dies on one misrouted job.
        other => Err(CadError::UnsupportedFormat {
            format: other.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRACKET: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

    /// A binary STL of one degenerate triangle: three vertices at the same point.
    ///
    /// Parses perfectly — the header, the count and the record are all well formed — and has
    /// no extent. That is the shape of the file that failed in the owner's corpus, built here
    /// rather than committed so it can be read.
    fn zero_extent_stl() -> Vec<u8> {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // Normal, then three identical vertices, then the attribute byte count.
        for _ in 0..12 {
            bytes.extend_from_slice(&0f32.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    /// **A mesh that parses is a model, even when nothing can be built from it.**
    ///
    /// This used to be an error, and the caller's only option was to fail the ingest — so a
    /// file like this was absent from the library rather than present with no picture. A
    /// measured run over 1,095 real files found exactly one of these.
    #[test]
    fn a_mesh_with_no_extent_still_yields_its_measurements() {
        let mesh = parse(&zero_extent_stl(), "stl")
            .expect("a file that parses is not an error, whatever can be made of it");
        assert_eq!(
            measure(&mesh).0.triangle_count,
            1,
            "the geometry is real and measured, which is what makes it a part"
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error() {
        let err = parse(b"this is not an STL at all", "stl").expect_err("must fail");
        assert!(
            matches!(err, CadError::MalformedMesh { .. }),
            "a parse failure reports the parse, not something downstream: {err}"
        );
    }

    #[test]
    fn a_real_stl_yields_its_measurements() {
        let (measurements, _) = measure(&parse(BRACKET, "stl").expect("parses"));
        assert!(measurements.triangle_count > 0);
        assert!(measurements.surface_area_mm2 > 0.0);
        assert!(
            measurements.bbox_mm.iter().all(|mm| *mm > 0.0),
            "a real bracket has extent on all three axes, got {:?}",
            measurements.bbox_mm
        );
    }

    #[test]
    fn an_obj_file_is_parsed_by_the_obj_parser() {
        let bytes = include_bytes!("../../../fixtures/idler-bracket-lp-2210-01.obj");
        let mesh = parse(bytes, "obj").expect("parses");
        assert_eq!(measure(&mesh).0.triangle_count, 20);
    }

    #[test]
    fn a_3mf_file_is_parsed_by_the_3mf_parser() {
        let bytes = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");
        let mesh = parse(bytes, "3mf").expect("parses");
        assert!(measure(&mesh).0.triangle_count > 0);
    }

    /// The extension decides, whatever case it arrives in: `source_format` lowercases, but a
    /// `.STL` reaching here directly must still find the STL parser rather than the refusal.
    #[test]
    fn the_format_is_matched_without_regard_to_case() {
        assert!(parse(BRACKET, "STL").is_ok());
        assert_eq!(kernel_version("STL"), kernel_version("stl"));
    }

    #[test]
    fn a_format_with_no_parser_is_an_error_rather_than_a_panic() {
        // "step" stands in for any format the scan cannot route here: it is refused by name
        // at ingest (R2), and a job misrouted here must still fail rather than panic.
        let err = parse(b"anything", "step").expect_err("must fail");
        assert!(matches!(err, CadError::UnsupportedFormat { .. }), "{err}");
    }

    /// **Written as literals on purpose.** `kernel_version` is a stored cache key, and
    /// `enqueue_stale_derivatives` compares it against every export row in the corpus — so the
    /// test that guards it must fail when the string changes, which a test written against the
    /// function's own constants never would.
    #[test]
    fn the_stored_version_is_exactly_what_it_has_always_been() {
        assert_eq!(kernel_version("stl"), "mesh stl-1+glb-3+cpu-1");
        assert_eq!(kernel_version("obj"), "mesh obj-1+glb-3+cpu-1");
        assert_eq!(kernel_version("3mf"), "mesh 3mf-1+glb-3+cpu-1");
    }

    /// The correctness case the per-format version exists for: the same part ingested as STL
    /// and as OBJ produces different export bytes from different code, so the two must be
    /// tellable apart by the column that exists to tell them apart.
    #[test]
    fn each_parser_reports_its_own_version() {
        let versions: std::collections::BTreeSet<String> = lapidary_core::MESH_EXTENSIONS
            .iter()
            .map(|format| kernel_version(format))
            .collect();
        assert_eq!(
            versions.len(),
            lapidary_core::MESH_EXTENSIONS.len(),
            "each parser needs its own version: {versions:?}"
        );
    }
}
