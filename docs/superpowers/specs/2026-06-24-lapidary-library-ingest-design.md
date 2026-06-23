# Lapidary — Real-Library Ingest & Auto-Images: Design Spec

> **Status:** Approved design (2026-06-24). Next step: implementation plan via writing-plans.
> **Supersedes scope priority of** the `files/` plan set: the Rust migration and the two
> backlog features (Import-from-URL, Send-to-Printer) become *supporting tracks sequenced
> after this goal*, not the backbone.

---

## 1. Goal (the north star everything sequences around)

A tested, working Lapidary that you point at `/mnt/Storage2/All/STL Files/Creators`, which then
— **in the background** — indexes your archived STLs (`.zip`/`.rar`/`.7z`), generates
thumbnails/previews, and **auto-fetches matching images** (MyMiniFactory-first, generic
fallback), so you can browse your whole real library *with pictures* in the UI.

**Acceptance is proven end-to-end on the Creature Caster folder first (7 archives), then scaled
to all 41 creators, with the test suite green.**

## 2. Decisions (confirmed with the owner)

| # | Decision | Choice |
|---|---|---|
| D1 | Build path | **Build on the existing Node app now; do the Rust migration later.** The Node app is the parity spec the migration ports from, so this work is not throwaway. |
| D2 | Image-match behavior | **Auto-accept high-confidence matches; queue uncertain ones for one-click review.** |
| D3 | Image source | **MyMiniFactory API v2 where a key is set (richest: name, tags, license, images), per-item rate-limited + cached; generic Open Graph / JSON-LD as fallback for user-pasted URLs.** |
| D4 | Storage | **Index in place.** Treat the Creators folder as read-only; only small derived artifacts (thumbnails, LOD, fetched images) land in `DATA_DIR`. No copying of the archives. |
| D5 | UX polish | **Phase 4 includes** the UX-Spec detail **backdrop** + **SimilarRail** (not deferred). |

## 3. Ground truth — current state (verified, do not re-derive)

**Works today (Node stack):** Fastify + `better-sqlite3` (synchronous) backend with 13 service
modules; React + Vite + Three.js frontend (~18 components, full gallery + detail + add/scan);
a standalone `rust-mesh` CLI (dependency-free STL/OBJ parse, exact bbox + triangle count,
vertex-clustering LOD, binary-STL writer, `--json`). SQLite schema: 9 tables at
`PRAGMA user_version = 1` (`models, tags, model_tags, groups, model_groups, printer_types,
model_printer_types, printer_settings, images, pins`). `DATA_DIR` layout:
`models/ lod/ thumbnails/ images/ profiles/ lapidary.db`.

**Three gaps that block the goal (each is net-new work on *either* stack):**

1. **The scanner imports zero of the real library.** `server/src/services/libraryScan.service.ts`
   recursively collects only loose `.stl/.3mf/.obj` files and **walks past every
   `.zip/.rar/.7z`**. The real library is 100% archives, organized
   `Creators/<Creator>/<Miniatures|Sets|Terrain>/<Item>.{zip,rar,7z}` (41 creators). Today's
   scan would import nothing, and it hardcodes `creator: 'Imported'`, `type: 'Miniature'`, no
   tags, deduping by `name`.
2. **There is no background worker or job queue** anywhere in the repo. Scanning is synchronous
   on the HTTP path. "Automated background ingest + image fetch" requires a worker + queue.
3. **Auto-image-fetch does not exist.** The `files/Lapidary-Import-From-URL-Plan.md` feature is
   post-v1.0, and is explicitly *single-item, user-confirmed, never bulk*. For a **private local
   library**, the defensible automated form is per-item MMF **API** calls (rate-limited, cached,
   attribution stored) — not catalog scraping. The real risk is **match accuracy/coverage**
   (fuzzy-matching a filename to an object; some creators are not on MMF at all), which D2's
   review queue is designed to absorb.

**Not yet in the repo:** the Rust workspace scaffold ("Phase 0 done" per the build plan) exists
only inside `files/lapidary-handoff-complete.zip` / `lapidary-rust-scaffold.zip` — there is no
top-level `Cargo.toml` or `crates/`. Unpacking it is step 0 of the *later* migration track.

## 4. Architecture (Node-now, drawn on the future Rust boundaries)

```
server/src/index.ts    Fastify HTTP — serves /api + web/dist (role unchanged)
server/src/worker.ts    NEW second process — interval poller over the `jobs` table,
                        p-limit concurrency, idempotent + retryable.
                        Does: archive peek, thumbnail render, image fetch.
        ↕  shared SQLite (WAL → safe across 2 processes) + shared DATA_DIR
web/                    React/Vite — gallery fills in thumbnails/images live; new
                        "Needs review" surface; Phase-4 backdrop + SimilarRail.
```

