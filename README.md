# Lapidary

**A visual index for 3D part libraries, with a return path.**

Scan hundreds of parts, decide if one fits, hand it to the tool that actually edits it,
and get the result back versioned. Hobbyist STL collections and industrial STEP
assemblies, same architecture.

Lapidary is not a CAD editor, not a slicer, and not a PDM system. It is the layer that
makes the parts you already have findable, inspectable, and reachable from the tools you
already use.

## Status

**Pre-alpha.** `main` holds the Node/Fastify prototype that validated the product idea. It
is a reference implementation to read, not a base to build on. The Rust implementation
described in `docs/` is being built fresh on `rust-rewrite`, and that is the only thing
that will ship.

`rust-rewrite` now runs: the first slice of Phase 1 ingests a directory of STL files and
renders them as a grid of cards with real thumbnails. Everything else in the list below is
still ahead — no viewer, no search, no versioning, no queue, and the scan is synchronous
and non-recursive.

## What it does

- **Ingest** — drop a folder of STL, 3MF, OBJ, STEP or IGES. Content-addressed and
  deduplicated. Today the scan is a synchronous request; the queue that makes it
  non-blocking and crash-resumable is slice 2.
- **Triage** — a fast virtualized grid with real thumbnails, full-text and part-number
  search, faceted filters.
- **Inspect** — a 3D viewer with measurement that snaps to analytic B-rep entities, so
  the numbers are exact rather than tessellated approximations.
- **Version** — immutable content-addressed revisions with a lineage DAG and geometric
  diff. Open a part in FreeCAD, save it, and a new revision appears automatically.
- **Route** — automatic format negotiation. Slicers get 3MF, CAD gets STEP, the viewer
  gets glTF. Never hand a mesh to someone who needed B-rep.
- **Plan** — a node-based board for authoring manufacturing process graphs, and a
  mobile-shaped guide view that answers "what do I make next".

## Running it

Container-first. Podman is recommended; Docker is supported. Copy the env file and set
a password before the first run, then pass it explicitly with `--env-file` — Podman and
Docker do not agree on auto-loading it.

```sh
cp deploy/.env.example deploy/.env   # then edit it and set POSTGRES_PASSWORD
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
curl -X POST http://localhost:8081/api/libraries/01931b6e-0000-7000-8000-000000000001/scan
open http://localhost:3000
```

The scan walks the directory mounted at `/ingest`, which defaults to this repository's
`example/parts` — a DN40 flange, a module-2 spur gear, a vee block, a mounting plate, a
hex spacer and an idler pulley, so a first run shows a populated grid rather than an empty
one. They are modelled to real dimensions by `example/parts/generate.py` (stdlib Python,
deterministic, watertight), which is committed so the STLs have a source rather than being
opaque binaries. Point `LAPIDARY_INGEST_DIR` in `deploy/.env` at your own folder to scan
that instead; see `deploy/.env.example`. The UUID is the library seeded by migration
`0002_parts.sql`; slice 1 has no library picker, so it is the only one there is.

Scanning is idempotent. BLAKE3 is computed before anything else, and a hash already in
the blob store short-circuits the whole pipeline — no parse, no raster, no write. Run the
same scan twice and the second reports `{"ingested":0,"skipped":6,"failed":[]}`.

### Why the scan is on port 8081 and the grid is on 3000

Three services, and the port tells you which one you are talking to:

| Port | Service | What it is |
|---|---|---|
| 3000 | `web` | The SPA, with `/api/*` reverse-proxied to `api` |
| 8080 | `api` | The grid and the open path |
| 8081 | `worker` | Ingest — the only place the scan route exists |

Posting the scan to `:8080` returns 404, and that is the design working, not a routing
mistake. Opening a part must never invoke the CAD kernel, so `lapidary-api` is forbidden
from depending on `lapidary-cad` at all (`cargo xtask check-layers`) — which means the
scan handler, which parses and rasterizes, cannot live in it. It lives in
`lapidary-ingest`, and only the `worker` image compiles that in:

```sh
cargo tree -p lapidary-server                        | grep lapidary-cad   # nothing
cargo tree -p lapidary-server --features mock-kernel | grep lapidary-cad   # two lines
```

One `Containerfile` builds both images; `deploy/compose.yaml` passes
`SERVER_FEATURES: mock-kernel` to `worker` and nothing to `api`, and `cargo xtask
check-deploy` fails the build if that ever stops being true. So the `api` container does
not merely decline to serve `/scan` — the code behind it is not linked into the binary.

Ingest is synchronous today: the POST returns when the whole directory is done. The job
queue, progress over SSE, and a scan button in the UI arrive in later slices of Phase 1.

## Documentation

Start at [`docs/README.md`](docs/README.md) for the map.

| Doc | Contains |
|---|---|
| [`CLAUDE.md`](CLAUDE.md) | Non-negotiable product and technical rules |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Crate graph, deployment, tech stack, supply chain |
| [`docs/DATA.md`](docs/DATA.md) | Storage, schema, search, versioning, transfer |
| [`docs/FEATURES.md`](docs/FEATURES.md) | Full feature list with phase tags and non-goals |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Ten phases with exit criteria, commercial model |

## Licence

**AGPL-3.0-only**, for the entire workspace including `lapidary-enterprise`. The Ed25519
licence file gates fleet size and support entitlement as a contractual boundary, not as
technical DRM. Contributions are taken under the DCO; there is no CLA.
