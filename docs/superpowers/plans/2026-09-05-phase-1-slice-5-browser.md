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

**Test:** a soft-deleted part's revision returns `None`; a live one returns all four fields.
**Mutation:** drop the `deleted_at` filter. The deleted-part test must fail.

## Task 3 — `GET /api/revisions/{id}/download?variant=original`

New `crates/lapidary-api/src/download.rs`, mounted `Role::Api` only. The one file allowed
to name `SourceReader`.

Order of operations, and it matters: resolve the row → 404 if `None` → validate `variant`
→ 400 if missing or unknown, naming `original` → read bytes → **re-hash and 500 on
mismatch** (spec §2.5) → `touch_blob` → respond.

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

`POST /api/libraries/{id}/scan` on `Role::Api`, enqueue-only, returning the existing
`ScanAccepted`; 404 for a library id matching no row. The worker keeps the directory walk —
only it mounts `ingest_dir`. Same shape slice 4's `derive` job established; reuse
`enqueue`, do not write a second one.

The worker's `:8081` scan route stays — `README.md`'s first-run curl documents it.

Frontend: a scan control in the action bar, wired to the existing batch-status polling so
progress appears without a reload.

**Test:** the route enqueues a job with the right kind and payload; a phantom library id is
404, not 202.
**Mutation:** mount it under `Role::Worker`. The route test must fail — and that is the
finding worth encoding: `deploy/web/Caddyfile` proxies only to `api:8080`, so a route on the
worker is unreachable from a browser even though the worker is listening.

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
| `HandlerError` has no `Display` | Pre-existing since slice 3b |
