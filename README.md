# Lapidary

**A visual index for 3D part libraries, with a return path.**

Look through pictures, search the metadata, download the file. Scan hundreds of parts,
decide if one fits, hand it to the tool that actually edits it, and get the result back
versioned. Hobbyist STL collections and industrial mesh libraries, same architecture.

Lapidary is not a CAD editor, not a slicer, and not a PDM system. It is the layer that
makes the parts you already have findable, inspectable, and reachable from the tools you
already use.

## Status

**Pre-alpha.** `main` is the Rust implementation described in `docs/`, and it is the only
thing that will ship. The Node/Fastify prototype that validated the product idea is gone;
what it established is written up in `docs/prototype-notes.md`, which is the part worth
keeping.

A library ingests STL, 3MF and OBJ from a mounted directory or a browser drop, through a
crash-resumable job queue, and shows a paged grid of cards. Each card carries the part's
own picture, or names its format when nobody has given it one. Parts live in a folder tree
mirrored on disk — one directory per model, its file under its own name with a
`metadata.json` beside it — under categories you can create and drag models between. Full
text and part-number search, format and category facets, saved filters, a quick-look panel,
revisions and locks for a `controlled` library, a dashboard, and sharing between two
installations that were paired by hand. Removing a part is reversible; purging it is a
separate, explicit action.

**The pivot of 2026-10-03 is the thing to know before reading anything older.** Lapidary is
a visual library: pictures and search, downloads, and a 3D view only when somebody presses
**view**. The render pipeline, the CAD kernel, measurement, the assembly tree and shape
similarity were built, measured, and then deliberately removed — about 19,000 lines.
`docs/ROADMAP.md` § "The pivot" carries the ledger and the shas; `CLAUDE.md` carries the
rules that came out of it. Anything in `docs/superpowers/` is a record of intent at a
moment and predates it.

## What it does

- **Ingest** — drop a folder of STL, 3MF or OBJ, from a mounted directory or a browser
  upload, through a crash-resumable job queue. Content-addressed: BLAKE3 first, and a hash
  already in the store short-circuits the whole pipeline. A STEP or IGES file is **refused
  by name**, in a sentence that says to export a mesh instead — never skipped in silence.
- **Triage** — a fast virtualized grid of pictures, full-text and part-number search,
  faceted filters, saved filters.
- **Inspect** — the part's figures (bounding box, volume, surface area, triangle count),
  every one read from the mesh and so labelled approximate, always. **View in 3D** loads the
  original file in the browser; there is no measuring here, because measuring a mesh against
  entities that were never placed would be a number that lies.
- **Version** — immutable content-addressed revisions with a lineage DAG, pessimistic locks
  and a figure-by-figure comparison. Open a part in FreeCAD, save it, and a new revision
  appears automatically.
- **Route** — automatic format negotiation. A slicer gets the part's own file when it
  already reads it, and a `*.lapidary.*` export written from its mesh when it does not.
  Nothing is silently converted.
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

**Upgrading an existing deployment?** Read `docs/DATA.md` §1.1, *"Upgrading a store created
before this layout"*, **before** you bring the stack up. The store moved out of a named volume
and into a host directory, and nothing copies it across for you. Nothing fails at boot if you
skip it — the grid still renders, because a card's picture comes from `part_image` and not from
the store — so the first sign is a download returning 500.

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
the blob store short-circuits the whole pipeline — no parse, no measurement, no write. Run
the same scan twice and the second batch settles every file as `skipped` rather than
`ingested`, which the grid reports as "6 already here".

### Why the ports are split, and why both of them scan

Three services, four with sharing switched on, and the port tells you which one you are talking to:

| Port | Service | What it is |
|---|---|---|
| 3000 | `web` | The SPA, with `/api/*` reverse-proxied to `api` |
| 8080 | `api` | The grid, the open path, and the scan trigger the browser uses |
| 8081 | `worker` | Ingest — the directory walk, and everything that touches a file |
| 8082 | `peer` | Sharing, only with `deploy/compose.sharing.yaml`: installations paired by hand, and nobody else |

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

The split the ports draw is the one that matters and is unchanged: opening a part must never
read a model file on the server, so `lapidary-api` is forbidden from depending on
`lapidary-cad` at all (`cargo xtask check-layers`) — which means the ingest handler, which
parses a mesh and measures it, cannot live in it. (Pressing **view** does read the original
file, and it is the *browser* that reads it: the api serves the ingested bytes through
`download?variant=original` and parses nothing.) It lives in `lapidary-ingest`, and only the
`worker` image compiles that in:

```sh
cargo tree -p lapidary-server                    | grep lapidary-cad   # nothing
cargo tree -p lapidary-server --features worker | grep lapidary-cad   # two lines
```

One `Containerfile` builds both images; `deploy/compose.yaml` passes
`SERVER_FEATURES: worker` to `worker` and nothing to `api`, and `cargo xtask
check-deploy` fails the build if that ever stops being true. So the `api` container does
not merely decline to serve `/scan` — the code behind it is not linked into the binary.

Ingest is asynchronous: the POST returns a batch id as soon as the work is queued, and
`GET /api/libraries/{id}/jobs/{batch}` reports how it is going — which is what the grid
polls, and where a failed file's reason comes from. Progress also streams over SSE at
`GET /api/libraries/{id}/jobs/{batch}/events`, which is what keeps the line moving on a
tab you have switched away from — a browser holds an `EventSource` open on a hidden
document where it will not run a poll. The one-second poll is still there underneath it,
deliberately: the timer is what makes delivery correct, and the stream is what makes it
prompt.

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

**AGPL-3.0-only**, for the entire workspace. Contributions are taken under the DCO; there is no CLA.
