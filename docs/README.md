# Documentation map

| Doc | Contains | Read before |
|---|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Non-negotiable product and technical rules, style | Always loaded |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Crate graph and layering, deployment topology, full tech stack, kernel trait, supply chain policy, licensing conflict | Any structural work, new crate, dependency, or deployment change |
| [`DATA.md`](DATA.md) | Blob CAS, compression and tiering, deletion semantics, fast-open path, metadata extraction, schema, search, source links, upload/download, versioning, watcher | Anything touching storage, the database, ingest, search, or the round-trip |
| [`FEATURES.md`](FEATURES.md) | Complete feature list by area with phase tags and explicit non-goals; detailed build-graph spec | Scoping any feature, or checking whether something is deliberately excluded |
| [`ROADMAP.md`](ROADMAP.md) | Ten phases with hard exit criteria, commercial model, open items | Planning work order, or deciding whether something is in scope yet |
| [`prototype-notes.md`](prototype-notes.md) | What the deleted Node prototype established: domain shape, search payload, LOD approach | Designing `lapidary-core` types, `lapidary-index` search, or `lapidary-cad` LOD |

## Fast answers

- **Which database?** PostgreSQL 18.6, official `postgres:18` image. No SQLite, no
  embedded Postgres, no Mongo.
- **Which container runtime?** Podman leads the docs, Docker is supported. Compose Spec
  syntax, `Containerfile` naming, `:Z` labels, Quadlet for systemd.
- **REST or gRPC?** REST + `ts-rs` + SSE. gRPC was evaluated and rejected —
  `ARCHITECTURE.md` has the reasoning.
- **Where does the CAD kernel run?** Native OCCT in the worker container, always. There
  is no WASM variant.
- **Can I add a dependency?** Check `cargo-deny`'s `[sources]` allow-list, and prefer the
  boring multi-maintainer option.
- **Is this feature planned?** `FEATURES.md`. Items marked `[—]` are deliberate
  non-goals, not oversights.

## Things that look like bugs but are decisions

- Generated columns are explicitly `STORED` — PG 18 defaults to virtual, and virtual
  columns cannot be indexed.
- `blob.last_accessed_at` is updated in batches every 5 minutes, not per read.
- Bundle ZIPs use STORE, not DEFLATE.
- Thumbnails under 64 KB live in Postgres as `bytea`, deliberately.
- The file watcher lives in the native `lapidary agent` binary, never in a container —
  inotify does not propagate through Docker Desktop bind mounts on macOS or Windows.
