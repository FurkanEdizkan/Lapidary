# Roadmap

Each phase has a hard exit criterion. Do not start the next phase until it passes.
Phases 1–4 are the product; everything after is expansion.

---

## Phase 0 — Foundation (days, not weekends)

Container-first removed the OCCT bundling gate. This is no longer a go/no-go risk.

- Cargo workspace with the crate graph from `docs/ARCHITECTURE.md`
- CI layering check: L2 crates may not depend on each other or on L3
- `Containerfile` builds OCCT from source, produces `occt-bridge`
- Compose stack up: `web`, `api`, `worker`, `db` (`postgres:18`)
- `cargo-deny` with `[sources]` allow-list, lockfiles committed, Actions pinned to SHAs
- `ts-rs` export pipeline wired into the frontend build

**Exit:** `podman compose up` on a clean machine serves a page, and `occt-bridge` converts
a 200-part STEP assembly to glTF + tree + entities in under 30 s.

**Measured 2026-09-13, both clauses pass.** Phase 0 was cut in two — 0a without OCCT, 0b with
it — and the halves passed eleven days apart.

| Clause | Measured | Verdict |
|---|---|---|
| `podman compose up` on a clean machine serves a page | Phase 0a, 8 of 8 exit criteria from a clean clone — `superpowers/plans/2026-09-01-phase-0a-verification.md` | pass |
| `occt-bridge` converts a 200-part STEP assembly to glTF + tree + entities in under 30 s | **111 ms**: `fixtures/step/fixture-plate-assembly-lp-9000-00.step` (190 KB, 200 placed parts, 8 prototypes) to an L0 GLB rung of 53.7 KB, the assembly tree, 75 entities and a thumbnail — 28,576 triangles | pass |

How it was measured: `cargo xtask verify occt`, which builds the `occt-test` stage of
`deploy/Containerfile` — OCCT 8.0.1 built from source, `occt-bridge`, and a release build of
`OcctKernel` — and runs `crates/lapidary-cad/tests/occt_bridge.rs` there. The timing runs from
the input bytes to the finished `KernelOutput`: the bridge process, its B-rep read, meshing and
JSON, then the mesh pipeline's clustering and GLB writer and the thumbnail. Kernel version
`occt-8.0.1-bridge-1+deflection-0.1+glb-1+cpu-1`, on the 12-core development machine.

**What the number does not say.** The fixture is generated, so it is licence-clean and
reproducible, and its parts are boxes, cylinders and booleans of them. A real assembly's
freeform and trimmed surfaces take OCCT far longer to read, heal and mesh, so 111 ms is where
the kernel starts on the simplest honest input, not what a user's file will take. The criterion
passes as written; the first corpus of real STEP files is the measurement worth taking next.
Same run, same stage: the 22 mm cylinder reads with its volume exact to 1e-9 and one
cylindrical face of radius 11; the same cylinder written in inches reads back in millimetres;
IGES reads with no volume claimed; half a STEP file is refused rather than crashing the kernel.

**Verify here, not later:** `pgvector` installs against `postgres:18`; the Turkish
snowball `tsvector` config is present.

---

## Phase 1 — Ingest and grid

- Blob CAS: BLAKE3, 2-level sharding, `ref_count`, zstd -3 on source
- Postgres job queue: `FOR UPDATE SKIP LOCKED` + `LISTEN/NOTIFY`, crash-resumable
- Upload: client-side WASM BLAKE3 → probe → chunked resumable transfer
- Mesh ingest (STL/3MF/OBJ) → thumbnail (per-library, on by default) + L0
<!--
  This line read "+ L0; L1/L2 on demand" until 2026-09-13, and the "on demand" half was
  never Phase 1's. Slice 4 moved it: ingest stores L0 only, and the route that would
  enqueue L1/L2 is assigned to **Phase 3's viewer** — see
  `superpowers/plans/2026-09-05-phase-1-slice-4-HANDOFF.md:255` ("the three trigger routes
  only render thumbnails, which is all any consumer needs today") and
  `superpowers/specs/2026-09-04-phase-1-slice-4-derivatives-design.md:46`. The amendment
  was recorded in `2026-09-05-phase-1-remaining-slices.md:103` and never reached this file,
  so a Phase 1 audit read it as an unbuilt row and nearly built it.

  The kernel produces all three rungs today (`lapidary-cad`); nothing asks it for the top
  two, which is why they are not written. Phase 3's prefetch line below is the consumer.
-->
- Virtualized grid, keyset pagination, inline `bytea` thumbnails
- SSE progress; UI never blocks
- Download `variant=original` with hash displayed
- First run seeds a bundled licence-clean example part — never an empty grid

**Exit:** drop a folder of 1,000 STLs, grid is interactive immediately, every part
appears — with a thumbnail where the library renders them automatically, and with "No
preview yet" plus a working `POST /api/libraries/{id}/thumbnails` where it does not —
re-dropping the same folder completes in seconds via hash short-circuit, and grid page
load is under 80 ms warm.

**Measured 2026-09-07. Three clauses passed as measured; the fourth failed, was fixed, and
passes on re-run.**

Corpus: `Bases/` from the owner's own library — **1,095 STL files, 15.67 GB**, nested two
to six directories deep, no basename collisions. A real folder rather than a generated one.
Ingested into a library of its own on the development machine (12 cores, 15 GB RAM,
PostgreSQL 18.6 in a container, worker concurrency 4).

| Clause | Measured | Verdict |
|---|---|---|
| A folder of 1,000 STLs in | **130.3 s** for 1,095 files / 15.67 GB — 8.4 files/s, ~120 MB/s | pass |
| Re-drop completes in seconds via hash short-circuit | **40.1 s**, 1,094 skipped, **0 re-ingested** | pass |
| Grid page load under 80 ms warm | **5.3 ms** median at page size 50 (p95 5.9 ms, 2.4 MB body). At 500: **54.7 ms** median, 28.8 MB body | pass |
| Every part appears | **1,094 of 1,095** at measurement; **1,095 of 1,095** after the fix below | pass, after a fix |

**The one that failed, and what it turned out to be.** `Trench battlefield-80mm(B).stl` is
635,470 triangles that parse cleanly and cluster to nothing. `MeshKernel::process` produced
each requested derivative with `?`, so a derivative that could not be made came back
indistinguishable from "this file is not readable" — the ingest job failed, **no part row
was written**, and the model was absent from the library. The criterion asks for the
opposite in as many words, and the machinery for it already existed: `auto_thumbnail =
false` leaves a part with no thumbnail and the grid renders "No preview yet". Only a
*failed* derivative was fatal where a *skipped* one was not.

Fixed in `490ba34`. Parsing stays fatal — a mesh nobody can read has no measurements and no
part to hang them on — and a derivative that cannot be made is recorded in
`KernelOutput::unproduced` with its kind and reason, which the handler logs at `warn`.
Re-run against the same file afterwards: **ingested, one part, 635,470 triangles.**

And the measurement corrected its own diagnosis. `CadError::Unrenderable` is raised by the
thumbnail rasterizer *and* the glTF writer, and its message said "Could not render a
thumbnail" for both — so this was written up as a thumbnail failure. It was the **L0
tessellation rung**. The re-run's log names `tessellation_l0`, and the part carries a
perfectly good thumbnail. The wording is about a view of the mesh now, and which view is
recorded where the caller asked for it.

Timing note: the two figures above measure the **scan** path — the worker walking a mounted
directory — rather than a browser drop, which hashes client-side and uploads. Both reach the
same ingest pipeline after the bytes are in reach; the drop adds transfer and client-side
BLAKE3 that this run did not measure. And "interactive immediately" was observed rather than
timed: the grid answered throughout the ingest, which is what the job queue is for, but no
number was taken for it.

---

## Phase 2 — CAD ingest and search

- `lapidary-cad` drives the sidecar with timeout and crash handling
- STEP + IGES; assembly tree persisted and navigable
- Metadata extraction stages 1–4, each committing independently
- `tsvector` + `pg_trgm` dual search with identifier-aware ranking
- Faceted filters with the 10k exact-count threshold
- Failed-file drawer with actionable errors

**Exit:** ingest a mixed folder of STEP and STL with no manual steps; searching a part
number like `A1234-56-B` by the fragment `1234` returns it at position one.

**Measured 2026-09-13, both clauses pass. The phase is not finished:** the facets are format,
material and tag (`0022`) — lifecycle waits for revision states, which are Phase 8's. An AP242
file's PMI is read since bridge 6 and stored as its own `pmi` derivative, not in stage 4's
`metadata_json`; Phase 5 records how it was checked. The failed files are listed inline under the progress line rather than in a drawer:
every failure, a page past the first hundred at a press, with a Retry per file and for the batch. Sorting by a measured figure shipped without an
index, for the reason `DATA.md` §3.2 records.

| Clause | Measured | Verdict |
|---|---|---|
| Ingest a mixed folder of STEP and STL with no manual steps | One scan of a folder holding `cad/` (three STEP files and one IGES, from `fixtures/step`) and `mesh/` (three STLs): **7 of 7 ingested, 0 failed, in 0.62 s** from the scan request to a finished batch. Each CAD part has its L0 rung, thumbnail, assembly tree, entities and header. The 200-part assembly's tree reads back with 200 parts and 8 prototypes, served as `application/json`. The 22 mm cylinder's volume (11,403.98 mm³), area and box are stored as exact | pass |
| Searching `A1234-56-B` by the fragment `1234` returns it at position one | Through the API: `PUT /api/parts/{id}/part-number` gave the cylinder the number `A1234-56-B`, and `GET /api/libraries/{id}/parts?q=1234` returned it first in 2.6 ms, ahead of `bracket-1234-mount.stl`, a part only named for the digits | pass |

How it was measured: `docker compose -p lapidaryphase2` built the `api` and `worker` targets of
`deploy/Containerfile` from commit `11d5f07`, with its own env file, storage and ingest
directories and database volume. `curl` drove the scan, the batch status, the detail and blob
routes and the search, and `psql` read the stored headers and derivative kinds. Worker
concurrency 2, kernel `occt-8.0.1-bridge-2+deflection-0.1+glb-1+cpu-1`, on the 12-core
development machine.

**Addendum, the same day.** The tree those 200 parts read back into was not yet navigable in the
sense the phase means: a browser check on an isolated stack found every placed part named
`=>[0:1:1:9]`, OCCT's name for an instance the file leaves unnamed. Bridge 3 falls back to the
prototype's name (`2152e3b`), and the kernel now reads
`occt-8.0.1-bridge-3+deflection-0.1+glb-1+cpu-1`. The measurement above stands as taken; the
names it did not check are what changed.

**Where the part number came from.** Nothing reads a part number out of a file. Ingest writes
none, because one made up from a filename is one nobody gave the part, and the fixtures' STEP
headers carry only OCCT's defaults. The number in the second clause was set by a request, which
is the only way one enters a library. The ranking is held in general by
`a_part_number_fragment_returns_the_part_at_position_one` in `crates/lapidary-db/tests/repo.rs`,
and through the router by `a_part_number_set_through_the_api_is_found_first_by_its_fragment`.

**What the numbers do not say.** The CAD files are small generated fixtures, so 0.62 s is the
pipeline's own overhead on honest input, not what a real assembly takes. The Phase 0 entry above
says the same of the kernel.

---

## Phase 3 — Viewer and measurement

- three.js + glTF/meshopt, LOD streaming, immutable blob caching
- Prefetch L0 on hover, L1 on inspector open, pool bounded at 2
- Measurement: point-to-point, edge, diameter, angle, wall thickness
- Snap to analytic entities; mesh values labelled approximate
- Derivative downloads with `.lapidary.` infix

**Exit:** measure a known cylinder from STEP and get the exact nominal diameter, not a
tessellated approximation. Part open to first paint under 120 ms warm.

**Measured 2026-09-13, both clauses pass. The phase is not finished:** the rungs moved to
`EXT_meshopt_compression` after this measurement, and the addendum below measures that against
these numbers. Opening the quick look prefetches its neighbours' L0, not their L1 (`routes/index.tsx` says
why). The assembly tree hides and isolates parts since bridge 5 and glb-3; the last addendum
below checks it.

