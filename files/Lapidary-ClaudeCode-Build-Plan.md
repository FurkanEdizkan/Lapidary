# Lapidary — Claude Code Build Plan (paste-ready)

> **How to use this.** Each phase below is a self-contained brief you paste into Claude Code,
> one at a time, in order. They assume three docs live in the repo: the migration ultraplan
> (`docs/Lapidary-Rust-Migration-Ultraplan.md`), the UX spec (`docs/Lapidary-UX-Spec.md`), and
> the ADRs (`docs/adr/`). Phase 0 (the Cargo workspace scaffold) is **done and committed**.
> Run each phase on its own branch, land via squash-merge PR, and don't start a phase until the
> previous phase's gate is green. Decisions D1–D5 are fixed in the ADRs — Track A (React),
> sqlx, in-process mesh, AGPL+commercial, Cycles default.

**Golden rule for every phase:** *behaviour & data parity first.* Migration `0001` reproduces
the current schema verbatim; the Phase 2 parity test is the arbiter. No schema or API
"improvements" until Phase 6 cutover is done.

---

## Phase 1 — Core domain + DB + mesh

```
You are working in the Lapidary repo. Read docs/Lapidary-Rust-Migration-Ultraplan.md
(Phase 1) and docs/adr/0002-sqlite-driver.md and 0003-mesh-in-process.md first.

Goal: stand up lapidary-db and finish lapidary-core::mesh with zero behaviour change.

Scope:
1. lapidary-db: add sqlx (sqlite, runtime-tokio, macros, migrate). Create migrations/0001
   that reproduces the EXISTING schema VERBATIM — copy it from the old server's
   db/database.ts migrate() (9 tables: models, tags, model_tags, groups, model_groups,
   printer_types, model_printer_types, printer_settings, images, pins; same columns,
   defaults, indexes; WAL; foreign_keys ON). Opening an existing data/lapidary.db must not
   alter it. Add typed repositories that return lapidary-core DTOs.
2. lapidary-core::mesh: move the rust-mesh parsing/bbox/LOD logic into the library (keep the
   CLI as a thin re-export). Then ADD: 3MF parse (zip + quick-xml) incl. embedded
   /Metadata/thumbnail.png extraction, GLB image extraction, g-code thumbnail extraction,
   watertight flag + volume/surface-area. Fill out MeshStats.
3. Port db/seed.ts so a fresh DB seeds the 20 sample models.

Acceptance:
- cargo build/clippy/test/fmt all green.
- A snapshot test serializes ModelDto/ModelDetailDto and asserts the JSON is byte-identical
  to captures from the live Node API (capture them first against the running old server).
- mesh module reads fixtures/cube.stl with bbox 20x20x20 and 12 triangles, and reads a 3MF
  fixture extracting its embedded thumbnail.

Agents: codebase-analyst (capture live JSON fixtures + endpoint inventory), implementer,
test-engineer (parity snapshots), backend-reviewer, code-quality-reviewer.
```

---

## Phase 2 — axum server (API parity)

```
Read the ultraplan Phase 2 and docs/Lapidary-UX-Spec.md (so endpoints match what the UI needs).

Goal: lapidary-server reaches FULL contract parity with the old Fastify API.

Scope:
1. axum + tokio + tower-http. Port all 18 services as modules following the
   .claude/skills/modular-services contract (one responsibility, typed in -> typed out, one
   entrypoint): model, tag, group, pin, printer_type, printer_settings, profile_import,
   image, search, library_scan, asset_pipeline, cache (folding meshSidecar into core::mesh).
2. Reproduce the EXACT /api surface: models list/detail (ModelFilter query), tags, groups,
   pins, printer-types, printer-settings, images, search/suggest, scan, profile-import, and
   asset serving for thumbnails/lod/originals. tower-http ServeDir serves web/dist.
3. search: SQLite FTS5 backing the grouped Models/Creators/Tags suggest dropdown.
4. cache: moka in-process LRU default, optional redis when REDIS_URL set; fallback never errors.
5. asset_pipeline: three-tier storage — zstd-compressed original, decimated LOD, thumbnail;
   content-address by sha256 for dedup. Respect DATA_DIR layout (models/ lod/ thumbnails/
   images/ profiles/).

Acceptance:
- PARITY TEST: a harness hits every endpoint against BOTH the old Node server and the new
  Rust server over the SAME data dir and diffs responses — zero diffs modulo ordering.
- Fresh DB serves the 20 seeded models; gallery JSON matches.
- All gates green.

Agents: implementer, test-engineer (owns the Node-vs-Rust diff harness), backend-reviewer,
security-auditor (path traversal on scan + asset serving), code-quality-reviewer.
```

---

## Phase 3 — Worker + Blender render pipeline

