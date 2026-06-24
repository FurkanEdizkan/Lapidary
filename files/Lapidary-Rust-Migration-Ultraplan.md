# Lapidary → Full-Rust Migration: `/ultraplan` Spec

> **Purpose.** Feed this to Claude Code `/ultraplan`. It is a *migration* plan, not a
> greenfield build. Lapidary already works (Fastify + SQLite + React, optional Rust
> sidecar). The objective is to re-implement the **core and server fully in Rust** with
> **zero loss of the current feature set or data**, then layer on the four gaps that were
> never built: a Blender render worker, commit/branch enforcement, dual licensing + funding,
> and CI/CD.
>
> **Prime directive: behaviour parity first, new features second.** The current API contract
> and on-disk data layout are the spec. Do not "improve" the data model until parity is
> proven and the old stack is decommissioned.

---

## 0. Ground truth (what exists today — do not re-derive)

**Stack as built:** Fastify + `better-sqlite3` (sync) backend, React + Vite frontend, an
optional spawned `rust-mesh` CLI sidecar, Docker/Podman compose with optional Redis. Seeded
with 20 sample models.

**Data directory layout (must be preserved byte-for-byte):**
```
$DATA_DIR/
  models/        # compressed originals (zstd/gzip)
  lod/           # decimated LOD meshes
  thumbnails/    # tiny gallery thumbnails
  images/        # printed/painted result photos
  profiles/      # imported slicer profiles
  lapidary.db    # SQLite (WAL, foreign_keys ON, user_version migration)
```

**SQLite schema (9 tables, port 1:1):** `models` (id TEXT PK, name, creator, type, mesh_kind,
color, format, file_size_bytes, bbox_x/y/z, triangle_count, created_date, added_date,
original_path, lod_path, thumbnail_path, notes), `tags`, `model_tags`, `groups`,
`model_groups`, `printer_types`, `model_printer_types`, `printer_settings` (model_id, ord, k,
v, source, raw_json, profile_path), `images`, `pins`. Indexes on model_tags.tag_id,
model_groups.group_id, models.creator, printer_settings.model_id, images.model_id.

**18 services to port (`server/src/services/`):** model, tag, group, pin, printerType,
printerSettings, profileImport, image, search, libraryScan, assetPipeline, cache, meshSidecar
(+ `types.ts` DTOs, `db/database.ts`, `db/seed.ts`, `config.ts`, `routes/api.ts`).

**DTO contracts (must serialize identically — camelCase):** `ModelDTO`, `ModelDetailDTO`,
`SettingRow`, `ImageDTO`, `ModelFilter`. The React frontend formats everything off these, so
serde output must match the current JSON exactly.

**`rust-mesh` crate (already Rust — promote, don't replace):** dependency-free STL (ascii +
binary) and OBJ parser, exact bbox + triangle count, vertex-clustering LOD decimation, binary
STL writer, `--json` output. This becomes the foundation of the in-process mesh module.

**Config surface (env):** `PORT`, `DATA_DIR`, `REDIS_URL` (optional), `LIBRARY_PATH`
(optional read-only scan target), `MESH_SIDECAR_BIN` (will be retired — mesh goes in-process).

---

## 1. Decision points (resolve in Phase 0 before any code)

These gate everything. The plan assumes the **recommended** answer unless you change it.

- **D1 — Frontend scope.** "Fully Rust" most cleanly means *core + server*. The React/Vite UI
  works and the Three.js viewer needs JS interop regardless of backend language.
  - **(A, recommended)** Keep the React frontend; point it at the new Rust API (contract is
    unchanged, so this is near-zero frontend work). Ship full-Rust *backend* now.
  - **(B)** Rewrite the UI in **Leptos** or **Dioxus** (WASM). Maximal Rust purity, but a
    large rewrite of ~20 working components, and you *still* drop to JS via `wasm-bindgen` for
    Three.js. Treat as an optional Phase 4B *after* backend parity, never as a blocker.