| Clause | Measured | Verdict |
|---|---|---|
| Measure a known cylinder from STEP and get the exact nominal diameter | In Chrome, with real pointer events, Diameter on the side of `cylinder-d22-lp-9010-00.step` reads **22.000 mm**, titled as read from an analytic CAD entity, with no ≈. From its cap, Wall thickness reads 30.000 mm, exact; through its side, 21.874 mm ≈. On the 200-part assembly, diameters of 30.000 and 10.000 mm and walls of 30.000 and 6.000 mm read exact up to three levels down the tree. On an STL, a diameter takes three points and reads ≈ | pass |
| Part open to first paint under 120 ms warm | Hover a grid card, click it, and time `pointerdown` to the viewer's first frame with the part in it, over the library's ten parts, three rounds each. Parts opened before: **86.6 ms** median (p90 90.7, max 94.9) on SwiftShader, **54.5 ms** median (p90 65.3, max 70.7) on the GPU. A part's first open, its L0 prefetched by the hover: 100.5 ms median (max 141) on SwiftShader, 56.2 ms median on the GPU | pass, at the median |

How it was measured: `docker compose -p lapidaryviewer`, with its own env file, storage, ingest
directory and database volume, built `api` and `worker` from `c50df2c` and `web` from `d89af5a`;
nothing in Rust changed between the two. The ingest folder held `fixtures/step` under `cad/`
beside the seeded example STLs, and every part's L1 was built before timing. Kernel
`occt-8.0.1-bridge-4+deflection-0.1+glb-1+cpu-1`, worker concurrency 2. Chrome 152.0.7977.82,
headless, driven over the DevTools protocol at 1440 × 900, where the quick look is a pane beside
the grid, with a throwaway profile per run. WebGL ran twice: on SwiftShader (ANGLE on Vulkan, on
the CPU) and on the machine's GeForce RTX 3060 Ti (ANGLE on OpenGL 4.5). The 12-core development
machine: Ryzen 5 5600X, 15 GB RAM. The time is `pointerdown` to the
`lapidary:viewer-first-frame` mark, which `Viewer.tsx` sets on its first frame with a part in it.

**Why the exact values can be trusted.** A pick snaps when all three corners of the triangle it
hit lie on an entity's surface within 1e-3 mm, and the triangle faces the way the surface does.
Corners, not the clicked point: the mesher puts nodes on the B-rep, and a triangle's middle sags
inside a curved surface by up to the deflection. A script ran `measure.ts`'s own
`placeEntities` and `snap` over every triangle of both fixtures' L2 rungs. **All 128 of the
cylinder's and all 28,576 of the assembly's** landed on a placed entity. That exercises the
transforms three levels down, not just a part at the origin.

**Found by the exit.** Two fixes were needed before these numbers meant anything:
- **The first-frame mark** fired on the resize observer's first frame, before any part was in
  the scene. It now waits for the part (`d5e1edf`), and nothing timed before that commit counts.
- **The viewer was not keyed by part.** The quick look reused it when moving to a part whose
  detail was cached, and so did the part page when its URL changed. The camera stayed framed for
  the last part, and a measuring tool kept its picks for the next one (`65442a9`).

**What the numbers do not say.**
- **Cold is over DATA.md's 400 ms.** The first open in a session loads the 641 kB (161 kB gzip)
  viewer chunk and starts WebGL. It took 511 ms on SwiftShader and 826 ms on the GPU.
- **First opens have a tail above 120 ms.** After the chunk, the GPU's first two opens took 363
  and 158 ms, compiling shaders, and two of SwiftShader's first opens took 125 and 141 ms.
- **A URL typed straight into the browser** boots a whole document: 369 ms median to first frame
  with a warm HTTP cache.
