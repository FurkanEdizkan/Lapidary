# Library Ingest — Project Status & Continuation

> Portable snapshot of where development stands, so it can be resumed on another machine.
> (Session memory does not transfer between machines; this file does.)

## North star

Point Lapidary at the real STL library (`Creators/<Creator>/<Miniatures|Sets|Terrain>/<item>.{zip,rar,7z}`,
41 creators) and, in the background, **index the archived STLs in place** and **auto-fetch matching
images** (MyMiniFactory-first), so the whole library is browsable with pictures. Tested/working,
proven on the Creature Caster folder first, then scaled to 41 creators.

**Key decision:** build on the existing **Node** stack now; the Rust migration is a *later track*
(the Node app is the parity spec it would port from). Index-in-place (no copying hundreds of GB).

Full design + phase breakdown: [`specs/2026-06-24-lapidary-library-ingest-design.md`](specs/2026-06-24-lapidary-library-ingest-design.md).

## Phases

| Phase | What | Status | Branch / PR |
|---|---|---|---|
| 0+1 | Worker + jobs queue + archive-aware indexing (zip/rar/7z, in place; creator/category/name from path) | **Done** | `feat/library-ingest` → PR #1 |
| 2 | Thumbnails + viewer LOD: `rust-mesh` software rasterizer, `thumbnail` worker job, archive-aware `/original`, gallery "rendering…" skeleton | **Done** | `feat/library-thumbnails` → PR #2 (stacked on #1) |
| 3 | **NEXT** — MyMiniFactory image auto-fetch + review queue (auto-accept high-confidence, queue uncertain; MMF API key + generic OG/JSON-LD fallback) | Not started | — |
| 4 | Browse polish (detail backdrop + similar rail) + scale to all 41 creators (keyset pagination, grid virtualization) | Not started | — |
| 5 | Test/harden pass | Not started | — |

Plans are written per-phase (spec → writing-plans → subagent-driven-development) when the phase starts —
Phase 3/4/5 detailed plans intentionally do not exist yet. Per-phase plans live in `docs/superpowers/plans/`.

Both gates PASSED 7/7 on `…/Creators/Creature Caster` (indexed → thumbnails + working 3D viewer).

## Set up on a new machine

```bash
git clone <repo> && cd Lapidary
git checkout feat/library-thumbnails    # has all Phase 0+1+2 code (or chore/dev-portability for code + this dev env)
npm install                              # rebuilds better-sqlite3 natively

# Rust toolchain for the thumbnail renderer (rust-mesh):
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"                      # cargo installs to ~/.cargo/bin (NOT on PATH by default)
npm run build:mesh                        # builds rust-mesh/target/release/rust-mesh (auto-detected by config)

npm run dev                               # server + worker + web (concurrently)
```

Then scan a library (or set `LIBRARY_PATH` and omit `folderPath`):

```bash
curl -X POST localhost:5174/api/scan -H 'content-type: application/json' \
  -d '{"folderPath": "/path/to/Creators/Creature Caster"}'
# → models index in the background; tiles + 3D viewer fill in as the worker renders
```

**Notes for the next session:**
- Without the `rust-mesh` binary the app still runs and indexes; thumbnail jobs just fail gracefully (tiles keep the placeholder). Build it with `npm run build:mesh`.
- `MESH_SIDECAR_BIN` env overrides the auto-detected binary path.
- `node_modules/`, `rust-mesh/target/`, `data/` (SQLite + thumbnails), and `graphify-out/` are gitignored — reinstall/rebuild/regenerate locally. The indexed DB does not transfer; re-scan on the new machine.
- The reference library used for the gates is at `/mnt/Storage2/All/STL Files/Creators/` (41 creators).

## Custom dev environment (committed for portability)

- **Skills**: `.agents/skills/` + `.claude/skills/` (incl. Impeccable for UI design, modular-services, conventional-commits/branches). Pinned by `skills-lock.json`.
- **Impeccable**: `.impeccable/config.json` + `design.json` (design-system data); the design-detector hook is wired in `.codex/hooks.json` → `.agents/skills/impeccable/scripts/hook.mjs`. UI work under `web/` should read `PRODUCT.md` + `DESIGN.md` first.
- **Project instructions**: `CLAUDE.md` (root). Original input plans + Rust scaffold: `files/`.
