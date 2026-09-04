# Documentation map

| Doc | Contains | Read before |
|---|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Non-negotiable product and technical rules, style | Always loaded |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Crate graph and layering, deployment topology, full tech stack, kernel trait, supply chain policy, licensing | Any structural work, new crate, dependency, or deployment change |
| [`DATA.md`](DATA.md) | Blob CAS, compression and tiering, deletion semantics, fast-open path, metadata extraction, schema, search, source links, upload/download, versioning, watcher | Anything touching storage, the database, ingest, search, or the round-trip |
| [`FEATURES.md`](FEATURES.md) | Complete feature list by area with phase tags and explicit non-goals; detailed build-graph spec | Scoping any feature, or checking whether something is deliberately excluded |
| [`ROADMAP.md`](ROADMAP.md) | Ten phases with hard exit criteria, commercial model, open items | Planning work order, or deciding whether something is in scope yet |
| [`prototype-notes.md`](prototype-notes.md) | What the deleted Node prototype established: domain shape, search payload, LOD approach | Designing `lapidary-core` types, `lapidary-index` search, or `lapidary-cad` LOD |
| [`superpowers/specs/`](superpowers/specs) | Per-slice design specs: the decisions, data flow, schema and testing plan for one slice | Working on a slice — the spec for it is the closest thing to a contract for what it does |
| [`superpowers/plans/`](superpowers/plans) | The execution plans those specs were built from, and historical handoffs | Tracing why something was built the way it was; not a description of the current system |

The docs above describe the system. `superpowers/` describes particular pieces of work
on it, so a spec is authoritative for its own slice and silent about everything else,
and a plan is a record of intent at a moment rather than of what shipped. Where a spec
and the code disagree, one of them is a defect — say which, in the spec.

## Working on a new machine

`.claude/settings.json` is committed and declares the plugins this repo's workflow
assumes, plus the marketplaces they come from. Open the repo on a new machine and Claude
Code registers those marketplaces and enables the plugins; `/plugin` shows their state and
`/plugin update` refreshes them.

| Plugin | Why the repo needs it |
|---|---|
| `superpowers@claude-plugins-official` | The slice plans name `superpowers:subagent-driven-development` as a required sub-skill, and the handoffs call its `scripts/` to regenerate a task ledger |
| `my-skills@furkan-skills` | `conventional-branches`, `conventional-commits`, `modular-services` — this repo's commit and branch conventions |
| `rust-analyzer-lsp@claude-plugins-official` | A twelve-crate Rust workspace |
| `ponytail@ponytail` | Bias toward the smaller solution |

Skills are **declared, not vendored**. The three in `my-skills` used to be copied into
`.claude/skills/`, which forked them from `FurkanEdizkan/My-Skills` — edit one and the
other silently stops matching, on every machine independently. Editing them upstream and
running `/plugin update` is the supported path; a copy in this repo would shadow it.

`.claude/settings.local.json` is for per-machine overrides and is gitignored, so nothing
you put there travels. To turn one of the above off on a single machine, set it to `false`
there — local settings win over project settings.

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