- **The parts are small.** A warm open read its rung from the HTTP cache and its detail from a
  local API in under 4 ms, so these figures are the viewer's own cost. How long a million
  triangles take to arrive is what the meshopt step measures.
- **The frame was not presented.** Headless Chrome has no display, so the mark is when a frame
  was drawn, not when it reached a screen.

**Where measurement stops being exact, on purpose.** Point to point and edge length are always
≈: an edge is the straight line between the two triangle corners nearest the clicks. An angle is
exact only between two planes, and a wall only between two parallel planes. Cones, spheres and
tori are read but not snapped to. Surfaces match untrimmed, so a pick cannot say which of two
coplanar faces it hit; the value is the same from either.

**Addendum, the same day: meshopt, measured and kept.** `a591bf1` writes every rung with
`EXT_meshopt_compression`, losslessly. It does not use `KHR_mesh_quantization`, whose 16-bit grid
would move the assembly's corners 0.005 mm off the surfaces measurement snaps to. The kernel now
reads `…+glb-2+cpu-1`. The fixtures are too small for a codec to show anything, so one large real
mesh was added: `Gauss Vertical Arc.stl` from the owner's library, 432,344 triangles. It was timed
at glb-1 first; then the same stack was reset at `a591bf1`, re-ingested and timed again.

| | glb-1 | glb-2 |
|---|---|---|
| Gauss rungs, L0 / L1 / L2 | 81 kB / 339 kB / 7.78 MB | 26 kB / 101 kB / 2.19 MB |
| Assembly rungs, L0 / L1 / L2 | 54 kB / 174 kB / 518 kB | 15 kB / 45 kB / 125 kB |
| Gauss L2, cold, 100 Mbit | 867 / 863 ms | **268 / 261 ms** |
| Gauss L2, cold, unthrottled | 58 / 50 ms | 39 / 31 ms |
| Gauss L2, warm | 47 / 44 ms | 32 / 27 ms |
| Gauss open → first frame, cold, 100 Mbit | 619 / 405 ms | 380 / 379 ms |
| Gauss open → first frame, cold, unthrottled | 434 / 510 ms | 386 / 405 ms |
| Gauss open → first frame, warm | 88 / 46 ms | 93 / 45 ms |
| Ten parts reopened, warm (the clause above) | 86.6 / 54.5 ms | 84.1 / 45.6 ms |
| First open in a session | 511 / 826 ms | 587 / 820 ms |

How to read the table:
- **Timings** are SwiftShader / GPU medians: of three runs, of five for warm Gauss opens, and of
  twenty reopens for the ten parts.
- **L2** is the time from pressing a measuring tool to the moment the measuring line stops saying
  it is loading the full-detail mesh: fetched, decoded, parsed and drawn once.
- **100 Mbit** is Chrome's own network emulation, 12.5 MB/s with 2 ms of latency.

**What changed besides time.**
- **Sizes.** Every rung over 10 kB shrank to 0.24–0.36 of its glb-1 size, and the smaller ones to
  0.39–0.62. The 28-triangle bracket grew 7 %, from 1,096 to 1,168 bytes, to the codec's headers.
- **The viewer chunk** grew by the decoder, 641 → 668 kB (161 → 169 kB gzip). That is the likeliest
  cause of SwiftShader's slower first open in a session; the GPU's did not move.
- **Snapping still holds.** This time the rungs were decoded by meshoptimizer's JavaScript decoder
  rather than the Rust encoder's own: all 128 and all 28,576 L2 triangles land on an entity, and
  Chrome reads the cylinder at 22.000 mm, exact.

**Kept**, because the large mesh's L2 arrives three times sooner over a network. Two warm costs did
move, both on SwiftShader, and both are most likely the decoder's 26 kB: a session's first open,
511 → 587 ms, and the warm Gauss open, 88 → 93 ms.

**What it does not change.**
- **Old rungs stay until a worker restarts.** Rungs already stored as glb-1 still load: the decoder
  only runs when a file asks for it. `PgParts::derivative_hash` takes a kind's newest row whatever
  kernel wrote it, so when this was measured nothing rewrote them. A worker now queues a rebuild of
  every rung whose `kernel_version` differs from its own kernel's as it starts
  (`WorkerHandler::enqueue_stale_rungs`), so a library ingested before `a591bf1` gets the smaller
  rungs after its next worker restart.
- **L0 and L1 could shrink further.** Nobody measures on them, so they could be quantized; they
  are not yet. `write_glb` does not know which rung it writes, so doing it means passing the
  `Lod` from `cluster()` down, and L2 must stay lossless.

**Addendum, the same day: the viewer warmed on hover.** The two points above that missed a target,
a session's first open over 400 ms and first opens with a tail over 120 ms, were the viewer starting
up: its chunk loaded on the first open, every open made a new WebGL renderer, and shaders compiled
on first draw. Hovering a card now also fetches the chunk and compiles the view's shaders
(`warmViewer` → `Viewer.prepare`), and every open reuses one renderer.

| Twelve parts, three rounds each | Before | After |
|---|---|---|
| First open in a session | 691 / 834 ms | 385 / 336 ms |
| Other parts' first opens, median (max) | 95 (186) / 55 (262) ms | 26 (27) / 25 (26) ms |
| Parts opened before, median (max) | 85 (105) / 52 (76) ms | 19 (20) / 18 (20) ms |
| Shaders linked during an open | not counted | none, on every open |
| The assembly, opened in a fresh session, median of three | 568 ms | 378 ms |

How to read it:
- **Timings** are SwiftShader / GPU, `pointerdown` to the first frame, with the pointer resting on
  the card for 250 ms before it presses. The assembly row is SwiftShader only.
- **Both targets are met** at that dwell, on both renderers: a session's first open under 400 ms,
  and every first open under 120 ms.
- **Shaders linked** counts WebGL `linkProgram` calls between `pointerdown` and the first frame. It
  caught the first attempt, whose first open still linked one: three compiles a separate program
  for points with no position attribute, and the view's marks gained one where the warm-up's did
  not. Both now start from the same empty buffer.
- **Reopens got faster as well**, because a reopen no longer creates a WebGL context.

How it was measured: `web/scripts/open-timing.mjs` (`DATA.md` §2.5) against
`docker compose -p lapidaryopen`, with `api` and `worker` from `528c186`, and `web` first as on
`main` and then rebuilt from this branch; nothing else changed between the columns. The ingest
folder held the two STEP fixtures and four fixture meshes beside the six seeded examples, and every
L1 was built before timing. Chrome 152.0.7977.82, headless at 1440 × 900 with a throwaway profile per
run, on the machine above.

**What it does not change.**
- **A press with no hover first is still cold.** With no dwell, a session's first open took 546 ms
  on SwiftShader and linked two shaders, because the warm-up had not finished. A touch screen, or a
  part page reached from a link, starts that way.
- **L2 did not move** beyond noise: 15–37 ms before and 11–38 ms after, over three fresh sessions
  each on the assembly.

**Addendum, the same day: isolate and hide, checked.** The bridge now meshes an assembly by
walking its tree and writes how many triangles each placed part has (`parts.json`, bridge 5). The
rung carries those counts as `extras.parts`, with each part one run of the index buffer (glb-3),
and the viewer leaves a hidden part's run out of what it draws and what a pick can meet. Checked in
Chrome against the 200-part fixture by reading how many triangles each frame drew:

| On the L1 rung: 9,476 triangles, 200 counts for 200 placed parts | Drawn | From `extras.parts` |
|---|---|---|
| Everything shown | 9,476 | 9,476 |
| Hide the first bracket station | 8,908 | 8,908 |
| Show only the V-block rail | 144 | 144 |
| Show all parts | 9,476 | 9,476 |

With every part but one stop pin hidden, the pin, 112 pixels of a 350-pixel view, was picked with
the Diameter tool and read **10.000 mm**, exact. Walking the tree meshes the fixture to the same
28,576 triangles at L2 as meshing the whole shape did, and `occt_bridge` checks that the counts
are one per placed part and add up to the rung.

