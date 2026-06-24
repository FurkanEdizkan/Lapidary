# Lapidary

A fast, dark-themed gallery for your 3D-printable models (STL · 3MF · OBJ). Browse an
image-first grid, open any model in an interactive 3D viewer, and **tag, pin, group, and
search** your library. Each model carries rich metadata — size, creator, dates, type,
compatible printers, suggested print settings (importable from slicer profiles), and photos
of the printed/painted result.

Built to run locally (`npm run dev`), as a single web server, or containerized with **Docker
or Podman**.

## Features

- **Image-first gallery** with Grid / Cards / List views and hover metadata.
- **Smart search** dropdown suggesting matching models, creators, and tags (try `dragon`).
- **Pins & groups** — pin favorite creators/tags/groups; organize models into personal or
  shared folders.
- **3D viewer** — drag to rotate, scroll to zoom; real STL/3MF/OBJ via Three.js, seeded
  sample shapes via a built-in procedural renderer.
- **Three-tier asset storage** — originals are kept **compressed** (zstd/gzip); a tiny
  thumbnail and a decimated **LOD** mesh make the gallery instant. The full mesh is only read
  when you open a model and ask to *View full mesh*.
- **Print settings** — editable rows plus import from PrusaSlicer/Orca `.ini` or Cura `.json`.
- **Printed results** — attach printed/painted photos per model.
- **Optimization layer** — optional **Redis** cache (falls back to in-process LRU) and an
  optional **Rust mesh sidecar** for server-side bbox/triangle-count/LOD. Both degrade
  gracefully; the app is fully functional without either.

## Run locally

```bash
npm install
npm run dev          # Fastify API on :5174, Vite UI on :5173 (proxies /api)
# open http://localhost:5173
```

To enable server-side thumbnails and LOD generation, build the Rust mesh sidecar once
(requires a [Rust toolchain](https://rustup.rs/); run `. "$HOME/.cargo/env"` first if
`cargo` is not yet on your PATH):

```bash
npm run build:mesh
```

The server auto-detects `rust-mesh/target/release/rust-mesh` on startup; no env var needed.
Set `MESH_SIDECAR_BIN` only to override the path (see `.env.example`).

Production (single server serving the built UI + API):

```bash
npm run build
npm start            # serves web/dist + API on :5174 -> http://localhost:5174
```

## Run with Docker / Podman

```bash
docker build -t lapidary .        # or: podman build -t lapidary .
docker compose up                 # or: podman compose up  /  podman-compose up
# open http://localhost:5174
```

- Library data persists in the `lapidary-data` volume (`/data` in the container).
- To index an existing on-disk library, point `LIBRARY_PATH` at it before `up`; it is mounted
  read-only at `/library` and indexed by **Scan** (or `POST /api/scan`).
- Remove the `redis` service to run with the LRU cache fallback.

## Configuration

See `.env.example`. Key variables: `PORT`, `DATA_DIR`, `REDIS_URL` (optional),
`LIBRARY_PATH` (optional scan target), `MESH_SIDECAR_BIN` (optional path to `rust-mesh`).

## Scan a library (background ingest)

Lapidary indexes archived models (`.zip`/`.rar`/`.7z`) and loose meshes
(`.stl`/`.3mf`/`.obj`) **in place** — nothing is copied out of your library.

1. Start the app and the background worker:
   ```bash
   npm run dev          # runs server + worker + web
   ```
2. Point a scan at a folder (or set `LIBRARY_PATH` and omit `folderPath`):
   ```bash
   curl -X POST localhost:5174/api/scan \
     -H 'content-type: application/json' \
     -d '{"folderPath": "/path/to/Creators/Creature Caster"}'
   # -> { "scanned": N, "enqueued": N, "skipped": 0 }
   ```
3. Watch rows appear (the worker peeks each archive and creates a model that
   points at the source file):
   ```bash
   curl -s localhost:5174/api/models | jq 'length'
   ```
4. **Thumbnails and the 3D viewer mesh populate automatically in the background.**
   After `index_archive` jobs finish, the worker enqueues `thumbnail` jobs that
   extract the best mesh entry, run the Rust sidecar to produce a decimated
   LOD mesh (for the in-browser 3D viewer) and a rendered PNG thumbnail, then
   store both under `$DATA_DIR/lod/` and `$DATA_DIR/thumbnails/`. Gallery tiles
   show a skeleton while rendering and switch to the thumbnail when ready.
   Full-resolution meshes are extracted on demand from the source archive when
   you open a model and click *View full mesh*.

   > **Prerequisite:** build the Rust mesh sidecar once before starting the
   > worker (see above: `npm run build:mesh`). Without the binary the worker
   > still runs but thumbnail/LOD jobs will be skipped.

Models are grouped by creator and category derived from the folder layout
(`Creators/<Creator>/<Miniatures|Sets|Terrain>/<item>`). Re-scanning the same
folder is safe — already-indexed archives are skipped (`enqueued: 0, skipped: N`)
and nothing is written to the source library.

## Architecture

- `server/` — Fastify + SQLite (`better-sqlite3`). Modular services (one responsibility each)
  under `server/src/services`, thin routes in `server/src/routes`.
- `web/` — React + Vite. Design reproduced from the design handoff bundle in `design/`.
- `rust-mesh/` — optional Cargo crate: bbox + triangle count + decimated LOD.
- `data/` — runtime library storage: `models/` (compressed), `lod/`, `thumbnails/`,
  `images/`, `profiles/`, `lapidary.db`.

A fresh install is seeded with 20 sample models so the gallery is populated immediately.