- **D2 — SQLite driver.**
  - **(recommended)** `sqlx` (sqlite, tokio, compile-time-checked queries, async-native,
    built-in migrations).
  - **(alt)** `rusqlite` + `deadpool-sqlite` on `tokio::task::spawn_blocking` — closer to the
    current sync model, no async-SQLite quirks. Pick this if `sqlx` SQLite friction bites.
- **D3 — Mesh in-process vs sidecar.** Promote `rust-mesh` to a **library crate** called
  in-process (recommended — no subprocess per file, shared types). Keep a thin CLI `bin` for
  debugging only. Retire `MESH_SIDECAR_BIN`.
- **D4 — License.** AGPL-3.0 + commercial exception (`COMMERCIAL.md`) is the recommended dual
  model (real OSS, copyleft forces large orgs to comply or buy). Alternative: PolyForm Small
  Business if you want an explicit company-size gate over OSS purity. **Requires a CLA** to
  retain relicensing rights — decide before accepting outside PRs.
- **D5 — Render engine default.** Blender **Cycles** at low samples as the default headless
  renderer (no display needed). `stl-thumb` as the fast path. EEVEE only behind `xvfb-run`.

---

## 2. Target architecture (Cargo workspace)

```
lapidary/
├── Cargo.toml                 # workspace
├── crates/
│   ├── lapidary-core/         # domain types + DTOs (serde, camelCase) + mesh module
│   │                          #   (absorbs rust-mesh: STL/OBJ/3MF parse, bbox, LOD,
│   │                          #    watertight/volume, embedded-thumbnail extraction)
│   ├── lapidary-db/           # sqlx schema + migrations + repositories (typed queries)
│   ├── lapidary-server/       # axum app: routes (thin) → services (one responsibility)
│   ├── lapidary-worker/       # job runner: poll jobs table → extract/render → write back
│   └── rust-mesh/             # thin CLI bin re-exporting lapidary-core::mesh (debug only)
├── web/                       # React/Vite (Track A) — unchanged contract
├── render/render.py           # Blender headless script (Cycles)
├── migrations/                # sqlx migrations (0001 = current schema verbatim)
├── compose.yaml               # app + worker (+ optional redis) two-container split
├── Dockerfile.app             # lean axum image (~50MB, distroless/debian-slim)
├── Dockerfile.worker          # axum-worker binary + Blender (1–4GB, GPU-capable)
├── cog.toml                   # cocogitto: conventional commits + changelog/bump
├── .github/workflows/         # CI: fmt/clippy/test/build + ghcr push + release
├── .github/FUNDING.yml        # Turkey-friendly tip link (iyzico self-host / Shopier)
├── LICENSE                    # AGPL-3.0
├── COMMERCIAL.md              # commercial-license terms for large orgs
└── CONTRIBUTING.md            # commit/branch conventions + CLA
```

**Service → Rust module map** (each follows the repo's existing `modular-services` skill:
one responsibility, typed input → typed output, one public entrypoint):

| TS service | Rust module (in `lapidary-server::services`) | Notes |
|---|---|---|
| model | `model` | CRUD + DTO assembly from joins |
| tag / group / pin | `tag` / `group` / `pin` | many-to-many + pin kinds |
| printerType / printerSettings | `printer_type` / `printer_settings` | editable rows |
| profileImport | `profile_import` | parse PrusaSlicer/Orca `.ini`, Cura `.json` |
| image | `image` | printed/painted photos |
| search | `search` | move to SQLite **FTS5** for the suggest dropdown |
| libraryScan | `library_scan` | index a read-only `$LIBRARY_PATH` |
| assetPipeline | `asset_pipeline` | three-tier: zstd original → LOD → thumbnail |
| cache | `cache` | `moka` in-process LRU + optional `redis` crate, graceful fallback |
| meshSidecar | *(folded into `lapidary-core::mesh`)* | now in-process |