How it was measured: `docker compose -p lapidaryopen` with `api`, `worker` and `web` from
`0c66256`, a fresh ingest of the two STEP fixtures and one STL, and Chrome headless over the
DevTools protocol on SwiftShader, reading `info.render.triangles` from the page's renderer.

**What it does not change.**
- **Rungs stored before glb-3 carry no counts,** so their tree offers no buttons until a worker
  restart rebuilds them, which the stale-rung sweep does.
- **A mesh file is one part,** and has nothing to hide.

**Addendum, 2026-09-14: the viewer warmed with no hover.** For the point above that a press with no
hover first is still cold, the grid now also warms the viewer once the browser is idle
(`warmViewerWhenIdle`). A part's own page starts the warm-up as it mounts, alongside its detail
fetch. Timed over the six example parts, medians of 18 opens each:

| | Before (`39ff56e`) | After (`cabddbc`) |
|---|---|---|
| A part's page from a link, navigation start to first frame, SwiftShader | 1,230 ms | 1,222 ms |
| The same on the GPU | 1,176 ms | 802 ms |
| The grid, pressed with no dwell, a session's first open, SwiftShader | 564 ms, 2 shaders linked in it | 583 ms, none |
| The same on the GPU | 335 ms, 2 shaders linked in it | 339 ms, none |

- **A link got faster on the GPU only**, by 374 ms at the median. SwiftShader did not move; why was
  not measured.
- **A press with no hover no longer compiles during the open, and is no faster for it.** Taking the
  two links out saved nothing measurable on either renderer, so a session's first open spends its
  time somewhere else, which was not measured. It stays over 400 ms on SwiftShader and under it on
  the GPU.
- **A link is over 400 ms on both.** That time runs from navigation start, so it includes loading
  the page and fetching the part. It is not the grid's open, and not the target's.
- **A grid visitor who never opens a part now pays for the viewer anyway:** its 669 kB chunk
  (167 kB gzipped) and a WebGL context.
- **Only before and after compare with each other.** The Phase 3 lines above were served through
  compose's `web` service. These came from `vite preview` in front of a natively run
  `lapidary-server` (`api` and `worker`, `mock-kernel`), which serves with different compression
  and caching.

How it was measured: `web/scripts/open-timing.mjs` against that stack, over the bundled example
parts, in headless Chrome on SwiftShader and on the machine's GPU, three rounds each, after one
throwaway pass that built every part's L1. `--direct` opened each part in a fresh session.
`--dwell 0` pressed right away, after the script's own 500 ms pause once the grid shows. Under
`--direct`, the shaders-linked count includes the warm-up's own compile, because in a fresh session
every link comes before the first frame, so it reads 2 before and after.

---

## Phase 4 — Versioning, agent, round-trip

**This phase is the differentiator. Everything before it is a file browser.**

- Immutable revisions, lineage DAG, `origin` tracking, pessimistic locks
- Geometric diff + visual overlay; version history strip in the inspector
- `Target` trait with automatic format negotiation
- **`lapidary` binary ships** — `agent`, `worker`, `up` subcommands
- `lapidary://` scheme, checkout to workspace, launch external tool
- Native watcher with debounce, write-settle, hash-before-believing, Windows buffer
  overflow rescan, macOS file-level FSEvents
- Storage tiering job, and the instance-wide storage view that can report quarantined
  bytes (a purged blob belongs to no library, so no library's panel can count it)
  <!-- Three-step deletion and quarantine landed early, in Phase 1 slice 7, on the
       owner's priority call: a library you can add to and never remove from was the
       more pressing gap. What stayed here is tiering, because tiering is what makes
       quarantine's physical layout matter, and building a layout before the job that
       uses it is building for a caller that does not exist. -->

**Exit:** open a STEP from Lapidary in FreeCAD, change it, save, and a new revision
appears automatically with a correct volume delta — on Linux, macOS and Windows.

### Slice 1 — revisions, geometric diff, check-out locks, the Linux agent (2026-09-14)

Spec: `docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md` (`e4d7f49`).
Merged locally, not pushed.

**Why first.** A file whose bytes changed at a path a library already indexed was reported
"already here" and kept nowhere, in every library. That is the gap this slice closes.

- **Revisions** (`fa3300f`, `627c9ae`; merge `21bea70`).
  - In a controlled library, a changed file becomes the next revision. The part row is locked
    first. The previous file moves to `<model>/revisions/<its label>/`, the new bytes go on top,
    and `metadata.json` lists every revision.
  - A revert is a revision.
  - A hobby library answers `unkept` and writes nothing. The batch line says how to switch, and
    `POST /api/libraries/{id}/controlled` switches one way.
  - A History section on the part page.
- **Geometric diff** (`c1e23a9`; merge `6dbc49b`).
  - Figures: volume, surface area, bbox per axis and triangle count.
  - Each change is approximate when either figure is, and absent (never zero) when either
    revision did not record the figure.
  - History rows carry the change from their parent, and `GET /api/parts/{id}/diff` compares
    any two revisions.
- **Check-out locks** (`fed1ac9`, `efffd84`; merge `5135257`).
  - One active lock per part, held by free text, with no auth.
  - The revision transaction refuses a change that does not carry the lock, naming the holder,
    or naming who released it and when.
  - The part page shows the holder and can release the lock behind a dialog.
- **The Linux agent** (`a944037`; merge `ed5f016`).
  - `lapidary checkout`, `checkin` and `agent`.
  - Polls every 500 ms, waits for a 2 s settle, and hashes with BLAKE3 before believing anything.

**Exit check.** Native stack: a debug build with the mock kernel, database `lapidary_p4`, the six
example STLs, and headless Chrome.

| Step | Result |
|---|---|
| 1. Scan into a controlled library | 6 ingested, 0 failed |
| 2. `lapidary checkout` of the flange as `mira@workshop-pc`, agent running | the file and `.lapidary-checkout.json` in `flange-dn40-lp-3310-02_1/`, watched |
| 3. Scaled ×1.1 along X, saved as a temporary file renamed over | revision 2, 2.7 s after the save: origin `agent`, parent revision 1, volume 243.10 → 267.41 cm³ (+10.0%, ≈). `revisions/1/` and revision 1's original download are byte-identical to the example file. `metadata.json` lists 1 `ingest`, 2 `agent`. The page shows History and Compare with every mesh figure marked ≈ |
| 4. `touch` | no revision, no agent output |
| 5. `flange.tmp` and `flange.bak` beside it | nothing |
| 6. A second checkout as `jonas@laptop` | refused, naming mira and when she took it |
| 7. Forced release, then another save | refused, naming `jonas@laptop` and the time; still 2 revisions |
| 8. `lapidary checkin` | after the release: prints the server's answer and marks the folder checked in, files kept. A fresh vee-block checkout checked in: lock cleared |
| 9. Hobby library, flange changed, re-scanned | 5 skipped, 1 unkept, 0 failed |

**Not met, and not in this slice.**
- The phase exit is not met: the check ran on STL, on Linux, with the mock kernel. No STEP in
  FreeCAD, and no macOS or Windows.
- Slice 2 holds:
  - the overlay diff;
  - `lapidary://` and launching a tool;
  - the macOS and Windows watchers;
  - the `Target` trait;
  - storage tiering;
  - face and edge deltas, which need STEP entities and OCCT;
  - mass and centre of mass.

**Recorded rather than fixed.**
- Fixed in slice 2 (`b34e8b3`): a new part's first revision now records its route, not always
  `ingest`.
- Fixed in the correctness goal (`455609b`): two different jobs racing different bytes onto one *new*
  path.
- Known test weaknesses, found by mutation checks:
  - Fixed in slice 2 (`b34e8b3`): the rename and still-writing tests now assert the change poll too.
  - The database revision tests and every lock test were written alongside their code, and
    were mutation-checked instead of being seen failing first.
- The purge coverage test caught `part_lock`'s `ON DELETE CASCADE` before merge. Purge now
  deletes the rows by name.
- The old deleted-part test counted a rung the skip path leaked. It now counts one.

### Slice 2 — overlay, `lapidary://`, render cache, bundles (2026-09-15)

- **Spec:** `docs/superpowers/specs/2026-09-15-phase-4-slice-2-design.md`.
- **Goal:** `docs/superpowers/plans/2026-09-15-phase-4-slice-2-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger, and grows as each stage merges.

**Overlay diff** (`88dd8cd`, `de6ee47`; merge `bbe33da`).
- **What it draws.** Compare's From revision can be drawn in the 3D view as a translucent amber ghost:
  - drawn through the part;
  - cut by the same section;
  - never picked.
- **Which mesh.**
  - `PartRevision` carries `tessellationL0` and `tessellationL1`.
  - The ghost is L1, else L0.
  - A coarse ghost says so, and a revision with no mesh says so.
- **Why amber.** DATA §6.1 says grey, but the first shot of a grey ghost over the grey part was barely
  visible.

**Checked** in headless Chrome on SwiftShader.
- **Setup:**
  - a native stack: a `mock-kernel` build, with STL through the mesh kernel;
  - the flange, in a controlled library;
  - the viewer shot at 350 × 350 px with the ghost off and then on;
  - "changed" means pixels that differ between the two shots by more than 12 in any channel.

| Current revision | Ghost | Part's width on screen | Width the ghost changed | Changed pixels outside the part |
|---|---|---|---|---|
| 2, ×1.1 in X (164.8 mm) | 1 (149.8 mm), its L0 | 235 px | 223 px, inside the part | 124, along antialiased edges |
| 3, ×0.9 in X (134.8 mm) | 1 (149.8 mm), its L0 | 235 px | 247 px, past the part | 3,025 |

- **The coarse-ghost line** showed in both cases, because revision 1 had only its L0.
- **Tests:**
  - The history's rung fields: seen failing first.
  - An earlier revision's rung is still served once a newer one is current: a guard, which already
    passed before the change.
  - The toggle, the coarse line and the no-mesh line: mutation-checked. A Compare that never hands the
    ghost up fails the test.
- **Not shot:** the ghost with a section cut on, and an assembly with hidden parts.

**`lapidary://` and launching a tool** (`84712df`).
- **Commands:** `lapidary register`, `unregister` and `open`.
  - `register` writes an XDG handler whose `Exec` carries the server and workspace, so a link chooses
    a part and nothing else.
  - `open` accepts only `lapidary://open?part=<uuid>`. It reuses this computer's checkout of the part,
    or takes one, and hands the file to `xdg-open`.
  - `unregister` removes only what `register` wrote.
- **The part page** links to it in a controlled library, with DATA §6.3's line on which tools save
  back.
- **The `Target` trait is not built.** Download and open both hand out `variant=original`, and
  neither negotiates a format, so the trait would have one caller. It arrives with the first target
  that needs a format the source is not in, which needs OCCT exports.
- **Found by the check:** quoted `Exec` arguments broke under `xdg-open`'s own launcher.
  - The quoting followed the Desktop Entry spec, but that launcher splits on spaces and keeps the
    quotes, so `env` was handed `"LAPIDARY_SERVER=…"` literally.
  - Arguments are now written unquoted, and `register` refuses a server, workspace or install path that
    would need quoting.

**Checked** on the native stack.
- **Setup:**
  - throwaway `HOME`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` under `target/`;
  - a stand-in editor registered for `model/stl` that saves the file 5% wider in X;
  - `lapidary agent` running.

| Step | Result |
|---|---|
| `lapidary register` | The handler was written, and `x-scheme-handler/lapidary=lapidary-url.desktop` set |
| Five hostile links: `&server=`, `part=../../.ssh/id_ed25519`, a value that is not an id, `lapidary://download?`, a fragment | Each refused, naming what was wrong. No folder was made, and the part still had 3 revisions |
| A part `jonas@laptop` has checked out | Refused, with the server's own message naming him and when |
| `xdg-open 'lapidary://open?part=<flange>'` | The checkout folder appeared and the stand-in editor ran. Revision 4, origin `agent`, parent 3; bbox X 134.84 → 141.59 mm, 3.8 s after the click |
| The same link again | The same folder was reused, with no second checkout |
| `lapidary unregister` | The handler and its default were removed; the stand-in editor's file and default were untouched |

- **Tests, mutation-checked** (every mutation was caught):
  - link parsing;
  - handler arguments;
  - removing the line from `mimeapps.list`;
  - reusing a checkout;
  - the part page's link, shown in controlled libraries only.
- **Not checked:**
  - a real desktop session's launcher (GNOME's `gio`, KDE);
  - a browser's own prompt before it hands a link over.

