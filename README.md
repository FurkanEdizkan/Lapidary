# Manifold Print Library

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

Production (single server serving the built UI + API):

```bash
npm run build
npm start            # serves web/dist + API on :5174 -> http://localhost:5174
```

## Run with Docker / Podman

```bash
docker build -t manifold .        # or: podman build -t manifold .
docker compose up                 # or: podman compose up  /  podman-compose up
# open http://localhost:5174
```

- Library data persists in the `forge-data` volume (`/data` in the container).
- To index an existing on-disk library, point `LIBRARY_PATH` at it before `up`; it is mounted
  read-only at `/library` and indexed by **Scan** (or `POST /api/scan`).
- Remove the `redis` service to run with the LRU cache fallback.

## Configuration

See `.env.example`. Key variables: `PORT`, `DATA_DIR`, `REDIS_URL` (optional),
`LIBRARY_PATH` (optional scan target), `MESH_SIDECAR_BIN` (optional path to `rust-mesh`).

## Architecture

- `server/` — Fastify + SQLite (`better-sqlite3`). Modular services (one responsibility each)
  under `server/src/services`, thin routes in `server/src/routes`.
- `web/` — React + Vite. Design reproduced from the Manifold handoff bundle in `design/`.
- `rust-mesh/` — optional Cargo crate: bbox + triangle count + decimated LOD.
- `data/` — runtime library storage: `models/` (compressed), `lod/`, `thumbnails/`,
  `images/`, `profiles/`, `manifold.db`.

A fresh install is seeded with 20 sample models so the gallery is populated immediately.
