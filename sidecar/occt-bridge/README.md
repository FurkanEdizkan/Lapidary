# occt-bridge

The C++ sidecar wrapping Open CASCADE (OCCT): STEP and IGES reading, tessellation, analytic
B-rep entities for measurement, and format conversion. A separate process rather than a
linked library, so an OCCT crash takes down one job instead of the worker.

**Status: Phase 0b, step 2.4.** Four commands — `version`, `selftest`, `convert` and
`generate-fixtures` — driven from Rust by `OcctKernel` in `crates/lapidary-cad`, behind the
`occt-kernel` feature. Its unit tests drive a fake bridge, a shell script, so they run
everywhere. `cargo xtask verify occt` builds the `occt-test` stage of `deploy/Containerfile`,
which runs `crates/lapidary-cad/tests/occt_bridge.rs` against this bridge and real OCCT and
times the Phase 0 exit. Ingest sends STEP and IGES files here, and stores the tree, the entities
and the header beside the part (Phase 2).

## `convert`

```
occt-bridge convert --in <file> --format step|iges --out <dir> [--deflection <mm>]
```

Writes five files into `<dir>` and prints a one-line JSON summary on stdout
(`{"parts":200,"prototypes":8,"solids":200,"triangles":28576}`):

| File | What it holds |
|---|---|
| `mesh.stl` | Every placed part triangulated in world coordinates, as binary STL. The worker's existing mesh pipeline — clustering into LOD rungs, the thumbnail, the GLB writer — reads it, so this program does not grow a second one. |
| `structure.json` | The assembly tree: names, a prototype id per node, and each node's 4×4 transform relative to its parent. `parts` counts the leaves. |
| `entities.json` | Analytic faces (plane, cylinder, cone, sphere, torus) and circular edges, **once per prototype**, in that prototype's own coordinates. `structure.json` places them. Two hundred instances of eight parts would otherwise repeat the same geometry two hundred times. |
| `measurements.json` | Volume, surface area and bounding box from the B-rep — not the mesh — in millimetres. `volume_mm3` is `null` when nothing in the file is a solid. |
| `header.json` | What the file says about itself: the STEP header (file name, time stamp, authors, organizations, originating system, preprocessor, descriptions, schemas) or the IGES global section, and the names of the materials XCAF reads. Empty fields are `null` or `[]`; nothing is inferred. |

**Units come from the file.** The document is set to millimetres before transfer, and the
readers scale into it, so a part written in inches arrives converted.

**stdout is the answer.** OCCT's default messenger prints transfer statistics to stdout, in
colour; the bridge removes that printer before doing anything else.

**Refusals and crashes are different exits.** A file OCCT cannot read, or an exception OCCT
raises while reading it, exits 2 with `{"kind":"refused","detail":...}` on stderr — another
attempt reads the same bytes and fails the same way. Any other non-zero exit, or a signal, is a
crash.

## Fixtures

`generate-fixtures <dir>` writes the files in `fixtures/step/`. They are generated rather than
downloaded, so they are licence-clean by construction, and the generator is the record of what
is in them. Regenerating changes their bytes — STEP headers carry a timestamp — but not their
geometry.

| File | What it is |
|---|---|
| `fixture-plate-assembly-lp-9000-00.step` | The Phase 0 exit fixture: a welding fixture of **200 placed parts** from 8 prototypes through three levels of assembly — plate, 4 levelling feet, 12 bracket stations of 12 parts each, a rail of 6 V-blocks and a rack of 45 stop pins. AP242. |
| `cylinder-d22-lp-9010-00.step` | A 22.000 mm cylinder, 30 mm long — the one Phase 3's exit measures. |
| `cylinder-d22-inch-units-lp-9011-00.step` | The same cylinder, written in inches, to prove units are read from the file. |
| `angle-bracket-60x60x40-lp-9004-00.igs` | One part as IGES. |

## How it is built

The `occt` stage of `deploy/Containerfile` builds OCCT, then this directory against it.

**OCCT 8.0.1, pinned twice.** Tag `V8.0.1` (released 2026-07-30) and the SHA256 of GitHub's
archive for that tag, `6297cc55a1720523a437c54d07dc16b9da8c5f5ab5800da3c6ebded568d53c18`. A
tag is a name the host can serve different bytes under; the digest is what makes the build
reproducible. Bump both together, from the release's own archive.

**Shared libraries.** OCCT is LGPL-2.1, and `ARCHITECTURE.md` requires its output to stay a
separately replaceable set of files. `BUILD_LIBRARY_TYPE` already defaults to `Shared`.