**Storage view and render cache** (`46945c4`).
- **`GET /api/storage` gains `renderCacheBytes`.** It counts blobs that only L1 or L2 rungs point at,
  once nobody has read them in 90 days.
  - A blob never read counts from when it was written.
  - A blob shared with L0, a file or any other row is never counted.
- **`POST /api/storage/render-cache`** is "Free cache space…", behind a dialog on the instance
  storage line.
  - It removes those rows and recounts their blobs through the statement purge uses, now one shared
    function.
  - It answers with the rungs it removed and the bytes that entered quarantine.
  - Nothing leaves the disk that day: the hourly sweep takes the bytes after 30 days.
- **Wording:**
  - "free cache space";
  - no model file is touched;
  - the space comes back when the quarantine ends;
  - never "freed" or "deleted" (a test checks the result line holds neither).

**Checked** live on the native stack.
- **Setup:**
  - a debug `mock-kernel` build;
  - 15 parts across two libraries;
  - every blob aged to 120 days in SQL;
  - the spur gear's L1 built first.

| Step | Result |
|---|---|
| `GET /api/storage` | `renderCacheBytes` 16,280: three L1 rungs |
| Free cache space | 3 rungs removed and 16,280 bytes into quarantine. Derivative bytes 49,356 → 33,076; `renderCacheBytes` 0 |
| What stayed | 15 L0 rows, 15 thumbnails, 15 source files |
| The spur gear opened again (`POST …/rungs/l1`) | Queued and rebuilt with the same hash. Its blob left quarantine, so quarantined bytes went 16,280 → 9,784 |

- **Tests:**
  - The figure and the eviction in `lapidary-db`, mutation-checked. Both mutations were caught:
    - dropping the shared-blob exclusion counts 57,350 bytes;
    - dropping the age condition evicts a rung read yesterday.
  - The route end to end, with the rebuild request, in the api tests.
  - The dialog and the result line in web, mutation-checked.
- **Found by the check, and fixed on its own branch:** rebuilding the flange's L1 failed with
  "Unknown frame descriptor".
  - Revision 4 came in through the agent's upload. Its `file` row says raw (level 0), but its `blob`
    row still says zstd 3, from the upload's staging copy.
  - Two readers, `revision_source` and the detail's source lateral, still read the `blob` row's level,
    which migration `0013` retired.
  - **Fixed in `568d799`.** Both readers now take `file.zstd_level`, and the detail its size from `file`
    too. A `lapidary-db` test that stages a zstd copy and then files the bytes raw was seen failing
    first: it read level 3 where the file said 0.

**Slice 1's debts** (`b34e8b3`).
- **Origin.** `IngestRequest` carries the origin, so a part that arrives by upload says `upload` on
  its first revision, and a scanned one `ingest`. The handler test was seen failing first: the upload
  said `ingest`.
- **Watcher tests.** The rename and still-writing tests assert `Wait` on every change poll.
  - Mutation: a watcher that hashes a change at once.
  - Before, only the settle test caught it. Now all three watcher tests do.

**Bundle export** (`da9ff39`).
- **`POST /api/libraries/{id}/bundle`** streams the grid's selection as one ZIP:
  - every revision's original bytes;
  - the current one at the part's source path, earlier ones under `revisions/<label>/`;
  - a `manifest.json` with parts, sources and licences, and each revision's label, parent, origin,
    hash and path.
- **Integrity.** Each file is hashed as it goes out. A mismatch ends the body short of its exact
  `Content-Length`.
