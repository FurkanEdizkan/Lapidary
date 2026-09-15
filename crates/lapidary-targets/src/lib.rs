//! Automatic format negotiation: slicers get 3MF/STL, CAD gets STEP, the viewer gets
//! glTF.
//!
//! A [`Target`] names the formats a tool reads, and [`negotiate`] decides what it is handed: the part's
//! original bytes when the tool reads them, else a mesh export Lapidary writes (`*.lapidary.3mf`,
//! `*.lapidary.stl`), and never a mesh to a tool that reads only B-rep. Export bundles live here too
//! (`bundle`). See `docs/DATA.md` §5.

pub mod bundle;

use lapidary_core::DerivativeKind;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TargetsError {
    #[error(
        "No format satisfies {target}. This part has {available} available, and Lapidary will not hand a mesh to a target that needs B-rep. Generate a compatible derivative, or export from the original in the CAD tool."
    )]
    NoFormatMatch { target: String, available: String },

    #[error(
        "No format satisfies {target}. This part has {available} available, and Lapidary writes meshes only as 3mf and stl. Download one of those and convert it in a mesh tool."
    )]
    NoExport { target: String, available: String },

    #[error(
        "Export failed: {reason}. Retry, and if it keeps failing, check the source derivative is not corrupt."
    )]
    ExportFailed { reason: String },
}

/// A format a part can be handed to a tool in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Step,
    Iges,
    Stl,
    ThreeMf,
    Obj,
}

impl Format {
    /// A format by its name: the extension Lapidary records a source file under, or a download variant.
    pub fn named(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "step" | "stp" => Some(Format::Step),
            "iges" | "igs" => Some(Format::Iges),
            "stl" => Some(Format::Stl),
            "3mf" => Some(Format::ThreeMf),
            "obj" => Some(Format::Obj),
            _ => None,
        }
    }

    /// The MIME types a desktop may know this format by: the registered one first, then its
    /// shared-mime-info aliases and the older spellings apps still register under.
    ///
    /// Every one is asked for because missing an app is not symmetric: a CAD app missed for STEP
    /// would be handed a mesh. shared-mime-info 2.4 has no STEP type at all, so for STEP these
    /// spellings are all there is.
    pub fn mimes(self) -> &'static [&'static str] {
        match self {
            Format::Step => &[
                "model/step",
                "model/step+zip",
                "application/step",
                "application/STEP",
                "application/x-step",
                "application/p21",
            ],
            Format::Iges => &["model/iges", "application/iges", "application/x-iges"],
            Format::Stl => &[
                "model/stl",
                "model/x.stl-ascii",
                "model/x.stl-binary",
                "application/sla",
            ],
            Format::ThreeMf => &["model/3mf", "application/vnd.ms-3mfdocument"],
            Format::Obj => &["model/obj"],
        }
    }

    /// The name a download variant and a file extension use.
    pub fn name(self) -> &'static str {
        match self {
            Format::Step => "step",
            Format::Iges => "iges",
            Format::Stl => "stl",
            Format::ThreeMf => "3mf",
            Format::Obj => "obj",
        }
    }

    /// The derivative Lapidary writes a part's mesh to in this format, if it writes one.
    pub fn export(self) -> Option<DerivativeKind> {
        match self {
            Format::ThreeMf => Some(DerivativeKind::Export3mf),
            Format::Stl => Some(DerivativeKind::ExportStl),
            Format::Step | Format::Iges | Format::Obj => None,
        }
    }

    /// Whether this format holds triangles rather than B-rep.
    pub fn is_mesh(self) -> bool {
        matches!(self, Format::Stl | Format::ThreeMf | Format::Obj)
    }
}

/// The formats Lapidary writes from any part's mesh, most useful first: a 3MF keeps its units, and every slicer
/// reads an STL. Both are meshes.
pub const EXPORTS: [Format; 2] = [Format::ThreeMf, Format::Stl];

/// Where a part is going: a tool, and the formats it reads, most wanted first.
pub trait Target {
    /// The tool, as a refusal names it.
    fn name(&self) -> String;
    fn accepts(&self) -> Vec<Format>;
}

/// What a target is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handover {
    /// The ingested bytes, unconverted.
    Original,
    /// A file Lapidary writes, named `*.lapidary.*`.
    Export(Format),
}