**Key crates:** `axum`, `tokio`, `tower-http` (ServeDir for `web/dist`, compression, etc.),
`sqlx`/`rusqlite`, `serde`, `zstd`, `zip` + `quick-xml` (3MF), `image`, `moka`, optional
`redis`, `sha2` (content-addressing/dedup), `tracing`.

---

## 3. Phased plan (gated; each phase has an exit gate)

### Phase 0 — Decisions & scaffold
- Resolve D1–D5. Record them in an `ADR/` entry each.
- Create the Cargo workspace and empty crate skeletons. CI runs `cargo fmt --check` +
  `clippy -D warnings` on an empty build.
- **Gate:** workspace compiles; decisions recorded.

### Phase 1 — Core domain + DB + mesh
- Port DTOs to `lapidary-core` as serde structs with `#[serde(rename_all = "camelCase")]`;
  **snapshot-test** the JSON against captures from the live Node API.
- `lapidary-db`: migration `0001` = the current schema *verbatim* (WAL, FK on). Repositories
  for each table. Verify it opens an existing `lapidary.db` untouched.
- Promote `rust-mesh` into `lapidary-core::mesh`; keep parse/bbox/LOD as-is, then **add**:
  3MF (zip+XML) parse + embedded `/Metadata/thumbnail.png` extraction, GLB image extraction,
  g-code thumbnail extraction, watertight flag + volume/surface-area.
- **Gate:** DTO JSON byte-identical to Node; mesh module reads existing fixtures (`cube.stl`)
  with matching bbox/triangle output.

### Phase 2 — axum server parity
- Port all 18 services + routes. Thin handlers, logic in service modules.
- Reproduce the **exact** `/api` surface (models, tags, groups, pins, printer-types,
  printer-settings, images, search/suggest, scan, profile-import, asset serving for
  thumbnails/lod/originals). `tower-http::ServeDir` serves built `web/dist`.
- `cache` module: `moka` LRU default, `redis` when `REDIS_URL` set, fallback never errors.
- Port `seed.ts` so a fresh DB still gets 20 sample models.
- **Gate:** **contract parity test** — a script hits every endpoint against both Node and
  Rust servers over the same `DATA_DIR` and diffs responses. Zero diffs (modulo ordering).

### Phase 3 — Worker + Blender (new capability)
- Add a `jobs` table (id, model_id, kind, status, attempts, error, timestamps) — migration
  `0002`. Idempotent + retryable.
- `lapidary-worker`: tokio interval poll → for each job, **try embedded thumbnail first**
  (pure Rust, free), else render: `stl-thumb` fast path or spawn `blender -b -P render.py`
  for hero shots. Write thumbnail + Draco-GLB viewer mesh back; bump status.
- Two-container compose: lean `app` + heavy `worker` sharing the `lapidary-data` volume. GPU
  toggle via env + `nvidia-container-toolkit` (`gpus: all`); Cycles-CPU fallback when absent.
- **Gate:** dropping an STL into `$LIBRARY_PATH` + Scan produces thumbnail + LOD + GLB with no
  manual step; worker survives a killed Blender (job re-runs).

### Phase 4 — Frontend wiring (Track A) / *optional* Leptos (Track B)
- **A:** point `web/src/api/client.ts` at the Rust server; confirm gallery, viewer, search
  suggest, pins/groups, print settings import, printed-results all work. Near-zero churn.
- **B (optional, later):** Leptos/Dioxus rewrite; Three.js viewer via `wasm-bindgen`.
- **Gate (A):** full UI smoke test green against Rust backend only.

### Phase 5 — Governance, licensing, CI/CD, funding
- `cog.toml` (cocogitto): enforce Conventional Commits as a hook + generate CHANGELOG/bump.
  The repo's `.claude/skills/conventional-{commits,branches}` already describe the convention.
- Branch protection on `main`; PRs squash-merge with the PR title as the conventional commit.
- GitHub Actions: `fmt` + `clippy -D warnings` + `cargo test` + build both images; push to
  `ghcr.io`; release via cocogitto on tag.
