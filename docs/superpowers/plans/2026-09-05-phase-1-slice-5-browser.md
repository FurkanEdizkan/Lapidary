# Phase 1 slice 5 — drive it from a browser

**Spec:** `docs/superpowers/specs/2026-09-05-phase-1-slice-5-browser-design.md` — binding.
Read it before task 1. §1 already settled the decision the phase plan called a blocker.

**Branch:** `feat/drive-from-browser`, forked from `820157e` on `main`.
**Exit:** scan a folder from the browser, browse it, download a file, `diff` it against the
one you started with — no terminal.

Run the suite with `export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"`
and the standalone test container on 55432 (`lapidary-test-db`, not compose's `db`). Read
the test count from cargo; do not trust a number written in any document — slice 4's plan
asserted a baseline that moved four times.

## The bar, every task

Ten gates, not eight. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
`cargo xtask check-layers`, `check-deploy`, `check-strings`, `export-bindings` (tree clean
after), `check-commit-msg`, **`cargo deny check`**, and **the web suite**
(`npm run typecheck && npm test && npm run build`). The last two joined the bar at slice 4
Step 0 because `tsc --noEmit` had been red for nine tasks without a single gate noticing:
`export-bindings` proves the generated files are current, and nothing compiled the
TypeScript that consumes them.

Every task names a code change that makes its new test fail, runs it, captures the real
output, and reverts byte-identically (`md5sum` before and after). A test whose mutation was
never run is not a test.

---

## Task 1 — `SourceReader`, and the gate that keeps it in one file

`crates/lapidary-storage/src/lib.rs`. Read-only handle: `open(root)` with no `WorkerRole`,
`get(&self, hash, zstd_level: Option<i16>)`, no `put`, no `remove`. It shares `read_blob`
with the two existing stores — the decode flag becomes `zstd_level.is_some_and(|l| l > 0)`.

Module doc says why it exists and why it is not `SourceStore`: reading stored bytes for a
user who asked for those exact bytes is not the open path, and the write surface is the half
`WorkerRole` is worth spending on. Amend the crate module doc's "the API-level half of *the
open path never touches a source file*" sentence to say **open**, not **api** — spec §1.3.

`xtask/src/deploy.rs`: `check_open_path_boundary` gains a second rule — `lapidary-api` may
name `SourceReader` only in a file whose path ends `download.rs`. Same shape as the existing
grep, new `Violation` variant, message naming the file that broke it and what to do. The
existing `SourceStore` rule is untouched.

**Test:** a `SourceReader` reads back what a `SourceStore` wrote, both compressed and raw;
`check_open_path_boundary` accepts `SourceReader` in `download.rs` and rejects it in
`parts.rs`.
**Mutation:** make the new rule accept any path. `check-deploy`'s unit test must fail.

## Task 2 — `source_for_download`

`crates/lapidary-db/src/repo.rs`. One row: blob hash, format, part name, `zstd_level`.
Filters `part.deleted_at IS NULL` — spec §2.1. Orders `f.created_at DESC, f.id DESC` and
`role = 'source'`, identically to `revision_source`, which it sits next to and does not
replace (that one is library-scoped for the derive payload; this one has no second id to
cross-check — the spec says why at length, do not re-derive it).

Reuses `DbError::CorruptBlobHash` on a non-hex column, as `revision_source` does.

**Ruling T1-A — `zstd_level` stays `Option<i16>` end to end.** Task 1 asked whether the
`Option` buys anything, since `StoredBlobRow.zstd_level` is a bare `i16`. It does: that
struct is *write*-side only — it mirrors `lapidary_storage::StoredBlob` and is never
decoded from a row — so it is no precedent. The column is nullable and NULL is real: every
derivative blob is written with `zstd_level NULL` (`repo.rs:479-483`). Source blobs always
carry a concrete level today, so NULL on one means nobody recorded how those bytes were
stored.

**Corrected after task 2 — the ruling's conclusion held, its reasoning did not.** It said
a `COALESCE(zstd_level, 0)` would "serve a zstd frame as the file". It would not:
`SourceReader::get` decodes on `zstd_level.is_some_and(|level| level > 0)`, so `None` and
`Some(0)` both take the raw branch and are byte-identical through today's code. The hash
check catches either. Keep the `Option` for the reason task 2 gave instead — the DB layer
has no business destroying information a nullable column carries, and preserving `None`
lets **task 3 refuse an unrecorded level with a message that names it** rather than
falling through to a generic hash mismatch. Decode the `Option`, pass it through
unchanged.

**Test:** a soft-deleted part's revision returns `None`; a live one returns all four fields.
**Mutation:** drop the `deleted_at` filter. The deleted-part test must fail.

## Task 3 — `GET /api/revisions/{id}/download?variant=original`

New `crates/lapidary-api/src/download.rs`, mounted `Role::Api` only. The one file allowed
to name `SourceReader`.

Use `lapidary_db::DownloadSource`, which task 2 already exported — do not invent a second
name for the same four fields.

Order of operations, and it matters: resolve the row → 404 if `None` → validate `variant`
→ 400 if missing or unknown, naming `original` → **500 if `zstd_level` is `None`, with a
message naming the blob** (spec §2.5.1, added after task 2 pointed out the original order
had nowhere to put it) → read bytes → **re-hash and 500 on mismatch** (spec §2.5) →
`touch_blob` → respond.

The `None` branch and the hash-mismatch branch are both 500 and both refuse to serve. They
are separate because their messages are: one says an operator has a blob row no ingest path
wrote, the other says the bytes on disk are not the bytes we recorded. Collapsing them
loses the only part an operator can act on.

Headers per spec §2.3: `application/octet-stream` always, `ETag` the hash, and
`Content-Disposition: attachment` with both the ASCII fallback and RFC 5987
`filename*=UTF-8''…`. Filename synthesized `{part.name}.{format}`, sanitized (path
separators, control characters, quotes; length capped) — spec §2.4.

**Fixture uses a Turkish part name**, not an ASCII one with a comment about Turkish.
`CLAUDE.md` requires real content in fixtures and `DATA.md` §5.1 names ğ, ş, ı as the reason
this header shape exists.

**Tests:** bytes are byte-identical to what was ingested, for a compressed source and an
`AsIs` one; `variant=3mf` and a missing `variant` are 400 with different messages; a
soft-deleted part is 404; `last_accessed_at` moves on success and **not** on a 404; the
Turkish name appears percent-encoded in `filename*` and transliterated-or-stripped in the
ASCII fallback.
**Mutation:** serve the bytes without decoding zstd. The byte-identity test must fail — and
if the corpus fixture is an `AsIs` format it will not, so pick a format the ingest policy
compresses. Slice 4 shipped a byte-identity test that passed under exactly the drift it
existed to catch because the fixture was too coarse; do not repeat it.

## Task 4 — the grid learns what a part costs

`PartSummary` grows `revision: RevisionId`, `source_hash: BlobHash`, `source_bytes: u64`,
`stored_bytes: u64`, `compressed: bool`. One more LATERAL in `PgParts::page`, mirroring the
thumbnail one; no migration — every column is already on `blob`.

The grid needs `revision` to build a download URL at all, which is why this precedes the
frontend task rather than trailing it as "storage visibility".

Regenerate bindings in this task so `export-bindings` leaves the tree clean.

**Test:** the page returns the real stored size for a compressed part and equal sizes for an
`AsIs` one.
**Mutation:** report `size_bytes` in both fields. The compressed-part assertion must fail.

## Task 5 — download and storage in the UI

`web/src/routes/index.tsx`, strings through `web/src/lib/strings.ts` — no bare user-facing
strings, `check-strings` enforces it.

Per card: a download action that is a plain `<a href download>` to the API URL — the browser
handles `Content-Disposition`, so no fetch, no blob URL, no JS. The short hash renders next
to it, because `DATA.md` §5.1 requires the hash be visible for the user to verify against.
Size on disk beside it, with the compressed state legible without a tooltip.

Per library: source total, derivative total, and the ratio. New `GET
/api/libraries/{id}/storage` returning one aggregate row.

Motion rules apply — 120/180/280 ms, `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity
only, `prefers-reduced-motion` respected. Dark only.

**Test (vitest):** the card's link carries the revision id and `variant=original`; the
visible hash matches the fixture's.
**Mutation:** drop the `variant` query parameter. The link test must fail. A frontend
assertion that also passes for an *absent* element is not an assertion — slice 4's SET-B
ruling caught exactly that shape.

## Task 6 — scan from the UI

**Amended after task 1. The original brief could not have been built.** It said the route
"validates and enqueues" while "the worker keeps the directory walk" — but `enqueue_scan`
takes a *path list*, and that list comes from walking `/ingest`, which the api container
does not mount. There is nothing for an api-side route to enqueue.

`crates/lapidary-ingest/src/scan.rs`'s module doc also argues directly against the shape
this task needs, and it is right on its own terms:

> It would be tidier to enqueue a single "scan this directory" job and answer immediately,
> but the walk is the one part of a scan that can fail in a way the user must see *now*: a
> missing or unreadable `/ingest` mount is a deployment mistake, and behind a job it
> becomes a batch that quietly fails a poll or two later, with the request having already
> answered 202.

**Reverse it, deliberately, and record the reversal in `scan.rs` where the reasoning is
written — not only here.** A new `JobPayload::ScanDirectory` job kind, dispatched on
`job.kind` exactly as slice 4's `derive` is. The api route enqueues one; the worker's
handler does the walk and enqueues the per-file jobs.

Rejected alternatives, with the reason each fails:

- **A Caddy route to `worker:8081`.** Three lines and no Rust, and same-origin through the
  proxy — the spec's §1.2 aside about "a second CORS surface" was wrong and does not apply
  here. It fails on something else: the browser's API surface would span two backends by
  URL pattern, with no gate watching that the pattern still matches the route.
- **Mount `/ingest` on the api too.** Re-litigates why scan lives in `lapidary-ingest` at
  all, and puts the ingest directory on the container whose whole point is that it reads
  metadata and derivatives.

**What makes the reversal honest rather than convenient:** the module doc's objection is
that a failure goes unseen. Today it half-does — `index.tsx:342` renders `failedTotal` as
a *count*, and `batch_status` already returns a `failures` list with `last_error` that
nothing displays. So this task **renders the failure reasons**. A scan that fails on a bad
mount must say so in the browser, or the reversal just moved the terminal round-trip
somewhere less obvious.

### The batch, which is the part that will bite

`enqueue` mints a fresh `BatchId` every call. If the `ScanDirectory` job enqueues its files
into a new batch, the UI polls the scan's own batch, sees `1 of 1` complete, and reports a
finished scan while 150 files are still ingesting.

So: `enqueue_into(batch, library, jobs)`, and the scan job puts its children in **its own**
batch. `batch_status` counts rows by `batch_id` with no stored total (`jobs.rs:293`), so a
growing batch already works — the aggregate is computed, not cached. Verify that rather
than assuming it.

The worker's `:8081` route becomes a thin enqueue of the same job kind. `README.md`'s
first-run curl keeps working, and there is **one** walk implementation rather than two that
drift.

**Tests:** the api route enqueues a `scan_directory` job with the right kind and payload; a
phantom library id is 404, not 202; the handler's walk enqueues one `ingest_file` per mesh
candidate **into the batch it was given**, and `batch_status` on that batch reports the
grown total; an unreadable ingest directory fails the job with a message naming the mount.
**Mutation:** have the scan handler call `enqueue` instead of `enqueue_into`. The
grown-total test must fail. This is the whole reason the task is shaped this way — if that
test passes under the mutation, it is not pinning it.

Second mutation, the one the original brief already had: mount the api route under
`Role::Worker`. The route test must fail. `deploy/web/Caddyfile` proxies only to
`api:8080`, so a route on the worker is unreachable from a browser even though the worker
is listening on 8081.

## Task 7 — docs, then the exit run

Amend, each where it is written:

- `docs/ARCHITECTURE.md` and `lapidary-storage`'s module doc — *open*, not *api* (spec §1.3);
- `docs/DATA.md` §5.1 — the route exists now, and it verifies the hash before serving;
- `docs/superpowers/plans/2026-09-05-phase-1-remaining-slices.md` — slice 5's items land;
- **slice 4 spec §3.3's `unique (part_id)`** is reversed by slice 6's ordered gallery. The
  phase plan carries this item; record it where the spec says it, not only in the plan.

Exit run on the live stack against `/home/jbo/lapidary-ingest-real`, cold
`down -v` + `up -d --build`:

- scan started **from the browser**, parts appear, no terminal touched;
- a downloaded file `diff`s clean against the source on disk — do this for a compressed
  format and for 3MF, since they take different paths through §2.5;
- the displayed hash matches `b3sum` on the downloaded file;
- a Turkish-named part downloads with its name intact in the browser's save dialog;
- storage figures on the card match `du` on the blob, and the library totals match `SELECT
  sum(...)`;
- `last_accessed_at` moves for the downloaded blob and for nothing else;
- 0 WARN, 0 ERROR on cold start.

Then the handoff, then `superpowers:finishing-a-development-branch`, then merge to `main`
with `--no-ff`.

---

## Carried risks

| Risk | Where it bites |
|---|---|
| 65 unpushed commits, CI unrun since slice 2 | The push at Phase F. Both gates CI has that the local bar lacked are in the bar now |
| Source blobs have exactly one warm input | Spec §2.6. A browse-only library goes cold and slice 7 compresses it. Slice 7 decides, deliberately |
| Downloads buffer the whole file | `read_blob` already does. Fine at Phase 1 sizes, wrong for a 2 GB STEP. Streaming is its own slice |
| Two `source` rows on one revision would make `ORDER BY` load-bearing, and it is unpinned | Task 2, deliberately: nothing writes a second source row today. The stakes differ from `revision_source`'s identical gap — a wrong pick there renders the wrong thumbnail, here it hands the user the wrong bytes under a byte-identity claim |
| `HandlerError` has no `Display` | Pre-existing since slice 3b |