/// The original when `target` reads `source`, else the first export it reads.
///
/// Every export is a mesh, so a target that reads only B-rep formats is never handed one: a part it cannot
/// read in its original format is refused, naming what there is.
pub fn negotiate(target: &dyn Target, source: Format) -> Result<Handover, TargetsError> {
    let accepts = target.accepts();
    if accepts.contains(&source) {
        return Ok(Handover::Original);
    }
    if let Some(export) = accepts
        .iter()
        .copied()
        .find(|format| EXPORTS.contains(format))
    {
        return Ok(Handover::Export(export));
    }
    let (target, available) = (
        target.name(),
        std::iter::once(source)
            .chain(EXPORTS.into_iter().filter(|export| *export != source))
            .map(Format::name)
            .collect::<Vec<_>>()
            .join(", "),
    );
    // A target that reads a mesh Lapidary does not write, OBJ, is not one that needs B-rep.
    if accepts.iter().any(|format| format.is_mesh()) {
        return Err(TargetsError::NoExport { target, available });
    }
    Err(TargetsError::NoFormatMatch { target, available })
}

/// A tool by the formats it reads: a download of the one format somebody picked (`DATA.md` §5.1), or the apps
/// this computer opens files with.
pub struct Tool {
    pub name: String,
    pub accepts: Vec<Format>,
}

impl Target for Tool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn accepts(&self) -> Vec<Format> {
        self.accepts.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, accepts: &[Format]) -> Tool {
        Tool {
            name: name.to_owned(),
            accepts: accepts.to_vec(),
        }
    }

    fn slicer() -> Tool {
        tool("PrusaSlicer", &[Format::ThreeMf, Format::Stl])
    }

    #[test]
    fn a_target_that_reads_the_original_is_handed_the_original() {
        assert_eq!(
            negotiate(&slicer(), Format::Stl).expect("an STL"),
            Handover::Original
        );
        assert_eq!(
            negotiate(&tool("FreeCAD", &[Format::Step]), Format::Step).expect("a STEP"),
            Handover::Original
        );
    }

    #[test]
    fn a_slicer_is_handed_a_cad_part_as_the_export_it_reads_first() {
        assert_eq!(
            negotiate(&slicer(), Format::Step).expect("an export"),
            Handover::Export(Format::ThreeMf)
        );
        assert_eq!(
            negotiate(&tool("Cura", &[Format::Stl]), Format::Iges).expect("an export"),
            Handover::Export(Format::Stl)
        );
    }

    #[test]
    fn a_target_that_needs_b_rep_is_never_handed_a_mesh() {
        let refused = negotiate(&tool("FreeCAD", &[Format::Step, Format::Iges]), Format::Stl)
            .expect_err("no mesh for a B-rep tool");
        assert!(
            refused.to_string().contains("FreeCAD")
                && refused.to_string().contains("has stl, 3mf available"),
            "names the tool, and what there is once each: {refused}"
        );
        assert!(refused.to_string().contains("B-rep"));
    }

    /// OBJ is a mesh Lapidary does not write, so a tool reading only OBJ is refused for that, not
    /// told it needs B-rep.
    #[test]
    fn a_mesh_format_lapidary_does_not_write_is_refused_as_one() {
        let refused = negotiate(&tool("MeshLab", &[Format::Obj]), Format::Step)
            .expect_err("no OBJ is written");
        assert!(
            refused
                .to_string()
                .contains("writes meshes only as 3mf and stl")
                && !refused.to_string().contains("B-rep"),
            "{refused}"
        );
    }

    #[test]
    fn a_format_is_read_off_its_name_in_any_case() {
        assert_eq!(Format::named("STP"), Some(Format::Step));
        assert_eq!(Format::named("igs"), Some(Format::Iges));
        assert_eq!(Format::named("3MF"), Some(Format::ThreeMf));
        assert_eq!(Format::named("fcstd"), None);
    }

    /// Each format's registered type first, and STEP under the spellings apps still register it by,
    /// since the MIME database has none of its own.
    #[test]
    fn a_format_is_asked_for_under_every_type_a_desktop_may_know_it_by() {
        assert_eq!(Format::ThreeMf.mimes()[0], "model/3mf");
        assert_eq!(Format::Step.mimes()[0], "model/step");
        assert!(Format::Step.mimes().contains(&"application/step"));
        assert!(Format::Iges.mimes().contains(&"application/iges"));
        assert!(Format::Stl.mimes().contains(&"model/x.stl-binary"));
    }

    #[test]
    fn every_export_is_written_and_nothing_else_is() {
        assert_eq!(
            EXPORTS.map(Format::export),
            [
                Some(DerivativeKind::Export3mf),
                Some(DerivativeKind::ExportStl)
            ]
        );
        assert_eq!(Format::Step.export(), None);
        assert_eq!(Format::Obj.export(), None);
    }

    /// The download route negotiates from the extension a part's file is recorded under, so each must be a format.
    #[test]
    fn every_extension_ingest_records_is_a_named_format() {
        for extension in lapidary_core::MESH_EXTENSIONS
            .iter()
            .chain(&lapidary_core::CAD_FORMATS)
        {
            assert!(
                Format::named(extension).is_some(),
                "`{extension}` has no format"
            );
        }
    }
}
