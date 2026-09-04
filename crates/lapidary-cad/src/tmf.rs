//! 3MF: a mesh in XML inside an OPC package, which is a ZIP.
//!
//! Unlike `stl.rs` and `obj.rs` this reader is not hand-rolled. The container is a
//! security boundary — zip64, data descriptors, local-versus-central header mismatch —
//! and a bug here is a vulnerability rather than a wrong mesh. See spec §3.2.

use crate::kernel::CadError;
use crate::stl::{Mesh, finish};
use std::io::Read;

pub(crate) const FORMAT: &str = "3MF";

/// Limits on what an archive may expand to. `DATA.md` §5.4 requires all three.
///
/// A struct rather than three constants so tests can inject small values: proving the
/// 2 GiB cap fires would otherwise need a 2 GiB fixture in the repository. The mechanism
/// is the part that can break, and small caps exercise the same mechanism.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Caps {
    pub(crate) max_decompressed: u64,
    pub(crate) max_entries: usize,
    pub(crate) max_ratio: u64,
    /// A ceiling on triangles across an object's own mesh *and* everything its
    /// `<components>` recurse into. `MAX_DEPTH` bounds recursion depth, not breadth: an
    /// object can hold many components, each one recursing, so emitted triangles grow as
    /// branching^depth rather than depth alone. This is not one of the three archive
    /// caps `DATA.md` §5.4 requires -- it bounds amplification after decompression, not
    /// the ZIP itself.
    pub(crate) max_triangles: usize,
}

impl Caps {
    /// Sized in spec §3.4 against `DATA.md`'s 1 MB – 2 GB source range. Compiled in and
    /// deliberately not configurable: a security control with an environment override is
    /// one an operator can switch off by accident.
    pub(crate) const DEFAULT: Caps = Caps {
        max_decompressed: 2 << 30,
        max_entries: 1024,
        max_ratio: 200,
        // A triangle is `[[f32; 3]; 3]` = 36 bytes, so 8 million is ~288 MB of `Vec`.
        // `deploy/compose.yaml` runs the worker at `LAPIDARY_WORKER_CONCURRENCY: 2`
        // against a 2 GiB ceiling, so two jobs at this budget at once is ~576 MB -- room
        // to spare. It is also roughly 200x the largest part in the project's test
        // corpus (35,774 triangles, per the slice-3 handoff doc), so it refuses bombs
        // without refusing real work.
        max_triangles: 8_000_000,
    };
}

/// Read at most `cap` bytes, refusing rather than allocating when the stream is longer.
///
/// `DATA.md` §5.4 says to abort "on breach, not after", and that is this function's whole
/// reason to exist: `Read::take(cap + 1)` means a hostile entry costs `cap + 1` bytes of
/// memory regardless of what it claims to expand to. Reading first and measuring second
/// is the bomb working exactly as designed.
///
/// `reason` names which bound `cap` came from (e.g. "expands past the 4096-byte limit" or
/// "compresses more than 20:1") so the refusal names a number that exists in
/// configuration, rather than a derived value the caller computed and threw away.
pub(crate) fn read_capped<R: Read>(reader: R, cap: u64, reason: &str) -> Result<Vec<u8>, CadError> {
    let mut out = Vec::new();
    // cap + 1: reading exactly `cap` cannot distinguish "ended at the cap" from
    // "continues past it", and a file of exactly the documented maximum size is legal.
    reader
        .take(cap.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|source| CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("the archive could not be read: {source}"),
        })?;
    if out.len() as u64 > cap {
        return Err(CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("one entry {reason}"),
        });
    }
    Ok(out)
}

pub(crate) type Archive<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

/// The 3MF core specification's relationship type for the model part.
const MODEL_REL_TYPE: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

fn refused(detail: String) -> CadError {
    CadError::ArchiveRefused {
        format: FORMAT.to_owned(),
        detail,
    }
}

fn malformed(detail: String) -> CadError {
    CadError::MalformedMesh {
        format: FORMAT.to_owned(),
        detail,
    }
}

pub(crate) fn open_archive<'a>(bytes: &'a [u8], caps: &Caps) -> Result<Archive<'a>, CadError> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|source| malformed(format!("it is not a readable ZIP package: {source}")))?;
    if archive.len() > caps.max_entries {
        return Err(refused(format!(
            "it holds {} entries, past the {} allowed",
            archive.len(),
            caps.max_entries
        )));
    }
    // Names are checked once, up front, so no later lookup can reach a rejected one.
    for name in archive.file_names() {
        if is_unsafe_name(name) {
            return Err(refused(format!("it holds an unsafe entry path: {name}")));
        }
    }
    Ok(archive)
}