- **The ZIP writer** (`lapidary-targets`' `bundle`) is hand-rolled: STORE entries with data
  descriptors, because `zip` 2.4.2 needs a seekable writer. It has no ZIP64, marked `ponytail:`. The plan
  refuses past an import's own limits, 2 GiB and 10,000 files (the review fixes).
- **`POST …/bundle/plan`** makes the download's checks first:
  - at most 500 parts;
  - all of them in this library and not removed;
  - no two files at one path;
  - under 4 GiB.

  The selection bar plans, then posts the form the browser saves, and shows a refusal in the
  server's words.
- **Tests:**
  - The writer, read back by `zip::ZipArchive`: STORE entries, their bytes, and the promised length.
  - The route: three revisions byte-identical, the manifest's labels, parents, origins and licence,
    `Content-Length` equal to the plan's figure, the refusals, and a changed file ending the body in
    an error.
  - Mutation-checked, and every mutation was caught: dropping the hash check, dropping the collision
    check, and downloading before planning.
- **Not in the manifest:** materials. Import runs the kernel, which reads them from the file again.
- **The live check** runs with import: 40 parts exported, then imported into a fresh library.

**Bundle import** (`4d3e05c`).
- **The route.** `POST /api/libraries/{id}/imports` stores a bundle sent through the chunked upload and
  queues one `import_bundle` job. The api never opens the archive: reading stored source bytes is the
  worker's.
- **The bundle job** reads the archive back and checks it whole before anything is written:
  - safe names;
  - STORE entries only;
  - a manifest format and version it knows;
  - every revision's file present at the size and BLAKE3 the manifest names.

  It then queues one `import_part` per part into the same batch, as a scan does.
- **A part's job** replays its revisions through `index`, oldest first.
  - Labels, parents and origins survive, and every figure and rung is this library's own.
  - A hobby library gets each part's newest revision.
  - A part already holding one of the bundle's revisions resumes after it, so a stopped or repeated
    import finishes.
  - A part holding anything else is refused rather than grafted.
- **The grid's toolbar** gains Import a bundle.

**Checked** on the native stack, with a debug `mock-kernel` build.
- **The source library:** 40 mesh parts in a controlled library, the six examples scaled by part.
- **Revision 2 of each:** the file scaled ×1.05 and re-scanned.
- **Export:** through the plan and form routes.
- **Import:** the bundle uploaded in 8 MiB chunks, then imported into a fresh controlled library.

| Step | Result |
|---|---|
| Scans | 40 ingested, then 40 revised, 0 failed |
| Plan | 40 parts, 80 revisions, 2,520,335 bytes |
| Export | 200, a body of 2,520,335 bytes (its `Content-Length`) in 0.19 s. 81 entries, every one STORE; Python's `zipfile.testzip` reports no bad CRC |
| Import | 202 with one job queued. The batch finished in 4.0 s: 40 ingested, 1 scanned, 0 failed |
| Lineage | All 40 parts identical, row by row: labels, parent labels, origins and source hashes |

- **Tests:**
  - The reader's refusals, in `lapidary-targets`: an escaping name, no manifest, a wrong hash, a newer
    version, an unknown origin, not a ZIP.
  - The handler, all seen passing after being written with the code, then mutation-checked (every
    mutation caught):
    - labels, parents and origins kept, and a second import all skipped;
    - a hobby library gets the newest revision only;
    - a graft is refused;
    - a tampered bundle queues nothing.
  - The api route, and the client's upload-then-import order.
  - Mutations: dropping the graft refusal, importing every revision into a hobby library, and
    resuming from the start.
- **What this does not cover:**
  - Phase 5's exit asks for a 40-part STEP *assembly*, which needs OCCT.
  - `created_at` becomes the time of import.
  - The bundle is read whole into the worker's memory, marked `ponytail:`.
  - A hobby import does not count the earlier revisions it left out.
  - The lineage check's revisions all came by scan, so agent origins were checked only by the
    handler test.

**Review fixes** (`3ef304a`). A fresh reader reviewed the whole slice (`f1f6ae8..50d9099`) and found the
following, all fixed on one branch.
- **Bugs:**
  - **The render cache could not see a part in daily use.** Rungs are served `immutable`, so a
    browser that holds one never asks the blob route again, and after 90 days the rungs looked cold.
    Opening a part (`GET /api/parts/{id}`, not cached) now records its rungs as read.
  - **An uploaded bundle stayed on disk, uncounted:** its blob had `ref_count` 0, and nothing ever
    quarantined it. The unpacking job now releases it into the 30-day quarantine, whether the bundle
    imports or is refused.
  - **A bundle's hash alone imported it:** the route skipped the staged upload when the store already
    held those bytes. It now reads only this library's own upload, and the client always sends the
    bundle.
  - **Import resumed from the first matching hash.** A history with a revert imported twice, and a
    part whose history began elsewhere was grafted onto. Import now resumes only after a matching
    prefix of the bundle's history, and refuses anything else.
- **Risks:**
  - `lapidary open` reused a checkout whose lock had been released. It now checks the part's lock
    against the folder's first.
  - "Free cache space" removed rows its own figure never counted, such as an L2 sharing L0's blob.
    The removal now has the figure's two exclusions.
  - Every part job hashed the whole bundle. The unpacking job now hashes every file once, and each
    part job hashes its own.
  - Export could make a bundle that import refuses (4 GiB and 65,534 files, against 2 GiB and
    10,000). The plan now refuses at the import's limits.
- **Smaller:**
  - The 500-part cap is checked before de-duplicating.
  - `register` refuses an install path holding `=`.
  - An export refused with no message says `exportFailed` rather than an HTTP status.
  - The bundle downloads into a hidden frame, so a late refusal never replaces the grid.
  - The path guard now runs inside `index`, where every route into the pipeline passes.
- **Tests:**
  - New: the rungs an open records; the bundle's release; a revert imported twice; a graft by
    prefix; an import by hash alone refused; the cap checked before de-duplication.
  - Mutation-checked, all caught: the prefix, the open's touch, the staged-only import, the release.
  - The render cache test now keeps the L2 that shares L0's blob.
- **Not covered:** the agent's lock check has no automated test. The agent's HTTP half is still the
  exit check's.

---

## Phase 5 — Source links, bundles, collections

- `part_source` + `part_image` with the full SSRF control set
- OpenGraph preview fetch behind an explicit button
- Streaming ZIP bundles with `manifest.json`
- Saved filters, custom fields, section plane, PMI display
- Turkish search config

**Early, 2026-09-13: PMI listed on the part.** The bridge reads an AP242 file's semantic PMI
(`pmi.json`, bridge 6) and ingest stores it as the `pmi` derivative. The part's page lists it under
Dimensions and tolerances, each annotation on the face measurement reads there, said as specified
and never marked exact or approximate. Checked in Chrome on an isolated stack built from `786af76`,
against the generated `cylinder-d22-pmi-lp-9012-00.step`: the page listed ⌀22 mm +0.05 / 0 on a
cylindrical face, ⏥ Flatness 0.02 mm on a planar face, ⟂ Perpendicularity 0.05 mm to A on a
cylindrical face and Datum A on a planar face, with no ≈, and the plain cylinder showed no section.
`occt_bridge` checks that each annotation lands on the face it was written on.

What it does not do:
- **No 3D annotation.** The viewer draws nothing for PMI; that needs per-face triangle ranges.
- **One writer.** The fixture is OCCT reading what OCCT wrote, so AP242 files from other CAD
  systems are untested.
- **Datums through tolerances only.** OCCT reads a datum while reading a tolerance that refers to
  it, so a datum nothing refers to is not listed. OCCT's writer likewise drops a tolerance whose
  datum has no place in its reference frame, which the fixture generator had to set.
- **Parts ingested before bridge 6 have none.** The stale-rung sweep rebuilds rungs, not PMI, and
  a known file's hash skips the kernel, so nothing re-reads a stored file for it yet.

**Early, 2026-09-14: section plane.** The viewer cuts the part along X, Y or Z, anywhere across its
box, and keeps either side (`SectionBar`, and `sectionPlane` in `viewer-math.ts`). The cut clips the
part's material. three's raycaster ignores clipping, so a pick and the wall-thickness ray count a
hit only on the side still drawn (`kept`).

Checked in Chrome on SwiftShader, on a natively run stack built from `148e73a`, against
`flange-dn40-lp-3310-02.stl`, 16 mm thick. Wall thickness was clicked at 81 points on a 9 × 9 grid
over the view, once uncut and once with a cut at Z = 8 mm:
- 26 points met the part uncut. The 5 whose picks both lay below the cut read the same with it, to
  the last digit. The 21 that had landed on the removed half met nothing.
- Of all 81 clicks with the cut on, none picked a point above it.

What it does not do:
- **The cut is open.** Behind it, only surfaces facing the viewer are drawn and can be picked, so a
  cut solid reads as its outline and the walls of its holes rather than as a filled face.
  - A cap needs the stencil buffer.
  - Drawing back faces as well would need picks kept off them, since their normals point away
    from the viewer.
- **One mesh.** A cut together with an assembly's hidden parts, and a STEP part's exact readings
  under a cut, need OCCT, which this check did not have.
- **The first cut in a session compiles a program** for the clipped material, and how long that
  takes was not measured. Turning the cut off, or leaving the part, should cost no compile on the
  next open.
  - three 0.186 keeps each material's programs by configuration until the material is disposed
    (`WebGLRenderer.getProgram`), and the session's material never is.
  - That was read from three's source, not timed.

**Early, 2026-09-14: saved filters.** The grid's filters (search, category, format, material and
tag) save under a name per library (`saved_filter`, `0023`). They are listed in the grid's rail
above the facets, and the one the grid is showing is marked.

