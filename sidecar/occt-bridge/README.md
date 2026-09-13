# occt-bridge

The C++ sidecar wrapping Open CASCADE (OCCT): STEP and IGES reading, tessellation, analytic
B-rep entities for measurement, and format conversion. A separate process rather than a
linked library, so an OCCT crash takes down one job instead of the worker.

**Status: Phase 0b spike.** Two commands exist — `version` and `selftest` — to prove that
OCCT builds from source in the worker image, links, and runs. `convert` and
`generate-fixtures` follow; the plan is `~/.claude/plans` item 2 and the shape is
`docs/ARCHITECTURE.md`'s kernel section.

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

## Kernel version

`occt-bridge version` prints `occt <OCCT version> bridge <BRIDGE_VERSION>`. Different OCCT
builds tessellate identical input differently, so this string is what the worker fleet pins
(`ARCHITECTURE.md`); bump `BRIDGE_VERSION` in `src/main.cpp` whenever the bridge changes what
it writes.
