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
that will ship. There is no runnable application on `rust-rewrite` until Phase 1.

## What it does

- **Ingest** — drop a folder of STL, 3MF, OBJ, STEP or IGES. Content-addressed,
  deduplicated, non-blocking, crash-resumable.
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
podman compose --env-file deploy/.env -f deploy/compose.yaml up
```

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
