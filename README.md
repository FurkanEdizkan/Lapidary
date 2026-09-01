# Lapidary

**A visual index for 3D part libraries, with a return path.**

Scan hundreds of parts, decide if one fits, hand it to the tool that actually edits it,
and get the result back versioned. Hobbyist STL collections and industrial STEP
assemblies, same architecture.

Lapidary is not a CAD editor, not a slicer, and not a PDM system. It is the layer that
makes the parts you already have findable, inspectable, and reachable from the tools you
already use.

## Status

**Pre-alpha, mid-rewrite.** `main` holds a working Node/Fastify prototype that validated
the product idea. The Rust implementation described in `docs/` is being built alongside
it on `rust-rewrite`. See `docs/MIGRATION.md` for what survives the cutover and what
does not.

Nothing here is production-ready and the licence is not yet decided.

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

Container-first. Podman is recommended; Docker is supported.

```sh
podman compose up
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

**Not yet decided.** See the licensing section in `docs/ARCHITECTURE.md` — the choice
between a fully AGPL-3.0-only project and an AGPL core with a separately licensed server
must be settled before the first external contribution is accepted. Until a `LICENSE`
file exists, all rights are reserved.
