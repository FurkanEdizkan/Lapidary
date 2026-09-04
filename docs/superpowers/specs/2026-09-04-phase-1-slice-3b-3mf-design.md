# Phase 1 slice 3b — 3MF ingest

**Status:** design. Execution follows the plan built from this document.

3MF is the last of the three mesh formats `ROADMAP.md`'s Phase 1 line commits to
("Mesh ingest (STL/3MF/OBJ) → thumbnail + L0/L1/L2"). STL and OBJ landed in slices 1 and
3. This slice adds the third and closes that line.

---

## 1. Why this slice exists

3MF was split out of slice 3 rather than shipped with OBJ, and the reason is recorded in
slice 3's spec §2: it "needs the workspace's first ZIP and first XML dependencies plus
`DATA.md` §5.4's zip-bomb caps — a different review problem from geometry, and the only
part of the roadmap's mesh-format line that is not dependency-free".

Two formats with opposite risk profiles under one heading is what makes a slice overrun.
OBJ is plain UTF-8 that hand-rolls in the style `stl.rs` already justifies; a bug there
produces a wrong mesh. 3MF is a compressed container parsed from untrusted bytes; a bug
there is a denial of service. They deserve separate reviews, and this is the second one.

Everything this slice needs from slice 3 already exists. `mesh_kernel.rs:17`'s `parse`
dispatches on format, `kernel.rs:78`'s `MalformedMesh { format, detail }` carries the
format in its message, `mesh_kernel.rs:37`'s `version` names the parser, and
`stl.rs:232`'s `finish` is the shared gate for a file that parsed but yielded no
geometry. Slice 3's spec §12 said "the seam is the only thing 3MF needs that this slice
builds". This slice tests that claim.

---

## 2. Scope

**In:**

- `parse_3mf(bytes) -> Result<Mesh, CadError>` in `lapidary-cad`, behind the kernel
  boundary beside `stl.rs` and `obj.rs`
- Two dependencies: `zip` (deflate, pure Rust) and `quick-xml`
- `DATA.md` §5.4's archive caps, enforced during extraction
- Unit conversion to millimetres
- Build items and their transforms; `<components>` resolved recursively with a depth cap
- `DATA.md` §1.2's **as-is** storage policy for 3MF, which `SourceStore` does not
  currently support
- `"3mf"` added to `MESH_EXTENSIONS` (`scan.rs:95`)

**Out, with the trigger that brings each back:**

