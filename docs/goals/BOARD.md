# The board

**Only the lead edits this file, on `main`.** Lanes claim a goal with `scripts/claim-goal.sh` (the branch is the
claim) and tell the lead; see [`PROTOCOL.md`](PROTOCOL.md). A goal may be claimed when every goal in its **Depends
on** is `merged`. At most two **Rust** goals are `claimed` at once.

Status: `open` → `claimed (lane n)` → `ready` → `merged <sha>`.

## Phase 6 and the loose ends (planned 2026-09-21)

| Id | Goal | Wave | Kind | Depends on | Owns | Migrations | Status | Branch |
|---|---|---|---|---|---|---|---|---|
| [B0](B0.md) | Several sessions at once | 0 | lead | — | `docs/goals/`, `scripts/`, `xtask/src/lane.rs`, `CLAUDE.md`, `.claude/settings.json` | — | merged `31f282c` | `chore/parallel-sessions` |
| [W0](W0.md) | Phase 6 contracts | 0 | lead, Rust | B0 | `lapidary-core/src/{shape,event,link}.rs`, `lapidary-db/src/shapes.rs`, `repo.rs` purge + `rows_by_id`, `lapidary-api/src/{likeness,dashboard}.rs` (types only), `strings.ts` blocks | 0047, 0048 | merged `322d382` | `feat/phase-6-contracts` |
| [G2](G2.md) | Shape profiles in the worker | 1 | Rust | W0 | `lapidary-cad/src/{shape,glb}.rs`, `JobPayload::ProfileShape`, `lapidary-ingest/src/shape.rs`, dispatch in `handler.rs`/`derive.rs`, `lapidary-db/src/shapes.rs` reads | — | merged `5fdd5af` | `feat/shape-profiles` |
| [G3](G3.md) | Likeness API | 1 | Rust | W0 | `lapidary-db/src/likeness.rs`, `lapidary-api/src/likeness.rs` handlers, `crates/lapidary-api/tests/likeness.rs` | — | merged `f18e0f0` | `feat/likeness-api` |
| [G6](G6.md) | Likeness UI | 1 | web | W0, G3 | `web/src/lib/likeness.ts`, `routes/duplicates.tsx`, `components/Likeness.tsx`, its mount in `PartDetail.tsx`, "folded into" on `removed.tsx`, the look-alike line in `index.tsx`, `likeness` strings block | — | merged `4479e1b` | `feat/likeness-ui` |
| [T1](T1.md) | The whole app, up and driven | 1 | tooling (Docker allowed) | B0 | `scripts/e2e/**` | — | merged `9818ad2` | `test/e2e-rig` |
| [L1](L1.md) | Sharing screens, seen and finished | 2 | web (+ one db read) | B0, T1 | `routes/sharing*.tsx`, `components/ShareDialog.tsx`, `sharing` strings block, `PgShares::requests` | — | merged `8602d52` | `fix/sharing-screens` |
| [G4](G4.md) | Dashboard resolve | 2 | Rust | W0, G3 | `lapidary-api/src/dashboard.rs` handler, the grid helper in `parts.rs`, `lapidary-db/src/dashboard.rs` | — | merged `36a0ad2` | `feat/dashboard-resolve` |
| [G1](G1.md) | App-wide event stream | 2 | Rust | W0 | `lapidary-db/src/events.rs`, `lapidary-api/src/events.rs`, the headers in `jobs.rs`, `bin/lapidary-server/src/main.rs`, `deploy/web/Caddyfile` if needed | 0049 | merged `8b9f94c` | `feat/events-stream` |
| [G5](G5.md) | Dashboard UI | 2 | web | W0 | `web/src/routes/dashboard.tsx`, `components/dashboard/*`, `lib/{dashboard,events}.ts`, the nav in `AppFrame.tsx`, `dashboard` strings block | — | merged `a20b3aa` | `feat/dashboard-ui` |
| [L2](L2.md) | Sharing protocol debt | 3 | Rust | B0, L1 | `lapidary-peer/src/{pull,sync,shares}.rs`, `lapidary-db/src/{mirror,pulls}.rs`, `bin/lapidary-server/tests/peer_*.rs`, the asks-first line on the shared folder's page | 0052 | claimed (lane 1) | `fix/sharing-protocol-debt` |
| [L3](L3.md) | Mass and materials leftovers | 3 | Rust + web | B0, G2 | `lapidary-api/src/densities.rs`, the part page's mass section, `repo.rs` (wave 3 only), a re-derive path in `lapidary-ingest` | 0051 | claimed (lane 2) | `fix/mass-and-materials` |
| [L4](L4.md) | A query that gave up stops running | 3 | Rust (small) | G4 | `lapidary-db/src/lib.rs`'s pool (`statement_timeout`, `lock_timeout`) | — | open | `fix/statement-timeouts` |
| [P3](P3.md) | Tags as places | 3 | web | W0 | `web/src/routes/tags*.tsx`, the tags read, `tags` strings block | — | claimed (lane 3) | `feat/tag-browse` |
| [P1](P1.md) | Creator releases and packs | 4 | Rust (ingest) | G2, L3 | `lapidary-ingest/src/import.rs`, `lapidary-targets/src/bundle.rs`, the release rows | 0053 | open | `feat/release-import` |
| [P2](P2.md) | Browse by creator | 4 | Rust + web | W0, P1 | the creator read and facet, `web/src/routes/creators*.tsx`, `creators` strings block | — | open | `feat/creator-browse` |
| [P4](P4.md) | The explore landing | 5 | web | G3, G4, G5, P2, P3 | `web/src/routes/explore.tsx`, `explore` strings block | — | open | `feat/explore-landing` |

