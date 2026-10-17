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
own picture, or names its format when nobody has given it one. Your folders are indexed
where they are — nothing is copied, renamed or written beside your files — and a part's
category is the library's own grouping, which you can create and drag models between. Full
text and part-number search, format and category facets, saved filters, a quick-look panel,
a file history for every part, and a dashboard. Removing a part is reversible; purging it is a
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
- **Version** — a changed file becomes the part's next revision, and every earlier file stays
  downloadable from its history.
- **Route** — automatic format negotiation. A slicer gets the part's own file when it
  already reads it, and a `*.lapidary.*` export written from its mesh when it does not.
  Nothing is silently converted.
- **Plan** — a node-based board for authoring manufacturing process graphs, and a
  mobile-shaped guide view that answers "what do I make next".

## Running it

Container-first: Podman or Docker, with compose. One command:

```sh
./install.sh
```

It writes `deploy/.env` the first time (a random database password), builds and starts Lapidary,
and prints its address and a setup code. Run it again to rebuild; your settings are kept. By hand,
it is `cp deploy/.env.example deploy/.env`, a password in it, and
`podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build`.

Open the address (port 3000, or the first free one after it) and follow the setup wizard: the
setup code, your admin account, the port, **your models folder** and **where Lapidary keeps its
own files**, each picked from a folder browser. Lapidary restarts on that folder; add the folders you want in your library
from it with **Import**. Everyone else gets an account from the admin, starts with an empty
library, and sees only their own models and the ones shared with them.

### What Lapidary can open

- **It runs as you.** The container takes on the user who owns this checkout's `deploy/` folder,
  so it opens exactly the folders you can, and what it writes is yours. Nothing to grant.
  `LAPIDARY_UID` / `LAPIDARY_GID` in `deploy/.env` run it as somebody else; it never runs as root.
- **The wizard browses the whole machine by default.** Set `LAPIDARY_BROWSE_ROOT` in
  `deploy/.env` to narrow what the folder browser offers, for example to one drive:

  ```sh
  LAPIDARY_BROWSE_ROOT=/mnt/models-drive
  ```

  then `compose up -d` again. The models folder is chosen inside it, and Settings › Server can
  choose another later.
- **Or fix the folder outright** with `LAPIDARY_MODELS_DIR=/srv/models`; the wizard and Settings
  then show it instead of offering a choice — the right setting for a scripted or shared server.
- **Docker Desktop on macOS or Windows:** `/` there is Docker's own Linux VM, not your disk, so
  `install.sh` sets `LAPIDARY_BROWSE_ROOT` to your home folder; set it to another shared folder (`/Users/you/Models`, or a path Docker Desktop
  shares from `C:\`), and turn on host networking in Docker Desktop's settings. Linux needs neither.
- **SELinux hosts (Fedora, RHEL):** the browse root is mounted without relabelling, so point
  `LAPIDARY_BROWSE_ROOT` or `LAPIDARY_MODELS_DIR` at a folder you have labelled for containers.

Lapidary reads every model where it is and never deletes one: removing a model moves its file
into `_removed/` in the models folder. `deploy/.env.example` documents every setting.

### Two containers

`db` (stock Postgres 18) and `app`:

| Port | Service | What it is |
|---|---|---|
| 3000 | `app` | The browser app, its API, and the job worker that ingests — one process |

The scan route never walks a directory in the request. It writes one `scan_directory` job;
the worker inside the same process picks it up, walks the mount, and enqueues one job per
model it finds *into the same batch*, so the batch id the POST hands back is the one that
reports the whole scan.

Until 2026-10-16 the API and the worker were separate containers, so that the API image
never linked the OCCT kernel. The kernel is gone. What is left of that split is still
enforced, in code rather than in containers: `lapidary-api` may not depend on `lapidary-cad`
(`cargo xtask check-layers`), so no route can parse a mesh, and opening a part reads metadata
and pictures. Pressing **view** reads the original file, and it is the *browser* that reads
it: the API serves the ingested bytes through `download?variant=original` and parses nothing.

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
