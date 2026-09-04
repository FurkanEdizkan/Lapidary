# Phase 1 slice 3b — handoff

3MF ingest. A `.3mf` dropped in the ingest folder becomes a part with a thumbnail and
three LOD rungs, exactly like an STL — closing `ROADMAP.md`'s Phase 1 mesh-format line
(STL/3MF/OBJ).

Ten tasks, seventeen commits, on `feat/3mf-ingest`. **371 Rust tests, 0 failing**, every
gate at exit 0. Not pushed: branches merge to `main` locally and CI is the release gate.

## What landed

| Task | Commit | What it does |
|---|---|---|
| 1 | `475051c` | `zip` (deflate, pure Rust) and `quick-xml`, audited with `cargo deny` |
| 2 | `3bb7455` | `SourceStore` takes a `Compression` policy — 3MF stores as-is |
| 3 | `f68544b` | `Caps` and `read_capped`: refuse as bytes arrive, not after |
| 4 | `a965def` | Open the package, cap entries, reject traversal, find the model via `_rels/.rels` |
| 5 | `4cbcb4d` | Model XML: units, vertices, triangles |
| 6 | `c49f4ed` | Build transforms, composition, recursive components with a depth cap |
| 7 | `39d50ce` | A real fixture — a carrier plate placed twice |
| 8 | `b931837` | Dispatch, `MESH_EXTENSIONS`, and a `kernel_version` naming the parser |
| 9 | `65fb47c` | End to end, stored as it arrived |
| 10 | this | The exit run |

Six commits are documentation: five recording rulings made during execution, one capping
container resources at the owner's request mid-slice.

## What the exit run showed

Run on 2026-09-04 against a mixed directory — the repository's STL fixtures, the example
parts, the OBJ from slice 3, and the new 3MF — on `docker compose` rebuilt from this
branch, with the new per-service ceilings in force.

| Claim (spec §10) | Result |
|---|---|
| Every part gets one thumbnail and three rungs, whatever the format | **160 parts → 640 derivatives**: 160 `thumbnail` inline, 480 `tessellation_l*` by hash. Three formats, identical shape |
| The 3MF's `file.format` and `kernel_version` | `3mf` and `mesh 3mf-1+glb-1+cpu-1`. Three distinct versions across the three parsers |
| A 3MF source blob is stored uncompressed | **3921 → 3921 bytes, `zstd_level` 0, 0.0% saved** — and the OBJ beside it saved 49.6%, the STLs 79.7–92.7%. The 3MF is the only row that does not compress, which is what makes it a policy rather than compression having stopped |
| Each rung opens in an independent validator | **3 / 3** 3MF-derived rungs clean through Khronos `gltf-validator`: 0 errors, warnings, infos, hints |
| A multi-item build plate is one part, triangles summed | **768 triangles** (2 × 384) and **bbox 108 × 48 × 15 mm** — the 48 mm carrier plus the 60 mm offset of its second placement |
| `GET /api/blob/{hash}` serves a 3MF-derived rung | **200**, `model/gltf-binary`, `immutable`, quoted ETag |
| A capped 3MF is refused, leaving no part and no orphaned blob | `failedTotal 1` at **attempts 1** — Permanent, not retried — with *"Refused this 3MF — one entry expands past the 13046400-byte limit."* Parts stayed 10, blob files stayed 26 |
| Re-scanning drains to `skipped`, writing no new blobs | **skipped 10, ingested 0**; blob rows 26 → 26, files 26 → 26 |
| Grid page under `DATA.md` §2.5's 80 ms warm | **0.8 – 1.6 ms** |
| Zero WARN on a cold start | **0** in both `api` and `worker`, across the whole run |
| Throughput within 3× of slice 3's 89.4 files/s | **42.8 files/s** (160 files in 3.73 s). Passes, but see below |

### Throughput halved, and it is not 3MF's doing

Slice 3 measured 89.4 files/s. This run measured 42.8. The cause is the resource ceilings
added mid-slice at the owner's request, not the new format:

| | Slice 3 | This run |
|---|---|---|
| Worker CPU | unlimited on a 12-core host | **2.0 cores** |
| Ingest concurrency | 4 (the default) | **2** |

Six times less CPU and half the concurrency, for a bit under half the throughput. Peak
worker memory was **40 MiB of its 2 GiB ceiling** — 2% — so memory was never the
constraint, and the trade was deliberate: the stack now fits a machine that does not have
gigabytes to spare. Raising `LAPIDARY_WORKER_CONCURRENCY` recovers throughput on a host
with room.

`deploy.resources.limits` were verified as actually applied, not merely declared:
`docker inspect` reports `mem=2147483648` on the worker. Compose v2 honours them outside
swarm.

## What the slice found that was not about 3MF

**`quick-xml` 0.37 carried two live advisories.** Task 1's audit — the reason this slice
was split out of slice 3 — failed `cargo deny` on RUSTSEC-2026-0194 (quadratic-time parse)
and RUSTSEC-2026-0195 (unbounded namespace-declaration allocation, a memory-exhaustion
DoS). Both sit in the one component that reads attacker-controlled XML, so they would have
hollowed out the caps §3.4 exists to provide. Pinned at 0.41 and recorded in the spec as a
security floor with the RUSTSEC ids.

**A mutation check took the machine down twice.** The plan told task 3 to prove
`read_capped` aborts mid-stream by running the naive implementation against an infinite
reader and expecting a hang. `read_to_end` on an infinite reader does not hang — it
allocates until the OOM killer fires. Kernel log: 10.9 GB, then 13.2 GB, on a 15 GB
machine, both inside the editor's process tree. The test now uses a bounded 64 KiB source
that counts bytes pulled, which is both safe and strictly better: both implementations
return an error, so only the byte count distinguishes aborting *during* from aborting
*after*.

