# Phase 1 — the remaining slices, re-cut

**Why this document exists.** Slices 1–3b built ingest: blob storage, a job queue, three
mesh formats, an LOD ladder, a blob route. What they did not build is a system anyone can
*use*. The web UI is a single read-only page; to put a file into Lapidary you drop it in a
mounted folder and `curl` the worker from the host, because `deploy/web/Caddyfile` proxies
only to `api:8080` and the browser cannot reach the worker at all.

This re-cut sequences the rest of Phase 1 so that a **usable storage-and-display system**
arrives at the end of slice 5, rather than at the end of the phase.

## What exists today

| | State |
|---|---|
| Ingest: STL, OBJ, 3MF → thumbnail + L0/L1/L2 | done (slices 1, 3, 3b) |
| Blob CAS, `ref_count`, zstd -3 on source | done (slice 1) |
| Job queue, crash-resumable | done (slice 2) |
| `GET /api/blob/{blake3}` with reachability check | done (slice 3) |
| Grid page, keyset paginated, inline thumbnails | done (slice 1) — **not virtualized** |
| Everything the UI can do | `GET` parts, `GET` batch status, `GET` a blob. **Nothing else** |
| Putting a file in | drop it in a mounted folder, `curl` the worker from the host |
| Getting a file out | not possible |
| Seeing storage use | not possible |

## The re-cut

### Slice 4 — Derive less, and track use

*Storage. No UI.* Spec written: `2026-09-04-phase-1-slice-4-derivatives-design.md`,
amended by this document.

- Ingest stores **L0 only**; L1 and L2 built on demand
- `library.auto_thumbnail`, default on
- A `derive` job kind, a generic enqueue, `Outcome::Rendered`
- **`blob.last_accessed_at` starts being written** — the column exists (`0002_parts.sql:25`)
  and nothing writes it, and every storage feature after this depends on it

**Measured target:** derivatives from ~665 MB to ~48 MB per 1,000 parts.

**Moved out:** `part_image` upload goes to slice 5. It is UI work and belongs beside the
rest of it; keeping it here made slice 4 two slices wearing one coat.

**Not doing, and why — measured on 143 MB of the real corpus:**

| Level | Compress | Size | Saved | Decompress |
|---|---|---|---|---|
| `-3` (today) | 0.72 s | 70.0 MB | 51% | 0.11 s |
| `-19` ("when cold") | 27.7 s | 64.0 MB | 55% | 0.14 s |

`DATA.md` §1.2 specifies "zstd -3 at ingest → -19 when cold". For **mesh** files that is a
bad trade: 38× the compression CPU for four percentage points. §1.2's 6–10× estimate is for
STEP, which is text; its own binary-STL estimate of ~2–2.5× is what the measurement
confirms. The cold tier stays unimplemented until Phase 2 brings STEP, where it may pay.

Decompression is level-independent and effectively free — 143 MB in 0.11 s means a single
1 MB part decompresses in about a millisecond. **No "decompressing…" indicator is needed**;
opening a compressed part is indistinguishable from opening a stored one.

### Slice 5 — A library you can drive from a browser

*This is the slice that makes Lapidary usable.* Everything here removes a reason to reach
for `curl`.

- **Scan from the UI.** The api cannot see `ingest_dir`, so it enqueues a `Scan` job and
  the worker performs the walk — the same shape slice 4's `derive` job establishes, and the
  reason the payload became a tagged enum. `POST /api/libraries/{id}/scan` moves to
  `Role::Api` as an enqueue; the worker keeps the walking.
- **Download `variant=original`.** Byte-identical ingested bytes, with the hash displayed —
  `CLAUDE.md`'s "downloads are never silently converted" becomes testable for the first
  time.
- **A part detail view.** Name, part number, format, triangle count, bounding box,
  watertightness, measurement provenance, revision, and the approximate badge the product
  rules require.
- **Storage visibility.** Per part: original size, size on disk, whether it is compressed.
  Per library: totals, and what derivatives cost against what sources cost.
- **`part_image`**: upload an image that outranks the generated thumbnail, and delete it.

**Exit:** ingest a folder, browse it, open a part, download the original and diff it against
the file you started with — without a terminal.

### Slice 6 — Phase 1's exit criterion

The roadmap bullets nothing above covers.

- Browser upload: client-side WASM BLAKE3 → probe → chunked resumable transfer
- SSE replacing slice 2's polling; the UI never blocks
- Virtualized grid
- First run seeds a bundled licence-clean example part — never an empty grid

**Exit (`ROADMAP.md`):** drop a folder of 1,000 STLs, the grid is interactive immediately,
all thumbnails land, re-dropping completes in seconds via the hash short-circuit, and the
grid page loads under 80 ms warm.

## What this changes about the roadmap

Nothing in `ROADMAP.md`'s Phase 1 bullet list is added or dropped. The re-cut changes only
*where the slice boundaries fall*, so that the system becomes usable at slice 5 instead of
at the end of the phase. The one substantive addition is storage visibility, which the
roadmap never listed and which the owner asked for.

One roadmap bullet is now inaccurate and is corrected when slice 4 lands: "Mesh ingest
(STL/3MF/OBJ) → thumbnail + L0/L1/L2" becomes "→ thumbnail (optional) + L0, with L1/L2 on
demand".

## Open risks carried into these slices

| Risk | Where it bites |
|---|---|
| 38 commits on `main` have never seen CI | Everything. The local bar is green, but the GitHub matrix has not run since slice 2 |
| The fan-out cap is proven by a manual run, not an automated test through `parse_3mf` | Slice 4 touches `KernelParams`; the `parse_3mf_with(bytes, caps)` seam that closes it is cheap to add while in there |
| `deploy.resources.limits` verified under Docker only | `CLAUDE.md` commits to Podman too. Worth one check before relying on the ceilings |
| Throughput is half slice 3's, from the ceilings | Expected and reversible via `LAPIDARY_WORKER_CONCURRENCY`. Slice 4 should *improve* it — two clustering passes and a render per file disappear |
| `HandlerError` has no `Display` | Slice 4 adds a job kind and will touch its error paths |