Checked in Chrome on a natively run stack built from `6e59072`, over the six example parts with two
of them tagged `stock`:
1. Filtered to `stl` and `stock`, the grid showed two cards. Saved as "Stock STL", the filter was
   listed and marked.
2. With the filters cleared, all six cards showed, and the filter was listed and not marked.
3. Reopened from the list, the URL read `?format=stl&tag=stock`, the same two cards showed, and it
   was marked again.
4. It was still listed after a reload.
5. Once removed, it was gone from the list and from the API.

What it does not do:
- **No users.** A library's saved filters are everyone's who opens it.
- **A saved category does not follow its deletion.** A filter saved on a category that is then
  deleted still opens to it: an empty grid saying nothing is filed in that category yet, though
  the category is gone from the tree.
- **No rename or reordering.** To change one, remove it and save again.

**Exit:** paste a Printables URL, get title, licence and cached image; export a 40-part
assembly as a bundle another user can import with full lineage intact.

---

## Open points, 2026-09-15

These were swept from this file's records, FEATURES and DATA, and checked against the code at
`4aaef44`.
- **Exits.** Phases 0–3 are met. Phase 4's and Phase 5's are not.
- **Features.** Of the Phase 1–5 feature rows, 36 are done, 7 are partial and 8 are missing.

**Three goals, run in this order.** Each goal is one long `/goal` session with its own file under
`docs/superpowers/plans/`, and keeps its record below as it merges.

| Order | Goal file | Holds |
|---|---|---|
| 1 | `2026-09-15-phase-4-slice-2-goal.md` | overlay diff; `lapidary://` and launching a tool; render cache and quarantined directories in the storage view; slice 1's debts; bundles if time remains |
| 2 | `2026-09-15-correctness-debt-goal.md` | batched access tracking (DATA §1.4); phantom bytes in the purge and sweep reports; two jobs racing onto one new path; re-parenting a category; tests never seen failing; the untimed drop path, tree fan-out and link open |
| 3 | `2026-09-15-local-product-goal.md` | custom fields; Turkish search; saved-filter rename, reorder and deleted categories; a capped section cut; watched-folder ingest through the agent; bundles if slice 2 did not reach them |

**Decided by the owner, 2026-09-15.**
- **Tiering.** Source tiering is retired, and library files stay raw (DATA §1.2, §1.3).
- **OCCT and FreeCAD wait.** Checks stay on mesh parts with the mock kernel.
- **macOS and Windows watchers** wait for a machine to run them on.

**Blocked, and what unblocks each.**

| Unblocked by | Items |
|---|---|
| Building the OCCT image (root disk, Docker shared with another project) | Timing Phase 0 and Phase 2 on real STEP files and assemblies; face and edge deltas; snapping to cones, spheres and tori; PMI drawn in 3D; datums no tolerance refers to; PMI for parts ingested before bridge 6; a section cut through an assembly with hidden parts; explode view (3MF components are flattened, so only an OCCT assembly has parts to explode); B-rep format negotiation behind `Target`; a 40-part STEP assembly as a bundle |
| FreeCAD installed, plus OCCT | Phase 4's exit (a STEP opened in FreeCAD, saved, and a revision appears); AP242 files written by other CAD tools |
| A macOS or Windows machine | The FSEvents and `ReadDirectoryChangesW` watchers, the Windows overflow rescan, and the rest of Phase 4's exit |
| Pulling a pgvector image | Checking pgvector against `postgres:18`, before Phase 6 (see Open items) |
| Phase 8 | The lifecycle facet, per-user saved filters, auth on locks, `lapidary worker` |

**Open questions for the owner.**
- **Phase 5's exit cannot be met as written.**
  - Its first clause, a Printables URL giving title, licence and a cached image, needs OpenGraph
    fetching.
  - FEATURES §6 rules OpenGraph out by owner decision.
  - Recommended: change the exit to a source URL kept with a title and licence entered by hand, and an
    image fetched from a pasted image URL, which already works.
- **Mass and centre of mass** need a density.
  - Recommended: a density per material, entered by hand, with ≈ on every figure derived from it.
- **`lapidary up`** writes compose files and pulls images.
  - Recommended: it stays a stub until the managed-local or Tauri work.
- **A grid visitor who never opens a part** still pays for the viewer chunk and a WebGL context (Phase
  3 record).
- **113 local commits are unpushed.** `origin/main` was last pushed on 2026-09-08. When to release is
  the owner's call.
- **Old housekeeping** from before the folder tree, not rechecked: the `lapidary_lapidary-blobs`
  volume, and a `storage/` directory left at `chmod 777`.

### Goal 2: correctness and debt (2026-09-15)