## Four tests that could not fail

The recurring defect in this slice was not wrong code. It was tests that passed for the
wrong reason, and all four were in the plan rather than the implementations:

1. **The cap test** used `std::io::repeat`, whose `read_to_end` std specialises to fail
   immediately — so the naive implementation also returned an error and the test passed
   against the exact bug it existed to catch.
2. **The transform/scale ordering** was correct in the code and unpinned by every test: a
   translation in a millimetre file has scale 1, and a pure scale matrix commutes with a
   scalar, so both orderings gave both tests identical answers.
3. **The composition order** was likewise unpinned — both transforms in its fixture had
   identity 3×3 blocks, so a transposed product gave the same answer.
4. **The zip-bomb test** built an archive with no `_rels/.rels`, which `parse_3mf` reads
   first, so it failed on the missing part and never reached the ratio cap it was named
   for. It proved the reap and would have passed identically with no cap at all.

Each was caught by an implementer or reviewer refusing to accept a comment, and each fix
was the same shape: assert the specific number or the specific message, never merely that
an error occurred or a value came back. A bare `expect_err` is how this keeps happening.

## The final review found a Critical that ten per-task reviews missed

Every task passed its own review with spec compliance ✅. The whole-branch review then
found that **component fan-out was unbounded**, and measured it. Reproduced independently
against this branch's own parser:

| Package | Emitted | Amplification |
|---|---|---|
| 722 bytes | 65,536 triangles (2.2 MiB) | 3,267× |
| 723 bytes | 1,679,616 triangles (57.7 MiB) | 83,632× |
| 604 bytes | 16,777,216 triangles (~592 MB RSS) | ~10⁶× |

`MAX_DEPTH = 8` bounds recursion *depth*; nothing bounded *breadth*, and the work is
branching^depth. A sub-kilobyte file would kill a worker limited to 2 GiB — worse than
refusing it, because the process dies, the lease drops, and the retry feeds the same file
back in.

`Caps::max_triangles = 8_000_000` closes it, checked as triangles accumulate. After the
fix, the 725-byte branching-8 package is refused in 7.7 s at a peak RSS of **278 MB**,
against the 288 MB its arithmetic predicts. The 7.7 s is a known ceiling recorded in spec
§11: refusing *at* the budget means doing the work up to it.

Why the per-task reviews could not see it: each was asked "does this code match its
brief?", and it did — the depth cap was in the brief. The defect was in the plan, and only
a reviewer looking at the whole path with permission to be adversarial had the standing to
find it.

The same review found a **fifth** test that could not fail for its stated reason:
`an_entry_past_the_size_cap_is_refused` filled its entry with 8,192 identical bytes, which
deflate to ~26 — so the *ratio* bound fired at 520, not the 4,096 size cap. The absolute
cap was untested anywhere in the archive path. It hid because both breaches shared one
error message; the fix makes the message name which bound fired, which fixes the message
and the test together.

One more worth recording, because it is a habit worth keeping: the fix agent was told to
prove the default cap refuses a 723-byte package. That was arithmetically impossible —
1.68M triangles is under an 8M budget. It could have lowered the budget to satisfy the
instruction. It kept 8,000,000 as specified, proved the mechanism with an injected small
cap, and reported that the instruction was wrong.

## Rulings made during execution

Recorded in full, with what each costs if wrong, in
`.superpowers/sdd/2026-09-04-phase-1-slice-3b-3mf/progress.md`. In brief: bump `quick-xml`
to 0.41; correct the `arbitrary`/`derive_arbitrary` characterisation (`cfg(fuzzing)`-gated,
never compiled — ten lock lines, eight compiled crates); move the dead-code allow from task
4 to task 8, where `pub use` actually makes the module reachable; accept
`normalized_value` for the deprecated `unescape_value`; rewrite the OOM-causing mutation;
add the two tests that pin ordering and composition; make the transform count check
symmetric; and give the bomb a relationships part so it reaches the cap.

## Ledger

**Closes:** `ROADMAP.md`'s Phase 1 mesh-format line — STL, OBJ and 3MF all ingest to the
same thumbnail and the same three rungs. No migration was needed, which is the evidence
slice 3's seam was cut in the right place.

**Opens:**

| Item | Trigger |
|---|---|
| Archive ingest — one archive, many parts | The next slice. Needs `Outcome`, `part_name_unique_per_library`, `insert_part_chain` and the batch counts reworked for one job producing N parts |
| The RAR licence decision | That same slice, before any code — spec §11 |
| `zopfli` in the tree, unused | A `zip` major that fixes the `deflate-flate2` feature |
| `HandlerError` has no `Display` | It is a library error type, and `CLAUDE.md` says `thiserror` in libraries. Pre-existing; worth fixing when something else touches `lapidary-jobs` |
| `is_unsafe_name` and `model_part_name` edge cases | Recorded as deferred minors in the ledger; the final review should triage them |
| Streaming the parse rather than buffering | Phase 2, for every format at once — a 2 GB STL already buffers today |

## The exact next action

Slice 4 — *derive less, and let a part carry a real picture* — is planned and approved.
Ingest stores L0 only with L1/L2 built on demand, auto-thumbnails become a per-library
setting, and a part can carry an uploaded image. It targets ~665 MB → ~48 MB of
derivatives per 1,000 parts.

Before that, this branch merges to `main`:

```sh
git checkout main && git merge --no-ff feat/3mf-ingest
```