/// Absolute paths and `..` segments. Defence in depth here — spec §3.5 — because nothing
/// in this module writes an extracted file anywhere.
fn is_unsafe_name(name: &str) -> bool {
    name.starts_with('/')
        // APPNOTE 4.4.17 requires forward slashes as the only path separator in a ZIP
        // entry name, so any backslash makes the name malformed regardless of what it is
        // trying to do. This also closes the gap a `..` check alone leaves on Unix:
        // `Path::components()` only splits on `/` there, so `..\..\secret.txt` parses as
        // one ordinary component and never yields a `ParentDir`.
        || name.contains('\\')
        || name.contains(':')
        || std::path::Path::new(name)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

pub(crate) fn entry(
    archive: &mut Archive<'_>,
    name: &str,
    caps: &Caps,
) -> Result<Vec<u8>, CadError> {
    let file = archive
        .by_name(name)
        .map_err(|_| malformed(format!("the package has no {name} part")))?;
    let compressed = file.compressed_size().max(1);
    // The ratio bound and the absolute bound, whichever is tighter. Ratio catches the
    // classic bomb: a few kilobytes claiming to be gigabytes. Naming which one fired
    // matters: the ratio bound is derived from this entry's compressed size and appears
    // nowhere in configuration, so a message that just quoted the number would leave an
    // operator unable to find it anywhere.
    let ratio_cap = compressed.saturating_mul(caps.max_ratio);
    let (cap, reason) = if caps.max_decompressed <= ratio_cap {
        (
            caps.max_decompressed,
            format!("expands past the {}-byte limit", caps.max_decompressed),
        )
    } else {
        (
            ratio_cap,
            format!("compresses more than {}:1", caps.max_ratio),
        )
    };
    read_capped(file, cap, &reason)
}

/// The StartPart target from `_rels/.rels`, normalised to an archive entry name.
///
/// Spec §3.6: read the relationships rather than assuming `3D/3dmodel.model`. The cost is
/// one small parse; the benefit is that a legal package that moved its model still opens.
pub(crate) fn model_part_name(rels_xml: &[u8]) -> Result<String, CadError> {
    let mut reader = quick_xml::Reader::from_reader(rels_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let mut target = None;
                let mut is_model = false;
                for attr in e.attributes().flatten() {
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap_or_default()
                        .into_owned();
                    match attr.key.local_name().as_ref() {
                        b"Target" => target = Some(value),
                        b"Type" => is_model = value == MODEL_REL_TYPE,
                        _ => {}
                    }
                }
                if is_model {
                    let t = target.ok_or_else(|| {
                        malformed("its model relationship names no target".to_owned())
                    })?;
                    // Relationship targets are package-absolute (`/3D/x.model`); ZIP
                    // entry names are not.
                    return Ok(t.trim_start_matches('/').to_owned());
                }
            }
            Err(source) => {
                return Err(malformed(format!(
                    "its relationships are not valid XML: {source}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    Err(malformed(
        "it declares no 3D model relationship, so there is nothing to read".to_owned(),
    ))
}

/// Millimetres per unit of the file's declared unit.
///
/// An absent attribute is millimetres, which the 3MF core specification states. An
/// unrecognised one is refused rather than defaulted — spec §3.3.
pub(crate) fn unit_scale(unit: Option<&str>) -> Result<f64, CadError> {
    match unit.unwrap_or("millimeter") {
        "micron" => Ok(0.001),
        "millimeter" => Ok(1.0),
        "centimeter" => Ok(10.0),
        "inch" => Ok(25.4),
        "foot" => Ok(304.8),
        "meter" => Ok(1000.0),
        other => Err(malformed(format!(
            "it declares the unit {other}, and only micron, millimeter, centimeter, inch, \
             foot and meter are understood"
        ))),
    }
}

#[derive(Default)]
struct Object {
    vertices: Vec<[f64; 3]>,
    triangles: Vec<[usize; 3]>,
    components: Vec<(String, [f64; 12])>,
}

pub fn parse_3mf(bytes: &[u8]) -> Result<Mesh, CadError> {
    let caps = Caps::DEFAULT;
    let mut archive = open_archive(bytes, &caps)?;
    let rels = entry(&mut archive, "_rels/.rels", &caps)?;
    let model_name = model_part_name(&rels)?;
    // `rels` and the model part are never needed together, and the model entry's own cap
    // allows up to 2 GiB -- against the worker's 2 GiB ceiling, holding both live at once
    // is the difference between headroom and none.
    drop(rels);
    let model = entry(&mut archive, &model_name, &caps)?;
    let (objects, build, scale) = read_model(&model)?;
    let mut triangles = Vec::new();
    for (id, transform) in &build {
        emit(&objects, id, *transform, scale, 0, &caps, &mut triangles)?;
    }
    finish(FORMAT, triangles)
}

fn attr(e: &quick_xml::events::BytesStart<'_>, want: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == want)
        .map(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .unwrap_or_default()
                .into_owned()
        })
}

fn number(e: &quick_xml::events::BytesStart<'_>, want: &[u8]) -> Result<f64, CadError> {
    let raw = attr(e, want).ok_or_else(|| {
        malformed(format!(
            "a {} attribute is missing",
            String::from_utf8_lossy(want)
        ))
    })?;
    let value: f64 = raw
        .parse()
        .map_err(|_| malformed(format!("{raw:?} is not a number")))?;
    if !value.is_finite() {
        return Err(malformed(format!("{raw:?} is not a finite number")));
    }
    Ok(value)
}

fn index(
    e: &quick_xml::events::BytesStart<'_>,
    want: &[u8],
    count: usize,
) -> Result<usize, CadError> {
    let raw = attr(e, want)
        .ok_or_else(|| malformed("a triangle is missing a vertex reference".to_owned()))?;
    let i: usize = raw
        .parse()
        .map_err(|_| malformed(format!("{raw:?} is not a vertex reference")))?;
    if i >= count {
        return Err(malformed(format!(
            "a triangle names vertex {i}, but its object has defined {count}"
        )));
    }
    Ok(i)
}

type Model = (
    std::collections::BTreeMap<String, Object>,
    Vec<(String, [f64; 12])>,
    f64,
);

fn read_model(xml: &[u8]) -> Result<Model, CadError> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut objects: std::collections::BTreeMap<String, Object> = Default::default();
    let mut build = Vec::new();
    let mut scale = 1.0;
    let mut current: Option<String> = None;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|source| malformed(format!("its model XML is not valid: {source}")))?;
        match &event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                match e.local_name().as_ref() {
                    b"model" => scale = unit_scale(attr(e, b"unit").as_deref())?,
                    b"object" => {
                        let id = attr(e, b"id")
                            .ok_or_else(|| malformed("an object has no id".to_owned()))?;
                        objects.entry(id.clone()).or_default();
                        current = Some(id);
                    }
                    b"vertex" => {
                        if let Some(o) = current.as_ref().and_then(|id| objects.get_mut(id)) {
                            o.vertices
                                .push([number(e, b"x")?, number(e, b"y")?, number(e, b"z")?]);
                        }
                    }
                    b"triangle" => {
                        if let Some(o) = current.as_ref().and_then(|id| objects.get_mut(id)) {
                            let n = o.vertices.len();
                            let t = [
                                index(e, b"v1", n)?,
                                index(e, b"v2", n)?,
                                index(e, b"v3", n)?,
                            ];
                            o.triangles.push(t);
                        }
                    }
                    b"component" => {
                        if let Some(id) = current.clone() {
                            let target = attr(e, b"objectid").ok_or_else(|| {
                                malformed("a component names no object".to_owned())
                            })?;
                            let m = matrix(attr(e, b"transform").as_deref())?;
                            if let Some(o) = objects.get_mut(&id) {
                                o.components.push((target, m));
                            }
                        }
                    }
                    b"item" => {
                        let target = attr(e, b"objectid")
                            .ok_or_else(|| malformed("a build item names no object".to_owned()))?;
                        build.push((target, matrix(attr(e, b"transform").as_deref())?));
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::End(e) if e.local_name().as_ref() == b"object" => {
                current = None;
            }
            _ => {}
        }
        buf.clear();
    }
    Ok((objects, build, scale))
}