| Deferred | Trigger |
|---|---|
| Archive ingest — a `.zip`/`.tar`/`.7z` holding many models | Its own slice, next. A 3MF is one model that happens to be a ZIP and becomes one part; an archive is N models and becomes N parts, which breaks `Outcome::Ingested`, `part_name_unique_per_library`, `insert_part_chain` and the batch counts. Different pipeline, not an extension of this one |
| RAR specifically | The same slice, but it needs a licence decision first — see §11 |
| `variant=3mf` download (`DATA.md` §5.1's route list) | Phase 1 slice 4, with the rest of the download surface |
| Materials, colours, textures, print settings | Phase 3 at the earliest. Nothing renders them and nothing measures them |
| 3MF **writing** — saving back to a slicer | Phase 4's round-trip. This slice reads only |
| Beam lattice, slice, and production extensions | No consumer. The core spec's mesh is what the ladder needs |

**Explicitly not re-decided here:** the ladder, the glTF writer, the derivative schema
and `GET /api/blob/{blake3}` all shipped in slice 3 and this slice consumes them
unchanged.

---

## 3. Decisions

### 3.1 One 3MF is one part, with every build item merged

A 3MF from a slicer often holds several objects arranged on a build plate. The
alternative — one part per object — was considered and rejected: every layer below ingest
assumes one file yields one part. `Outcome::Ingested` names a single outcome,
`part_name_unique_per_library` keys on one name per file, `insert_part_chain` writes one
revision per call, and `ScanAccepted.queued` is a count of files that the batch status
then compares against parts. Changing all four is a slice of its own, and it is the same
work archive ingest needs, so it belongs there rather than here.

So: walk `<build>`, apply each `<item>`'s transform to its object's mesh, and merge into
one triangle soup. A plate holding one object — the ordinary case for a downloaded part —
is unaffected. A plate holding five becomes one card showing the plate, which is what the
file depicts.

### 3.2 `zip` and `quick-xml`, both pure Rust

This slice was split out so the dependency addition would get a review of its own. The
review:

| Crate | New crates in our lock | Licence | Why |
|---|---|---|---|
| `zip` 2, `default-features = false`, `features = ["deflate"]` | 9 in the lock, 7 that compile: `zip`, `flate2`, `miniz_oxide`, `adler2`, `simd-adler32`, `crc32fast`, `zopfli` — plus `arbitrary` and `derive_arbitrary`, which zip declares under `[target."cfg(fuzzing)".dependencies]` and a normal build never compiles | MIT (deps MIT/Apache-2.0/Zlib/0BSD) | ZIP is a security-sensitive container — zip64, data descriptors, local-versus-central header mismatch, encryption flags. The hand-rolled parsers in this crate are *geometry* parsers, where a bug is a wrong mesh; a hand-rolled archive reader's bugs are vulnerabilities |
| `quick-xml` 0.41 | 1: `quick-xml` (`memchr` already present) | MIT | Streaming pull parser. A 3MF's model XML is the mesh in text form and can reach hundreds of megabytes; a DOM parser such as `roxmltree` would hold all of it at once |

Both are pure Rust, so no C toolchain enters the worker image and `cargo vendor` still
builds offline — the same constraint that decided slice 3 §3.2 against meshopt. Every
licence is permissive and compatible with AGPL-3.0-only.

**`quick-xml` is pinned at 0.41, and that is a security floor rather than a
preference.** 0.37 carries RUSTSEC-2026-0194 (quadratic time checking a start tag for
duplicate attribute names) and RUSTSEC-2026-0195 (unbounded namespace-declaration
allocation in `NsReader`, a memory-exhaustion denial of service). Both are exactly the
threat this slice's caps exist to stop, on the one component that reads attacker-controlled
XML — shipping them would undercut §3.4 entirely. `cargo deny check` fails on both, which is
how they were found.

Two costs are accepted knowingly. `zopfli` is a *compressor* we never call: it arrives
because zip 2.4.2's `deflate-flate2` feature is broken — it gates code that needs
`flate2` without enabling the optional dependency, so it does not compile — leaving
`deflate` as the only working pure-Rust path. And seven new crates is the largest single
dependency addition this workspace has made. Hand-rolling the container would have cost
five new crates and about 250 lines of ours; two crates is not worth owning zip64 and
header-mismatch handling in the one part of this slice where a mistake is exploitable.

`ARCHITECTURE.md:159` lists `async-zip` under Archives. That is not contradicted here: it
is for *writing* streaming export bundles in Phase 5, STORE not DEFLATE, and nothing in
this slice writes an archive.

### 3.3 Units are converted, and an unknown unit is refused

`<model unit="...">` is one of `micron`, `millimeter`, `centimeter`, `inch`, `foot`,
`meter`. This is the first ingest path where the source states a unit at all — STL and OBJ
carry none and are assumed millimetres.

`CLAUDE.md` makes measurement a product rule: measurement must not lie. So coordinates are
scaled to millimetres on read, and every downstream number — bounding box, surface area,
volume, the LOD grids — is in the unit the rest of the system already assumes.

An **absent** `unit` attribute defaults to `millimeter`, which is what the 3MF core
specification says. An **unrecognised** value is refused rather than defaulted: guessing
millimetres for a file that said `inch` scales the part by 25.4 and produces measurements
that are wrong, plausible, and silent. A refusal is a message someone can act on.

### 3.4 The caps are a struct, not bare constants

`DATA.md` §5.4 requires capping decompressed size, entry count and compression ratio, and
aborting "on breach, not after". The values:

| Cap | Value | Sized against |
|---|---|---|
| `max_decompressed` | 2 GiB | `DATA.md:16` puts source files at 1 MB – 2 GB, so this admits the largest legitimate file the system claims to handle |
| `max_entries` | 1024 | A real 3MF has roughly 5–20 entries |
| `max_ratio` | 200:1 | Mesh XML deflates around 10–20×. A bomb is 1000× and up |

They are compiled in and not configurable. A security control with an environment
override is a security control an operator can switch off by accident, and nothing about
an air-gapped deployment makes these numbers wrong.

They live in a `Caps` struct with a `Caps::DEFAULT` holding the values above, for one
practical reason: a test that proves the 2 GiB cap fires would otherwise need a
multi-gigabyte fixture in the repository. With injectable caps the hostile fixture is a
few hundred bytes and the test asserts the mechanism, which is the part that can break.

Breach is checked **as bytes arrive**, through `Read::take` on each entry. Inflating an
entry and then measuring it is the bomb working exactly as designed.

### 3.5 Path traversal is checked, and this spec says why it is not the live risk

`DATA.md` §5.4 also requires rejecting absolute paths and `..` segments in entry names.
That rule exists for code that extracts an archive to disk, where a crafted name writes
outside the extraction directory.

This slice never extracts to disk. It opens the archive from a byte slice already in
memory, reads two entries by name, and hands the bytes to a parser — there is no
extraction directory to escape. The check is implemented anyway, because the rule should
not depend on an implementation detail continuing to hold, and it costs three lines.

The spec records the distinction so that a later reader does not conclude the codebase is
protected against a traversal it has never actually been exposed to. When something here
does extract to disk, that will be the moment the check starts earning its place.

### 3.6 The model part is found through the relationships, not by convention

`3D/3dmodel.model` is where the model almost always sits, and reading that path directly
would work for almost every file. Instead, `_rels/.rels` is parsed and the StartPart
relationship followed to whatever part it names.

The cost is one more small XML parse. The benefit is that a legal 3MF whose model lives
elsewhere ingests rather than failing with a confusing "no model found", and the failure
that remains — a package with no StartPart relationship — is a genuinely malformed
package, which is a message worth giving.

### 3.7 `SourceStore` learns a compression policy

`DATA.md:40` says 3MF is stored **as-is**, because it is already a deflate ZIP.
`SourceStore::put` compresses unconditionally (`lapidary-storage/src/lib.rs:241`,
`write_blob(&self.root, bytes, true)`), so it cannot express that today. Re-compressing a
ZIP with zstd spends CPU on every ingest to make the file very slightly larger.

`put` gains a policy argument:

```rust
pub enum Compression { Zstd, AsIs }

impl Compression {
    /// DATA.md §1.2's table. STEP, STL and OBJ compress; 3MF, PDF and images are
    /// already packed and are stored as they arrived.
    pub fn for_source_format(format: &str) -> Self { … }
}
```

The table lives in one place with its reasoning beside it, rather than as a boolean at a
call site where the next reader cannot tell what `true` meant. A 3MF then stores with
`zstd_level` unset and `stored_bytes == size_bytes` — exactly the shape derivatives
already use, so the blob row needs no schema change.

### 3.8 Component recursion is capped

An `<object>` may contain `<components>` referencing other objects, each with its own
transform, forming a tree. Resolution is therefore recursive, and a file whose objects
reference each other in a cycle is an infinite loop on input we do not control.

A depth cap ends it. This is the same class of decision as §3.4 and costs one counter.

---

## 4. Architecture

### 4.1 Where each piece lives

| Piece | Crate | Why there |
|---|---|---|
| `parse_3mf`, the archive and XML reading | `lapidary-cad` | Geometry, behind the kernel boundary, so the open path structurally cannot reach it |
| `Caps` and the breach checks | `lapidary-cad` | They guard the parse, and only the parse |
| `Compression` policy | `lapidary-storage` | It is a storage rule (`DATA.md` §1.2), and `SourceStore` is what applies it |
| `"3mf"` in `MESH_EXTENSIONS` | `lapidary-ingest` | The walk decides format, as slice 3 §3.8 settled |

No new crate, and no change to `lapidary-api` — the open path reads derivatives and never
learns that 3MF exists.

### 4.2 The ingest pipeline

Unchanged in shape. The scan admits `.3mf`, the handler derives the format from the
extension (`handler.rs:315`), and `MeshKernel::process` dispatches to `parse_3mf` instead
of `parse_stl`. Everything after the parse — measure, rasterize, cluster, three rungs,
one thumbnail — is slice 3's code operating on a `Mesh` it cannot distinguish from an
STL's.

The one new step is at storage: the handler passes
`Compression::for_source_format(&params.format)` to `SourceStore::put`.

---

## 5. Data flow

```
bytes (already read and hashed by the handler)
  │
  ├─ ZipArchive::new                     ← entry count cap
  │
  ├─ read "_rels/.rels"                  ← §3.6, names the model part
  │    └─ quick-xml → StartPart target
  │
  ├─ read that entry, through Read::take ← §3.4, size and ratio caps
  │
  └─ quick-xml, streaming
       ├─ <model unit>                   ← §3.3, scale factor to mm
       ├─ <resources>
       │    └─ <object id>
       │         ├─ <mesh><vertices><triangles>
       │         └─ <components>         ← §3.8, recursive with depth cap
       └─ <build>
            └─ <item objectid transform> ← §3.1, merged
                    │
                    ▼
                  Mesh  →  finish("3MF", …)   ← stl.rs:232, the shared empty gate
                    │
                    ▼
      measure · render_thumbnail · ladder      ← slice 3, unchanged
```

---

## 6. Schema

**No migration.** Three rungs and one thumbnail per revision is slice 3's shape and 3MF
produces exactly that. `file.format` already carries an arbitrary string (slice 3 task 8
replaced the `'stl'` literal), so `'3mf'` needs no vocabulary change. `blob.zstd_level`
is already nullable.

This slice adds no column, no table and no constraint — which is the evidence that slice
3's seam was cut in the right place.

---

## 7. Domain types

```rust
// lapidary-cad/src/tmf.rs
pub fn parse_3mf(bytes: &[u8]) -> Result<Mesh, CadError>;

pub(crate) struct Caps {
    max_decompressed: u64,
    max_entries: usize,
    max_ratio: u64,
}
impl Caps { pub(crate) const DEFAULT: Caps = …; }

/// micron | millimeter | centimeter | inch | foot | meter → scale to mm
fn unit_scale(unit: Option<&str>) -> Result<f64, CadError>;

// lapidary-storage
pub enum Compression { Zstd, AsIs }
impl Compression { pub fn for_source_format(format: &str) -> Self; }
```

`Mesh` is unchanged, which is the point: everything downstream already handles it.

---

## 8. Error handling

| Condition | Error | Why it reads this way |
|---|---|---|
| Not a ZIP, no StartPart, malformed XML, bad vertex index, no triangles | `MalformedMesh { format: "3MF", detail }` | Slice 3's variant, reused. The advice — re-export from your tool — is right for all of these |
| Any cap breached | `ArchiveRefused { format, detail }` (new) | A bomb may be perfectly well-formed. "Re-export it" is wrong advice for a file refused on size, and an operator needs to know a limit fired rather than a parser failed |
| Unrecognised `unit` | `MalformedMesh` with the unit named | §3.3. The message says which unit was seen and which are understood |

`ArchiveRefused` is the only addition to `CadError`. Both map to `HandlerError::Permanent`
at the ingest boundary: the bytes are immutable, so neither answer changes on a retry.

---

## 9. Testing

Unit, in `tmf.rs`:

- each of the six units converts, and an absent `unit` is millimetres
- an unrecognised unit is refused, and the message names it
- a build item's transform actually moves geometry
- nested `<components>` resolve, with their transforms composed
- a component cycle terminates at the depth cap rather than hanging
- each of the three caps breaches independently, with `Caps` injected small
- an entry named `../../etc/passwd` is rejected
- `.rels` pointing somewhere other than `3D/3dmodel.model` still resolves
- a file with no triangles fails through `finish`, identically to STL and OBJ
- the real fixture parses with its real triangle count

In `lapidary-storage`:

- `Compression::for_source_format` returns `AsIs` for 3MF and `Zstd` for STL and OBJ
- an `AsIs` blob round-trips byte-identically and records `stored_bytes == size_bytes`

End to end, in `lapidary-ingest`:

- a 3MF ingests to four derivative rows and `kernel_version` reads
  `mesh 3mf-1+glb-1+cpu-1`
- its `file.format` is `3mf` and its source blob is stored uncompressed

**The fixture** is generated, not downloaded: a real part with plausible geometry and a
plausible number, as `CLAUDE.md` requires, written by a script the way
`example/parts/generate.py` and slice 3's OBJ fixture were. It must contain more than one
build item so §3.1's merge is exercised by the end-to-end tests and not only by unit
tests.

---

## 10. Exit criterion

A directory holding the repository's STL and OBJ fixtures plus a 3MF scans, and every
part gets one thumbnail and three tessellation rungs regardless of source format. The
3MF's `file.format` is `3mf`, its `kernel_version` is `mesh 3mf-1+glb-1+cpu-1`, and its
source blob is stored uncompressed with `stored_bytes == size_bytes`.

A 3MF whose declared expansion exceeds any cap is refused with `ArchiveRefused`, recorded
as a per-file `Permanent` failure, and leaves no part and no orphaned blob.

A multi-item build plate ingests as one part whose triangle count is the sum of its
items, and whose bounding box reflects their transforms rather than their untransformed
positions.

Throughput on the live stack stays within 3× of slice 3's measured 89 files/s, and the
grid page stays under `DATA.md` §2.5's 80 ms warm. Measured and recorded in the handoff,
not asserted.

---

## 11. Risks

**RAR is a licence dead end, and it will be asked for.** The corpus this project is tested
against is distributed as `.rar`. The only complete decoder is the `unrar` C library,
whose licence is not OSI-approved and forbids using the sources to re-create the
compression algorithm. That collides with AGPL-3.0-only, with the `cargo-deny` licence and
`[sources]` allow-list `ARCHITECTURE.md` makes CI-enforced, and with `cargo vendor`. The
archive slice must decide explicitly: ship RAR as an optional non-distributable component,
or require the user to extract `.rar` themselves. `tar`, `gz`, `xz` and `zip` are clean;
`7z` is clean but adds LZMA. **Not a decision for this slice**, recorded here because it
shapes the next one.

**Seven new crates is the largest dependency addition this workspace has made.** Accepted
in §3.2 with reasoning. The mitigation is that all are pure Rust and permissively
licensed, so the offline-build and licence-compatibility properties hold; `cargo-deny`
enforces both in CI from the moment they land.

**`zopfli` is dead weight.** A compressor in the tree that nothing calls. If zip's feature
graph is fixed upstream, `deflate-flate2` plus an explicit `flate2` drops it — worth a
recheck at the next `zip` major, not worth a fork now.

**A 2 GiB decompressed cap still permits a 2 GiB allocation.** The caps bound the damage,
they do not make it free. This is the same exposure the existing pipeline already has —
`std::fs::read` on a 2 GB STL — so it is not new, and streaming the parse rather than
buffering it is a Phase 2 concern for every format at once, not a 3MF one.

---

## 12. What this unblocks

Phase 1's mesh-format line closes: STL, OBJ and 3MF all ingest to the same thumbnail and
the same three rungs.

The archive slice follows immediately and inherits two things from here — a working ZIP
reader with caps already sized and tested, and the proof that `Compression::AsIs` works
end to end. What it does not inherit is the one-file-one-part assumption, which is the
work that slice is actually about.

Phase 3's viewer is unaffected: it consumes rungs, and a rung from a 3MF is a rung.