Both processes run under `npm run dev` via `concurrently`. Splitting the worker out now makes
the eventual Rust `app`/`worker` container split a 1:1 port.

**Module contracts (each: one responsibility, typed in → typed out, one entrypoint — and named
to mirror the Rust crate it will become):**

| New/added TS module | Responsibility | Future Rust home |
|---|---|---|
| `archive` | peek/enumerate `.zip/.rar/.7z`, extract a chosen entry to temp | `lapidary-core::archive` |
| `libraryScan` (rewritten) | walk tree → derive creator/type/name from path → enqueue jobs | `library_scan` |
| `jobs` | enqueue/claim/complete/retry rows in `jobs` | `lapidary-worker` queue |
| `thumbnail` | representative mesh → rust-mesh bbox/LOD → rendered PNG | `core::mesh` thumbnail |
| `metadataSource` (trait) + `mmfSource` + `genericSource` | resolve item → candidate images/metadata + confidence | `lapidary-import` |
| `imageFetch` | download + SSRF-guard + cache + store attribution | `lapidary-import` worker job |
| `secrets` | AES-GCM encrypt/decrypt the MMF key | `lapidary-server::secrets` |

**Index-in-place rule:** `models.original_path` references the archive on `/mnt/Storage2`
(read-only) plus the internal entry path. The full original is extracted only on an explicit
"View full mesh"; the worker extracts a single representative entry to temp purely to render a
thumbnail/preview.

## 5. Data model additions — migration to `user_version = 2`

The 9 existing tables are **untouched** (preserves the parity contract the later migration
depends on). Additions:

- **`jobs`** — `id TEXT PK, model_id TEXT, kind TEXT, status TEXT, attempts INTEGER,
  error TEXT, payload_json TEXT, created_at TEXT, updated_at TEXT`.
  `kind ∈ {index_archive, thumbnail, image_fetch}`; `status ∈ {queued, running, done, failed}`.
  Idempotent + retryable (claim by id, bump attempts, backoff on failure).
- **`secrets`** — `ref TEXT PK, ciphertext BLOB, nonce BLOB, created_at TEXT`. AES-GCM; key
  derived from `LAPIDARY_SECRET_KEY` env. Holds the MMF API key. Never logged, never in any
  API response.
- **`models`** gains `source_url TEXT, license TEXT, creator_url TEXT` (attribution for fetched
  data).
- **`images`** gains `source_url TEXT, attribution TEXT, confidence REAL` (auto-matched images
  are traceable and reversible).

## 6. Build phases (gated; each gate is proven on Creature Caster's 7 archives)

### Phase 0 — Worker foundation
- Migration `user_version = 2`: `jobs` table.
- New `server/src/worker.ts` interval poller (p-limit), added to `npm run dev`.
- Refactor scan to **enqueue** jobs instead of working inline.
- **Gate:** worker claims + completes a job; survives restart (in-flight job re-runs, no dupes);
  a failed job retries with backoff and lands in `failed` after N attempts.

### Phase 1 — Archive-aware indexing (in place)
- `archive` module: peek entries of `.zip/.rar/.7z`; select `.stl/.3mf/.obj` entries.
- Rewrite `libraryScan`: the tree-walk is fast (`readdir` only) and enqueues one
  `index_archive` job per archive. That job peeks the archive, derives **creator** from
  `Creators/<X>`, **type/category** from `Miniatures|Sets|Terrain`, and **name** from the
  archive filename (strip a leading `"<Creator> - "`), creates the `models` row (dedup by
  `original_path`), then enqueues the `thumbnail` + `image_fetch` jobs. Heavy archive work
  thus stays off the HTTP path.
- **Gate:** scanning Creature Caster indexes all 7 items with correct creator =
  `"Creature Caster"`, category, and name; a re-scan imports 0; nothing is copied off
  `/mnt/Storage2`.
- **Feasibility to confirm at implementation:** `.rar` via `node-unrar-js` (pure-WASM, MIT, no
  system dependency); `.7z` via `node-7z` + `7zip-bin`. `.zip` is trivial. Multi-part/nested
  archives and an extraction size cap are handled.

### Phase 2 — Thumbnails + viewer mesh
- `thumbnail` job: extract one representative mesh per archive → `rust-mesh` for
  bbox/triangle/LOD → a rendered thumbnail PNG into `thumbnails/`; LOD into `lod/`.
- Gallery thumbnails fill in live (skeleton → image); detail viewer loads the LOD;
  "View full mesh" extracts the original on demand.
- **Gate:** all 7 items show a rendered thumbnail and open in the 3D viewer; the gallery only
  ever loads thumbnails (never a mesh).
- **Feasibility to confirm:** extend dependency-free `rust-mesh` with a small CPU rasterizer
  (project + z-buffer + Lambert) → PNG, reusable verbatim in the Rust `core::mesh` later.
  Fallback: `stl-thumb`.

