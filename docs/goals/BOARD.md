# The board

**Only the lead edits this file, on `main`.** Lanes claim a goal with `scripts/claim-goal.sh` (the branch is the
claim) and tell the lead; see [`PROTOCOL.md`](PROTOCOL.md). A goal may be claimed when every goal in its **Depends
on** is `merged`. At most two **Rust** goals are `claimed` at once.

Status: `open` → `claimed (lane n)` → `ready` → `merged <sha>`.

## Phase 6 and the loose ends (planned 2026-09-21)

| Id | Goal | Wave | Kind | Depends on | Owns | Migrations | Status | Branch |
|---|---|---|---|---|---|---|---|---|
| [B0](B0.md) | Several sessions at once | 0 | lead | — | `docs/goals/`, `scripts/`, `xtask/src/lane.rs`, `CLAUDE.md`, `.claude/settings.json` | — | claimed (lead) | `chore/parallel-sessions` |
| [W0](W0.md) | Phase 6 contracts | 0 | lead, Rust | B0 | `lapidary-core/src/{shape,event,link}.rs`, `lapidary-db/src/shapes.rs`, `repo.rs` purge + `rows_by_id`, `lapidary-api/src/{likeness,dashboard}.rs` (types only), `strings.ts` blocks | 0047, 0048 | open | `feat/phase-6-contracts` |
| [G2](G2.md) | Shape profiles in the worker | 1 | Rust | W0 | `lapidary-cad/src/{shape,glb}.rs`, `JobPayload::ProfileShape`, `lapidary-ingest/src/shape.rs`, dispatch in `handler.rs`/`derive.rs`, `lapidary-db/src/shapes.rs` reads | — | open | `feat/shape-profiles` |
| [G3](G3.md) | Likeness API | 1 | Rust | W0 | `lapidary-db/src/likeness.rs`, `lapidary-api/src/likeness.rs` handlers, `crates/lapidary-api/tests/likeness.rs` | — | open | `feat/likeness-api` |
| [G6](G6.md) | Likeness UI | 1 | web | W0 | `web/src/lib/likeness.ts`, `routes/duplicates.tsx`, `components/Likeness.tsx`, its mount in `PartDetail.tsx`, "folded into" on `removed.tsx`, the look-alike line in `index.tsx`, `likeness` strings block | — | open | `feat/likeness-ui` |
| [L1](L1.md) | Sharing screens, seen and finished | 1 | web (+ one db read) | B0 | `routes/sharing*.tsx`, `components/ShareDialog.tsx`, `sharing` strings block, `PgShares::requests` | — | open | `fix/sharing-screens` |
| [G4](G4.md) | Dashboard resolve | 2 | Rust | W0, G3 | `lapidary-api/src/dashboard.rs` handler, the grid helper in `parts.rs`, `lapidary-db/src/dashboard.rs` | — | open | `feat/dashboard-resolve` |
| [G1](G1.md) | App-wide event stream | 2 | Rust | W0 | `lapidary-db/src/events.rs`, `lapidary-api/src/events.rs`, the headers in `jobs.rs`, `bin/lapidary-server/src/main.rs`, `deploy/web/Caddyfile` if needed | 0049 | open | `feat/events-stream` |
| [G5](G5.md) | Dashboard UI | 2 | web | W0 | `web/src/routes/dashboard.tsx`, `components/dashboard/*`, `lib/{dashboard,events}.ts`, the nav in `AppFrame.tsx`, `dashboard` strings block | — | open | `feat/dashboard-ui` |
| [L2](L2.md) | Sharing protocol debt | 3 | Rust | B0, L1 | `lapidary-peer/src/{pull,sync,shares}.rs`, `lapidary-db/src/{mirror,pulls}.rs`, `bin/lapidary-server/tests/peer_*.rs`, the asks-first line on the shared folder's page | 0052 | open | `fix/sharing-protocol-debt` |
| [L3](L3.md) | Mass and materials leftovers | 3 | Rust + web | B0, G2 | `lapidary-api/src/densities.rs`, the part page's mass section, `repo.rs` (wave 3 only), a re-derive path in `lapidary-ingest` | 0051 | open | `fix/mass-and-materials` |

Merge order the lead follows: B0 → W0 → G3 → G2 → G6 → L1 → G4 → G1 → G5 → L2 → L3 → close (W3).

**W3, the close (lead, after L3):** both Phase 6 exits measured on the merged `main` through a compose stack on a lane
port block; the ROADMAP Phase 6 ledger; FEATURES rows for similarity and near-duplicates and §8 ("per browser until
auth"); DATA.md §3.8 (`part_shape`, `part_link`, the profile version rule); ARCHITECTURE's layout and events rows.

**Migration numbers:** 0047–0048 W0 · 0049 G1 · 0050 spare (lead) · 0051 L3 · 0052 L2. Nobody takes the next free
number.

## Backlog — recorded, not scheduled

Each is a candidate goal for a later board; the source is `docs/ROADMAP.md` or a `ponytail:` comment.

- **Ingest and storage races:** the `renameat2(RENAME_NOREPLACE)` fallback (`lapidary-storage/src/lib.rs:216`); a lock
  across the adopting and reaping jobs (`lapidary-ingest/src/handler.rs:1230`); `write_manifest` doing blocking I/O
  while the part row is held; a file whose 12-digit and whole-hash names are both quarantined failing transiently.
- **Bundles:** streaming import through a temporary file (`lapidary-ingest/src/import.rs:261`); ZIP64 past 4 GiB
  (`lapidary-targets/src/bundle.rs:21`); an import's `created_at` and a hobby import's revision count.
- **Performance:** a facet rollup table (`repo.rs:1735`); an expression index per custom number field (`repo.rs:302`);
  one jsonb insert when replacing a catalogue (`mirror.rs:360`); a stale-derivative rebuild trickled rather than
  queued at once (`jobs.rs:377`).
- **Sharing:** choosing several parts to pull at once; part names from the manifest; a controlled destination taking
  a sharer's revisions; a grant or a new share waking the other side's peer role (deferred three times); progress
  moving a file at a time.
- **Web:** lossy WebP at about q85 (`images.rs:184`); per-triangle face ids for measurement snapping
  (`web/src/lib/measure.ts:97`); the 1,000-part UI sweep and GPU frame times; category re-parenting.
- **Other:** the `notify` watcher for `lapidary folder` (`bin/lapidary/src/folder.rs:18`); a cached OCCT layer for the
  worker image; building `api` and `peer` as one image.

## Phase 7 — build graph (outline only)

The roadmap says it is comparable in size to everything before it and **do not start early**. It gets its own plan and
board once Phase 6 is closed.
