# Phase 1 slice 3 — handoff

The LOD ladder. Every part now carries three glTF rungs beside its thumbnail, OBJ ingests
alongside STL, and `GET /api/blob/{blake3}` serves a rung to callers that can reach it.

Twelve tasks, twelve commits, on `feat/lod-ladder`. **336 Rust tests, 0 failing**, every
gate at exit 0. Not pushed: per the standing workflow, branches merge to `main` locally and
CI runs on a real release.

## What landed

| Task | Commit | What it does |
|---|---|---|
| 1 | `2ec62f4` | Migration 0004: a derivative is stored inline **or** by hash, never both or neither, and `derivative.blake3` is now a foreign key to `blob` |
| 2 | `85e9098` | One `KernelOutput`, and the trait takes bytes rather than a path |
| 3 | `7ca67d1` | Vertex clustering to an indexed mesh, three grids |
| 4 | `c882ece` | The glTF 2.0 binary writer |
| 5 | `0f521db` | `MeshKernel` implements `Kernel` and returns the whole ladder |
| 6 | `ce0d0bd` | `parse_obj`, sharing `parse_stl`'s `finish` gate |
| 7 | `5284a2f` | Format dispatch on the extension; `kernel_version` names the parser |
| 8 | `2ae5975` | Three `derivative` rows and their `blob` rows, per revision |
| 9 | `b37caa5` | `DerivativeStore`'s first production use, and the reap that matches it |
| 10 | `ef69a6f` | `GET /api/blob/{blake3}`, with reachability as the authorization check |
| 11 | `d5de696` | End-to-end: both formats produce a real ladder |
| 12 | this | The exit run |

`KernelOutput`'s shape was Phase 0a follow-up item 2, open since Phase 0a. Task 2 closes it.

## What the exit run actually showed

Run on 2026-09-04 against the same 150 real STLs slice 2 used, plus the new OBJ fixture, on
`docker compose` rebuilt from this branch.

| Claim (spec §10) | Result |
|---|---|
| Every part has four derivative rows | **151 × 4 = 604**: 151 `thumbnail` all inline, 151 each of `tessellation_l0/l1/l2` all by hash — zero rows with both or neither |
| `GET /api/blob/{hash}` returns the `.glb` with `ETag` and immutable caching | **200**, `etag: "6b8d…1bf2"`, `cache-control: public, max-age=31536000, immutable`, `content-type: model/gltf-binary` |
| 404 for a hash nothing references | **404**, body `{"message":"No such file."}` — the same bytes an unknown hash returns |
| The worker does not serve blobs | **404** on `:8081` for a hash the api serves 200 for |
| Each `.glb` opens in an independent validator | **15 / 15** through Khronos `gltf-validator`: 0 errors, 0 warnings, 0 infos, 0 hints |
| `L0 < L1 ≤ L2` | **2246 → 29302 → 35774** on the densest part; holds on every part checked |
| `L2`'s count equals the source mesh's | **yes** on all six densest parts (35774, 35138, 34940, 34934, 34912, 34696) |
| Re-scanning drains to `skipped`, writing no new blobs | **skipped 151, ingested 0**; blob rows 598 → 598, files on disk 598 → 598 |
| Throughput within 3× of slice 2's 74 files/s | **89.4 files/s** (151 files in 1.69 s) — faster, not slower |
| Grid page under 80 ms warm | **4.2 – 6.3 ms**, 50 cards — faster than slice 2's 8.9 ms |
| Zero WARN lines on a cold start | **0** in both `api` and `worker` |

Throughput went **up** despite four times the derivative work. That is machine variance
between runs rather than a real speedup, and the honest reading is only what spec §10 asks:
the ladder did not cost 3×. It cost nothing measurable.

### What the ladder is actually worth

| Rung | Total | Average | Largest |
|---|---|---|---|
| L0 | 7.3 MB | 49 kB | 87 kB |
| L1 | 41 MB | 280 kB | 539 kB |
| L2 | 52 MB | 350 kB | 629 kB |

**L0 is 86.1% smaller than L2 across the corpus**, and 150 of 151 parts decimate. The one
that does not is the OBJ idler bracket: 20 triangles, coarser than L0's own grid, so it
clusters to itself. That is spec §3.6's stated behaviour, and content addressing makes its
three identical rungs one blob with three references.

447 rung blobs back 453 rung rows — six rungs deduplicated against another revision's bytes.

## Two things this slice's execution corrected in the plan

**Task 8's `tessellations` type could not work as specified.** The plan gives
`&[(String, BlobHash)]`. A hash is not enough: task 1's foreign key means every rung needs a
`blob` row before a `derivative` may name it, and a `blob` row needs sizes. The field is
`&[TessellationRow]`, carrying the `StoredBlobRow` and the grid. The grid is persisted as
`params_json`, because `DATA.md` justifies evicting derivatives on the grounds that
`kernel_version` and `params_json` reproduce them — and for a rung the grid is the half
`kernel_version` does not carry.