```
Read ultraplan Phase 3 and docs/adr/0005-render-engine.md.

Goal: lapidary-worker produces thumbnail + LOD + Draco-GLB per model, in its own container.

Scope:
1. Migration 0002: a jobs table (id, model_id, kind, status, attempts, error, created_at,
   updated_at). Idempotent + retryable.
2. lapidary-worker: tokio interval poll. For each job: try embedded-thumbnail extraction
   FIRST (pure Rust, free) via core::mesh; else render — stl-thumb fast path, or headless
   Blender (blender -b -P render/render.py, Cycles low-samples) for hero shots. Write a
   Draco-compressed GLB viewer mesh + thumbnail back; bump status. A killed Blender must
   re-run on the next tick.
3. render/render.py: headless Cycles script (no display). GPU via env toggle.
4. Two-container compose: lean app + heavy worker (Blender) sharing the lapidary-data volume.
   GPU passthrough via nvidia-container-toolkit (gpus: all); Cycles-CPU fallback when absent.
   One `docker compose up` (and podman) brings both up locally.

Acceptance:
- Drop an STL into LIBRARY_PATH + Scan -> thumbnail + LOD + GLB appear with no manual step.
- Worker survives a killed render (job retries, no dupes).
- compose up runs on a GPU box AND a plain laptop (CPU fallback).

Agents: implementer, infra-deployer (compose, Dockerfiles, GPU toggle), test-engineer
(idempotency), backend-reviewer, security-auditor (Blender subprocess sandboxing).
```

---

## Phase 4 — Frontend on the Rust API (Track A) + the two UX enhancements

```
Read docs/Lapidary-UX-Spec.md IN FULL and docs/adr/0001-frontend-scope.md. Keep React/Vite.

Goal: the existing UI runs against the Rust API, then add the detail-page backdrop + similar rail.

Scope (in order):
1. Verify theme tokens against UX-Spec section 1 (the #121214 / #2cb4f5 / Archivo +
   JetBrains Mono language). Fix any drift.
2. Point web/src/api/client.ts at the Rust endpoints; confirm camelCase DTO shape matches.
3. Gallery parity: Grid/Cards/List, hover metadata, FTS search suggest, rail filters (pins/
   creators/groups All-Shared/tags), live MODELS·N count, loading skeletons, "No models
   match" empty state.
4. Detail parity: 3D viewer (streams Draco-GLB, NOT the raw original; "View full mesh" loads
   the original only on click), SpecTable, PrinterCompat, SettingsTable (with .ini/.json
   import), PrintedResults, Add/Scan modal.
5. ENHANCEMENTS (UX-Spec section 4): extend DetailOverlay with the blurred+darkened
   printed-photo BACKDROP layer (fallback: any photo -> color-derived gradient), and add a
   SimilarRail component scored by shared tags + same type + shared group (SQL join).
6. A11y + responsive pass: rail collapses to a drawer below ~900px, cyan focus rings,
   keyboard nav of gallery + viewer, prefers-reduced-motion respected.

Acceptance: full UI smoke test green against the Rust backend ONLY (old server stopped);
performance rule honored (gallery loads only thumbnails; detail streams GLB).

Agents: implementer, frontend-ux-reviewer (fidelity to UX-Spec), code-quality-reviewer.
```

---

## Phase 5 — Governance, licensing, CI/CD, funding

```
Read ultraplan Phase 5 and docs/adr/0004-license.md.

Scope:
1. cocogitto already configured (cog.toml). Verify `cog install-hook --all` rejects a
   non-conventional commit locally; ensure the CI commits job runs on PRs.
2. Branch protection on main; PRs squash-merge with the PR title as the conventional commit.
3. CI/CD: extend .github/workflows to build BOTH images (Dockerfile.app, Dockerfile.worker),
   push to ghcr.io on main, and cut a release via cocogitto on tag (CHANGELOG + version bump).
4. Licensing: add LICENSE (AGPL-3.0-only), COMMERCIAL.md (commercial-license terms for large
   orgs), and the CLA referenced in CONTRIBUTING.md. (Flag for legal review — do not invent
   legal text; use the standard AGPL-3.0 text and a clearly-marked commercial-terms template.)
5. .github/FUNDING.yml: a Turkey-workable tip link (self-hosted iyzico or Shopier — NOT
   PayPal-based services). Verify current availability before committing.

Acceptance: non-conventional commit rejected; CI builds + pushes app and worker images to
ghcr.io; Sponsor button renders; LICENSE + COMMERCIAL + CLA present.

Agents: infra-deployer (CI/CD, ghcr, images), security-auditor (secrets in CI, license/CLA
correctness), implementer.
```

---

## Phase 6 — Cutover & decommission Node

```
Read ultraplan Phase 6.

Scope:
1. Run the full Rust stack against a COPY of a real lapidary.db + data dir; verify gallery,
   detail viewer, search, scan end to end.
2. Delete server/ (Node) and remove the MESH_SIDECAR_BIN path/env. Update README to the Rust
   stack. Keep web/ (Track A).
3. Tag v1.0.0 via cocogitto.

Acceptance: Node tree gone; `docker compose up` brings up app+worker; both a fresh DB and an
existing DB work; v1.0.0 released.

Agents: implementer, test-engineer (end-to-end on real data), infra-deployer, backend-reviewer.
```

---

## Sequencing & parallelism notes

- **Strictly ordered:** 1 → 2 → 3 and 5 depend on earlier phases; **4 can start once Phase 2
  is green** (the API contract is what the UI needs) and run in parallel with Phase 3.
- **The parity harness from Phase 2 is reused** in Phase 6 — keep it.
- If any phase would exceed ~30 internal steps, let Claude Code's `/ultraplan` expand that
  single phase rather than cramming it.
- Each brief names the read-only analyst agents (reviewers/auditors) and the write-capable
  builders (implementer/test-engineer/infra-deployer) from your 8-agent pool.