const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// Deep enough for any real assembly, shallow enough that a cycle ends quickly. A 3MF
/// nested eight levels is already pathological; a 3MF that references itself is hostile.
const MAX_DEPTH: u32 = 8;

/// 3MF's `transform`: nine numbers of a row-major 3×3, then a translation.
fn matrix(raw: Option<&str>) -> Result<[f64; 12], CadError> {
    let Some(raw) = raw else { return Ok(IDENTITY) };
    let mut m = IDENTITY;
    // Counted up front rather than inferred from the loop. `zip` stops at the shorter
    // side, so a loop that counts as it goes rejects a transform with too FEW numbers and
    // silently truncates one with too many -- an asymmetric hole in exactly the input
    // validation CLAUDE.md says never to simplify away, on an untrusted file.
    let count = raw.split_whitespace().count();
    if count != 12 {
        return Err(malformed(format!(
            "a transform has {count} numbers, and a 3MF transform has exactly twelve"
        )));
    }
    for (slot, token) in m.iter_mut().zip(raw.split_whitespace()) {
        *slot = token.parse().map_err(|_| {
            malformed(format!(
                "a transform holds {token:?}, which is not a number"
            ))
        })?;
        if !slot.is_finite() {
            return Err(malformed(format!(
                "a transform holds {token:?}, which is not finite"
            )));
        }
    }
    Ok(m)
}