**Task 9's reap was specified for one branch and needed to be on both, with a guard.** The
plan puts the derivative reap on the `record` error arm only. But `link_existing` writes
rungs too, so that would leak three files per failure. The larger problem is the other
direction: a rung is usually bytes an earlier revision already stores, so reaping on any
failed ingest deletes a part that ingested perfectly well. The reap is keyed on *this job
introduced these bytes*, asked of `PgBlobs::exists` — the same authority the source blob
asks. `a_failed_link_to_existing_bytes_leaves_the_first_parts_blobs_alone` pins it, and the
mutation that removes the guard shows the first part's rung being deleted by the second
part's failure.

Two smaller ones. **Task 7's format-aware `version()`** conflicts with the trait's
`fn version(&self)`; the trait now takes `&KernelParams`, and the one caller with no file in
hand — the startup log — passes a representative one and says so, because what that line
answers is which kernel is linked. **Task 11's first test already existed** as task 9's
`a_real_stl_writes_three_tessellation_blobs_and_rows`, which asserts more, so it was not
written twice.

## Mutations that did not bite as written

Every task's mutation was run and reverted byte-identical. Three needed the *test* changed
before they bit, and all three were the same mistake — a fixture too coarse to discriminate:

- **Task 3's "collapsed cells are dropped"** does not bite on the 20-triangle bracket, nor
  on a 20k sphere, because a collapsed cell is usually also touched by a surviving triangle.
  It needed a deliberate fixture: one large triangle plus one tiny isolated one.
- **Task 11's ladder mutation** would not bite on the bracket either — at 20 triangles its
  rungs are identical by design. It uses the spur gear (848 against 1232).
- **Task 3's `span <= 0.0` guard** was unreachable: `.max(FINEST_MM)` already handled it.
  The dead branch was removed rather than tested.

One mutation is recorded as not biting and was not fixed: dropping
`part_name_unique_per_library` leaves `resumption.rs` green, because `library_holds`
short-circuits before the insert. That is slice 2's finding, still open.

## Rulings made on the owner's behalf

- **The api role now requires `LAPIDARY_BLOB_ROOT`.** It serves derivative bytes, and a
  router with that route and no root would answer 500 for every part opened. `compose.yaml`
  already mounted the volume for the api service and had a comment anticipating this slice;
  it just never named the variable.
- **A malformed hash gets the same 404 as an unknown one.** A 400 would separate
  well-formed-but-absent from malformed, which is a smaller version of the oracle the
  reachability check exists to close.
- **Uncompressed glTF, as the plan decided.** `DATA.md` §2.2's choice of meshopt stands and
  is not revisited; its decoder is Phase 3's viewer and its Rust binding wraps C, which
  would put a C toolchain in the worker image. `kernel_version` carries `glb-1` so Phase 3
  can find these and re-encode.

## Ledger items this slice opens

| Item | Trigger |
|---|---|
| meshopt encoding | Phase 3, when a viewer exists to decode it |
| `params_json` holds two shapes (`{px}` and `{grid}`) | A third shape. Then it becomes a tagged enum in `lapidary-core`, as `job.payload` is scheduled to |
| A referenced derivative missing from disk logs but has no regeneration path | Phase 3, or the first eviction |
| `xtask`'s `EXEMPT` list is pinned by line number | It went stale five times during this slice. Any edit above an exempt line shifts it. Keying on a content fingerprint would end it |

## What this slice proved, and what it did not

**Proved.** Three rungs are generated at ingest for both formats, stored content-addressed,
reference-counted, reaped correctly on failure in both branches, served over HTTP with
immutable caching, and accepted without complaint by a validator that shares nothing with
this code. The grid did not slow down.

**Not proved.** Nothing reads a rung yet — there is no viewer, so "the ladder is useful" is
Phase 3's claim, not this one. The 3MF path is slice 3b and untouched. `Entity` is still
uninhabited, so no measurement snaps to analytic geometry. And the corpus is 150 tabletop
scenery STLs, which are dense and organic; a library of machined parts with flat faces would
cluster differently, and the 86% figure should not be quoted as universal.

## The exact next action

Slice 3b: 3MF ingest. It is the workspace's first ZIP and first XML dependency, and
`DATA.md` §5.4's zip-bomb caps — decompressed size, entry count, ratio, path traversal —
are its real content. It was split out of this slice so that dependency addition gets a
review of its own rather than riding along with geometry work.

Before that, this branch merges to `main`:

```sh
git checkout main && git merge --no-ff feat/lod-ladder
```