- **Goal file:** `docs/superpowers/plans/2026-09-15-correctness-debt-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger.

**Batched access tracking** (`812567e`).
- **Recording.** A read records its blob in `lapidary_db::Touches`, in the api process.
- **The flush** runs every five minutes, and once when the server stops. It is one
  `UPDATE … FROM unnest`, writing only rows more than a day stale.
- **What went.** `PgBlobs::touch_blob` and its `UPDATE` per read.
- **Tests:**
  - A flush writes only the blobs that were read.
  - 300 reads of one blob are one write.
  - A row read today is not rewritten; one two days stale is.
  - Mutation: dropping the day filter was caught by both the db test and the api test.
  - The blob and download tests now flush before they read the column.
  - "A second read moves the timestamp forward" became "a second read the same day writes nothing",
    which is what §1.4 asks for.
- **Not measured:** how long a flush takes on a real corpus.

**Phantom blob bytes** (`5da147f`).
- **The cause.** A blob row's `stored_bytes` describes its content-addressed copy. Ingest, a revision
  and `migrate_storage` still wrote a filed file's size onto the blob row of a hash that had no such
  copy, so purge, the sweep and the instance figure counted a phantom copy beside the real model file.
- **The fix.**
  - Those writers write 0.
  - Migration `0026` corrects the rows already written.
  - Purge now counts the model files it quarantines, once each.
- **Test:** a filed source plus one rung; purge, the instance figure and the sweep must all report the
  bytes that leave the disk.
  - Seen failing first: the instance figure counted 11,484 bytes for 7,388 on disk.
  - Mutation: writing the file's size onto the blob row again was caught.
- **Checked live** on the native stack's database, against parts imported by the bundle check.

| Step | Result |
|---|---|
| Old build: purge of a two-revision filed part | Reported 0 bytes, though its two model files went into quarantine |
| New build: migration `0026` | 86 blob rows now hold no copy |
| New build: purge of another two-revision part | 2 files and 60,968 bytes (2 × 30,484); the instance quarantine figure rose by exactly 60,968 |

- **Under-counts, recorded:** a 3MF upload's staged copy under `blobs/` is real, but reads as phantom
  to the migration and becomes 0.

**Two jobs, one new path** (`455609b`).
- **The race** (slice 1 spec §3.3). Two jobs with different bytes both found no part at one path.
  - If both resolved the same directory, the loser's `put_at` renamed its file over the winner's.
    Its insert then failed, and it settled as `skipped`.
  - If the winner's directory came first, the loser took a disambiguated one, and still settled as
    `skipped`, beside an orphan copy.
- **The fix.**
  - `SourceStore::put_new_at` hard-links its temp file to the name, which fails if the name is
    taken. Only a new part's file uses it. `metadata.json`, a revision's file and the migration
    still replace.
  - Other bytes already at the path are left alone, and the job is retried. The same bytes are this
    file, left by an attempt that stopped before its row, so they are used as written.
  - A lost insert with other bytes reaps the loser's own file, and the job is retried. The retry
    reads the winner's part: `unkept` in a hobby library, a revision in a controlled one. The same
    bytes still settle as `skipped`.
- **Tests:**
  - Seen failing first:
    - other bytes already at the path were replaced, reported `Ingested`;
    - a loser held in the kernel while the winner filed was `skipped`, in both library modes.
  - `put_new_at` refuses an existing file and leaves no temp file. Seen failing against a stub.
  - Mutation-checked, all caught:
    - the hard link swapped for a rename;
    - the same bytes refused;
    - the loser keeping its own file;
    - other bytes skipped.
- **Decided without the owner:**
  - The loser decides again by being retried, not inline, since the winner may not have committed
    yet. A lost race costs one of the job's three attempts.
  - A race's rungs are never reaped, since the winner may serve the same ones. A loser's own rungs
    stay on disk with no row.
  - A filesystem that refuses hard links (FAT, exFAT, some network mounts) gets a check then a
    rename. That narrows the race to the gap between them and does not close it, marked
    `ponytail:`, with `renameat2(RENAME_NOREPLACE)` as the upgrade. No test covers that fallback.

---

## Phase 6 — Dashboard and similarity

- Widget registry, drag-resize layout, named groups
- Single batched `/api/dashboard/resolve` with per-key timeouts and partial results
- Live patches over the existing SSE stream
- Geometry embeddings + pgvector; near-duplicate clustering with merge/link-as-variant

**Exit:** a 12-widget dashboard settles in one round trip; uploading a known part surfaces
its near-duplicates.

---

## Phase 7 — Build graph

Comparable in size to everything before it. **Do not start early.** A half-working
planner attached to a good browser makes the whole app feel unfinished.

- Process types with JSON Schema params; ~15 builtins + user-defined
- Board on `@xyflow/react`, auto-layout, cycle rejection at write
- Graph versioning; runs separate from graphs; quantity multiplication
- Ready-set with critical path + batch affinity
- Guide view: queue and sequential modes, mobile-shaped, photo capture
- Auto-generated draft steps; PDF export

**Exit:** model a three-level assembly where sub-parts are printed and moulded, start a
run of 4 units, and the queue correctly answers what to make next with correct quantities
— then follow it to completion on a phone.

---

## Phase 8 — Enterprise and fleet

- Auth, RBAC, audit log, lifecycle states and approvals
- Settings → Compute: enrolment tokens, worker registry, drain, revoke
- Lease protocol with heartbeats; kernel-version pinning enforced
- Ed25519 offline licence with `max_workers` and grace-period expiry
- Air-gapped image bundle + Quadlet units
- Export-everything bundle
- Subgraph references; Hausdorff diff

**Exit:** register three remote workers on separate machines, process a 500-part library
across the fleet, revoke one mid-run and watch its leases expire and requeue cleanly.

---

## Phase 9 — Cloud

Only after the local product has users.

- Zero-egress object storage (R2 or B2 behind Cloudflare) — **day one, not an
  optimization**
- Per-tenant dedup only; never cross-tenant
- Egress instrumented per tenant from the first beta day
- Share links, remote viewer access, lock authority
- Lapsed subscription → read-only with full download, never deletion

---

## Phase 10 — Tauri shell

- Three connection modes: remote server, managed local stack, embedded (never built)
- Bundles **only our own binaries**. If you are tempted to bundle Postgres or OCCT, stop
- Authenticode + Apple notarization

---

# Commercial model

## Shape

| Tier | What | Price mechanism |
|---|---|---|
| **Local** | Everything, unlimited, single machine | Free, AGPL |
| **Team** | Self-hosted server, worker fleet up to N | Annual, MoR checkout |
| **Enterprise** | Large/unlimited fleet, air-gapped, support SLA | Invoice + wire, perpetual + maintenance option |
| **Cloud** | Hosted, per-GB | Subscription |

## Why the worker fleet is the right gate

Distributed compute is **inherently multi-machine**. A hobbyist on one laptop loses
nothing. A shop with eight workstations gets real value. Near-perfect price/value
alignment with zero crippling of the free app.

**Meter per worker node, not per seat.** Seats are annoying to administer, easy to fudge
and resented. Worker count is a hard number the coordinator already knows, scales with
delivered value, and drops into `max_workers` in an offline signed licence.

**Grace period, never a hard stop.** An industrial customer whose line halts over a
lapsed licence will not renew, and will tell people.

## Two payment rails, both required

- **MoR** (Paddle, Polar, Stripe Managed Payments) for cloud and small self-hosted card
  purchases. Note Stripe acquired Lemon Squeezy and is folding it into Stripe Managed
  Payments. Confirm current Turkey payout support directly with each provider — this
  changes often.
- **Invoice + purchase order + e-fatura + wire.** A €5,000 self-hosted licence does not
  go through a checkout page. This is where the highest-value customers live and it is
  the rail solo developers forget to build.

## Structural lever

Erciyes Teknopark is in Kayseri. Under law 4691, earnings derived exclusively from
software, design and R&D activity within a technology development zone are exempt from
income and corporate tax until 31/12/2028, with a matching VAT exemption on qualifying
software deliveries plus income tax withholding relief and SGK employer-share support on
R&D personnel.

Conditions are strict: `münhasıran` (only in-zone activity qualifies), management-company
approval and registration are prerequisites, IP-derived income is subject to a qualified-
expenditure ratio, and above 1M TL of exempt earnings 2% must go to venture capital funds
or forfeit 20% of the exemption.

Combined with the software export deduction on foreign-currency invoices, this is
plausibly worth more in year one than any pricing decision. **Take it to a mali müşavir
before incorporating** — some of it depends on day-one entity structure.

---

# Open items

- **Trademark.** "Lapidary" is a common English word — check TÜRKPATENT and EUIPO in the
  relevant software classes before registering a domain.
- **`pgvector` and Turkish `tsvector`** against `postgres:18`. Turkish is settled: checked on
  2026-09-14, `postgres:18`'s `pg_ts_config` lists `turkish`. pgvector is not: the official image
  offers no `vector` extension, `deploy/db/Containerfile` installs `postgresql-${PG_MAJOR}-pgvector`
  and `deploy/db/init/10-extensions.sql` creates it, and no test or gate checks either yet.
- **zstd dictionary gain** — moot while library files stay raw: tiering and dictionaries were
  retired on 2026-09-15 (DATA §1.2, §1.3). Measure on a real STEP corpus only if a raw-plus-compressed
  store is ever proposed.
- **MoR Turkey payout support** — confirm directly, do not rely on this document.
- **WebKitGTK variance** — test the Tauri shell on at least two Linux distros.
