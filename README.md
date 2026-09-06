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
still ahead — no viewer, no search and no versioning. The scan is a background job and
walks nested directories; files can also be uploaded from the browser.

## What it does

- **Ingest** — drop a folder of STL, 3MF, OBJ, STEP or IGES. Content-addressed and
  deduplicated. Today: STL, 3MF and OBJ, from a mounted directory or a browser upload,
  through a crash-resumable job queue. STEP and IGES need the CAD kernel and are Phase 2.
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
open http://localhost:3000
```

Then press **Scan the ingest folder** in the grid. Or, headless, post to the worker
directly — the same job, and the same batch to poll:

```sh
curl -X POST http://localhost:8081/api/libraries/01931b6e-0000-7000-8000-000000000001/scan
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
same scan twice and the second batch settles every file as `skipped` rather than
`ingested`, which the grid reports as "6 already here".

### Why the ports are split, and why both of them scan

Three services, and the port tells you which one you are talking to:

| Port | Service | What it is |
|---|---|---|
| 3000 | `web` | The SPA, with `/api/*` reverse-proxied to `api` |
| 8080 | `api` | The grid, the open path, and the scan trigger the browser uses |
| 8081 | `worker` | Ingest — the directory walk, and everything that touches a file |

Both ports accept `POST /api/libraries/{id}/scan`, and neither of them walks a directory
in the request. The route writes one `scan_directory` job; the worker picks it up, walks
the mount, and enqueues one job per model it finds *into the same batch*, so the batch id
either port hands back is the one that reports the whole scan. The `curl` above is the
`:8081` copy of that route, kept so a headless first run needs no browser; the Scan button
in the grid is the `:8080` one.

That is what lets the browser start a scan at all. `deploy/web/Caddyfile` proxies `/api/*`
to `api:8080` and to nothing else, so a route only the worker serves is a route no browser
can reach — and the api container mounts no ingest directory to walk, deliberately.
Enqueueing a job is a database write, which the api may do; walking a mount and parsing
geometry is not.

The split the ports draw is the one that matters and is unchanged: opening a part must
never invoke the CAD kernel, so `lapidary-api` is forbidden from depending on
`lapidary-cad` at all (`cargo xtask check-layers`) — which means the ingest handler, which
parses and rasterizes, cannot live in it. It lives in `lapidary-ingest`, and only the
`worker` image compiles that in:

```sh
cargo tree -p lapidary-server                        | grep lapidary-cad   # nothing
cargo tree -p lapidary-server --features mock-kernel | grep lapidary-cad   # two lines
```

One `Containerfile` builds both images; `deploy/compose.yaml` passes
`SERVER_FEATURES: mock-kernel` to `worker` and nothing to `api`, and `cargo xtask
check-deploy` fails the build if that ever stops being true. So the `api` container does
not merely decline to serve `/scan` — the code behind it is not linked into the binary.

Ingest is asynchronous: the POST returns a batch id as soon as the work is queued, and
`GET /api/libraries/{id}/jobs/{batch}` reports how it is going — which is what the grid
polls, and where a failed file's reason comes from. Progress over SSE, rather than a
one-second poll, is a later slice.

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
