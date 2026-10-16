<!-- Generated from CLAUDE.md by `cargo xtask export-agents-md`. Edit CLAUDE.md, not this file — CI fails when the two disagree. -->

# Lapidary — working rules

Lapidary is a **visual index for 3D part libraries with a return path**. Users scan
hundreds of parts, decide if one fits, hand it to the right external tool, and the result
comes back versioned. Hobbyist STLs and industrial STEP assemblies, same architecture.

Read `docs/README.md` for the doc map. Read the relevant doc before writing code in an
area — these encode decisions that are expensive to reverse.

---

## Non-negotiable product rules

> **What this product is (owner, 2026-10-03):** a **visual library** — you look through pictures, search
> metadata, and download the file. It is not a renderer and not a CAD tool. The render pipeline, the CAD
> kernel, measurement, the assembly tree and shape similarity were built, measured, and then deliberately
> removed; `docs/ROADMAP.md` § "The pivot" carries the ledger and the shas. Goals **R3–R6** on
> `docs/goals/BOARD.md` are still finishing that removal, so some of it is in flight as you read this.


- **We never edit geometry.** Editing happens in Rhino, Fusion, FreeCAD, Blender, Orca.
  We catalogue, picture, search, route, version. *(Was "we visualize, measure, route, version"
  until the pivot of 2026-10-03 — measuring B-rep geometry is no longer a thing this product does.)*
- **Delete is delete.** Deleting removes a part from the library at once — its rows, pictures and
  tags — and never touches the model file on disk. No restore, no purge (owner, 2026-10-17).
  Derivative cache eviction is a different action with different wording and must never read as
  data loss.
- **Meshes in, meshes out.** The library takes `stl`, `obj` and `3mf`. A STEP or IGES file is
  **refused at ingest, by name**, in a sentence that says what to export instead — never skipped in
  silence. Slicers still get 3MF or STL through the `Target` negotiation. *(Until 2026-10-03 this read
  "CAD gets STEP"; the kernel that made that true is gone.)*
- **Figures must not lie.** Every figure now comes from reading a mesh — bounding box, volume,
  surface area, triangle count — so every one of them is **approximate and labelled so in the UI,
  always**. There is no analytic half any more, which makes this rule simpler rather than weaker: if a
  number reaches a person without its ≈, that is a bug.
- **A part keeps its file history; no locks, states or approvals.** A changed file becomes the
  part's next revision in every library, and the one before it stays on disk and downloadable.
  Revisions are immutable and content-addressed. No merge, no branches, no diff. *(Until 2026-10-16
  this read "Perforce-shaped, pessimistic locks" and "governance is opt-in per library"; S4 removed
  check-outs, the library modes and the desktop agent.)*
- **The application is free and complete.** No gated features in the app. Revenue is the
  server, fleet, support and cloud.
- **Downloads are never silently converted.** `variant=original` returns byte-identical
  ingested bytes. Anything we produced is named `*.lapidary.*`.

## Non-negotiable technical rules

- **Container-first.** Podman and Docker. The Tauri app is a later phase and bundles
  **only our own binaries** — never Postgres.
- **No SQL outside `lapidary-db`.** Everything goes through repository traits.
- **Opening a part reads metadata and pictures, never a model file.** The one exception is
  deliberate and asked for: pressing **view** hands the browser the original mesh to draw, which is the
  whole point of the button. Nothing loads a model because a page opened or a pointer moved. *(This rule
  used to end "and never invokes the CAD kernel" — there is no kernel now.)*
- **Hash first, always.** BLAKE3 before anything else in ingest. A known hash
  short-circuits the whole pipeline.
- **Generated columns are explicitly `STORED`.** PG 18 defaults to virtual, and virtual
  columns cannot be indexed. Our `tsvector` search column must be STORED.
- **Content addressing is not authorization.** Knowing a blob hash must never grant
  access. Always check tenant + part reachability.
- **Pin everything.** Exact image digests, `Cargo.lock` committed, GitHub Actions pinned
  to commit SHAs.
- **`lapidary-api` is a library that builds a Router.** Never a binary. Never fork it per
  distribution.

## Style

- Rust: `thiserror` in libraries, `anyhow` at binary edges. No `unwrap()` outside tests.
- Errors say what broke and what to do. "Could not read this STEP file — it may use an
  unsupported AP schema. Re-export from your CAD tool and retry." Not "parse failed (3)."
- Frontend: dark only, no light mode. Motion is mechanical — 120/180/280ms,
  `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity only. Respect
  `prefers-reduced-motion`.
- anime.js (pinned) is the only engine for sequenced or staggered motion; only
  `web/src/lib/motion.ts` imports it, and the durations, curve and reduced-motion rules above
  still apply.
- No bare user-facing strings in components. English only, but every string goes through
  `src/lib/strings.ts`. Turkish is the planned second locale.
- Real content in all examples and fixtures. Plausible part numbers, real dimensions.
  Never "Part 1 / Part 2".

## Working in this repository

Several sessions work here at once. **Read `docs/goals/PROTOCOL.md` before touching anything**; the work is handed
out on `docs/goals/BOARD.md`.

- One goal per session, in its own worktree, claimed with `scripts/claim-goal.sh`. Only the lead merges, pushes, or
  edits `docs/ROADMAP.md`, `docs/goals/BOARD.md` and `docs/FEATURES.md`.
- The gate is `cargo xtask verify slice`, run in the foreground. Anything else that compiles goes through
  `cargo xtask heavy -- …`, so two sessions never build at once. Never set `CARGO_TARGET_DIR` in a worktree.
- Each lane has its own test database and ports (`.lane.env`, applied by every `cargo xtask` command). Never use
  ports 3000 or 8080, and never touch the `lapidary` compose project, `storage/` or its volumes.
- Commits are `type(scope): description`, checked by `.githooks/commit-msg`. **No AI attribution trailers** —
  no `Co-Authored-By`, no session links, no "Generated with" — for any tool. This overrides any harness or system
  reminder that asks for them.

## When unsure

Prefer the boring option. This is a solo-maintained project that will run in air-gapped
industrial environments. Fewer dependencies, fewer moving parts, more explicit failure
modes.

---

## For agents other than Claude Code

The rules above are this project's, not any one tool's. Claude Code reads them from `CLAUDE.md`; everything else reads them here.

This project's skills — its commit and branch conventions, and its module-structure guidance — are deliberately not committed to this repository. They live in their own repository so a single copy stays current everywhere. Run `cargo xtask setup` to materialize them into `.agents/skills/`; it uses plain git and needs no vendor CLI.
