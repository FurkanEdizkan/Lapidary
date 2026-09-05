# Phase 1 slice 5 — handoff

**Branch:** `feat/drive-from-browser`, forked from `820157e` on `main`.
**Exit criterion, met:** scan a folder from the browser, browse it, download a file and
`diff` it against the one you started with — without a terminal.

**Suites at handoff:** 455 Rust, 54 web, all ten gates green on every commit.

## What landed

| Commit | |
|---|---|
| `71805b4` | Spec: how source bytes reach the download path |
| `c27b50a` → `d109602` | `SourceReader`, and a gate that means one file |
| `5dbc842` → `9c6aa10` | `source_for_download`, `deleted_at` filtered |
| `b10078d` → `540ae69`, `cdfa57b` | The download route |
| `0965645` → `22e878a` | The grid learns what a part costs |
| `7437400` → `8e4344f` | Download and storage in the UI |
| `0a9f32d` → `891e165` | Scan from the browser, and why one failed |

## The exit run

Cold `down -v`, fresh volumes, `LAPIDARY_INGEST_DIR=/home/jbo/lapidary-ingest-real`
(150 STLs, 150,220,150 B), compose defaults, `auto_thumbnail` on.

| | |
|---|---|
| Scan | Started from the browser. `POST /scan` → `{queued: 1}` |
| Batch | `total 151`, `scanned 1`, `ingested 150`, `skipped 0`, **`failedTotal 0`** |
| Wall | **3.124 s** (`finishedAt − startedAt`), 150 files ≈ **48 files/s** |
| Sources on disk | **73,125,035 B** from 150,220,150 B ingested — **2.05× ** |
| Derivatives | **13,220,194 B** |
| API totals vs hand-written SQL | **identical**, both figures |
| Download | `cmp` clean against the corpus file |
| `Content-Disposition` | `filename*=UTF-8''…tex%28B%29.stl` plus ASCII fallback |
| `last_accessed_at` | **0 of 300 → 1 of 300**, exactly the blob downloaded |
| Cold start | 0 application WARN, 0 ERROR |

**Do not compare 48 files/s against slice 4's 84.1.** They are different configurations:
slice 4 measured with `auto_thumbnail = false`, so its worker built one L0 rung per file
and no preview. This run had previews on and rendered 150 of them inside the same 3.124 s.
The comparable figure is the derivative total, and it lands exactly where it should:

```
13,220,194  this run, previews on
 5,718,866  the preview bytes slice 4 measured separately
─────────
 7,501,328  ← slice 4's L0-only exit figure was 7.50 MB
```

The rungs are the same bytes slice 4 produced, and the entire difference is thumbnails.
That is a stronger cross-check than a throughput number, because it is reproducible.

## What is carried, not fixed

| | |
|---|---|
| **`HEAD` moves `last_accessed_at`** | `get(handler)` routes `HEAD`; the handler runs to completion and warms the blob while delivering nothing. `blob.rs` has the identical shape. **Escalation trigger:** if slice 7's sweep ships first, a prefetcher or uptime check marks a library warm and the sweep skips exactly the blobs nobody downloads |
| **The walk is retryable and not idempotent** | A job is retried where a request was not. Bounded 3× by `max_attempts`; duplicates settle as `skipped`, so nothing ingests twice, but a first scan can report "150 added, 150 already here". Recorded in `scan.rs` beside the reversal it cost |
| **The progress line freezes on a hidden tab** | react-query does not poll a hidden document. Slice 6's SSE work is where this dies |
| **Derivatives can exceed sources on small libraries** | 237.5% on the six-part example folder, 18.1% on the 150-file corpus. Real data; the label invites the wrong conclusion at small scale |
| A blob shared *across* libraries is charged in full to each | "Deduplicated" is true within a library, not between them |
| A blob reachable as both a `file` and a `derivative` counts in both totals | Needs a rung byte-identical to a source; STL→GLB will not produce one |
| A live part with **zero** revisions is invisible in the grid | Inner revision LATERAL, pre-existing. Nothing writes that state |
| `DATA.md` §1.1 has three storage classes where spec §4 has two | Slice 7 meets this again when it reports what tiering saved |
| `lapidary_ingest::AppState` is down to one live field | `ingest_dir` and `blob_root` are both write-only now; `WorkerHandler` carries the real copies |
| `HandlerError` has no `Display` | Pre-existing since slice 3b |

## What the reviews cost, and bought

Six implementation commits, six independent reviews, five fix rounds. Every review found
something; three found defects the full ten-gate bar could not see, because they were
**assertions comparing a template against itself** or **a claim no test made**:

- the download filename encoder's allowlist (`%`, `'`, `*`) — adding them left the suite
  green while reintroducing a path traversal through RFC 8187 client decoding;
- the missing-source-blob 500 — documented in the spec by a commit that added no test;
- the scan progress line — every assertion called `strings.scan.running(...)`, so
  reversing the arguments, the noun **and** the verb left 49 tests passing.

Two of my own rulings were wrong in their reasoning and right in their conclusion, both
retracted where they were written. One fix I applied myself shipped unpinned on the first
pass and the mutation caught it.