/// `outer` applied after `inner` — the order a component nested in an item needs.
fn compose(outer: [f64; 12], inner: [f64; 12]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = (0..3)
                .map(|k| inner[row * 3 + k] * outer[k * 3 + col])
                .sum();
        }
    }
    for col in 0..3 {
        out[9 + col] = (0..3)
            .map(|k| inner[9 + k] * outer[k * 3 + col])
            .sum::<f64>()
            + outer[9 + col];
    }
    out
}

fn apply(m: [f64; 12], v: [f64; 3]) -> [f64; 3] {
    [
        v[0] * m[0] + v[1] * m[3] + v[2] * m[6] + m[9],
        v[0] * m[1] + v[1] * m[4] + v[2] * m[7] + m[10],
        v[0] * m[2] + v[1] * m[5] + v[2] * m[8] + m[11],
    ]
}

fn emit(
    objects: &std::collections::BTreeMap<String, Object>,
    id: &str,
    transform: [f64; 12],
    scale: f64,
    depth: u32,
    caps: &Caps,
    out: &mut Vec<[[f32; 3]; 3]>,
) -> Result<(), CadError> {
    if depth > MAX_DEPTH {
        return Err(malformed(format!(
            "its objects are nested more than {MAX_DEPTH} deep, or reference each other in a cycle"
        )));
    }
    let object = objects.get(id).ok_or_else(|| {
        malformed(format!(
            "a build item names object {id}, which does not exist"
        ))
    })?;

    // Checked before the batch is pushed, not after: `MAX_DEPTH` bounds recursion depth,
    // but nothing bounds how many `<components>` one object holds, so triangles can grow
    // as branching^depth. Measuring `out.len()` after emitting the bomb is the bomb
    // working exactly as designed -- same principle as `read_capped`.
    if out.len() + object.triangles.len() > caps.max_triangles {
        return Err(refused(format!(
            "its components would emit more than {} triangles, past the amount this \
             reader allows",
            caps.max_triangles
        )));
    }

    for t in &object.triangles {
        // Transform first in the file's own units, then scale to millimetres: the
        // transform's numbers are expressed in those units too, so scaling first would
        // apply the unit twice to the translation.
        let corner = |i: usize| {
            let v = apply(transform, object.vertices[i]);
            [
                (v[0] * scale) as f32,
                (v[1] * scale) as f32,
                (v[2] * scale) as f32,
            ]
        };
        out.push([corner(t[0]), corner(t[1]), corner(t[2])]);
    }

    for (child, child_transform) in &object.components {
        emit(
            objects,
            child,
            compose(transform, *child_transform),
            scale,
            depth + 1,
            caps,
            out,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bounded source that records how many bytes were actually pulled from it.
    ///
    /// Bounded on purpose. An infinite reader proves the same point more elegantly, but
    /// the naive implementation this test exists to catch calls `read_to_end` on it and
    /// allocates until the machine dies — an OOM kill, not a test failure, and it takes
    /// the developer's editor down with it. That happened twice while this slice was
    /// being written. A finite source plus a byte counter gives a deterministic red test
    /// for 64 KiB.
    ///
    /// Also deliberately not `std::io::repeat`: std specialises `Repeat::read_to_end` to
    /// fail with `OutOfMemory` immediately, so the naive version would return an error
    /// too and the test would pass against the exact bug it exists to catch.
    struct Counted<'a> {
        remaining: usize,
        pulled: &'a std::cell::Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.remaining);
            buf[..n].fill(0);
            self.remaining -= n;
            self.pulled.set(self.pulled.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn reading_stops_at_the_cap_rather_than_after_it() {
        // `DATA.md` §5.4 says abort "on breach, not after", and the byte count is what
        // tells those two apart. The source holds 64 KiB against a 1 KiB cap: capping as
        // bytes arrive pulls about 1 KiB and stops, while reading everything and
        // measuring afterwards pulls all 64 KiB. Both return an error, so asserting on
        // the error alone would certify nothing.
        let pulled = std::cell::Cell::new(0);
        let source = Counted {
            remaining: 64 * 1024,
            pulled: &pulled,
        };
        let err =
            read_capped(source, 1024, "expands past the 1024-byte limit").expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
        assert!(
            pulled.get() <= 1024 + 4096,
            "pulled {} bytes for a 1024-byte cap: the cap must bound the read, not just \
             the result",
            pulled.get()
        );
    }

    #[test]
    fn a_stream_inside_the_cap_is_returned_whole() {
        let bytes =
            read_capped(&b"3MF"[..], 1024, "expands past the 1024-byte limit").expect("reads");
        assert_eq!(bytes, b"3MF");
    }

    #[test]
    fn a_stream_exactly_at_the_cap_is_allowed() {
        // Off-by-one guard: the cap is a maximum, not a strict bound. A 1024-byte entry
        // under a 1024-byte cap is legal, and a parser that refused it would reject
        // files for being exactly the documented size.
        let bytes =
            read_capped(&[7u8; 1024][..], 1024, "expands past the 1024-byte limit").expect("reads");
        assert_eq!(bytes.len(), 1024);
    }

    #[test]
    fn the_default_caps_are_the_documented_ones() {
        // Spec §3.4. These are a security control; a silent edit should fail a test.
        assert_eq!(Caps::DEFAULT.max_decompressed, 2 << 30);
        assert_eq!(Caps::DEFAULT.max_entries, 1024);
        assert_eq!(Caps::DEFAULT.max_ratio, 200);
        assert_eq!(Caps::DEFAULT.max_triangles, 8_000_000);
    }

    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    /// Builds a ZIP in memory. `(name, contents)` pairs, deflated.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            w.start_file(*name, opts).expect("start");
            w.write_all(body).expect("write");
        }
        w.finish().expect("finish").into_inner()
    }

    fn tiny_caps() -> Caps {
        Caps {
            max_decompressed: 4096,
            max_entries: 4,
            max_ratio: 20,
            // Irrelevant to what these tests exercise -- they never reach `emit` -- so
            // left wide open rather than picking a number that would look meaningful.
            max_triangles: usize::MAX,
        }
    }

    #[test]
    fn too_many_entries_is_refused_before_anything_is_read() {
        let many: Vec<(String, Vec<u8>)> = (0..9)
            .map(|i| (format!("f{i}.txt"), b"x".to_vec()))
            .collect();
        let refs: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let err = open_archive(&zip_of(&refs), &tiny_caps()).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn an_entry_past_the_size_cap_is_refused() {
        // `vec![b'A'; 8192]` deflates to about 26 bytes, so
        // `caps.max_decompressed.min(compressed * caps.max_ratio)` computes
        // `min(4096, 520) = 520` -- the RATIO bound fires, and `max_decompressed` is
        // never exercised. Deflate cannot compress a good pseudo-random stream, which
        // pins the absolute cap as the one under test. (Not a dependency: xorshift32,
        // a small deterministic PRNG -- a multiplicative hash of the index was tried
        // first and still deflated to 952 bytes, well past what the ratio cap allows.)
        let mut state: u32 = 0x2545_f491;
        let incompressible: Vec<u8> = (0..8192)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let bytes = zip_of(&[("3D/3dmodel.model", &incompressible)]);
        let mut a = open_archive(&bytes, &tiny_caps()).expect("opens");
        let compressed = a
            .by_name("3D/3dmodel.model")
            .expect("entry exists")
            .compressed_size();
        assert!(
            compressed > 8000,
            "compressed size {compressed} is not close to the original 8192 bytes -- the \
             fixture is not incompressible enough for the absolute cap to be the tighter \
             bound"
        );
        let err = entry(&mut a, "3D/3dmodel.model", &tiny_caps()).expect_err("must refuse");
        let CadError::ArchiveRefused { detail, .. } = &err else {
            panic!("wrong variant: {err}")
        };
        assert!(
            detail.contains("4096-byte limit"),
            "expected the absolute cap's message, got: {detail}"
        );
    }

    #[test]
    fn an_entry_past_the_ratio_cap_is_refused() {
        // 4000 zero bytes deflate to far less than 4000/20, so this breaches the ratio
        // while staying inside max_decompressed -- the two caps are independent and this
        // proves the ratio one fires on its own.
        let squishy = vec![0u8; 4000];
        let bytes = zip_of(&[("3D/3dmodel.model", &squishy)]);
        let caps = Caps {
            max_decompressed: 1 << 20,
            max_entries: 4,
            max_ratio: 20,
            max_triangles: usize::MAX,
        };
        let mut a = open_archive(&bytes, &caps).expect("opens");
        let err = entry(&mut a, "3D/3dmodel.model", &caps).expect_err("must refuse");
        let CadError::ArchiveRefused { detail, .. } = &err else {
            panic!("wrong variant: {err}")
        };
        assert!(
            detail.contains("20:1"),
            "expected the ratio cap's message, got: {detail}"
        );
    }

    #[test]
    fn a_traversing_entry_name_is_rejected() {
        // Defence in depth: nothing here extracts to disk, so this is not a live vector
        // in this design. See spec §3.5 -- the rule should not depend on that staying so.
        let bytes = zip_of(&[("../../etc/passwd", b"root:x:0:0")]);
        let err = open_archive(&bytes, &Caps::DEFAULT).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn a_backslash_delimited_traversal_is_rejected() {
        // On Unix, Path::components() only splits on '/', so "..\..\etc\passwd" parses as
        // one ordinary component and never yields a ParentDir -- the `..` check alone
        // misses it. The ZIP spec (APPNOTE 4.4.17) says entry names use forward slashes
        // only, so any backslash is malformed regardless of what it is trying to do.
        let bytes = zip_of(&[("..\\..\\etc\\passwd", b"root:x:0:0")]);
        let err = open_archive(&bytes, &Caps::DEFAULT).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn the_model_part_is_found_through_the_relationships() {
        // Deliberately NOT the conventional 3D/3dmodel.model path: reading that directly
        // would pass a test that used it, and fail on a legal file that moved it.
        let rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/carrier.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
        assert_eq!(model_part_name(rels).expect("resolves"), "3D/carrier.model");
    }

    #[test]
    fn a_package_with_no_model_relationship_says_so() {
        let rels = br#"<Relationships><Relationship Id="r" Target="/docProps/thumbnail.png" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"/></Relationships>"#;
        let err = model_part_name(rels).expect_err("must fail");
        assert!(matches!(err, CadError::MalformedMesh { .. }), "{err}");
    }

    #[test]
    fn every_unit_scales_to_millimetres() {
        assert_eq!(unit_scale(Some("millimeter")).expect("mm"), 1.0);
        assert_eq!(unit_scale(Some("micron")).expect("um"), 0.001);
        assert_eq!(unit_scale(Some("centimeter")).expect("cm"), 10.0);
        assert_eq!(unit_scale(Some("inch")).expect("in"), 25.4);
        assert_eq!(unit_scale(Some("foot")).expect("ft"), 304.8);
        assert_eq!(unit_scale(Some("meter")).expect("m"), 1000.0);
    }

    #[test]
    fn an_absent_unit_is_millimetres() {
        // The 3MF core specification's default. Not a guess.
        assert_eq!(unit_scale(None).expect("default"), 1.0);
    }

    #[test]
    fn an_unrecognised_unit_is_refused_and_named() {
        // Defaulting here would scale an `inch` file by 25.4 and produce measurements
        // that are wrong, plausible and silent. CLAUDE.md: measurement must not lie.
        let err = unit_scale(Some("furlong")).expect_err("must fail");
        assert!(err.to_string().contains("furlong"), "{err}");
    }

    /// A minimal but real OPC package around one model part, at a deliberately
    /// unconventional path so every test also exercises §3.6's relationship lookup.
    fn package(model_xml: &str) -> Vec<u8> {
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/carrier.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
        zip_of(&[
            ("_rels/.rels", rels),
            ("3D/carrier.model", model_xml.as_bytes()),
        ])
    }

    #[test]
    fn a_single_object_parses_with_its_unit_applied() {
        let mesh = parse_3mf(&package(r#"<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="1" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="2" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles>
</mesh></object></resources>
<build><item objectid="1"/></build>
</model>"#)).expect("parses");
        // 1 cm -> 10 mm, 2 cm -> 20 mm. Asserting coordinates, not just that it parsed:
        // a parser that ignored the unit would still return one triangle.
        assert_eq!(
            mesh.triangles,
            vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 20.0, 0.0]]]
        );
    }

    #[test]
    fn a_model_with_no_triangles_fails_through_the_shared_gate() {
        let err = parse_3mf(&package(
            r#"<model unit="millimeter"><resources><object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/></vertices><triangles/></mesh></object></resources>
<build><item objectid="1"/></build></model>"#,
        ))
        .expect_err("must fail");
        let CadError::MalformedMesh { format, detail } = err else {
            panic!("wrong variant")
        };
        assert_eq!(format, "3MF");
        assert!(detail.contains("no triangles"), "{detail}");
    }

    #[test]
    fn a_triangle_naming_a_missing_vertex_is_rejected() {
        let err = parse_3mf(&package(
            r#"<model><resources><object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/></vertices>
<triangles><triangle v1="0" v2="7" v3="9"/></triangles></mesh></object></resources>
<build><item objectid="1"/></build></model>"#,
        ))
        .expect_err("must fail");
        assert!(err.to_string().contains("vertex"), "{err}");
    }

    #[test]
    fn a_build_items_transform_moves_the_geometry() {
        // Translation only: (10, 20, 30). The triangle is at the origin, so every
        // coordinate must shift by exactly that.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 20 30"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[10.0, 20.0, 30.0], [11.0, 20.0, 30.0], [10.0, 21.0, 30.0]]]
        );
    }

    #[test]
    fn two_build_items_of_one_object_become_one_merged_mesh() {
        // Spec §3.1: one file is one part. Two placements of the same object produce two
        // triangles in one mesh, at different positions.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build>
<item objectid="1"/>
<item objectid="1" transform="1 0 0 0 1 0 0 0 1 100 0 0"/>
</build></model>"#)).expect("parses");
        assert_eq!(mesh.triangles.len(), 2);
        assert_eq!(mesh.triangles[1][0], [100.0, 0.0, 0.0]);
    }

    #[test]
    fn a_component_composes_its_transform_with_the_items() {
        // Object 2 holds object 1 shifted by x+5; the build item shifts object 2 by
        // x+100. The composed result is x+105 -- a parser that applied only one of the
        // two transforms would land on 5 or 100 and this asserts the composition.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
<object id="2"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object>
</resources>
<build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 100 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(mesh.triangles[0][0], [105.0, 0.0, 0.0]);
    }

    #[test]
    fn a_translation_is_scaled_by_the_unit_too() {
        // The ONE test that distinguishes transform-then-scale from scale-then-transform.
        // A pure scale matrix commutes with the unit scalar and a translation in a
        // millimetre file has scale 1, so neither of the other transform tests can tell
        // the two orderings apart -- both give the same answer. A translation in a
        // centimetre file cannot: correct is (v + t) * 10, wrong is v * 10 + t.
        let mesh = parse_3mf(&package(r#"<model unit="centimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[100.0, 0.0, 0.0], [110.0, 0.0, 0.0], [100.0, 10.0, 0.0]]],
            "scaling before transforming would give 10/20/10 -- the translation must be \
             scaled with the geometry, because it is expressed in the same units"
        );
    }

    #[test]
    fn a_components_rotation_composes_in_the_right_order() {
        // The 3x3 half of composition, which
        // `a_component_composes_its_transform_with_the_items` cannot pin: both of its
        // transforms have identity 3x3 blocks, so a transposed product gives the same
        // answer. Here the component rotates 90 degrees about z and the build item scales
        // x by two. Rotating first sends (1,0,0) to (0,1,0), which the scale leaves alone;
        // the other order gives (0,2,0).
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="1" y="0" z="0"/><vertex x="0" y="0" z="0"/><vertex x="0" y="0" z="1"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
<object id="2"><components><component objectid="1" transform="0 1 0 -1 0 0 0 0 1 0 0 0"/></components></object>
</resources>
<build><item objectid="2" transform="2 0 0 0 1 0 0 0 1 0 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles[0][0],
            [0.0, 1.0, 0.0],
            "the component's rotation must apply before the build item's scale"
        );
    }

    #[test]
    fn a_transform_with_too_many_numbers_is_rejected() {
        // `zip` stops at the shorter side, so counting inside the loop accepts thirteen
        // numbers by truncating to twelve while correctly rejecting eleven.
        let err = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 0 0 0 99"/></build></model>"#))
            .expect_err("must fail");
        assert!(err.to_string().contains("13 numbers"), "{err}");
    }

    #[test]
    fn a_component_cycle_terminates_instead_of_hanging() {
        // Object 1 contains object 2 contains object 1. Without a depth cap this
        // recurses until the stack dies.
        let err = parse_3mf(&package(
            r#"<model unit="millimeter"><resources>
<object id="1"><components><component objectid="2"/></components></object>
<object id="2"><components><component objectid="1"/></components></object>
</resources>
<build><item objectid="1"/></build></model>"#,
        ))
        .expect_err("must fail");
        assert!(err.to_string().contains("nested"), "{err}");
    }

    #[test]
    fn a_component_fan_out_bomb_is_refused() {
        // `MAX_DEPTH` bounds recursion depth, not breadth: an object can hold many
        // <components>, each one recursing, so emitted triangles grow as branching^depth
        // rather than depth alone. Branching factor 4 over 8 levels is 4^8 = 65,536
        // triangles (2.2 MiB) -- enough to prove the budget fires, small and fast enough
        // to build safely. Do NOT raise this branching factor: higher ones are exactly
        // how this machine has already OOM-killed itself twice this session (8^8 is 16.7
        // million triangles; 12^8 is roughly 155 GB).
        const BRANCHING: usize = 4;
        const LEVELS: usize = 8;
        let mut resources = String::from(
            r#"<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#,
        );
        for level in 2..=(LEVELS + 1) {
            let components: String = (0..BRANCHING)
                .map(|_| format!(r#"<component objectid="{}"/>"#, level - 1))
                .collect();
            resources.push_str(&format!(
                r#"<object id="{level}"><components>{components}</components></object>"#
            ));
        }
        let top = LEVELS + 1;
        let model = format!(
            r#"<model unit="millimeter"><resources>{resources}</resources>
<build><item objectid="{top}"/></build></model>"#
        );

        // A small injected budget: the test proves the mechanism without needing a
        // package that actually reaches the real 8,000,000-triangle default.
        let caps = Caps {
            max_triangles: 1000,
            ..Caps::DEFAULT
        };
        let bytes = package(&model);
        let mut archive = open_archive(&bytes, &caps).expect("opens");
        let rels = entry(&mut archive, "_rels/.rels", &caps).expect("reads rels");
        let model_name = model_part_name(&rels).expect("resolves model part");
        drop(rels);
        let model_bytes = entry(&mut archive, &model_name, &caps).expect("reads model");
        let (objects, build, scale) = read_model(&model_bytes).expect("model parses");

        let mut triangles = Vec::new();
        let err = build
            .iter()
            .try_for_each(|(id, transform)| {
                emit(&objects, id, *transform, scale, 0, &caps, &mut triangles)
            })
            .expect_err("must refuse");
        let CadError::ArchiveRefused { detail, .. } = &err else {
            panic!("wrong variant: {err}")
        };
        assert!(detail.contains("1000"), "{detail}");
        assert!(
            triangles.len() <= 1000,
            "the budget must stop growth as it accumulates, not after emitting the whole \
             bomb: got {} triangles",
            triangles.len()
        );
    }

    #[test]
    fn a_scale_in_the_transform_is_applied_with_the_unit() {
        // 2x scale in a centimetre file: 1 -> 2 cm -> 20 mm. Order matters, and getting
        // it backwards still produces a plausible number, so this pins it.
        let mesh = parse_3mf(&package(r#"<model unit="centimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="2 0 0 0 2 0 0 0 2 0 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(mesh.triangles[0][1], [20.0, 0.0, 0.0]);
    }

    #[test]
    fn the_real_fixture_parses_with_both_of_its_placements() {
        let bytes = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");
        let mesh = parse_3mf(bytes).expect("the fixture parses");
        // Two placements of one object, so the second half must be the first half moved
        // by exactly x+60. This is the assertion that fails if the merge drops an item,
        // applies the wrong transform, or emits the same placement twice.
        assert_eq!(
            mesh.triangles.len() % 2,
            0,
            "two placements, so an even count"
        );
        let half = mesh.triangles.len() / 2;
        for (near, far) in mesh.triangles[..half].iter().zip(&mesh.triangles[half..]) {
            for (a, b) in near.iter().zip(far) {
                assert!((b[0] - a[0] - 60.0).abs() < 0.001, "{a:?} vs {b:?}");
                assert!((b[1] - a[1]).abs() < 0.001, "{a:?} vs {b:?}");
                assert!((b[2] - a[2]).abs() < 0.001, "{a:?} vs {b:?}");
            }
        }
        // The second placement is offset by x+60, so the overall bounding box is wider
        // than one carrier: 48 mm for one, 108 mm for two.
        let xs: Vec<f32> = mesh.triangles.iter().flatten().map(|v| v[0]).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((width - 108.0).abs() < 0.01, "width {width}");
    }
}
