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

**Verify here, not later:** `pgvector` installs against `postgres:18`. The Turkish
snowball `tsvector` config was present and used, until Turkish search was removed (`8bb5ea4`).

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

Timed on 2026-09-15, in the correctness goal's record: a stand-in 150-file drop took 31.5 s from
handing over the folder to a finished batch. The grid's search answered in 18 ms at the median
while it ran.

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
  (`WorkerHandler::enqueue_stale_derivatives`), so a library ingested before `a591bf1` gets the smaller
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
  The correctness goal's record splits it. Rung transfer and decode are small, and the viewer's
  shader warm-up is the largest block.
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

Met on Linux in goal 4's stage 8 (2026-09-15, FreeCAD 0.21.2): revision 2 appeared 2.5 s after FreeCAD wrote
the file, with an exact volume delta. macOS and Windows are still open.

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
    Seen failing in the correctness goal: each fails when the line it guards is broken.
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
  that needs a format the source is not in, which needs OCCT exports. Built in goal 4 (`22ce8ad`):
  downloads and `open` both negotiate through it, as goal 4's record says.
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
- OpenGraph preview fetch: out, by owner decision (FEATURES §6)
- Streaming ZIP bundles with `manifest.json`
- Saved filters, custom fields, section plane, PMI display
- Turkish search config (built, then removed at the owner's word: `8bb5ea4`)

**Early, 2026-09-13: PMI listed on the part.** The bridge reads an AP242 file's semantic PMI
(`pmi.json`, bridge 6) and ingest stores it as the `pmi` derivative. The part's page lists it under
Dimensions and tolerances, each annotation on the face measurement reads there, said as specified
and never marked exact or approximate. Checked in Chrome on an isolated stack built from `786af76`,
against the generated `cylinder-d22-pmi-lp-9012-00.step`: the page listed ⌀22 mm +0.05 / 0 on a
cylindrical face, ⏥ Flatness 0.02 mm on a planar face, ⟂ Perpendicularity 0.05 mm to A on a
cylindrical face and Datum A on a planar face, with no ≈, and the plain cylinder showed no section.
`occt_bridge` checks that each annotation lands on the face it was written on.

What it does not do:
- **No 3D annotation.** The viewer draws nothing for PMI; that needs per-face triangle ranges. Since goal 4
  (`d0a9a86`), annotations on analytic faces are drawn beside them; a freeform face's still need those ranges.
- **One writer.** The fixture is OCCT reading what OCCT wrote, so AP242 files from other CAD
  systems are untested.
- **Datums through tolerances only.** OCCT reads a datum while reading a tolerance that refers to
  it, so a datum nothing refers to is not listed. OCCT's writer likewise drops a tolerance whose
  datum has no place in its reference frame, which the fixture generator had to set.
- **Parts ingested before bridge 6 have none.** The stale-rung sweep rebuilds rungs, not PMI, and
  a known file's hash skips the kernel, so nothing re-reads a stored file for it yet. Fixed since goal 4
  (`bc4f06e`): a worker reads an older CAD part's tree, entities and PMI again as it starts.

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

**Exit:** keep a part's source URL with a title and licence typed in, and an image fetched from a pasted
image URL; export a 40-part assembly as a bundle another user can import with full lineage intact.
Rewritten 2026-09-15 by the owner's answer, since OpenGraph fetching is out (FEATURES §6). The first clause
is met. The bundle is met for 40 mesh parts, and in goal 4's stage 2 for an OCCT assembly of 200 placed
parts, exported and imported with its lineage identical; read as the exit met, without the owner.

---

## Open points, 2026-09-15

These were swept from this file's records, FEATURES and DATA, and checked against the code at
`4aaef44`.
- **Exits.** Phases 0–3 are met. Phase 4's and Phase 5's are not. *(Overtaken: Phase 4's was met on
  Linux in goal 4's stage 8 and what remains of it needs a macOS or Windows machine; Phase 5's was
  rewritten to what already works, and is met.)*
- **Features.** Of the Phase 1–5 feature rows, 36 are done, 7 are partial and 8 are missing.

**Goals, run in this order.** Each goal is one long `/goal` session with its own file under
`docs/superpowers/plans/`, and keeps its record below as it merges. Goals 1–6 are merged. Goals 4–6 were planned after goal 3's code review, from the owner's answers
below.

| Order | Goal file | Holds |
|---|---|---|
| 1 | `2026-09-15-phase-4-slice-2-goal.md` | overlay diff; `lapidary://` and launching a tool; render cache and quarantined directories in the storage view; slice 1's debts; bundles if time remains |
| 2 | `2026-09-15-correctness-debt-goal.md` | batched access tracking (DATA §1.4); phantom bytes in the purge and sweep reports; two jobs racing onto one new path; re-parenting a category; tests never seen failing; the untimed drop path, tree fan-out and link open |
| 3 | `2026-09-15-local-product-goal.md` | custom fields; Turkish search; saved-filter rename, reorder and deleted categories; a capped section cut; watched-folder ingest through the agent; bundles if slice 2 did not reach them |
| 4 | `2026-09-15-occt-goal.md` | the OCCT image and a real kernel beside the native stack; fixture timings, the assembly's section cut and bundle; CAD derivatives on demand; face and edge counts; snapping to cones, spheres and tori; PMI in the view; explode view; Phase 4's exit on Linux; the `Target` trait if time remains |
| 5 | `2026-09-15-materials-and-mass-goal.md` | editable materials; density per material; mass; centre of mass; number ranges and counts per choice for custom fields |
| 6 | `2026-09-15-performance-debt-goal.md` | grid sort off the part row; the job index; idle warm-up loads code only; lazy folder and assembly trees; bundle planning in one query; a file that fails every attempt; watch and symlinks; the menu beside a dialog; the agent's lock test; step 10's manifest; two measurements; housekeeping |

**Decided by the owner, 2026-09-15.**
- **Tiering.** Source tiering is retired, and library files stay raw (DATA §1.2, §1.3).
- **OCCT and FreeCAD wait.** Checks stay on mesh parts with the mock kernel.
- **macOS and Windows watchers** wait for a machine to run them on.

**Decided by the owner after goal 3's code review, 2026-09-15.**
- **Build the OCCT image now** (goal 4). Only its stages are built, after `df -h /` shows 15 GB free, and
  only its own leftover `occt-test` images are removed.
- **STEP timings run on the repo's fixtures**, since the STL corpus holds no STEP or IGES file. Real-file
  timing stays open.
- **The owner installs FreeCAD** for Phase 4's exit.
- **Phase 5's exit is rewritten** (below): typed title and licence, not OpenGraph.
- **Mass:** a density per material, typed in, and a part's material editable like its tags; a file's
  material fills it only while nobody has typed one. Mass is always ≈.
- **The grid's idle warm-up loads the viewer's code only**; the renderer starts on hover.
- **Custom fields get number ranges and counts per choice.**
- **`lapidary up` stays a stub** until the managed-local or Tauri work.
- **The local commits stay unpushed** (210 on 2026-09-15). *(No longer true: goals 1–6 were pushed at
  02:18 on 2026-09-16, leaving `origin/main` at `07b78fc`. A goal is pushed once it merges, when the
  owner asks for it.)*

**Blocked, and what unblocks each.**

| Unblocked by | Items |
|---|---|
| Real-world STEP files on this machine; OCCT itself was **built in goal 4**, which did the rest of this row | Timing Phase 0 and Phase 2 on real STEP files and assemblies; datums no tolerance refers to (OCCT's reader creates only a datum a tolerance refers to) |
| Licence-clean AP242 files from other CAD tools. Phase 4's exit on Linux, which needed FreeCAD, was **met in goal 4's stage 8** | AP242 files written by other CAD tools |
| A macOS or Windows machine | The FSEvents and `ReadDirectoryChangesW` watchers, the Windows overflow rescan, and the rest of Phase 4's exit |
| Pulling a pgvector image | Checking pgvector against `postgres:18`, before Phase 6 (see Open items) |
| Phase 8 | The lifecycle facet, per-user saved filters, auth on locks, `lapidary worker` |

**Open questions, answered 2026-09-15.**
- **Phase 5's exit** is rewritten to what already works; see Phase 5.
- **Mass and centre of mass:** goal 5, with a density per material entered by hand.
- **`lapidary up`** stays a stub.
- **A grid visitor who never opens a part** will load the viewer's code but no WebGL context: goal 6,
  stage 3.
- **Unpushed commits** stay local, by the owner's answer. *(Reversed 2026-09-16: goals 1–6 are on
  `origin/main`, and a goal is pushed once it merges, when the owner asks.)*
- **Old housekeeping:** goal 6, stage 13, rechecks `storage/` (155 MB, now mode 755) and the
  `lapidary_lapidary-blobs` volume read-only, and asks before removing anything.

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

**Re-parenting refused** (`ae4c298`).
- **The route.** `PATCH /api/folders/{id}` answers `400 cannotMove` to any `parentId`, `null`
  included, before it writes anything.
  - A name sent beside it is not applied either.
  - The message says what to do instead: move the category's models.
- **What went:**
  - `renamedAfterMove`;
  - the `wouldCycle` refusal;
  - the route's move-then-rename branch, which no client ever reached.

  `parentId` is gone from the TypeScript `FolderPatch`, so no client here can send one.
- **What stays:** `PgFolders::reparent`, its cycle check and its tests, for when each folder stores
  its own directory path (DATA, "Re-parenting a category").
- **Tests:**
  - A move carrying a rename, and a move to the root, are both refused, and the tree is unchanged.
    Seen failing first, as a 200.
  - The cross-library refusal is now tested through create, the only route left that reaches it.

**Slice 1's revision and lock tests, seen failing** (no code change). For each test, the line it
guards was broken once, the test run, and the line restored. All eight failed as expected, so none
needed a `test/` branch.

| Test | Line broken | Failed with |
|---|---|---|
| `a_revision_goes_on_top_and_the_previous_file_is_set_aside_under_its_own_label` | the aside path takes the new label | `revisions/2/` where `revisions/1/` was expected |
| `a_parent_that_is_no_longer_current_is_a_conflict_and_moves_no_file` | the stale-parent check skipped | no conflict returned |
| `a_failed_file_move_records_nothing` | a failed move commits | 2 revisions where 1 was expected |
| `a_part_deleted_while_its_change_was_measured_is_not_revised` | the deleted-part filter dropped | the deleted part revised |
| `a_part_is_checked_out_once_and_the_next_asker_is_told_by_whom` | a held lock ignored | the second check-out errored instead of naming the holder |
| `a_hobby_library_and_a_missing_part_have_nothing_to_check_out` | the hobby refusal skipped | `Taken` in a hobby library |
| `only_the_held_lock_checks_in_and_then_the_part_is_free` | check-in without `released_at IS NULL` | a second check-in succeeded |
| `a_forced_release_is_recorded_as_forced_and_by_whom` | `forced = false` | `(false, "jonas@laptop")` |

**Measurements** (no code change).
- **The stack:** `lapidary-server` (`mock-kernel`, debug build) as `api` on 8080 and `worker` on
  8081, with `vite preview` in front.
- **Data:** a scratch database, and headless Chrome with a throwaway profile, driven over CDP.
- **The scripts** stay out of the repository, in `target/goal2-check/`.

*Phase 1's drop path.*
- **The corpus.** The 150-file corpus the Phase 1 exit ran on (`/home/jbo/lapidary-ingest-real`) no
  longer exists. The stand-in is the first 150 uniquely named STLs between 20 kB and 3 MB in the
  owner's STL corpus, 270,719,950 bytes. These figures do not compare file for file with the 3.124 s
  scan.
- **How.** `DOM.setFileInputFiles` handed the folder to the grid's `webkitdirectory` input. Handed
  a list of files instead, that input silently does nothing. Each phase is read from the page's
  own requests, and the batch's `startedAt` and `finishedAt` from the API.

| Phase | Time |
|---|---|
| BLAKE3 in the browser, 150 files | 826 ms |
| Probe, to the first chunk sent | 82 ms |
| Transfer, over loopback | 913 ms |
| The commit route | 9,808 ms |
| The batch, started to finished | 19,895 ms |
| **Folder handed over, to the batch finished** | **31,519 ms**: 150 ingested, 0 failed |

- **"Interactive immediately", timed.** While the batch ran, the grid's search was typed once a
  second.
  - 43 round trips: median 18 ms, p95 85 ms, max 117 ms, all 200.
  - With the worker idle afterwards: median 51 ms, p95 101 ms. Slower, because the library by then
    held 150 parts with their thumbnails inline.
- **Found, not fixed:** the commit route verifies and stores each staged file in turn, inside the
  request. That is `store_staged`, then `put_file`: BLAKE3, then zstd. It runs on the async runtime,
  not in `spawn_blocking`. At 9.8 s it is nearly a third of the drop, and the largest phase before
  the worker starts.
  - **Fixed since** (goal 3's open points, `588b0ca`): the files are stored in parallel on the blocking
    pool, and the commit took 1.5 s on the same 150 files.

*The folder tree's fan-out.*
- **How.** Categories were created through `POST /api/libraries/{id}/folders` into fresh libraries,
  flat and as a ten-way tree (depth 2 to 4).
  - The tree fetch was timed from Node, median of 10.
  - First render runs from navigation start to the frame where the sidebar holds a row for every
    category, median of 3.

| Categories | Shape | Tree fetch | Response | First render |
|---|---|---|---|---|
| 100 | flat | 2 ms | 12 kB | 53 ms |
| 100 | ten-way | 2 ms | 15 kB | 60 ms |
| 1,000 | flat | 11 ms | 122 kB | 127 ms |
| 1,000 | ten-way | 13 ms | 156 kB | 131 ms |
| 10,000 | flat | 111 ms | 1.2 MB | 1,389 ms |
| 10,000 | ten-way | 119 ms | 1.6 MB | 1,676 ms |

- **Where it degrades:** between 1,000 and 10,000 categories.
  - The fetch grows with the count, and barely with depth, so the recursive CTE is not the ceiling
    at these sizes.
  - The sidebar puts every category's row in the page at once, and at 10,000 that first render
    takes more than a second.
- **Nothing changed.** A library with thousands of categories would want a lazily rendered tree
  before it wants a closure table.

*A link open.*
- **How.**
  - Nine runs per renderer. Each run is a fresh profile that opens one part cold, then the next
    part warm.
  - The split comes from Resource Timing and the viewer's `lapidary:viewer-first-frame` mark.
  - One Chrome trace per renderer.
  - `WEBGL_debug_renderer_info` confirmed both renderers: SwiftShader, and the RTX 3060 Ti through
    ANGLE's OpenGL backend.
- **Not comparable with the addendum.** These parts are small bases, not the six example parts.
- **Every figure below is a median, measured from navigation start.**

| Navigation start to | SwiftShader, cold | GPU, cold | SwiftShader, warm | GPU, warm |
|---|---|---|---|---|
| HTML received | 126 ms | 121 ms | 4 ms | 3 ms |
| Part fetch sent (it takes 5 to 8 ms) | 165 ms | 158 ms | 28 ms | 23 ms |
| Viewer chunk sent (1 to 13 ms) | 341 ms | 377 ms | 35 ms | 28 ms |
| First rung sent (3 to 4 ms) | 611 ms | 678 ms | 83 ms | 67 ms |
| **First frame** | **671 ms** | **706 ms** | **114 ms** | **76 ms** |

- **A cold link stays over 400 ms on both renderers.** A warm one is well under.
- **Rung transfer and decode do not dominate.**
  - A rung takes 3 to 4 ms to arrive.
  - From its arrival to the first frame takes 59 ms on SwiftShader and 26 ms on the GPU.
  - So L0 and L1 stay unquantized.
- **The largest block is the viewer's shader warm-up,** 270 to 300 ms between the chunk being sent
  and the rung being asked for.
  - The part page calls `warmViewer()` as it mounts.
  - In the GPU trace that block is one task on a fully busy main thread. 225 ms of it is a
    synchronous wait on the GPU process, inside `GLES2::GetGLError`.
  - The log says this Chrome has no `KHR_parallel_shader_compile`.
  - Nothing changed. That warm-up is what made a GPU link 374 ms faster in the addendum, and
    compiling after the rung would spend the same time before the first frame.
- **Not established:** the 170 to 210 ms between the part fetch returning and the viewer chunk
  being sent. The traced run cannot answer it: `vite preview` held its part fetch for 230 ms, a
  stall no other run had, and its main thread was idle.

**Review fixes** (`b8ef753`).
- **What was reviewed.** A fresh reader went through the goal's four code changes. Phantom bytes and
  the re-parenting refusal were clean. The other two had five findings, and four are fixed here.
- **The race fix: an adopted file could be reaped.**
  - **What happened.** A job that adopts identical bytes (step 8) could commit a row naming a file
    that the job which wrote it then reaped, because that job's own insert had failed for another
    reason.
  - **The fix.** A job now reaps its own file only when no live part at its path names that file. If
    that check's query fails, the file stays.
  - **The other side.** The adopter also writes the file back if it is gone after its commit.
  - **Both narrow the window without closing it,** marked `ponytail:`. A lock across both jobs would
    close it.
  - **No test.** The window sits between one job's write and another's commit, which no test can reach
    without a hook.
- **The race fix: an orphan copy.**
  - **What happened.** A lost race with the same bytes, after writing under the disambiguated name,
    settled `skipped` and left that copy behind.
  - **The fix.** The copy now goes, by the same check.
  - **Test:** a loser held in the kernel, with the same bytes, leaves no copy. Seen failing first, with
    "the loser's copy … is left".
- **Access tracking: reads lost at shutdown.**
  - **What happened.** The last flush ran as soon as the signal arrived, while requests were still
    draining. It did not run at all when `serve` returned an error.
  - **The fix.** `main` now flushes after `serve` returns, however it returned.
  - **No test:** this is the server binary's shutdown path.
- **Access tracking: Free cache space.**
  - **What happened.** It read `last_accessed_at` without the reads the api still held in memory, so a
    part opened within the flush interval could lose its L1 and L2.
  - **The fix.** It now flushes first, and refuses if that flush fails.
  - **The storage figure flushes too, on a `GET`, deliberately.** A failure there only warns, because
    the figure is still worth showing.
  - **Test:** open an aged part, then free cache space with no explicit flush; the L1 stays. Seen
    failing first, as `removed: 1`.
- **Recorded, not fixed: a file that fails every attempt.**
  - **The state.** A controlled part in a disambiguated directory is revised, then purged. Its newest
    file then waits at that path, in quarantine, for 30 days.
  - **Why it fails.** Dropping the part's first revision back at the same source path resolves to the
    same directory and finds other bytes there. Every scan fails its three attempts until the sweep
    removes those bytes.
  - **Why that is better than before.** Before the race fix, the write replaced those quarantined bytes
    without a word, so a visible failure is the better of the two.
  - **What would fix it:** when the bytes differ and no part row exists at the path, take a longer
    disambiguated name.
- **No mutation runs.** Both new tests were seen failing first, which the goal accepts in their place.

### Goal 3: the local product (2026-09-15)

- **Goal file:** `docs/superpowers/plans/2026-09-15-local-product-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger.

**Preflight.** The test database was up, and `cargo deny check` was green on `main` (the gate's
`deny` step on `b31ad4f`).

**The spec** (`bc0a464`): `docs/superpowers/specs/2026-09-15-local-product-design.md`.
- It settles all five stages from the goal's defaults, with 15 decisions made without the owner.
- **Checked on `postgres:18` before building on it:**
  - A STORED column may use `to_tsvector(regconfig, …)` over its own row's config.
  - Over 22 real inflection pairs, the `turkish` config matched 12. The `ILIKE` search already there
    covers the prefix cases, so there is no prefix matching.
  - Capital I folds the English way under `en_US.utf8`. Recorded, not fixed.
- **A listing-only poll over the STL corpus** took 109 ms cold and 12 to 13 ms warm.
- **DATA:** §3.3, §3.5 (amended to one GIN index), §3.6 and §6.2 carry the decisions.

**Custom fields** (`2484c17`).
- **Schema.**
  - `custom_field` (`0027`).
  - Values under `part.metadata_json->'custom'`.
  - One GIN index over that object, `jsonb_path_ops`, and filters written as `@>`.
- **Rules.**
  - Keys are `[a-z0-9_]{1,40}` and never renamed.
  - Kinds are text, number and choice.
  - A library holds 32 fields, 8 of them offered as filters, counted under the library's row lock.
  - An option some part holds is not removed.
  - A removed field's values stay.
- **API.**
  - Routes: `GET` and `POST /api/libraries/{id}/fields`, `PATCH` and `DELETE …/fields/{key}`, and
    `PUT /api/parts/{id}/fields/{key}`.
  - The grid, its facets and saved filters take `field` and `fieldValue`, and refuse a field not
    offered as a filter.
  - The part detail carries `custom`.
  - `set_metadata` now merges `cad` instead of replacing the object, so a value survives the file's
    own statement.
- **UI.**
  - A Fields dialog in the library menu.
  - A Fields section on the part page.
  - A filter for each offered field beside the facets, carried in the URL.
- **Tests:**
  - **db:**
    - the cap of 8;
    - a duplicate key;
    - an option in use;
    - a removed field's values;
    - `set_metadata` leaving `custom` alone;
    - the `@>` filter through page, sort, search and a facet.
  - **api:**
    - a number field refusing "twelve";
    - a choice narrowing the grid, the facets and a saved filter;
    - a field not offered as a filter;
    - key and option checks;
    - a removed field.
  - **web:**
    - a refused number shown in the server's words, then 12 kept;
    - a key proposed from a Turkish label.
  - **Mutation-checked instead of seen failing first,** since they were written with the code. All six
    were caught:
    - the cap;
    - the option guard;
    - `set_metadata`'s merge;
    - the grid's predicate;
    - a number field's type check;
    - the filter check.
- **Browser check,** on the native stack in headless Chrome, through the UI only. No page errors.
  1. Supplier (a choice, offered as a filter) and Stock count (a number) were defined in the dialog.
  2. They were set on two example parts' pages: `hex-spacer-m4x20-lp-2145-01` got Misumi and 12, and
     `flange-dn40-lp-3310-02` got Hoffmann.
  3. The grid, filtered by Supplier → Misumi, showed one card. Its URL carried
     `field=supplier&fieldValue=Misumi`.
  4. The filter was saved as "From Misumi".
  5. Reopened from the unfiltered grid, it showed the same one card.
- **Noticed, not changed:** the library menu stays open beside a dialog it opened, as it already does
  for a new library.
- **Not in this stage** (spec §1.5):
  - number ranges;
  - facet counts per value;
  - `metadata.json`, which learned an edited value only on its next rewrite; fixed since (`fd3c018`).

**Turkish search** (`aad407e`).
- **Removed since** (`8bb5ea4`, migration `0031`), at the owner's word. What follows is what was built.
- **Schema** (`0028`).
  - `library.language`, `simple` or `turkish`.
  - `part.search_config`, copied from the library by the insert that makes a part.
  - `part.search` recomputed over it with `SET EXPRESSION`, as `0022` did. The column stays STORED
    and its index stays.
- **Queries.** Search and the three facets build their `tsquery` with the library's own language.
  The `ILIKE` substring search is unchanged.
- **API and UI.** `POST /api/libraries` takes `language`, defaulting to `simple`, and refuses an
  unknown one. The create-library dialog asks for it.
- **Tests:**
  - **db:**
    - a Turkish library finds "Şaft yatağı kapağı" by "yatak kapak" and by "yataklar kapakları";
    - a `simple` library finds it by neither;
    - the part holds its library's config;
    - a facet counts what search finds.

    Seen failing first: while the queries still used `simple`, the inflected query found nothing and
    the facet counted 0.
  - **api:** a library made with `turkish`, `simple` when none is asked for, and an unknown language
    refused.
  - **web:** the dialog sends the chosen language. Mutation-checked: dropping it was caught.
  - The existing search tests ran unchanged.
- **Corrected in the spec:**
  - The column is not dropped and re-added.
  - The base-form query passes before the change, so the inflected one is the test.
  - No move writes `search_config`, so there is no move test.
- **No browser check.** The goal names tests and the dialog choice for this stage.
- **Not in this stage:**
  - changing a library's language after it is made;
  - capital I under `en_US.utf8`, recorded in the spec's §2.1.

**Found by the gate, and not reproduced** (`621f57f`).
- **What failed.** The Turkish search branch's first gate run failed one test,
  `losing_the_race_for_a_file_is_a_skip_rather_than_a_failure`. Both concurrent jobs ingesting one
  file succeeded, and neither was `skipped`.
- **The reasoning.** The only outcome left is `unkept`. Step 3b gives it when step 3 asked before the
  other job committed and step 3b read after. Two queries leave that window open.
- **The guard.** Step 3b now skips a part whose current revision already holds these bytes, which is
  step 3's own answer. No other outcome changes.
- **Not reproduced.** Without the guard the test passed 30 runs alone and 10 runs of the whole handler
  file, and it passed as many with the guard. The guard rests on the reasoning, not on a measured fix.
- **Next time.** The test now prints both outcomes when it fails.

**Saved filters, finished** (`ce894a6`).
- **Order** (`0029`).
  - `saved_filter.position`, backfilled in the order filters were made.
  - A new filter goes last.
  - A move swaps one place under the library's row lock, renumbering first, and does nothing at either
    end.
- **Rename.** `PATCH /api/libraries/{library}/filters/{filter}`, by the rules a new filter's name
  follows, and still unique in the library.
- **A deleted category.**
  - The list route reports `folderGone`. It is read on every list, so a restored category clears it.
  - The list marks the filter.
  - A grid opened on a category the live tree does not hold says the category was deleted, and offers
    the same filters without it, instead of an empty grid.
  - The check is the grid's, so an old link is covered too.
- **Tests:**
  - **db:**
    - the list in saved order;
    - rename, and a taken name;
    - moves in the middle and at both ends;
    - a deleted category marked, and a restored one not.
  - **api:**
    - rename, a taken name, and an unknown filter;
    - moves up, and at the top;
    - `folderGone` after the category is deleted through its route.
  - **web:**
    - rename and move send their requests;
    - the arrows at either end are disabled;
    - the mark shows;
    - the grid's notice clears only the category.
  - **Mutation-checked, all seven caught:**
    - the list order;
    - the move's swap;
    - the deleted-category mark;
    - rename's taken-name refusal;
    - the API's `folderGone`;
    - the grid's notice;
    - the up button's direction.
- **Browser check,** on the native stack in headless Chrome. No page errors.
  1. A category, and a flange filed in it, were set up through the API.
  2. "Flanges in STL" and "All STL" were saved through the list.
  3. "All STL" was moved up and renamed "Every STL".
  4. The category was deleted, and the list marked "Flanges in STL".
  5. Opening it showed the notice, not "Nothing filed here yet".
  6. The notice's way out dropped the category, kept `format=stl`, and showed the 5 parts left.
- **Found by the browser check, and fixed.** Beside the mark, the filter's name was cut to "Flang…" in
  the narrow list. The mark now sits under the name, and the name shows in full.

**A capped section cut** (`13fd3bf`).
- **The cap** uses three's clipping-stencil technique, over the current rung.
  - The renderer now asks for a stencil buffer, which three 0.186 no longer gives by default.
  - Each mesh gets two stencil passes sharing its geometry, clipped by the section's plane: back faces
    count up, front faces count down. Neither writes colour or depth, and a raycast passes through
    them.
  - A flat plane on the cut, placed by `capPlacement`, is drawn where the count is not zero, in
    `0xc4665a`.
  - Draw order: the stencil passes, the cap, the part, the ghost, the marks.
- **Only closed meshes.** A part whose `isWatertight` is not `true` gets no cap. The section bar says
  why, in words that tell "the mesh is open" from "whether it is closed was not measured".
- **Not picked, and not on the ghost.** The cap lies outside the model that picks and wall rays are cast
  at, and the ghost stays uncapped.
- **Tests:**
  - `capPlacement` puts the cap on the cut, over the whole box, whichever side is kept and whichever
    axis.
  - The bar's note for an open mesh, for an unmeasured one, and none for a closed one.
  - Mutation-checked, both caught: the cap left at the box's middle, and the note suppressed.
- **Browser check,** on SwiftShader with the native stack, cutting the flange at Z = 8 mm:

| | Result |
|---|---|
| Cap-coloured pixels: uncut, cut, off, cut again | 0, 21,567, 0, 21,567 (of 350 × 350) |
| The bore, at the middle of the cap's ring | 0 of 49 sampled pixels cap-coloured |
| Shader programs linked: by the first cut, by cutting again | 4, then 0 (2 before cutting) |
| Click to the next frame: first cut, cutting again | 41 ms, 21 ms |

  - Programs were counted by wrapping WebGL's `linkProgram` from outside the page, so nothing in the
    product exposes its renderer.
  - The screenshots show a brick-coloured ring with the bore and all four bolt holes open.
  - No page errors.
- **Not checked: a part hidden in an assembly.** Its stencil passes draw the whole mesh, so a cap likely
  shows across a hidden part's section. Cutting assemblies still needs OCCT.
  - **Fixed since** (`8a1c239`): the passes leave a hidden part out. Seeing it in a browser still needs an
    assembly, and so OCCT.

**Watched-folder ingest through the agent** (`f737b6b`).
- **The command:** `lapidary watch <folder> --library <id>`, on Linux.
  - Every 2 s the folder is listed recursively, without following symlinks.
  - DATA §6.2's ignore list applies whole. What a model file is now lives in `lapidary-core`, so the
    scan and the agent read one list.
  - A change settles for 2 s and is hashed before anything is believed. A hash equal to the one last
    sent is not a change.
- **Uploads.**
  - The probe, the 8 MiB chunks and the commit came out of check-in, which now shares them. Watch
    sends no lock.
  - Files that settle in the same poll go up together, in uploads of at most 64 MiB each, so a first
    run over a large folder does not hold it all in memory.
  - The source path is the file's path under the folder.
  - The agent prints the batch's counts, and each failure with its path.
- **A deletion** is printed once and forgotten. Nothing is sent.
- **State:** `$XDG_STATE_HOME/lapidary/watch-<library>.json` keeps each path's size, mtime and BLAKE3
  as last sent. Nothing is written inside the watched folder. A first run uploads the whole folder,
  and the probe skips the bytes the server already holds.
- **The ceiling,** marked `ponytail:`, is the spec's measurement above: 12 to 13 ms a poll, warm, over
  the corpus's 2,778 files. `notify` replaces the poll when a tree is large enough for that to matter.
- **Tests:**
  - the ignore list;
  - source paths, nested, and never leaving the folder;
  - a deletion that sends nothing;
  - after a restart, an unchanged file sends nothing, and a file changed while stopped is hashed;
  - uploads split under 64 MiB;
  - a model file known by its extension in any case.
  - Mutation-checked, all 7 caught: `.lck` dropped from the ignore list, a `..` skipped instead of
    refused, the deletion filter inverted, deleted paths sent, every file starting unseen after a
    restart, an upload's running total ignored, and extensions compared case-sensitively.
- **Check,** on the native stack with the mock kernel. A copy of `example/parts` in `target/` was
  watched into a new controlled library:

| Step | Result |
|---|---|
| Start, with no state | all 6 files in one upload: ingested 6, parts listed 3.1 s after start |
| Add `hex-spacer-m4x30-lp-2146-01.stl` | ingested 1; 7 parts, 3.6 s after the copy |
| Change `hex-spacer-m4x20-lp-2145-01.stl`, bore 4.2 → 4.5 mm | revised 1; revision 1 → 2, 4.7 s after the copy |
| Write `flange-dn40-lp-3310-02.stl.tmp` | nothing printed or sent in 10 s; 7 parts |
| Delete `vee-block-lp-3072-02.stl` | printed once; the part is still on the server; 7 parts |
| Restart over the same state | nothing printed but the start line in 12 s; 7 parts |

  - The state file named the six files still present, and the watched folder held only the files put
    there.
- **Found by the review, and fixed below:** a file the library refused, such as a change to a part
  checked out to somebody, was recorded as sent, so neither the next poll nor a restart sent it again.

**Bundles** (the goal's stage 7). Slice 2 shipped export (`da9ff39`) and import (`4d3e05c`), so this goal
builds nothing for them.

**The review.** A fresh reader went over `bc0a464..f737b6b`. It found five defects and one minor one, and
each was checked against the code before anything changed.
- **Fixed:**
  - **A refused file was recorded as sent** (`3c3ce5f`). `lapidary watch` recorded every file of an accepted
    upload, the ones the batch failed included.
    - Now a refused file stays out of the state and is sent again after 5 minutes, sooner if it
      changes, and on a restart.
    - Refusals are matched by path. When the batch lists fewer failures than it has, or a failure names
      a path not sent, the whole upload counts as refused, and the probe skips what the server holds.
    - Tests: a refused file is not recorded, and is sent again only once the wait is over; failures
      that cannot all be matched refuse the whole upload. Mutation-checked, all 4 caught.
    - A test pins that a failed upload names its path, which the matching reads. It passed before any
      change, because an upload's payload already writes `source_path` as `path`.
    - **Check,** on the native stack. The flange was checked out, then changed in the watched folder.
      The refusal was printed with its path, the state kept the old hash, and the part stayed at
      revision 1. After check-in and a restart of the watch, the change went up: revised 1, revision 2.
  - **A field filter the library no longer takes broke the grid** (`d9d3147`). A saved filter or link naming
    a field no longer offered as a filter, or a value that no longer fits it, is refused by the server.
    - The grid said "check that the api service is running", which was untrue. The facets failed with
      it, and the facets are where the filter is cleared.
    - Now the grid says the field no longer filters it, and offers the same filters without the field,
      as it does for a deleted category. The server's refusal is unchanged.
    - Test: the notice shows instead of the failure line, and its way out clears the field. With the
      check disabled, the test fails.
  - **An option only removed parts hold** (`2450275`). The refusal said to change those parts' values, which
    a removed part does not allow. It now says how many are removed, and to restore them first.
    - Removed parts still count, because a restored part brings its value back.
    - Test: a removed part holding the option. With removed parts not counted, the test fails.
  - **The minor** (`d9d3147`). A field's filter box kept old text after the filter was cleared or another
    was applied. It is now keyed by the value in force.
- **Recorded at first, and fixed afterwards** (see the open points below): setting a value raced
  removing its option, and a field defined again under a removed field's key adopted its values whatever
  their kind.

**The open points.** The goal stayed open on the two findings above and on the known gaps. Each is built
with a test that a mutation turned red, or left with its reason.
- **Setting a value no longer races removing its option** (`6113ae2`).
  - `set_value` share-locks the field's row, reads it again, and writes only while the field is what the
    value was checked against. `update` locks the same row before it counts the parts holding an option.
  - A value that loses answers 409 `fieldChanged`.
  - Tests: a write waits for a removal's lock, then finds the field changed; a removal waits for a
    write's lock, then counts the part. Mutation-checked, all 3 caught: the share lock, the re-check and
    the removal's lock.
- **A field defined again takes back only the values it can show** (`6113ae2`). Values it could not show
  refuse the key with 409 `valuesDoNotFit`, naming how many parts hold them. Nothing is converted or
  removed.
  - Test: text values refuse a choice and a number under their key, and come back under a choice that
    offers them. Mutation-checked.
- **A section's cap leaves out a hidden part** (`8a1c239`). The stencil passes share each mesh's geometry
  but kept a single material, and three draws a mesh's groups only for an array.
  - Test: on the scene graph, a hidden part's passes take the array its mesh draws with. Mutation-checked.
  - Not seen in a browser: an assembly needs OCCT.
- **A custom value reaches `metadata.json` when it is set** (`fd3c018`). Setting a value queues a
  `describe_part` job, and the worker writes the file again from the rows. Outcome `described`, `0030`.
  - Tests: the job writes the value and refuses another library's part; the route queues the job.
    Mutation-checked, both caught.
- **The upload commit stores a drop's files in parallel** (`588b0ca`). Each file's hashing and
  compression run on the blocking pool, as many at once as there are cores (one fewer since the code
  review below), and each set of bytes is stored once.
  - Test: 13 files, one a copy, store 12 blobs and queue 13 jobs. With copies stored twice it failed 2
    runs of 3, since that is a race.
  - Measured on the Phase 1 stand-in, 150 STLs and 270,719,950 bytes, with the api alone on a debug build
    over 12 cores. Two of the before runs were meant as after runs, but a failed build left the old binary
    in place, so they timed the old code.

| The commit route | Runs |
|---|---|
| Before | 9,849 ms, 9,552 ms, 9,709 ms, 9,666 ms |
| After, as many at once as cores (`588b0ca`) | 1,525 ms, 1,516 ms |
| At most 4 at once (`f67184c`) | 2,634 ms, 2,708 ms |
| One fewer than the cores, 11 here (`455876b`) | 1,609 ms, 1,571 ms |

- **Capital I in Turkish search: not fixed, by the owner's answer.** Asked on 2026-09-15, the owner said
  Turkish search is not needed and regular word search is fine. Spec §2.1's record stands, and nothing was
  built for it. After the code review they chose to remove Turkish search itself (`8bb5ea4`).

**The code review.** A background reviewer went over goal 3 (`ae0cdbc..80db719`), looking for code that
is wrong, redundant, or slower or larger than it needs to be. It returned 15 findings, and each was
checked against the code first. Each fix has a test that a mutation turned red, or a check that was run.
- **Turkish search, removed at the owner's word** (`8bb5ea4`, `0031`). This closed two findings.
  - `pg_upgrade` refuses `0028`'s `regconfig` column in a user table.
  - A `tsquery` built from a library's language could not fold to a constant, so it was parsed again for
    every candidate row.

  Search is `simple` again. Test: no user table holds a `reg*` type; with `0031`'s drops taken out, it
  fails on `part.search_config`.
- **Fixed:**
  - **A field's box kept the first value it showed** (`16009d4`). A value changed elsewhere and read back
    left the old draft in the box, and a blur wrote it back.
    - The draft now follows the stored value, and the row is keyed by part and field.
    - Test: saving another field reads the part again, and the box shows the new value. Mutation-checked.
  - **A field whose key is all digits lost its filter from the URL** (`9aad6eb`). Mutation-checked.
  - **Locking a library's row blocked every insert that references it** (`8bd1234`).
    - Defining a field and moving a saved filter held the row `FOR UPDATE`, which waits out every part,
      job and folder insert's `FOR KEY SHARE`.
    - They now take `FOR NO KEY UPDATE`. The two lock-order tests pass unchanged, but no test shows an
      insert getting through.
  - **Two watches into one library shared a state file** (`fd084ab`). It is now named by library and
    folder. Test on the name, mutation-checked.
  - **One stuck batch stopped a whole watch** (`9cef484`).
    - The watch now asks about each batch once a round.
    - A send the server did not answer, and a batch whose status did not arrive, wait 30 s.
    - **Check,** against an api with no worker, so no batch finished:
      - A second file was sent while the first batch was still pending.
      - After the api stopped, each batch and the new file printed one error and waited, rather than
        retrying every round.
    - The loop itself has no unit test.
  - **The probe made two queries per file, and the commit one** (`f67184c`, `ece82e0`).
    - Both now take two queries and one, whatever the manifest's size.
    - The first version matched held files by path, so one path named twice with different bytes had
      both entries answered as held. It answers per entry now.
    - Test: a held path with its own bytes, with other bytes, and its bytes under another path. Taking
      the hash match out fails it, and 6 ingest tests.
    - Into an empty library, the batched probe took 78 ms and 76 ms over the 150 files above, and 74 ms
      and 83 ms once it answered per entry. The per-file version was never timed, so there is no
      comparison.
  - **A commit could take every core** (`f67184c`, `455876b`).
    - The first fix stored at most 4 files at once. On this 12-core machine that cost about a second
      over 150 files (the table above). What it was for was never measured: how much a busy commit slows
      the rest of what the api serves.
    - A commit now stores one fewer file at once than there are cores, which gave most of that second
      back. Whether the core left over helps is not measured either.
    - Under compose's one-CPU api, `available_parallelism` is 1, so a commit there stores one file at a
      time either way. Raising that limit is a deployment decision, not made here.
  - **Every value saved queued its own description** (`28e0324`). Only one is queued while it waits; a
    running one does not count, since it may have read the rows before the value.
    - Tests: in the db, pending and running; in the api, two values give one job. Mutation-checked, both
      caught.
  - **A description could overwrite a newer one** (`f68818f`).
    - `describe_part` and the rewrite after a revision now read the manifest and write the file while
      the part's row is held.
    - Test: the read waits for a write to the part and holds its value. Mutation-checked.
    - Not held: step 10's first manifest for a new part, which is built from the ingest's own ids.
  - **A scan and a watch ignored different files** (`e9bdb1f`). By the owner's answer, a scan skips DATA
    §6.2's whole list too.
    - Test: a scan skips a `~$` file and a `.bak` folder holding a model. Mutation-checked.
    - Not changed: the watch does not follow a symlinked file, and a scan does.
  - **Bare glyphs on small buttons** (`7205a34`). `↑ ↓ ✎ ×` now come from `strings.glyphs`.
- **Recorded, not fixed:**
  - **A matching hash alone attaches stored bytes to a library** (the upload commit). By the owner's
    answer, this is recorded under Phase 8: with auth, the commit must require the bytes, or a hash the
    caller can reach.
  - **`detail()` reads custom values as text and falls back to `{}`.** Not changed: `jsonb` rendered as
    text is JSON by construction, so the fallback cannot be reached. `PgRevisions::manifest` reads
    `metadata_json::text` the same way, and says so.

### Goal 4: OCCT, and what it unblocks (2026-09-15)

- **Goal file:** `docs/superpowers/plans/2026-09-15-occt-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger.

**Preflight.** The test database was up, `cargo deny check` was green on `main` (`19f2c3f`), and root had
24 GB free.

**The image, and a real kernel beside the native stack** (no code).
- **`cargo xtask verify occt`** built OCCT 8.0.1 and the bridge from source, then ran the kernel tests in
  `occt-test`. It took 849 s, of which OCCT's compile was 766 s, and all 7 tests passed.
  - Root went from 24 GB to 20 GB free. The two pinned base images were pulled as part of the build.
  - **The Phase 0 exit, again:** 113 ms for the 200-part fixture, against 111 ms on 2026-09-13. Kernel
    `occt-8.0.1-bridge-6+deflection-0.1+glb-3+cpu-1`.
- **Copied out, the kernel runs on this host.** `/opt/occt/lib` (74 MB, 148 files) and `occt-bridge` went to
  `target/occt/`.
  - The newest glibc symbol any of them needs is `GLIBC_2.38`, and the host has 2.39. The newest `GLIBCXX`
    is 3.4.33, which the host's libstdc++ has.
  - `occt-bridge version` and `selftest` pass natively with `LD_LIBRARY_PATH=target/occt/lib`.
  - **So the goal's checks run a native worker**, not the compose fallback:
    `lapidary-server --features mock-kernel,occt-kernel` as `LAPIDARY_ROLE=worker`, with `target/occt/bin`
    first on its `PATH` and `LD_LIBRARY_PATH=target/occt/lib`. `target/goal4-check/stack.sh` starts it with
    an api and `vite preview`.

**Checks on the fixtures** (no code).
- **The stack:** the api, a worker with the copied-out kernel, `vite preview` (all debug builds), a scratch
  database, and headless Chrome with a throwaway profile. The scripts stay in `target/goal4-check/`.
- **The files:** the repo's generated fixtures, not real-world files, by the owner's answer.

| Check | Result |
|---|---|
| Phase 0 exit, `verify occt` | 113 ms; 118 ms and 117 ms on stage 4's rebuilds |
| Scan of `fixtures/step` (4 STEP, 1 IGES) into a controlled library, request to finished batch | 754 ms: 5 ingested, 0 failed |
| Each STEP file saved again (its header's time stamp changed), scanned | 458 ms: 4 revised, 1 skipped |
| Bundle of those 5 parts and 9 revisions | 455,560 bytes, exported in 22 ms |
| Imported into another controlled library | 1,115 ms: 5 ingested, 0 failed |
| Lineage, row by row | identical: labels, parents, origins and source hashes; each current revision's derivative kinds too |
| Section cut along Z through the imported 200-part assembly: cap pixels on a 350 × 350 canvas | 0 uncut, 4,389 with every part shown, 2 with one stop pin isolated, 4,389 with every part shown again |

- **Phase 5's "40-part assembly"** is read as met by one assembly of 200 placed parts, exported and imported
  with its lineage. Decided without the owner.
- **A hidden part's section is not filled** (`8a1c239`, now seen). With 199 of 200 parts hidden, 2 cap
  pixels were left where 4,389 had been. The screenshots show the pin alone and uncapped. SwiftShader, no
  page errors.

**CAD derivatives on demand** (`bc4f06e`).
- **The sweep.** `enqueue_stale_rungs` is now `enqueue_stale_derivatives`. Besides the rungs, it queues a
  CAD source's `structure` row when a different kernel version wrote it.
- **The derive.** A derive of structure, entities or PMI asks the kernel for all three, since one bridge run
  reads them together. It writes whichever came back, `structure` last, so a job that stops partway is found
  again.
- **Why `structure`, not a missing `pmi` row.** A STEP file that specifies no PMI has no PMI row, as ingest
  writes it. So a sweep for missing rows would queue such a file at every start, and its job would fail as a
  bug. The `structure` row carries the version of the bridge that read the file, and the stale-rung sweep
  never rewrote it. Once it is rewritten at the current version, a file with no PMI is not queued again.
- **Tests:**
  - **db:** the sweep queues an old STEP read by its `structure` row, beside its old rung, and never an
    entities row on its own.
  - **ingest:** a part whose PMI row is gone and whose tree an older bridge wrote gets all three back from one
    job, at this kernel's version, and is not queued again.
  - **Mutation-checked, all 3 caught:** `structure` dropped from the sweep (both tests), and the derive
    writing only the kind it was asked for.
- **Checked on the native stack** with the OCCT worker.
  - On the AP242 cylinder's current revision, the PMI row was deleted and the tree and entities were aged to a
    `bridge-5` version, so the part's page served no PMI.
  - After a worker restart, the sweep queued 1 read, and its job finished 326 ms after the restart.
  - All three rows came back at `bridge-6`. The page's PMI hash was the one it had before, listing ⌀22
    +0.05/0, flatness 0.02 and perpendicularity 0.05 to A.

**Face and edge counts** (`718d8ad`, `3fceac7`).
- **The bridge,** now bridge 7, counts every face and edge of the placed shape into `measurements.json`:
  analytic or not, and once per placed instance.
  - Read natively: the ⌀22 cylinder has 3 faces and 3 edges, and the 200-part assembly has
    1011 faces and 1437 edges.
- **Stored on the revision** (`0032`: `face_count` and `edge_count`, both or neither).
  - Written after the revision commits, for a new part and for a revision, warn-only as the header is.
- **Read again for older parts.** Bridge 7 makes the stale sweep read every older CAD part again, and that
  read now writes the counts too, so a part ingested earlier gets them without a new ingest.
- **The diff** compares them exactly, and the Compare table has Faces and Edges rows. A mesh has no counts,
  and a pair missing them says so.
- **The two count errors** now say "count", since they guard faces and edges as well as triangles.
- **Tests:**
  - the fake bridge's counts reach the kernel output;
  - counts compare exactly, and only between two B-reps;
  - a revision's counts come back with its history;
  - a STEP ingest writes them, and an STL's revision has none;
  - the re-read of a part with no counts brings them back;
  - the Compare table shows both rows.
- **Mutation-checked, all 5 caught:** the new part's write skipped, the bridge's counts dropped, the face
  delta left out, the Faces row removed, and the re-read's write taken out.
- **On the real bridge** (`verify occt`, 39 s): all 7 tests pass, including the cylinder's counts.

**Snapping to cones, spheres and tori** (`2b19af8`, `b58d4f2`; fixture `95370a0`, `a43bc98`).
- **What each reads:**
  - a sphere, its diameter, exact;
  - a torus, its tube's diameter, the size a fillet or an O-ring groove is drawn with, exact;
  - a cone, its included angle, exact, from the click on it;
  - a cone's diameter at the height of the click, approximate, since the height comes from the mesh.
- **Decided without the owner:** a cone reads its included angle, not the goal's default half-angle, since a
  drawing states a countersink as 90°, not 45°.
- **A fixture with those surfaces.** `generate-fixtures` now also writes `ball-knob-d20-lp-9020-00.step`: a
  ⌀20 ball on a ⌀12 shaft, a 90° countersunk tip and an O-ring groove of ⌀3 section. The other fixtures were
  not regenerated. The native bridge reads 5 faces and 11 edges, and 7,571.5 mm³.
- **Tests:**
  - **web:** a ball's and a tube's diameters; a countersink's angle, and its diameter at the click; a cone
    clicked after another face; a triangle off a surface snapping to nothing.
  - **kernel** (`verify occt`, 40 s, 8 tests): the knob reads a sphere of radius 10, a cone at 45° to its
    axis, and a torus of 1.5 mm section on a 6 mm ring.
  - **Mutation-checked, all 6 caught:** sphere snapping dropped, the ring read for the tube, the half-angle
    reported, a cone snapping from anywhere, a cone's diameter called exact, and the angle asked of the first
    click only.
- **Checked in Chrome** on the native stack, clicking down the middle of the knob (SwiftShader, no page
  errors):

| Tool, where | Reading |
|---|---|
| Diameter, the ball | 20.000 mm |
| Diameter, the shaft | 12.000 mm |
| Diameter, the O-ring groove | 3.000 mm |
| Diameter, the countersunk tip | 2.389 mm ≈ |
| Angle, the countersunk tip | 90.000° |

- **Found by that check, and fixed** (`b58d4f2`). The angle tool asked only its first click whether it was on a
  cone, so a click on the cone that came second was measured against the first, approximately. The latest
  click decides now.

**PMI drawn in the view, on analytic faces** (`d0a9a86`).
- **A toggle** beside the Dimensions and tolerances list, "Show in the 3D view", off by default, draws each
  annotation beside the face it names.
- **Where a label sits:**
  - a plane, at its origin;
  - a cylinder, cone or torus, at a point on the rim it starts from;
  - a sphere, at its top.

  Each is placed through the assembly tree as measurement places entities, once per placed instance.
  Annotations on one face share one label, a line each.
- **Not drawn:** an annotation on a freeform face, which has no entity to say where it is, and one on the whole
  part. Once the toggle is on, the list says which and why.
  - Recorded: per-face triangle ranges in the GLB would place a freeform face's annotations.
- **How:** DOM elements that three's `CSS2DRenderer` keeps over the canvas. No new dependency, and a label never
  takes a click.
- **Tests:**
  - labels grouped per face and placed per instance;
  - an anchor for each surface kind;
  - an annotation on the whole part, and one on a face with no entity, not drawn;
  - the toggle handing the view its choice, while the list marks what was not placed.
- **Mutation-checked, all 4 caught:** a cylinder's label at its origin, a label per annotation, the whole part
  counted as drawn, and the note shown before the toggle.
- **Checked in Chrome** on the native stack, with `cylinder-d22-pmi-lp-9012-00.step` (SwiftShader, no page
  errors):
  - no labels before the toggle, and none once it was turned off again;
  - with it on, three labels, each inside the view: "⌀22 mm +0.05 / 0" with "⟂ Perpendicularity 0.05 mm to A"
    on the cylinder, "⏥ Flatness 0.02 mm" on the top face, and "Datum A" on the base;
  - the list marked nothing as not drawn, since every face of that part is analytic.
- **Seen, not changed:** a label is drawn over the part whether or not its face is turned towards the viewer, as
  a measure mark is. So "Datum A", at the base face's origin, shows through the cylinder.

**An assembly drawn apart** (`f128516`).
- **A slider,** "Explode", under the section controls, offered only for a rung that counts its parts
  (`extras.parts`). At full, each placed part moves straight out from the assembly's centre, by as far again as
  it already is.
- **How:** a mesh per placed part that shares the rung's buffers and draws only its own run of the index, so
  nothing is copied. The pieces are built on a rung's first explode and dropped with the rung.
- **While the parts are apart:**
  - hidden parts stay hidden;
  - measuring is off, and the bar says why: a distance between moved parts is not one on the assembly;
  - a section's cap is not drawn, since it fills the assembly's section and not where the parts are. Decided
    without the owner.
- **Tests:** a part's centre from its own triangles; offsets at none, half and full; the tools off, with their
  reason, while apart; and the slider's scale.
- **Mutation-checked, all 4 caught:** every centre at the origin, the amount ignored, the tools left on while
  apart, and the slider's scale.
- **Checked in Chrome** on the native stack with the 200-part fixture assembly (SwiftShader, no page errors).
  Pixels not in the view's background, of 122,500:

| Slider | Drawn pixels | Measuring |
|---|---|---|
| 0, assembled | 34,456 | on |
| 0.5 | 46,135 | off, with its note |
| 1 | 47,806 | off, with its note |
| Back to 0 | 34,456 | on |

  - Put back, the view draws exactly as many pixels as assembled.
  - The screenshots show the brackets, screws, dowels, V-blocks, pins and feet each moved out along its own line.
- **Seen, not changed:** the camera keeps the assembled framing, so at full some parts leave the view until it is
  zoomed out.

**Slicers get 3MF or STL, through the `Target` trait** (`600f745`, `22ce8ad`, `de9ea24`, `82e88d0`, `aa1bb18`).
- **Exports are derivatives.** `export_stl` and `export_3mf` are written in Rust from the mesh the kernel read (the
  bridge's own for a CAD file), never from a rung:
  - a binary STL;
  - a 3MF with shared vertices, stored ZIP entries and `unit="millimeter"`.
  - Written when asked, at `POST /api/parts/{id}/exports/{format}`, answered as a rung request is. The stale sweep
    rebuilds one an older kernel wrote.
  - Decided without the owner: no bridge `export` command, which the plan named. The mesh the bridge already hands
    over is the one a slicer would get either way.
- **Negotiation,** in `lapidary-targets`: a `Target` reads some formats, and `negotiate` hands it the original when
  it reads the part's format, else the first mesh export it reads, else a refusal naming what there is. A tool that
  reads only B-rep is never handed a mesh.
- **Two callers:**
  - **Downloads.** A format is a one-format `Target` (DATA §5.1):
    - the part's own file when it is already in that format;
    - else a `*.lapidary.*` export, a 404 naming its route until it is written;
    - and a 400 for a B-rep format on a mesh part.
  - **`lapidary open`.** This computer's apps are the target, as `xdg-mime query default` names them. When none
    opens the part's format but one opens 3MF or STL:
    - the export is written, checked against its hash, and opened read-only from `exports/` in the workspace;
    - no lock is taken, since nothing saved from it can come back. Decided without the owner;
    - the agent says, and notifies, that nothing saved from it comes back.
- **Part page:** "3MF for a slicer" beside Download, on a part whose file is neither STL nor 3MF. It asks, watches
  the batch, then links to the download.
- **Found and fixed:** serde's `snake_case` spelled `Export3mf` as `export3mf` in the job payload and the bindings,
  where `derivative.kind` holds `export_3mf`. It is named by hand now, and a test pins every kind's serde name to
  its database string.
- **Tests:**
  - negotiation's outcomes, OBJ refused as a mesh Lapidary does not write, every extension ingest records being a
    named format, and each format's MIME types;
  - the download: the original in its own format, the export, the 404, and the B-rep and OBJ refusals;
  - the export route's queue and refusal;
  - a derive job writing both files with every triangle, counted by readers that share nothing with the writers;
  - `open`'s hand-over for each set of apps;
  - the part page's button, batch and link;
  - an old export freed with the render cache.
- **Mutation-checked, all 10 caught:** the original handed over whatever the tool reads, the apps ignored, the
  original refused in its own format, every format queued as an STL, the button on an STL part, every export stored
  as an STL, the rename removed, OBJ refused as needing B-rep, `application/step` dropped from STEP's types, and
  exports left out of the cache.
- **Checked on the native stack** with the real bridge, on `ball-knob-d20-lp-9020-00.step`:

| Export | Before it is written | Ask to finished batch | Download | Read back by Python's zip and XML readers |
|---|---|---|---|---|
| 3MF | 404 naming `exports/3mf` | 192 ms | 107,669 bytes, `ball-knob-d20-lp-9020-00.lapidary.3mf`, BLAKE3 as answered | 1,686 triangles, millimetres, 19.893 × 19.941 × 54.000 mm |
| STL | 404 naming `exports/stl` | 191 ms | 84,384 bytes, `ball-knob-d20-lp-9020-00.lapidary.stl`, BLAKE3 as answered | 1,686 triangles, 19.893 × 19.941 × 54.000 mm |

  - Ingest recorded 1,686 triangles and an exact 20 × 20 × 54 mm box. The mesh's box is up to 0.11 mm under 20
    because the ball is drawn with chords.
  - `variant=step` on the same part: the fixture, byte for byte, under its own name.
  - `lapidary open` under throwaway XDG dirs, never the real `mimeapps.list`, with stand-in apps that record what
    they were started on:
    - a slicer for 3MF and no app for STEP: the 3MF at `exports/ball-knob-d20-lp-9020-00_1.lapidary.3mf`, mode
      `r--r--r--`, BLAKE3 as the server answered, in 289 ms. No lock was taken, and the agent printed its note;
    - a CAD app beside the slicer, declaring STEP only as `application/step`: a check-out of the `.step` file,
      opened in the CAD app with its lock taken, then checked in. Before the fix below, that computer was handed the
      3MF;
    - the same app declaring `model/step`: the same.
  - In Chrome, on the PMI cylinder: "Writing the 3MF…", then "Download the 3MF" 1,217 ms after the press. The link
    answered 200 with `cylinder-d22-pmi-lp-9012-00.lapidary.3mf`, 8,518 bytes. No page errors.
- **Found in review, and fixed** (`82e88d0`, `aa1bb18`):
  - `open` asked for an app under `model/step` alone, and this desktop's MIME database (shared-mime-info 2.4) has no
    STEP type at all. A CAD app declaring `application/step` went unseen, so a slicer beside it was handed the 3MF.
    `open` now asks under every spelling on record for each format, and hands over the part's own file when any
    answers.
  - `variant=obj` on an STL part was refused as needing B-rep. OBJ is a mesh Lapidary does not write, and is now
    refused for that.
  - Nothing could ever free an export. "Free cache space" now takes 3MF and STL exports nobody has read in 90 days,
    as it takes L1 and L2, and its wording names slicer files beside the previews (DATA §1.5). Decided without the
    owner.
- **Still true:**
  - an app declaring STEP under a spelling not on that list is missed;
  - `xdg-open` reads a `.step` file as `text/plain` here, whatever the agent decides. Stage 8 met this, and
    `open` now starts the app it found by name (`57598c8`).

**Phase 4's exit on Linux, met** (stage 8; `57598c8`).
- **When:** FreeCAD was not installed when stage 8 first came up, so the goal went on to stage 9 by the owner's
  answer. The owner installed it at the end of the goal: FreeCAD 0.21.2 from the distribution, whose desktop file
  declares `model/step`.
- **Found, and fixed** (`57598c8`): the link's check-out would have opened in GNOME Text Editor.
  - `xdg-open` picks an app by the type it detects in the file, and this desktop's MIME database has no STEP type,
    so a `.step` file is `text/plain` whichever app declares STEP.
  - `open` now starts the app it found for the format by its desktop file, with `gio launch`, and falls back to
    `xdg-open` when it found none or `gio` cannot start it.
- **The check:** on the native stack with the OCCT worker. The desktop side ran under throwaway XDG folders that
  still see the system's apps (`target/goal4-check/stage8.sh`).

| Step | Result |
|---|---|
| `lapidary register` | `lapidary://` links to `lapidary-url.desktop`, `model/step` to FreeCAD's desktop file; a `.step` file still reads as `text/plain` |
| The part's `lapidary://open` link, handed to `xdg-open` as a browser hands it over | a check-out of `cylinder-d22-lp-9010-00.step` with its lock taken, and FreeCAD started on that file, in the same second: the lock at 15:41:44.561, the file at .608 |
| FreeCAD's own STEP exporter bores a ⌀8 mm hole through and writes over the checked-out file | FreeCAD read 11,403.981 → 9,896.017 mm³, 3 → 4 faces, 3 → 6 edges |
| `lapidary agent`, running | "was saved; sending it back". Revision 2, origin `agent`, parent revision 1, listed 2,503 ms after the write |
| Revision 2 against 1 | volume −1,507.964 mm³ (−13.2%), surface area +653.451 mm², box unchanged, 3 → 4 faces, 3 → 6 edges; every figure exact |
| `lapidary checkin` | lock released; the folder keeps its files |

- The new volume is π·(11² − 4²)·30 mm³ to every digit shown.
- **Not by hand:** the change was written by FreeCAD's exporter from its command line (`freecadcmd`), not in the
  window the link opened, so the check could run unattended. Decided without the owner; the window's File → Export
  does the same.
- macOS and Windows are still open.

**Decided without the owner, across the goal.**
- The stale sweep queues a CAD part by its `structure` row's kernel version, not by a missing `pmi` row.
- Phase 5's "40-part assembly" is read as met by one assembly of 200 placed parts.
- A cone reads its included angle, where the plan said its half-angle.
- No section cap while an assembly is drawn apart.
- Exports are written in Rust from the mesh the kernel read, with no bridge `export` command.
- An export `lapidary open` hands out takes no lock.
- Exports are render cache, freed with L1 and L2.
- `lapidary open` starts the app it found for a format by name, with `gio launch`, rather than letting `xdg-open`
  pick one by the file's detected type.
- Stage 8's change was written by FreeCAD's exporter from its command line, not by hand in its window.

**Recorded, not built.**
- Datums no tolerance refers to: OCCT's STEP reader creates only a datum a tolerance refers to.
- AP242 files written by other CAD tools: there are no licence-clean files here.
- Timing Phase 0 and Phase 2 on real-world STEP files: none on this machine.
- The macOS and Windows parts of Phase 4's exit.
- A freeform face's PMI drawn in the view, which needs per-face triangle ranges.
- An app that declares STEP under a MIME type `open` does not ask for.
- A plane's or cylinder's PMI label on the face itself. It sits where the surface is defined from, which some CAD
  tools put far from the face, and a point on the face needs the bridge to write one.
- An older revision's export: the export route builds the newest revision's, as the rung route does.

**Teardown.**
- **Review.**
  - The advisor reviewed stage 9 before its merge: three findings, all fixed above.
  - A fresh reader, a subagent, then read the whole goal's code (`19f2c3f..62b69e0`): five findings, none high.
    The last two are recorded above. The first three were fixed (`b3ed99f`):
    - **The diameter tool read a rim, exact, from a triangle on the ball or torus beside it:** ⌀6 for a ⌀10 ball,
      because the two tied and the circle came later. A round face now wins, and a ring reads only from a triangle
      on no round face, such as the flat face around a hole.
    - **PMI labels stayed where the assembly drew their faces** while its parts were apart, and on parts hidden in
      the tree. They are off while the parts are apart, and the explode note says so; a hidden part's are not
      drawn. The tree labels are placed from is tested without its hidden parts. Turning labels off while apart is
      not tested, and was not seen in a browser, since no fixture assembly carries PMI.
    - **While a part's faces were loading, or could not be read,** the list said every annotation's face was not one
      the view draws. It now says nothing until they arrive, and says once when they could not be read.
- **Images:** this goal's four dangling `occt-test` images, 2.92 GB each, were removed. `lapidary-occt:goal4`
  (1.14 GB) was kept for the kernel it carries, then removed on 2026-09-16 along with goal 5's: the copy
  at `target/occt/` (74 MB, `bin/` and `lib/`) is what everything actually reads. Root has 18 GB free.

### Goal 5: materials, mass and field filters (2026-09-15)

- **Goal file:** `docs/superpowers/plans/2026-09-15-materials-and-mass-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger, updated as each stage merges.

**Preflight.** The test database was up, `cargo deny check` was green on `main` (`459001b`), goal 4 was merged, and
the copied-out OCCT bridge answered `occt 8.0.1 bridge 7` natively. Each stage's check starts the stack from that
stage's own build.

**A part's material, editable** (`a7b4399`).
- **The route:** `PUT /api/parts/{id}/materials` takes the whole list, cleaned as tags are: trimmed, blanks and
  repeats dropped, at most 8 materials of at most 64 characters each.
- **`part.materials_typed`** (`0033`) is set by the route. An empty list clears it and hands the part back to its
  file at once: the materials its file stated when last read (`metadata_json.cad.materials`). The goal said "on the
  next ingest"; handing them back at once is decided without the owner.
- **The guard:** `set_metadata` fills `materials` from a file only while `materials_typed` is false.
- **Found:** only a new part recorded what its file says. A revised CAD file's header and materials were never
  recorded, so the guard alone would never have been reached. A revision now records them too, under the same guard.
  Decided without the owner.
- **The part page** lists and edits materials beside tags, in hobby and controlled libraries alike, with the same
  list editor, and says when the list is what the file states. The facet is unchanged.
- **Tests:**
  - an ingest test: a typed material outlasts a revision whose file states another, the revision records the file's
    header again, and clearing the list brings the file's material back;
  - an api test: cleaned as tags are, marked typed, counted by the facet, refused past 8, and handed back by an empty
    list;
  - a web test: edited as tags are, with the file's note on a list nobody typed.
- **Mutation-checked, all 5 caught:** the guard removed, clearing handing back nothing, the revision not recording
  its file, the limit raised, and the note never shown.
- **Checked in Chrome** on the native stack, on `bracket-lp-1042-03.stl` in a hobby library (no page errors):
  - before: no materials, and none in the facet;
  - "EN AW-6082 T6" typed and added through the page: listed, marked typed, and counted once by the facet;
  - removed again: the list and the facet were empty.

**Density per material** (`fefc8c4`).
- **The table:** `material_density (library_id, material, density_kg_m3 numeric)` (`0034`), keyed by the material
  exactly as parts hold it and removed with its library. Its check refuses a density outside 0 to 25,000 kg/m³.
- **The routes:** `GET /api/libraries/{id}/densities`, and `PUT` and `DELETE` on
  `/api/libraries/{library}/densities/{material}`. A density that is not a finite number above 0 and below 25,000
  kg/m³ is refused in words, and so is a material no part could hold (blank, padded, or past 64 characters).
  Setting a density in a library that does not exist is a 404; listing one answers an empty list, as the category
  tree does.
- **The Densities dialog,** from the library menu: a box for each material the library's parts hold, and for each
  material that has a density though no part holds it now. Typed and shown in g/cm³, stored in kg/m³, a comma taken
  for the point.
- **Tests:** the bounds and a non-number, with nothing written; an upsert; a removal and a second one; a material no
  part holds; a padded name; a missing library; and the dialog's listing and its g/cm³ to kg/m³.
- **Mutation-checked, all 5 caught:** each bound loosened by one comparison, the upsert made `DO NOTHING`, the
  library check dropped, and the conversion left in g/cm³.
- **Checked in Chrome** on the native stack, with a bracket given "AISI 1045 steel" (no page errors):
  - 30 g/cm³ was refused in the server's words, and nothing was stored;
  - 7.85 g/cm³ was stored as 7850 kg/m³;
  - removed, nothing was stored and the box was empty.

**Mass** (`dc8238e`).
- **Worked out when read, never stored:** a revision's volume times the density of the part's one material as it is
  today (`PgDensities::of_part`), in grams. No mass unless the part holds exactly one material and its library has a
  density for it.
- **Always ≈:** a density is typed, not measured, so even an exact STEP volume gives an approximate mass.
- **The diff** carries the mass change, worked out from the same density on both sides. The history strip shows it
  beside volume's, and the comparison gains a Mass row, with a note saying both revisions use today's material and
  density. The row appears only when there is a mass: a part without one material and its density has no mass to
  compare, which is not a figure "not measured in both". Decided without the owner.
- **Tests:** a 21,478.5 mm³ part of a 7,850 kg/m³ steel reads ≈ 168.6 g, marked approximate; two materials, none,
  or a material with no density give none; a 21,478 mm³ widening changes mass by 168.602 g, approximate; and the
  page shows the change in the history, the comparison and the note.
- **Mutation-checked, all 5 caught:** the one-material rule loosened, mass reported as exact, the gram factor
  wrong, the history's mass change dropped, and the Mass row dropped.
- **Checked in Chrome** on the native stack: a controlled library's bracket (21,472 mm³), rescanned with a spacer's
  bytes (615.88 mm³), given "S235JR steel" at 7,850 kg/m³ (no page errors):
  - the history API read 168.555 g for revision 1 and 4.835 g for revision 2, both approximate;
  - the comparison's Mass row read "−163.72 g (−97.1%) ≈", under volume's "−20.86 cm³ (−97.1%)";
  - revision 2's history line showed the mass change beside the volume change, and the note was there.

**Centre of mass** (`1204353`, `bf2ba27`).
- **Mesh:** summed over the same signed tetrahedra as the volume and divided by the signed total, so a mesh wound
  inside out has the same centre. Closed meshes only, and ≈.
- **STEP:** bridge 8 writes OCCT's `CentreOfMass()` beside the volume, exact. `cargo xtask verify occt` was green
  (bridge 8, 8 tests, its test image removed), and the copied-out bridge answers `occt 8.0.1 bridge 8`.
- **Stored** when a new part, a revision or a derive is recorded, in `revision.mass_props_json` as
  `{"centre_mm": [x, y, z], "source": …}` with the volume's provenance. It needs no density. The kernel hands it
  back beside its measurements rather than inside `MeshMeasurements`, which every kernel and fixture builds.
  Decided without the owner.
- **The diff** carries a change per axis, approximate when either centre is. The comparison gains Centre of mass X,
  Y and Z rows; a revision recorded before centres has none, and its rows say "Not measured in both".
- **Found on the stack:** a cylinder revised into a knob showed its centre X moving "−0 mm (−34,741.9%)", a centre on
  its axis at float noise. A coordinate's percentage says only where the origin is, so a centre's change carries
  none. Any figure's change too small for two places now reads "under 0.01" rather than a signed zero, which "No
  change" would have hidden. Decided without the owner.
- **Tests:**
  - a unit cube moved by (10, 20, 30) centres at (10.5, 20.5, 30.5) whichever way it is wound, and an open one has
    no centre;
  - the bridge's plain cylinder centres at (0, 0, 15);
  - ingest records a closed mesh's centre as tessellated;
  - the api reads a centre back with its provenance, and a revision without one has no change;
  - the diff is exact only between two B-reps, with no percent;
  - the page shows the rows, and "under 0.01 mm".
- **Mutation-checked, all 10 caught:**
  - the signed volume made absolute;
  - each tetrahedron centred at a third;
  - the closed-mesh gate dropped;
  - the change approximate only when both centres are;
  - a new part not recording its centre;
  - the api dropping the centre;
  - the database not reading its source;
  - the rows relabelled;
  - the percent kept;
  - the "under 0.01" wording dropped.
- **Checked in Chrome** on the native stack with the OCCT worker, in a controlled library (no page errors, none
  logged):
  - `cylinder-d22-lp-9010-00.step`, rescanned with the ball knob's bytes: centres (0, 0, 15) then (0, 0, 27.01),
    exact. The rows read "under 0.01 mm", "under 0.01 mm" and "+12.01 mm", with no ≈.
  - The bracket, rescanned with the spacer's bytes: (44, 13.8, 6.3) then (0, 0, 0), tessellated. The rows read
    "−44 mm", "−13.8 mm" and "−6.3 mm", each ≈.

**Number ranges for custom fields** (`870ac66`, `8978484`).
- **The routes:** the grid, its facets and saved filters take `fieldMin` and `fieldMax` beside `field`, either or
  both and each inclusive, carried in the URL. Refused in words:
  - a value beside a range;
  - a range that runs backwards;
  - a range on a field that is not a number;
  - a bound that is not a number.
- **The query:** `@?` over the jsonpath `$."<key>" ? (@ >= min && @ <= max)`, one more parameter in each of the
  six grid, sort, search and facet queries. The goal's default was a `::numeric` cast between the bounds, but a cast
  behind a type check can still fail the query, since Postgres does not promise which side of an AND it evaluates
  first; `@?` passes over a value that is not a number by definition. Decided without the owner. The GIN index
  cannot serve a range, and the `ponytail:` note names an expression index per field as the upgrade.
- **The grid:** a number field's filter is two boxes, from and to, in place of its one value box.
  - A range typed backwards is written forwards.
  - A value from an older link fills both boxes.
  - A link holding a value beside a range, or a range that runs backwards, says the filter is not one the grid can
    use and offers the filters without it.

  Decided without the owner.
- **Found on the stack:** a refused field filter also failed the facets rail, which said to reload, and reloading
  could not help. It was already so for a field no longer offered. The rail now says nothing, and the grid says
  what is wrong.
- **Tests:**
  - db: a range through the page, a sort, a search and the format facet, passing over words stored under the key;
  - api: either bound or both, inclusive, a negative decimal, the facets, a saved filter, and the four refusals;
  - web: bounds kept in the URL as digits, negative and decimal; the two boxes start from the URL, write a
    backwards range forwards, and clear; the rail stays quiet on a refused filter.
- **Mutation-checked, all 15 caught:**
  - each bound made exclusive;
  - the backwards, number-only and value-beside-range refusals dropped;
  - a saved filter's range tidied away;
  - the range dropped from the page, the search, the sort and the format facet;
  - the page's range swapped for the goal's `::numeric` cast, which failed on the words ("invalid input syntax");
  - the boxes' swap dropped;
  - the bounds dropped from the request, and from the URL;
  - the rail's quiet dropped.
- **Recorded, not built:** the material and tag facets carry the range as the format facet does, and only the
  format facet's is tested.
- **Checked in Chrome** on the native stack, in a library whose three parts hold a hole diameter of 8.5, 10.5 and
  20 mm (no page errors, none logged):
  - 12 typed in From and 9 in To: the URL held 9 to 12, the boxes read 9 and 12, only the spacer (10.5) was left,
    and the format facet counted 1;
  - a link with `fieldMin=10` as bare digits: the boxes read 10 and nothing, and the spacer and the locating block
    were left;
  - a link from 22 to 8: the grid said the filter is not one it can use, and the rail said nothing.

**Counts per choice** (`0a193c4`).
- **The facets answer** carries `fields`: each choice field offered as a filter, with how many of the grid's parts
  hold each option. One query counts every such field, over the parts' values where they are an object, under the
  grid's category, search, format, material and tag.
- **A field's own filter never narrows its counts.** A field filter narrows every other field's counts, as the
  format, material and tag lists already do for each other, so another option can still be chosen.
- **The grid** shows each option's count beside it as the facets do, and names the button with the count for a
  screen reader. An option no part holds reads 0, and a count withheld past the threshold stays withheld.
- **Tests:**
  - db: a choice counted plainly, under a range on another field, and under a filter on itself;
  - api: the facets' counts under the supplier's own filter;
  - web: each option's count, none included.
- **Mutation-checked, all 5 caught:**
  - the own-field exemption dropped;
  - the other field's range ignored;
  - the api naming no filtered field;
  - an option no part holds shown without a count;
  - the counts not handed to the filter.
- **Checked in Chrome** on the native stack, in a library of three parts with a hole diameter and a supplier (no
  page errors, none logged):
  - the whole library: Hoffmann 1, Misumi 2, Norelem 0;
  - a link with the hole diameter from 10: Hoffmann 1, Misumi 1, Norelem 0, and two cards;
  - Misumi clicked: Hoffmann 1, Misumi 2 and pressed, Norelem 0, and the bracket and the spacer left.

**Decided without the owner, across the goal.**
- Clearing a part's typed materials hands it back to its file's at once, where the goal said on the next ingest.
- A revised CAD file records its header and materials, under the same guard, as a new part's always did.
- The comparison's Mass row appears only when there is a mass to compare.
- The kernel hands the centre of mass back beside its measurements, not inside `MeshMeasurements`.
- A centre's change carries no percent, and any change too small for two places reads "under 0.01".
- A range is `@?` over a jsonpath, where the goal's default was a `::numeric` cast between the bounds.
- A number field's filter is two boxes in place of its value box.
  - A range typed backwards is written forwards, and a comma is taken for the point.
  - A value from an older link fills both boxes.
  - A link holding a value beside a range, a range that runs backwards, or a bound that is not a number says the
    grid cannot use it.

**Recorded, not built.**
- A centre of mass for revisions recorded before bridge 8 and this goal: they have none until re-ingested, and the
  comparison says so.
- The material and tag facets' range is not tested; only the format facet's is.
- A part's own mass on its page. Only its change is shown, beside volume's in the history and in the comparison, so
  a hobby library, which keeps no revisions, shows no mass anywhere.
- Marking a part as holding no material where its file names one: the × on its only material hands it straight
  back, and the page says nothing.
- A STEP material named in more than 64 characters: its density box cannot save.

**Teardown.**
- **Review.** A fresh reader, a subagent, read the whole goal's code (`459001b..9a047b0`). It found nothing wrong in
  the SQL, the binds or the arithmetic, and four medium findings, six low ones and one record that said more than
  the code. The last three items above came from it. The rest were fixed (`b5688f5`), each mutation-checked:
  - The Densities dialog showed the server's refusal in kg/m³ under boxes typed in g/cm³, so following it asked for
    a density a thousand times too large. It now gives the range in g/cm³.
  - An open part's mass kept its old value after its materials or a density changed. Its history and comparison
    now read again.
  - The facets rail went blank on any failure while a field filter was set, which stage 5's fix made too broad.
    Only the field filter's own refusals quiet it now.
  - A bound typed into the range boxes that is not a number said the field was gone. It now says the filter is not
    one the grid can use.
  - Past the count threshold, the options no part held were the only ones showing a number. They show none either.
  - A file naming one material twice gave its part two, and so no mass. The bridge's list is read once per name.
  - Refusals that did not say what to do now do, the densities record no longer claims a 404 on a list, and
    FEATURES no longer says mass sits beside volume.

### Goal 6: performance and debt (2026-09-15)

- **Goal file:** `docs/superpowers/plans/2026-09-15-performance-debt-goal.md`.
- Merged locally, not pushed. This record is the goal's ledger, updated as each stage merges.

**Preflight.** The test database was up and `cargo deny check` was green on `main` (`d07629e`). Each timing runs a
debug `lapidary-server` as the api alone, over a scratch database inside `lapidary-test-db`, dropped afterwards.

**Grid sort off the part row** (`a7fc268`).
- **Before, on `main`:** 20,000 parts seeded in one library, one revision each, every tenth an open mesh.
  - Through the api, median of 31 requests after 3 warm-ups: 2.8 ms a page newest first, and 45–47 ms for every
    sort key, on the first page and the next alike.
  - The query alone, as prepared, under `EXPLAIN ANALYZE`: 65 ms. It scanned all of `part`, looked up 20,000
    latest revisions and sorted them.
- **The copy.** `0035` adds `latest_volume`, `latest_surface_area`, `latest_longest_side` and
  `latest_triangle_count` to `part`, backfilled from each part's latest revision, and one index per key on
  `(library_id, coalesce(key, '-infinity') DESC, id DESC)`. `insert_revision_chain` writes the copy in the same
  transaction. It is the only writer of a revision row, so a scan, an upload, the agent and a revision all keep it.
- **The query:** one per key, spelled as its index is, so a page is read off the index. Its filters are the
  newest-first query's.
- **After, on the same data by the same method:**
  - every sort key 3.0–3.2 ms a page, first and next, against 2.8 ms newest first;
  - the query 1.6 ms for the first page and 0.8 ms for the next, each an index scan with the keyset as its
    index condition;
  - on a fresh copy of the 20,000 parts, the api answered 1.2 s after it started, backfill included, and every
    part's copy matched its revision.
- **Tests,** beside the existing sort tests, unchanged and green:
  - a revision that widens a part moves it from second to first in volume order, paging one part at a time lists
    each part once before and after, and the part row holds the new revision's figures;
  - the migration copies the latest revision's figures, not an older one's, the longest side as the largest
    extent, and no volume for an open mesh.
- **Mutation-checked, all 4 caught:**
  - the copy never written (caught by the sort tests, and by the revision test run alone);
  - the longest side written as the smallest extent;
  - surface area's query reading volume;
  - the backfill taking the oldest revision.
- **Decided without the owner:**
  - The copies are named `latest_*`, so no query joining `part` and `revision` meets an ambiguous column.
  - A missing figure stays NULL on the row. The index and the query read it as `-infinity`, as the query did.
  - The copy is written by the one function every revision write goes through, not by a trigger; the schema has
    none.
  - The anchor reads the previous page's last part by id, filters aside. A part removed since that page no longer
    ends the paging early; one purged still does.

**The if-absent enqueues' index** (`4196010`).
- **The index:** `0036` adds `job_active_idx` on `job (library_id, kind)` where the state is pending or running.
  The `ponytail:` on `enqueue_if_absent` is gone.
- **Checked** with `EXPLAIN ANALYZE` over 10,001 done jobs (5,000 ingests and 5,001 rung builds) and three active
  ones, each check prepared and planned both for its values and generically:
  - **`enqueue_if_absent`'s `existing`,** for a pending build, an absent one, and a description that does not
    count a running job: 0.010–0.045 ms before, and 0.011–0.057 ms after. It was never a table scan: the
    planner already combined `job_dequeue_idx` (pending) and `job_expired_lease_idx` (running). After, it reads
    the new index in both plans.
  - **`enqueue_migration_if_absent`'s `state IN ('pending', 'running')`** scanned all 10,004 rows in 1.4 ms.
    After, it is an index-only scan, 0.009 ms.
- **Found:** the `ponytail:` said the enqueue's check had no index. It had none of its own, but two indexes
  served it together. The gain is the migration check's, and the migration's comment says so.
- No test: the goal asked for the `EXPLAIN` check, and every database test applies the migration.

**The grid's idle warm-up, code only** (`1320876`).
- **The change.** `loadViewerWhenIdle` takes `warmViewerWhenIdle`'s place on the grid. It imports the viewer's
  chunk and stops, so a visitor who never opens a part holds no WebGL context, as the owner decided. A hovered card
  and a part's page still call `warmViewer`, which prepares the renderer. `hasWebGL()` lets its probe context go
  once it has asked.
- **Measured** on the native stack (mock kernel, three fixture STLs) in headless Chrome on SwiftShader, a fresh
  profile per session: `main`'s web build first, then this branch's over the same stack. Medians of nine:

| | Before | After |
|---|---|---|
| Grid idle 4 s, nothing hovered: WebGL contexts made, still live | 2, 2 | 1, 0 |
| The same: shader programs linked, viewer chunk fetched | 2, yes | 0, yes |
| The same, then a press with no rest: pointerdown to first frame | 398 ms | 396 ms |
| `open-timing.mjs`, the grid then an open after a 250 ms rest | 393 ms | 393 ms |
| `open-timing.mjs`, the grid then an open with no rest | 393 ms | 399 ms |
| `open-timing.mjs --direct`, a cold link: navigation start to first frame | 668 ms | 676 ms |

- **What the numbers say:**
  - No open linked a shader program during itself, before or after.
  - Every grid open here begins with the pointer moving onto the card, which is a hover, so the renderer starts
    there and is ready before the open draws.
  - The differences sit inside each row's own spread: 387 to 407 ms for the grid, 650 to 697 ms for a link.
  - A link is unchanged by design, since a part's page warms as it mounts.
- **Not measured:** a touch tap with no hover at all, which now compiles as it opens, and a GPU renderer.
- **Tests:**
  - an idle grid loads the viewer's chunk, prepares nothing, and can be called off;
  - a hover prepares;
  - the probe's context is let go.
  - The two warm tests moved from `PartDetail.test.tsx` into `warm.test.ts`, where nothing else loads the viewer,
    so the chunk's load can be counted.
- **Mutation-checked, all 4 caught:** idle preparing the renderer, idle loading nothing, a hover preparing nothing,
  and the probe's context kept.
- **Decided without the owner:** `warmViewerWhenIdle` is renamed `loadViewerWhenIdle`, since it no longer warms.

**The folder tree, grouped once and drawn lazily** (`ba382d7`).
- **The change.**
  - The sidebar groups the categories by parent once per tree (`groupByParent`). Before, every row filtered the
    whole list for its children.
  - The top level is always drawn. Every branch below it starts closed, with a disclosure button beside each
    category that has children, and renders its rows only once opened.
  - The selected category's ancestors open as it is selected, during render, so the first frame already shows it.
    What is open is not remembered.
  - The move chooser uses the same grouping and still lists every category.
- **Measured** by goal 2's method on the native stack. Four libraries were made through
  `POST /api/libraries/{id}/folders`: 1,000 and 10,000 categories, each flat and as a ten-way tree. The time is from
  navigation start to the first frame whose sidebar holds the rows asked for, median of three fresh headless
  sessions, `main` and then this branch over the same libraries:

| Categories | Shape | Before: every row, the top level with it | After: the top level | After: every row | Opening a closed branch |
|---|---|---|---|---|---|
| 1,000 | flat | 366 ms | 399 ms | 399 ms | none is closed |
| 1,000 | ten-way | 362 ms | 330 ms | not drawn | 12 ms |
| 10,000 | flat | 1,113 ms | 490 ms | 490 ms | none is closed |
| 10,000 | ten-way | 1,368 ms | 528 ms | not drawn | 12 ms |

- **What the numbers say:**
  - A flat 10,000 still draws every row, in 490 ms against 1,113 ms: that gain is the grouping alone.
  - A ten-way 10,000 draws its top ten in 528 ms, against 1,368 ms for all of it, and a branch opens in 12 ms.
  - At 1,000 the page's own load dominates. The ten-way tree drew 32 ms sooner. The flat one drew 33 ms later: its
    runs took 383 to 408 ms, against 359 to 395 ms before.
  - Goal 2 measured 127, 131, 1,389 and 1,676 ms for the same shapes. The page loads more before its sidebar now,
    so these numbers compare with each other, not with those.
- **Tests:**
  - the categories are grouped under each parent in name order;
  - a closed branch draws no rows, and opens and closes;
  - the selected category's path is open, and nothing else is.
  - Seven existing tests now open `Terrain` before they reach `Rocks`; one of them is in `index.test.tsx`.
- **Mutation-checked, all 4 caught:** a closed branch drawn anyway, the selected path left closed, groups left
  unsorted, and a toggle that only opens.
- **Decided without the owner:**
  - The move chooser keeps every category listed. A closed category cannot take a drop, so the chooser is the path
    to one.
  - A drag held over a closed branch does not open it.
  - The disclosure is a button of its own beside the row, so selecting a category and opening it stay two actions.

**The assembly tree, drawn as it opens** (`27121e4`).
- **The change.** A branch of the part page's assembly tree renders its rows only once it is opened. Its native
  `<details>` opens and closes as before, and its `toggle` event now draws or drops the rows. The root opens as
  before. The `ponytail:` is gone.
- **Checked in Chrome** on the native stack with the OCCT worker, on `fixture-plate-assembly-lp-9000-00.step` (200
  placed parts), opening a closed branch by clicking its summary as a person would:
  - on `main`: 215 rows in the page and 15 branches, the root open. Opening `bracket-station-lp-9100-00` added no
    rows, since every row was already there.
  - on this branch: 20 rows, the same 15 branches, the root open. Opening the same station drew its 12 rows in the
    next frame, 32 rows in all.
  - No page errors either time.
- **Tests:**
  - a closed branch draws none of its rows, and opening it draws them inside it;
  - the hide and isolate test now opens the station before isolating a part inside it.
- **Mutation-checked, both caught:** a closed branch drawn anyway, and an opened branch never drawn.
- **Decided without the owner:** a branch closed again drops its rows rather than keeping them, so what is in the
  page is what is open.

**Bundle planning in one query** (`27ad4c7`).
- **The change.** `PgParts::bundle_parts` reads the whole selection in one query: each part that is in the library
  and not removed, in the order asked, with its sources and every revision's download source, oldest revision
  first. Before, `plan_bundle` called `library_of`, `detail`, `part_sources` and `history` for each part, and
  `source_for_download` for each revision. It now makes the same checks over the one answer, in the same order and
  words. The `ponytail:` is gone.
- **Counted:** planning 40 parts, one of them revised, ran 202 statements on `main` and runs 2 now: the library's
  row and the selection's. They are counted from sqlx's `sqlx::query` events on the request's thread.
- **Measured** through a debug api over 500 seeded parts, three revisions and two sources each, on the same data
  before and after. Median of 21 requests after 3 warm-ups:
  - 40 parts: 76.9 ms before, 6.9 ms after;
  - 500 parts, the cap: 947 ms before, 72.5 ms after.
- **Tests:**
  - the existing bundle tests, unchanged and green;
  - the statement count, which failed first at 202;
  - a removed part and a part of another library are refused. This one was written with the change and
    mutation-checked.
- **Mutation-checked, all 6 caught:** the library filter dropped, removed parts let in, revisions newest first, a
  source's licence read from its title, the missing-part check skipped, and a parent's label lost.
- **Decided without the owner:** a part's sources travel as one array per field, all in one order, rather than as
  JSON, so there is nothing new to fail decoding.

**A file that no longer fails past a purged part's bytes** (`c909d6b`).
- **The failure, as goal 2 recorded it.** A controlled part in a disambiguated directory is revised, then purged, so
  its newest file waits out its quarantine at that path. The part's first file, dropped back at the same source
  path, resolved to the same directory, met those bytes, and failed every attempt until the sweep removed them.
- **The change.** A new part's file that meets other bytes at a path `quarantined_file` records takes a longer name
  beside them: `{slug}_{12 hex}`, then the whole hash (`disambiguate_with`). Other bytes that are not quarantined
  are left alone and the job is decided again, as before. Nothing already on disk is replaced either way.
- **Test:** goal 2's state, built through the handler: a part filed under a disambiguated name, revised, removed
  and purged, then its first file dropped again. It failed first with the recorded message ("Another job wrote
  other bytes to libraries/default/bracket-lp-1042-03_5a36c0/bracket-lp-1042-03.stl…"). It now ingests under 12
  digits of its hash, and the purged part's bytes are still where they were.
- **Mutation-checked, all 5 caught:**
  - no longer name tried;
  - a longer name taken for bytes that are not quarantined, caught by the existing race test;
  - the directory left at six digits;
  - every path read as quarantined;
  - a suffix cut to six digits.
- **Decided without the owner:**
  - The goal said "no live part names that path". Bytes no row names include a racing job's file whose row has not
    committed yet, and `a_new_file_never_replaces_other_bytes_already_at_its_path` decides that job again. So the
    longer name is taken only for a path `quarantined_file` records, which is the state goal 2 recorded.
  - A quarantined path a part has claimed again takes a longer name too, rather than being decided again forever,
    since nothing there is replaced. The sweep's own guard for that case was tried here first, its mutation
    survived, and it was dropped as a rule with nothing to protect.

**The watch follows a symlinked file** (`db4729c`).
- **The change.** The agent's folder listing reads a model file's metadata with `std::fs::metadata`, which follows
  a symlink, so a symlinked STL is watched as a scan ingests it. Directories are still typed by
  `DirEntry::file_type`, which does not follow one, so a symlinked directory is not descended.
- **Test:** a folder holding a symlinked STL, and a symlinked directory with an STL inside, lists the one file at
  its target's size. It failed first, listing nothing.
- **Mutation-checked, both caught:** the link not followed, and a linked directory descended.

**The library menu closes for its dialogs** (`8ea41bc`).
- **The change.** The toolbar's menus are native popovers, which a click inside never dismisses, and the dialogs
  their items open are drawn outside them, so the library menu stayed open beside a dialog. A menu now carries
  `data-menu`. The four items that open a dialog (New library, Keep every change…, Fields… and Densities…) call
  `closeMenu`, which hides the popover they sit in. Scan, Sweep and the thumbnail setting act in place and leave
  the menu open, as before.
- **Test:** choosing each of the four closes the library menu and opens its dialog. It failed first for all four:
  the menu was never hidden. jsdom has no popovers, so the test stubs the menu's `hidePopover`.
- **Mutation-checked, all 4 caught:** `closeMenu` closing nothing, the menu left unmarked, New library leaving the
  menu open, and Fields leaving it open.
- **Decided without the owner:** only the items that open a dialog close the menu. A scan or a sweep reports on
  the page beside it, and the view menu's layout buttons are chosen while watching the grid.

**The agent's lock check, tested** (`8d1df2a`).
- **The change.** `lapidary open` reuses a checkout only while its lock is the part's active one, and compared the
  two inline in `main.rs`, which has no tests. The comparison is now `Checkout::holds` in `checkout.rs`, beside the
  checkout tests, and `open` calls it.
- **Test:** a checkout holds its part while its own lock is active, and not once that lock is released or another
  holder's is active.
- **Mutation-checked, all 3 caught:** any lock passing, every lock passing, and the held lock refused.

**Step 10's first manifest, held** (`d868210`).
- **The change.** A new part's first `metadata.json` was built by hand from the ingest's own ids and what the
  handler had recorded, and written without the part-row hold the other two writers take. It is now written
  through `PgRevisions::write_manifest`, from the rows, while the part's row is held, in the revision writer's
  shape. The hand-built manifest, its imports and the handler's local copy of the file's statement are gone.
- **Test:** a new part's `metadata.json` is the manifest its rows give. It passed on `main` too, since for an STL
  the hand-built manifest already matched the rows, so it was mutation-checked instead.
- **Mutation-checked, both caught:** another part's manifest asked for, so no file is written, and a manifest that
  is not the rows'.

**Two measurements** (no code).
- **An access-tracking flush.** `Touches::flush`'s statement, prepared, over 20,000 blob rows in a scratch
  database. Each run was rolled back; the figure is the median of five, server time from `EXPLAIN ANALYZE`:
  - 150 reads, every row stale, for a busy interval over the stand-in's 150 parts: 3.5 ms;
  - 20,000 reads, every row stale: 136 ms, plus 7 ms planning a statement that carries 20,000 hashes inline, which
    sqlx's binary binds do not;
  - 20,000 reads, every row already read today: 53 ms, writing nothing.
  - Nothing changed. A repeat flush still joins every hash, but a flush runs every five minutes, off any request.
- **Leaving a core free during a commit.** A 150-file commit through a debug api on 12 cores, into a scratch
  library of 20,000 parts, with the grid's search asked back to back while it ran. `main`, which stores 11 files at
  once, was alternated for three rounds with a local build storing 12, which was never committed:

| | 11 at once (`main`) | 12 at once |
|---|---|---|
| The commit | 1,425, 1,372, 1,358 ms | 1,377, 1,385, 1,351 ms |
| Search, api idle, median | 37 ms each round | 37 ms each round |
| Search during the commit, median | 72.2, 69.0, 65.2 ms | 72.5, 72.0, 68.8 ms |
| Search during the commit, p95 | 83.2, 79.8, 78.2 ms | 86.1, 81.0, 79.6 ms |

  - **The default stays one fewer than the cores.** Leaving a core free is 0.3 to 3.6 ms better at the median,
    inside the spread between rounds, and the commit takes the same time either way.
  - The stand-in is the first 150 uniquely named STLs between 20 kB and 3 MB in the owner's corpus, sorted by path:
    237,251,150 bytes, not goal 2's 270,719,950. The corpus has changed since.

**Housekeeping** (read only; nothing removed).
- **`storage/` in the repo root:** 155 MB, 484 files owned by the container's uid 10001, written from 2026-09-05 to
  2026-09-13, and ignored by git. It is still the compose stack's storage root: `compose.yaml` mounts
  `${LAPIDARY_STORAGE_ROOT:-../storage}`, and `deploy/.env` leaves that unset. Its rows are in
  `lapidary_lapidary-db`.
- **`lapidary_lapidary-db`:** 185.8 MB, the compose stack's Postgres, still declared.
- **`lapidary_lapidary-uploads`:** empty, still declared.
- **`lapidary_lapidary-blobs`:** 80 MB, 312 files under `blobs/`, written on 2026-09-05 and 2026-09-07.
  `compose.yaml` no longer declares it; its note calls it the store of a deployment from before the folder layout,
  which nothing copies across.
- **Answered, and carried out (2026-09-16).**
  - `lapidary_lapidary-blobs` (80.7 MB) and an orphaned test-database volume (116.3 MB) are gone.
  - The compose stack's store is kept: `storage/` (155 MB) and `lapidary_lapidary-db` stay together, with
    `lapidary_lapidary-uploads` beside them.
  - `lapidary-occt:goal4` and `lapidary-occt:goal5` are gone, about 1.14 GB between them. The kernel the
    build needs is the copy at `target/occt/`.
  - Left alone deliberately: another project's 15.2 GB builder image, the build cache, `postgres:18`, and the
    running test database's own anonymous volume. Nothing was pruned — every removal named its object.

**Recorded, not built.**
- **From the goal file,** until a real library needs each:
  - streaming bundle import, and ZIP64;
  - `notify` in place of the watch's poll;
  - per-file job stages, decided against (FEATURES §1);
  - the `renameat2(RENAME_NOREPLACE)` fallback;
  - a lock across the adopting and reaping jobs;
  - a 3MF upload's staged copy counted as phantom by `0026`.
- **Found along the way:**
  - A touch tap on the grid with no hover at all, which now compiles the viewer as it opens, was not measured,
    and neither was the warm-up on a GPU renderer.
  - A drag held over a closed category does not open it.
  - A new file whose 12-digit and whole-hash names are both held by quarantined bytes fails transiently until
    the sweep, and no test reaches that.
  - `write_manifest` still writes `metadata.json` with blocking file I/O while the part's row is held.

**Teardown.**
- **Review.** A fresh reader, a subagent, read the goal's code (`d07629e..08d7d69`, docs aside). It found nothing
  high. It found these sound: the copy's writers, `sorted_page!`'s binds, `bundle_parts`' refusals and order, step
  8's names and reaps, the watch, the menu and the lock check. It raised seven findings, all acted on
  (`1556601`), each fix's test seen failing first or mutation-checked:
  - **A category created inside the selected one stayed hidden** in its closed branch. That branch now opens. Seen
    failing first.
  - **A selected category the tree did not yet hold was never opened to** once a later read brought it. The
    sidebar now waits until it holds the category. Seen failing first.
  - **A part row with no revision ended a sorted grid early.** It took a place on the page, the cards' join dropped
    it, and the short page read as the last. `top` now leaves it out. Seen failing first. The check costs sorted
    pages over 20,000 parts about 0.2 ms: 3.1 ms against 2.9 to 3.0 ms.
  - **`write_manifest` read on a second pooled connection while holding the first,** so writers that filled the
    pool stalled it, and every ingest now writes its first manifest there. It reads on the held connection. Seen
    failing first, on a pool of one, as `PoolTimedOut`.
  - **`bundle_parts` aggregated a part's sources once per revision.** Now once per part. Over three alternated
    rounds on the same data, planning 500 parts went from 70.9–71.5 ms to 66.0–67.3 ms, and 40 parts from
    6.8–7.0 ms to 6.4–6.5 ms.
  - **The sorted anchor's doc** said a part revised since the last page still marks where the next one starts. It
    now says the anchor moves with it, so the next page repeats rows or skips them.
  - **Tests that proved less than they claimed:**
    - the first-manifest test now also checks the file against the fixture: its hash, its name and the triangle
      count;
    - a new test takes the whole-hash name past two quarantined names;
    - the bundle refusal test covers a part with no revision.
  - **Mutation-checked, all 6 caught:** the new branch left closed, a category opened to before the tree holds it,
    the revision check dropped, the second connection, a part with no revision bundled, and the 12-digit name
    alone.

---

## Shared libraries (2026-09-16)

Sharing models between people who already know each other, decided with the owner on 2026-09-16 and
sequenced ahead of Phase 8. What the work is built on:

- people who know each other, paired by hand: no public directory, no tracker, no swarm;
- a LAN, or a VPN they already run: no hole punching and no relay;
- a share is a category and everything under it, including parts added later;
- everyone paired sees every share, and a request is granted unless that share says ask first;
- the package is pulled straight from the sharer, in hash-checked pieces that resume;
- licences travel and are shown, with a warning to the sharer, and never a block;
- a `peer` role beside `api` and `worker`, off until switched on. `PRODUCT.md`'s line that the image
  fetch is the only outbound request is rewritten when that role lands, not before.

**What a bundle carries, carried** (`634df2c`).
- **The hole.** `import_part` replayed a part's revisions and read nothing else from the manifest, so
  a part's number, its tags and its sources — the licence among them — were dropped on every import.
  The export has written all three since Phase 4 slice 2, so the loss showed only on the receiving
  side.
- **The change.** A part the import creates is given its number, its tags and each of its sources,
  through `PgParts::set_part_number`, `set_tags` and `add_part_source`. A part this library already
  holds is left alone. Prices stay out, as they stay out of a manifest.
- **Tests:** an imported part arrives with its number, its tags and its source licence, seen failing
  first with `part_number` still `None`; a second import leaves a part's own tags alone.
- **Mutation-checked, all 5 caught:** the number never written, the tags never written, the sources
  never written, the licence dropped while the rest of a source travels, and the guard widened so a
  part already held has what was typed here overwritten — which only the second test catches.
- **Decided without the owner:**
  - Metadata is written for a part the import creates and not for one it revises. Nothing records
    whether a local value was typed here or imported, so the safe half is the one taken.
  - A part's *name* is still not taken from the manifest. `part_name()` derives it from the file name
    on every path into ingest, and changing that reaches scans and uploads too.

**Two installations that can tell each other apart** (`a62d133`, `3237f5c`).
- **The device id** (`crates/lapidary-core/src/device.rs`): the BLAKE3 of the public key an
  installation presents, in Crockford base32, in groups of five. It is what two people paste to each
  other, and it is the whole of the credential — an identifier, never a secret, exactly as a blob
  hash is.
- **The listener** (`crates/lapidary-peer`): an Ed25519 keypair per installation at
  `$LAPIDARY_PEER_DIR/identity.pkcs8`, written `0600` as it is created; RFC 7250 raw public keys on
  both ends, so no certificate is generated and no X.509 is parsed anywhere in the trust path; and a
  `GET /peer/v1/hello` that answers what this installation is called. A machine nobody paired with is
  refused *during the handshake*, so it reaches no route and learns nothing beyond this one's id.
- **No new dependency.** `rustls`, `tokio-rustls` and `ring` were already in the tree through
  reqwest's `rustls-tls-native-roots`, and are now named directly in `[workspace.dependencies]`.
  axum 0.8's `Listener` trait takes the TLS listener, so the peer role serves through the same
  `axum::serve` and the same shutdown token as the api and the worker. No `rcgen`, no
  `ed25519-dalek`, no `axum-server`. `cargo deny check licenses` is green with `ring` direct.
- **Two bugs, both in the trust path, both found by the tests rather than by reading.**
  - The id hashed `ring`'s raw 32-byte key while rustls presents a `SubjectPublicKeyInfo`, so the
    digests were of different bytes and *every* connection was refused. Both ends now hash exactly
    what goes on the wire, which is also why no DER has to be unwrapped to check it.
  - The signature check then delegated to `rustls::crypto::verify_tls13_signature`, which parses its
    argument as an X.509 certificate: handing it a bare key failed as `BadEncoding` on every
    connection. It now calls `verify_tls13_signature_with_raw_key`. There is no TLS 1.2 equivalent
    in rustls, so that arm refuses rather than falling back to the certificate path — both configs
    pin TLS 1.3, so it is unreachable, and a later version change must not quietly turn the pinning
    into something else.
- **`LAPIDARY_ROLE=peer`** serves the peer protocol and nothing else: the router `lapidary-api`
  builds for it is empty, and `lapidary-api` may never depend on `lapidary-peer`
  (`xtask/src/layers.rs`'s `FORBIDDEN_PAIRS`, by name). `lapidary-peer` is L3 rather than L2 because
  it reaches `lapidary-targets`, and an L2 crate may not depend on another.
- **Off unless asked for.** The service is in `deploy/compose.sharing.yaml`, an overlay, so the
  plain stack still opens no listener. `check-deploy` learned to read it: the per-service rules are
  split into `per_service`, and `check_overlay` applies those without the two whole-file rules that
  would fail an overlay for having no worker of its own. The two files cannot simply be concatenated
  — `parse_services` stops at the second `services:` key.
- **Tests, each seen failing first against `todo!()` bodies:** an identity is named by the digest of
  what it presents, two installations differ, an installation keeps its id across a restart, the
  handshake alone succeeds between paired installations with each end's answer read separately, and
  a paired installation is answered while a stranger never reaches a route. Four `check_overlay`
  tests, including one asserting that `check_compose` *would* refuse the overlay — which is why that
  function exists.
- **Mutation-checked, all 4 caught:** any key allowed, the paired list emptied, the digest taken over
  the wrong bytes, and raw-public-key negotiation switched off. Six more on the device id, all
  caught — one of which, a surviving mutation, is what found that a non-canonical last character let
  two different-looking ids name one machine.
- **The refusal names the machine** (`3e335a3`). S1a's own exit asked for this and the first cut did
  not do it: `PeerListener::accept` threw the error away and the test asserted only that the failure
  was not a status, so two people who mistyped one device id between them got silence at both ends.
  Each end now logs the id it turned away, once, and says which end it is — the accepting side was
  never paired with it, the connecting side expected somebody else at that address. The wording
  belongs to `Pinned::accepting` and `Pinned::connecting` rather than to each call site, so the test
  builds the same two ends the configs build. A second test covers the connecting end refusing a
  machine that answers at a right-looking address, which is what a wrong address actually produces
  and which nothing exercised before. Mutation-checked, all 4 caught: the line removed, the id
  dropped from it, and each end handed the other's wording.
- **Measured, but statically.** `deploy/compose.yaml` publishes 8080, 8081 and 3000 and holds the
  string `peer` nowhere; the peer role, its bind on 8082 and its port exist only in the overlay. The
  live `ss -ltn` check the plan asked for needs `compose up`, which builds images, which needs the
  owner's word — so it is recorded as not done rather than reported as done.
- **Decided without the owner:**
  - **Migration `0037` is deferred to S1b.** The plan put `peer_identity` and `peer` here, but the
    identity lives on disk by design and nothing reads those tables until S1b adds the routes. A
    table with no reader is what the ladder says to skip.
  - The peer service builds the `api` target of `deploy/Containerfile`: the peer role links no CAD
    kernel, so it must not build the target that carries OCCT.
  - The identity key is on disk rather than in Postgres, for `upload_dir`'s reason — a key in the
    database is a key the api's own credentials can read.
  - **Left for S4, recorded rather than fixed.** `PeerListener::accept` awaits the handshake inline,
    so one connection that opens TCP and never finishes holds up every other; and a `tcp.accept()`
    error that persists spins the loop rather than backing off. Both are the same subject as S4's
    rate and concurrency limits and are picked up there, so S4 inherits them instead of finding them
    again. Neither is exposed today: the role is off until an owner switches it on.

**The list of people you share with** (`b6bab3c`, `eb21f24`, `4dd9e6f`, `0271ebc`, `2c2d3d1`, `0035756`).
- **Tables** (`0037`): `peer_identity`, one row naming this installation, and `peer`, one row per installation
  paired by hand. The key stays on disk and only its digest is stored. Removal is soft. See `DATA.md` §7.1.
- **The hello round** (`crates/lapidary-peer/src/sync.rs`). Every 15 s the peer role reads who is paired and
  what this installation is called, brings its roster up to date, then asks each paired installation what it
  is and records the answer: seen, with the name it gave, or why not, in words. The TLS verifier reads that
  same roster at every handshake, so a pairing or a removal made through the api takes effect within a round,
  with no restart.
- **The api** (`crates/lapidary-api/src/sharing.rs`): `GET` and `PUT /api/sharing/identity`, `GET` and
  `POST /api/sharing/peers`, `DELETE /api/sharing/peers/{device}`. A mistyped id is refused in the id parser's
  own words, an address is a host and a port with no scheme or path, and this installation's own id is refused.
- **The page** (`web/src/routes/sharing.tsx`), linked as "Shared libraries" beside Removed. `LAPIDARY_API` now
  points the dev and preview proxy at another api, so two stacks on one machine each get their own page.
- **A bug the tests found.** A refusal from this end's own verifier reaches reqwest inside two nested
  `io::Error`s, and an `io::Error`'s `source` skips the error it carries, so the wrong machine at an address
  was first reported as nothing answering there. The walker now opens each `io::Error` it meets.
- **Tests.** Seen failing first against stubs: 7 for the tables, 5 hello rounds against real loopback
  listeners, 7 api routes and 7 page tests. The live-roster unit test and the name-cleaning test arrived with
  their code; the mutations below are what cover them.
- **The hello-round tests live in `bin/lapidary-server/tests/peer_sync.rs`,** not in the peer crate.
  `cargo deny` refused them there: `deny.toml` lets only a fixed list of crates take `sqlx`, and says that a
  longer list means SQL has leaked. `lapidary-peer` holds no SQL and now takes no `sqlx`; the binary that wires
  the peer role is already on the list for exactly these tests' reason.
- **Mutation-checked, 23 of 23 caught** across the four layers (`target/sharing-check/mutate-s1b.sh`), and
  S1a's four re-run against the roster, all caught. The script's first run reported seven caught while testing
  nothing: `timeout` cannot run a shell function, and its "No such file" exit read as a failure. It now counts
  a mutation caught only when a test or the compiler is seen failing, and says so when the tests did not run.
  One mutation, a host allowed to hold `/`, would have survived; `nas.local/lapidary:8082` is the test case
  added for it.
- **Measured, two stacks on one machine** (`target/sharing-check/two-stacks.sh`: one binary, two scratch
  databases, no image build).
  - Each pasted the other's id and address through its api. Both showed the other online **28 s** later —
    two hello rounds, as the order of the rounds predicts: the first hello can arrive before the other side's
    round has taken up the pairing.
  - A removed B. A's list emptied, and A's `peer` table still held the row, marked removed.
  - **16 s** later B showed A offline, with "turned this one away" as the reason, and A's peer log named B's
    device id as refused.
  - A paired with B again. Both were back online **13 s** later — B included, which happens only once A's
    listener stops refusing B — and A's `peer` table held the one row, no longer removed. So a removal both
    takes effect in the listener and comes undone there, not only in the tables.
  - All seven figures are one run from fresh databases. Screenshots of both pages, paired, after the
    removal, and after pairing again, in `target/sharing-check/shots/`.
- **Decided without the owner:**
  - **The hello round runs in the peer role, not as a worker job.** The plan put outbound peer requests in the
    worker, which would mean mounting the identity key into a second container. The round is a timer with no
    row to lease, the quarantine sweep's reason for not being a job either.
  - **`PUT /api/sharing/identity` sets this installation's name,** which its hello carries, so each list shows
    what the other side calls itself rather than a bare id. The plan did not say what it set.
  - **`last_error` beside `last_seen_at`:** "offline" alone cannot tell a machine that is switched off from one
    that has not added this one.
  - `PRODUCT.md`'s outbound-request line is rewritten here rather than later, because the hello is the first
    outbound peer request.
- **Left for later:**
  - 28 s from pairing to online is two rounds. A `NOTIFY` when a row is added would start a round at once.
  - **For S4:** a connection already open outlives a removal until it closes. Each hello builds its own client,
    so it does not show today; a long pull would.
  - Hellos go one at a time, each allowed 5 s. Past about six machines that do not answer, a round outlasts the
    online window (`ponytail:` in `sync.rs`).

**The listener, hardened** (goal 7 stage 1, `5be7d13`). What S1a recorded for S4, done first so everything after it
measures the listener that ships (`docs/superpowers/plans/2026-09-17-shared-libraries-goal.md`).
- **The bug, confirmed:** `PeerListener::accept` awaited each TLS handshake where it accepted the connection, so one
  connection that opened the peer port and said nothing held up every connection after it for as long as it stayed
  open. Each handshake now runs in a task of its own, allowed 10 s, and only finished connections reach axum.
- **A failing `accept()`** backs off, 100 ms doubling to 1 s and back to 100 ms after a success, instead of retrying at
  once for ever. The refusal comment is narrowed: the verifier's log line records a *pinning* refusal; a handshake
  that dies before any key is presented leaves no record.
- **Tests, each seen failing first for its own reason:** a paired hello behind a silent connection timed out
  (`Err(Elapsed)`); a silent connection was never closed (`Err(Elapsed)`); the backoff was a `todo!()`.
- **Mutation-checked, 4 of 4 caught** (`target/sharing-check/mutate-g7-s1.sh`): handshakes awaited inline, a handshake
  allowed a day, a backoff that never grows, a backoff with no ceiling. Not mutated, and said so in the script: the
  backoff's reset after a success and the accepting task's exit when the listener is dropped — no test can make
  `accept()` fail persistently or watch the task end.
- **Measured, two stacks, four TCP connections held open against A's peer port and re-opened every 12 s**
  (`target/sharing-check/silent.py`), the S1b binary against this one:

  | | S1b listener | hardened listener | S1b, no silent connections |
  |---|---|---|---|
  | pairing to both online | 36 s | 27 s | 28 s |
  | a removal seen by B | 34 s | 15 s | 16 s |
  | pairing again to online | 35 s | 13 s | 13 s |

  The live run shows a slowdown rather than a hang, because each silent connection closes every 12 s and the old
  listener's stuck handshake then fails and moves on. A silent connection that stays open indefinitely does hang the
  old listener; the first test above is that case.
- **Decided without the owner:** 10 s for a handshake; the backoff's 100 ms and 1 s; finished connections queued 64
  deep before `accept` takes them.
- **Closes** S1a's two items left for S4: the stalled handshake and the spinning accept.

**Share a category** (goal 7 stage 2, S2a: `913c7ee`, `788ad55`, `62d3631`, `a1d05d0`, `5ec4f0c`).
- **Tables** (`0038`): `share`, one live share per category, removed softly. Nothing lists parts: a share is the
  category and everything under it, so the catalogue walks the folder tree at read time, with the grid's own
  joins for a part's current file and thumbnail. Every read that follows a share requires the share and its
  category to be live, so deleting a shared category withdraws it.
- **The peer routes** (`crates/lapidary-peer/src/shares.rs`): `GET /peer/v1/shares`, a share's catalogue in pages
  keyed by `source_path`, and a part's thumbnail. Each asks which installation is asking before it reads
  anything, and the answer is the key the TLS handshake verified — `PeerDevice`, carried to the route through
  axum's connect info (`into_make_service_with_connect_info`), never anything the request says. A test serves a
  route through the real listener and checks it sees the guest's id.
- **The api** (`crates/lapidary-api/src/shares.rs`): a library's shares, a preview counting parts with no licence
  recorded and parts licensed non-commercially, sharing, everything shared, and stopping. The warning is shown
  and never blocks.
- **The page:** a Share action beside Rename and Delete, a "Shared" mark on the tree, a dialog that shows the
  counts before anything is sent, and the sharing page's list of what this installation shares.
- **Found by the tests and gates, not by reading:** a `use` of a `macro_rules!` macro is dead when the macro is
  defined above its uses (textual scope); ts-rs types an `i64` as `bigint` unless it carries
  `#[ts(type = "number")]`; the bare-strings lint keeps every strings.ts template segment of five characters or
  more, so `` `${n} parts` `` made the grid's `['parts', library]` query keys read as copied strings; and two test
  stubs that reject unknown requests needed a `/shares` arm.
- **Two test holes found while writing the mutations, closed before running them** (`5ec4f0c`): sharing another
  library's category was checked only by the value returned, not by what was written, and the preview's
  library check had no test.
- **Mutation-checked, 18 of 18 caught** (`target/sharing-check/mutate-g7-s2a.sh`, in the gates' own build
  configuration): the subtree, removed parts, removed peers, stopped shares, thumbnails outside the share, `NC`
  inside a word, the unrecorded count, a share across libraries, the device read from TLS, both route gates, the
  catalogue's `next`, the preview's library, and five in the page. Four not mutated, with reasons in the script.
- **Not measured live on its own:** S2b's exit browses a 1,000-part share on two stacks, which exercises these
  routes over TLS end to end.
- **Decided without the owner:**
  - a stranger and somebody whose share was withdrawn get the same answer (`404 notShared`), so nobody learns
    what is shared by asking;
  - non-commercial is `NC` as a token of its own, or the words spelled out, never `nc` inside a word;
  - sharing a category that is already shared answers with the same share;
  - the dialog opens on Cancel, as the delete confirmation does;
  - a catalogue page is 200 parts unless asked, and never more than 500;
  - a share's digest is its part count and the newest `part.updated_at` under it. S2b relies on it and must
    check that a new revision moves `updated_at`.

**Browse what somebody shares** (goal 7 stage 3, S2b: `17ac18b`, `2261583`, `ac8c4fd`, `b89ca31`, `0a5486e`).
- **The mirror** (`0039`): `peer_share` and `peer_share_part`, a cache of another installation's list and never this
  installation's data. Taking up an offer adds shares, keeps names and counts current, deletes with their parts the
  shares no longer offered, and answers only the shares never read or read under another digest. A catalogue is
  replaced whole in one transaction, so a read that fails part-way leaves the mirror as it was.
- **The hello round** (`sync.rs`): hellos go out eight at a time — three machines that accept and never answer cost a
  round 6 s rather than 15 s — and every installation that answered is then mirrored, beside the rounds rather than
  inside one, never twice at once. The round wakes on the sharing channel as well as the tick.
- **Browsing** (`crates/lapidary-api/src/sharing.rs`, `web/src/routes/sharing_.shares.$shareId.tsx`): under each person,
  what they share; a shared library's page with who shares it, when it was last read, and its parts in pages — each
  with a preview, its licence or that none is recorded, and its format and size. All read from the mirror.
- **A bug in S2a, confirmed and fixed:** recording a revision leaves `part.updated_at` alone, so S2a's digest never
  moved for a revised part and a puller would have kept a stale hash. The digest now counts revisions; a test saw it
  not move first.
- **Mutation-checked, 14 of 14 caught by tests** (`target/sharing-check/mutate-g7-s2b.sh`). One first counted only as
  "caught by the compiler" — pushing `None` for every thumbnail broke type inference, which proves nothing about the
  tests — and was rewritten to store empty thumbnails instead, which a test catches. Not mutated: reading every
  catalogue page rather than the first, which a unit test would need 500 parts for; the measurement below read 998
  across 2 pages.
- **Measured, two stacks** (`target/sharing-check/seed-a.sh`, `measure-s2b.sh`, `measure-s2b.log`):
  - **The sharer's library:** the 1,000 smallest loose STLs of the corpus (6.09 GB), scanned by a worker at concurrency
    2 in 435 s. **998 ingested**: two files were refused, rightly, for non-finite vertex coordinates
    (`BWB1_10_SUP.stl`, `GotNML3_1_SUP.stl`). Store 5.7 GB; all 998 with thumbnails.
  - **Pairing to both online: 15 s** with the notification, against S1b's 28 s.
  - **The licence preview** for sharing `STL Files`: 998 parts, 998 with no licence recorded, none non-commercial —
    a scan records no licences.
  - **A shared it; B had mirrored all of it 19 s later.** B's read: 2 catalogue pages, 391,764 bytes (about 196 KB a
    page of 500), 998 parts, 38,522,502 bytes of thumbnails, none skipped. The largest thumbnail was 65,532 bytes,
    just under the 64 KB cap.
  - **B browses all 998** through its api in 2 pages of 500 (270,008 bytes of JSON); a thumbnail answers
    `200 image/webp`, 30,626 bytes.
  - **An unchanged share is not read again:** 45 s later B had still read the catalogue once.
  - Screenshots: `target/sharing-check/shots/s2b-b-sharing.png`, `s2b-b-library.png`.
- **Decided without the owner:**
  - the mirror deletes what a sharer stops offering, as a cache may; the page says the sharer stopped offering it;
  - a thumbnail is kept only below 64 KB, the inline limit a sharer stores it under;
  - eight hellos at once; one mirror a device at a time; 30 s a mirror request;
  - a shared library's page is 100 parts unless asked, never more than 500;
  - one insert a part while replacing a catalogue (`ponytail:` in `mirror.rs`), about a second for 1,000.
- **Left for later:**
  - Sharing a category wakes only the sharer's own peer role; the other side reads it on its next tick, which is most
    of the 19 s. Telling a paired installation at once needs a request from sharer to puller, and belongs with S3.
  - Part names are still derived from file names, so a corpus library reads as file stems.

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
- **An upload's hash must be reachable, once there is auth.**
  - An upload commit attaches bytes some library already holds on a matching hash alone
    (`crates/lapidary-api/src/upload.rs`, `commit`). That is sound only while one owner holds every
    library.
  - With auth, the commit must require one of two things: the bytes uploaded by this caller, or a hash
    reachable from a library the caller may read. `import_bundle` already refuses a bundle's hash alone.
  - Recorded at the owner's word from the 2026-09-15 code review.
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