### Phase 3 — Auto image fetch + review queue
- `secrets` storage for the MMF API key; `source_url/license/creator_url` on `models`;
  `source_url/attribution/confidence` on `images`.
- `metadataSource` trait + `mmfSource` (search MMF API v2 by name + creator → score match →
  fetch images/tags/license/source_url) + `genericSource` (Open Graph / JSON-LD) as the
  fallback for **user-pasted URLs**. (Automated search needs an API; the generic path is for
  the manual review path, where a URL is supplied.)
- `imageFetch` job: download candidate image(s), **SSRF-guarded** (http/https only; reject
  loopback/private/link-local/metadata ranges; re-check IP after each redirect; cap redirects,
  timeout, max size), **rate-limited + cached**, attribution + license stored.
- Confidence routing: high → auto-attach; uncertain → a **"Needs review"** UI
  (confirm / reject / replace / paste-URL).
- **Gate:** with an MMF key, Creature Caster items auto-attach images where confident and queue
  the rest; attribution + license stored and displayed; a private-IP image URL is refused; with
  **no** key, the generic paste-URL path still works.

### Phase 4 — Browse polish + scale to all 41 creators
- Wire creators/categories/tags filters; add the UX-Spec detail **backdrop** layer (blurred +
  darkened primary image, gradient fallback) and the **SimilarRail** (shared tags + same type +
  shared group). Keyset pagination + grid virtualization; loading/empty states.
- Run the full ingest across all 41 creators; tune worker concurrency + MMF rate limits.
- **Gate:** the whole library is browsable with thumbnails + images; the gallery stays instant
  at scale; the review queue is manageable.

### Phase 5 — Test & harden ("tested" in tested-and-working)
- Test suite: archive-parsing fixtures (zip/rar/7z), path-derivation, match-scoring,
  worker idempotency/retry, SSRF matrix, cache behavior. CI. A documented
  "point-at-a-folder → browse-with-images" runbook.
- **Gate:** tests green; the runbook reproduces on a clean checkout against Creature Caster.

## 7. Compliance & guardrails (non-negotiable)

- **SSRF** on every server-side fetch of a user/match-supplied URL (the full matrix above).
  Owned by a security review pass.
- **Not a scraper.** Per-item, user-scoped, rate-limited, cached lookups against the MMF API
  for a *private local* library — never crawling/mirroring a catalog. State this in code
  comments + docs so a future contributor does not "optimize" it into a crawler.
- **Attribution & license.** Always store + display `source_url` + creator; record the object
  license; surface restrictive licenses; honor flags that forbid image reuse (warn + skip).
- **Secrets.** MMF API key only in `secrets` (AES-GCM), never logged, never in any response.
- **Never auto-overwrite** owner-set fields; tags **merge** (union), never replace.

## 8. Definition of Done

Point Lapidary at `/mnt/Storage2/All/STL Files/Creators/Creature Caster` → it background-ingests
the 7 archives → thumbnails + auto-fetched images appear → you browse it *with pictures* in the
UI; **then** the same against all 41 creators, with the Phase-5 test suite green.

## 9. Later tracks (explicitly not now)

- **Rust migration** — the existing 6-phase `files/Lapidary-Rust-Migration-Ultraplan.md`,
  re-anchored: the Node app *with these features* is the parity spec. Step 0 = unpack the
  scaffold zip into the repo. The TS modules were drawn on the Rust crate boundaries
  (§4) so they port mechanically.
- **Send-to-Printer** — `files/Lapidary-Send-To-Printer-Plan.md` (SP-A…SP-D), unchanged,
  after migration.

## 10. Risks & mitigations

- **`.rar/.7z` extraction friction** → choose pure-WASM/MIT libs (§Phase 1); cap extraction
  size; treat unreadable archives as `failed` jobs, not crashes.
- **Image mis-match** → D2 confidence routing + review queue; store `confidence` so any
  auto-attach is reversible.
- **Coverage gaps (creator not on MMF)** → generic paste-URL fallback in the review queue; an
  item simply keeps its rendered thumbnail with no photo (never blank — gradient fallback).
- **Worker starves HTTP** → separate process + bounded concurrency; index-in-place avoids bulk
  copies.
- **Scale (41 creators, hundreds of GB)** → index-in-place, keyset pagination, grid
  virtualization, lazy thumbnail loading.
- **Scope creep back into the migration** → the migration is a *later track*; the gate of every
  phase is the real-library acceptance, not Node-vs-Rust parity.

## 11. Dev-time parallelism (distinct from the runtime worker)

Once Phase 0 lands, **Phases 1, 2, and 3 are largely independent** (archive-walking vs.
rendering vs. image-providers) and can be built by parallel agents — an implementer +
test-engineer + reviewer per phase — converging at the Phase-4 integration. This is the
"fire up as many agents as possible" lever, applied only after the goal + gates are fixed.