**Wave 1 is closed** (2026-09-26): G3 `f18e0f0`, G6 `4479e1b`, G2 `5fdd5af`, T1 `9818ad2`, each gated on its merged
tree. Phase 6's second exit is met and measured through containers.

**Wave 2 is closed** (2026-09-27): G4 `36a0ad2`, G1 `8b9f94c`, L1 `8602d52`, G5 `a20b3aa`. **Both Phase 6 exits are
met**, measured on one stack built from `main` — see the ROADMAP.

**Wave 3 is claimed** (2026-09-27): L2 lane 1, L3 lane 2, P3 lane 3. **L4 waits for a Rust lane to free up**, because
the protocol allows two Rust goals claimed at once and L2 and L3 are both Rust. **Only one stack may be up at a time** — the machine has about 4 GiB of RAM free and `/` is at 9.6 GB, so G1 and
L1 ask the lead before `scripts/e2e/stack.sh up`. Phase 6's **first** exit (12 widgets in one round trip) is the lead's
to measure through the rig once G4 and G5 are both merged. Read [`W0.md`](W0.md)'s Record before building
against the contracts — it says where they differ from W0's stage text.

Merge order the lead follows: B0 → W0 → **G3 → G2 → G6** (T1 whenever it is ready — it owns only new files) → G4 → G1
→ G5 → L1 → L2 → L3 → L4 → P3 → P1 → P2 → P4 → close (W3).

**W3, the close (lead, after L3):** both Phase 6 exits measured on the merged `main` through a compose stack on a lane
port block; the ROADMAP Phase 6 ledger; FEATURES rows for similarity and near-duplicates and §8 ("per browser until
auth"); DATA.md §3.8 (`part_shape`, `part_link`, the profile version rule); ARCHITECTURE's layout and events rows.

**Migration numbers:** 0047–0048 W0 · 0049 G1 · 0050 spare (lead) · 0051 L3 · 0052 L2 · 0053 P1. Nobody takes the next free
number. `0047` restated `job_outcome_known` to add `profiled`: a later migration that restates it again keeps
`profiled` and every value before it.

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
- **From T1's two clean runs (2026-09-26), one line each.** `Permission denied (os error 13)` names the file and the
  errno but not what to do, which is half the error rule. `scripts/e2e/check.sh` is not wired into any gate: its **83**
  assertions over three shell files and two `.mjs` files ran by hand on every commit, and wiring it in is **not** a one-liner, because `docker compose config` is
  among them and CI has no docker — it needs a CI-safe subset first. `stack.sh up` takes the compile lock and compiles
  nothing, so it queued behind four `cargo test --workspace` runs, each wait longer than the 7 s of work. The
  `/duplicates` queue uses the left ~800 px of a 1440 viewport while wrapping long path headings onto three lines inside
  that column. A 1,000-part sweep is `CORPUS_SLICE=1000` away, and the backlog's UI-sweep row is now cheap.
- **A permanently-failing file is re-attempted by every scan** (the close's run, 2026-09-27): one corrupt STL in the
  corpus produced six `failed` job rows across three scans, because nothing records that this path at this hash has
  already been refused for good. The dashboard's "N failed" therefore climbs with every scan of a library that holds one.
  A `skipped`-like outcome keyed on (path, hash) would settle it; a corrected re-export must still be picked up.
- **From L1's three-installation run (2026-09-27), one line each.** `folders_in_common` never counts the owner of a
  folder this installation holds (`lapidary-db/src/sharing.rs`, `peer_columns!`), so every sharer reads zero — the page
  no longer shows the contradiction, the number is still wrong. A peer introduced but **not yet accepted by the other
  side reports a rejection** ("it has not added this installation's device id") for what is a normal transient step. A
  peer introduced by name shows "No name given yet" until a hello succeeds. "In your library" is dead text where sibling
  cards have a button — it wants a part id on `MirroredPart`. `/sharing` is `max-w-3xl`, so the right 45% of a 1440
  viewport is empty, the same shape filed for `/duplicates`. **WCAG 2.5.3 on this page's older controls:** `removeLabel`
  is "Stop sharing with Ayşe" on a button reading **Remove**, `grantLabel` is "Let Burak pull Terrain" on one reading
  **Let them pull** — one string each.
- **A decision cannot be taken back from any screen (G6, 2026-09-26).** `DELETE /api/parts/{id}/links/{other}` undoes
  a `variant` or `distinct`, and nothing offers it: the queue lists undecided pairs only, so a pair leaves it and never
  reappears. Wants either a "decided about" list per library or an undo on the row before it disappears. Also from the
  same goal: no paging on the duplicates queue (the wire carries no cursor), and no bulk fold — a group of eleven copies
  is ten presses, and `bulk.ts` is the shape to follow.
- **Accessibility, found by T1's rig (2026-09-26):** `ScanProgress`'s half is **done** — G6 gave the progress line
  `role="status"` and its unknown-batch sentence `role="alert"`. What remains: several regions label themselves with
  `useId()` for `aria-labelledby` — saved filters, the quick-look pane, every `Dialog` title, `FirstRun` — so they are reachable only
  by heading text, which every future harness pays for.
- **Web:** lossy WebP at about q85 (`images.rs:184`); per-triangle face ids for measurement snapping
  (`web/src/lib/measure.ts:97`); the 1,000-part UI sweep and GPU frame times; category re-parenting.
- **Other:** the `notify` watcher for `lapidary folder` (`bin/lapidary/src/folder.rs:18`); a cached OCCT layer for the
  worker image; building `api` and `peer` as one image.

## Phase 7 — build graph (outline only)

The roadmap says it is comparable in size to everything before it and **do not start early**. It gets its own plan and
board once Phase 6 is closed.