- `LICENSE` (AGPL-3.0) + `COMMERCIAL.md` + `CONTRIBUTING.md` (with CLA). `.github/FUNDING.yml`
  with a Turkey-workable tip link (self-hosted iyzico or Shopier — **not** PayPal-based
  services, which don't operate in Turkey; verify current availability).
- **Gate:** a non-conventional commit is rejected locally; CI builds + pushes both images; the
  Sponsor button renders on the repo.

### Phase 6 — Cutover & decommission
- Run Rust stack against a **copy** of a real `lapidary.db`; verify gallery + viewer + search.
- Remove `server/` (Node) and the `MESH_SIDECAR_BIN` path. Update README to the Rust stack.
- Tag `v1.0.0` via cocogitto.
- **Gate:** Node tree deleted; `docker compose up` brings up app+worker; fresh + existing DBs
  both work.

---

## 4. Agent assignments (your 8-agent pool)

- **`codebase-analyst`** — Phase 0/1: produce the authoritative endpoint + DTO inventory from
  `routes/api.ts` and capture live JSON fixtures for parity tests.
- **`backend-reviewer`** — Phases 1–3: review axum service ports, sqlx queries, error
  handling, async boundaries.
- **`frontend-ux-reviewer`** — Phase 4: verify the React contract is unbroken (Track A) or
  review Leptos (Track B).
- **`security-auditor`** — Phase 5 + cross-cutting: path traversal on scan/asset serving,
  Blender subprocess sandboxing, license/CLA correctness, secrets in CI.
- **`code-quality-reviewer`** — every phase: enforce the `modular-services` contract, clippy.
- **`implementer`** — primary builder across Phases 1–6.
- **`test-engineer`** — owns the parity test harness (Node-vs-Rust diff), mesh snapshot tests,
  worker idempotency tests.
- **`infra-deployer`** — Phases 3 & 5: two-container compose, GPU toggle, Dockerfiles, CI/CD,
  ghcr.

---

## 5. Risks & mitigations

- **Silent data-model drift.** The biggest risk. Mitigation: migration `0001` is the schema
  *verbatim*, and the Phase 2 parity test diffs real responses. No schema "improvements" until
  Phase 6.
- **`sqlx` SQLite async quirks** (WAL, busy timeout). Mitigation: D2 fallback to
  `rusqlite` + `spawn_blocking` is pre-authorized if friction appears.
- **Headless Blender.** EEVEE needs a GL context → `xvfb-run`/EGL. Mitigation: default to
  Cycles (D5), which renders headless with no display.
- **LOD/decimation quality.** Current vertex-clustering is crude. Acceptable for parity; flag
  `meshopt`/`pymeshlab`-grade decimation as a *post-cutover* enhancement, not in this plan.
- **camelCase serialization.** A single wrong `rename_all` breaks the frontend silently.
  Mitigation: DTO JSON snapshot tests in Phase 1.
- **Funding provider for Turkey.** PayPal-based tip jars don't work; Stripe-based GitHub
  Sponsors is unreliable for TR. Mitigation: self-hosted iyzico/Shopier link in FUNDING.yml;
  verify availability at build time.

---

## 6. Definition of done

1. `server/` (Node) is deleted; the entire backend + core is Rust.
2. An existing `lapidary.db` + `$DATA_DIR` works unmodified on the Rust stack.
3. Every current `/api` endpoint returns contract-identical responses (parity test green).
4. Dropping a model file auto-produces thumbnail + LOD + GLB via the Blender worker.
5. Non-conventional commits are rejected; CI builds + pushes `app` and `worker` images to
   ghcr.io; releases cut via cocogitto.
6. `LICENSE` (AGPL-3.0) + `COMMERCIAL.md` + `CONTRIBUTING.md` (CLA) + `.github/FUNDING.yml`
   present and correct.
7. `docker compose up` brings up the two-container stack locally on a GPU box *and* a plain
   laptop (Cycles-CPU fallback).