**Every module except Draw.** Cutting Visualization was the obvious saving, and it is not
available: in 8.0.1 `TKDESTEP` and `TKDEIGES` link `TKXCAF`, and `TKXCAF` links `TKService`,
`TKV3d` and `TKVCAF` (read from each toolkit's `EXTERNLIB.cmake`). What can be cut is
Visualization's third-party half, which a headless worker never uses:

| Flag | OCCT's default on Linux | Why off |
|---|---|---|
| `BUILD_MODULE_Draw` | on | the Tcl test harness; nothing here runs it |
| `USE_TK` | on | Draw's GUI |
| `USE_FREETYPE` | on | text rendering in a viewer |
| `USE_OPENGL`, `USE_GLES2` | on / off | no window, no GPU |
| `USE_XLIB` | on | no X server in a container |

FreeImage, FFmpeg, OpenVR, VTK, TBB, RapidJSON, Draco and Eigen already default off.

**The link line is the call list.** `CMakeLists.txt` names toolkits one by one — STEP and IGES
through XCAF, primitives, meshing, mass properties — rather than whole modules, so it reads
as what the bridge actually uses.

**It proves itself at build time.** The stage ends by running `occt-bridge version` and
`occt-bridge selftest /tmp`: a 22 mm cylinder written as STEP through XCAF, read back, its
volume checked against πr²h, and meshed. A toolkit missing from the link or from the runtime
fails the image build rather than a user's first ingest.

## Measured, 2026-09-13

On the development machine (12 cores, 15 GB RAM), building the `occt` stage from nothing:

| Step | Result |
|---|---|
| OCCT configure, compile and install (`--parallel 6`) | **758.6 s** |
| Installed shared libraries, `/opt/occt/lib` | **74 MB** — 49 libraries, 147 files with their version symlinks |
| Headers, `/opt/occt/include/opencascade` | 42 MB, build stage only; the worker image does not carry them |
| `occt` stage image (compilers, CMake, source tree removed) | 1.14 GB, never shipped |
| Bridge rebuild with OCCT cached | 18 s |
| `api` target image | **152MB** — no OCCT: no `/opt/occt`, no `libTK*` anywhere, no bridge |
| `worker` target image | **261MB** — `selftest` passes inside it as `lapidary`, every library resolved |

`occt-bridge version` printed `occt 8.0.1 bridge 0`, and `selftest` printed
`roots=1 volume=11403.981333 expected=11403.981333 meshed=yes`.

Two things the first build taught, both recorded where they bite:

- **OCCT installs its Unix layout on Linux** — `lib/`, `include/opencascade/`, `lib/cmake/` —
  even though its build tree links into `lin64/gcc/lib/`. The worker target copies
  `/opt/occt/lib`.
- **Nothing tells the loader where that is.** The bridge linked and then failed to start with
  `libTKDE.so.8.0: cannot open shared object file`. Both the `occt` stage and the worker target
  set `LD_LIBRARY_PATH=/opt/occt/lib`; `ldconfig` is not an option for a worker that runs as
  `lapidary`.

### `convert`, measured

Inside the `occt` stage image, deflection 0.1 mm, process start included:

| Fixture | Parts | Solids | Triangles | Time | Checked |
|---|---|---|---|---|---|
| `fixture-plate-assembly-lp-9000-00.step` (190 KB) | 200 | 200 | 28,576 | **91 ms** | 8 prototypes; bounding box 315 × 315 × 120 mm |
| `cylinder-d22-lp-9010-00.step` | 1 | 1 | 128 | 21 ms | volume πr²h, box 22 × 22 × 30, one cylinder face of radius 11 |
| `cylinder-d22-inch-units-lp-9011-00.step` | 1 | 1 | 128 | 20 ms | the same volume to ~1e-12, radius 11.0000000000068 — read back in mm |
| `angle-bracket-60x60x40-lp-9004-00.igs` | 1 | **0** | 28 | 20 ms | box 60 × 40 × 60; no volume, see below |

That 91 ms is the bridge alone. **The Phase 0 exit, end to end through `OcctKernel` — bridge,
mesh pipeline, GLB rung and thumbnail — measured 111 ms** for the 200-part assembly, in a release
build inside the `occt-test` stage (`cargo xtask verify occt`); `docs/ROADMAP.md` records it, and
what that number does not say about real files.

**IGES arrives as faces, not solids.** OCCT's IGES writer stores trimmed surfaces by default,
and most CAD tools' IGES files are the same shape, so `volume_mm3` is `null` rather than a
number integrated over surfaces that do not close. Sewing faces into solids
(`BRepBuilderAPI_Sewing`) is the follow-up that gives IGES a volume; it is not done yet.

## Kernel version

`occt-bridge version` prints `occt <OCCT version> bridge <BRIDGE_VERSION>` — `occt 8.0.1 bridge 1` today. Different OCCT
builds tessellate identical input differently, so this string is what the worker fleet pins
(`ARCHITECTURE.md`); bump `BRIDGE_VERSION` in `src/main.cpp` whenever the bridge changes what
it writes.
