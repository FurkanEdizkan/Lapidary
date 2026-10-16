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
| [L2](L2.md) | Sharing protocol debt | 3 | Rust | B0, L1 | `lapidary-peer/src/{pull,sync,shares}.rs`, `lapidary-db/src/{mirror,pulls}.rs`, `bin/lapidary-server/tests/peer_*.rs`, the asks-first line on the shared folder's page | 0052 | merged `bac30be` | `fix/sharing-protocol-debt` |
| [L3](L3.md) | Mass and materials leftovers | 3 | Rust + web | B0, G2 | `lapidary-api/src/densities.rs`, the part page's mass section, `repo.rs` (wave 3 only), a re-derive path in `lapidary-ingest` | 0051 | merged `3866a62` | `fix/mass-and-materials` |
| [L4](L4.md) | A query that gave up stops running | 3 | Rust (small) | G4 | `lapidary-db/src/lib.rs`'s pool (`statement_timeout`, `lock_timeout`) | — | merged `fd5c7dd` | `fix/statement-timeouts` |
| [P3](P3.md) | Tags as places | 3 | web | W0 | `web/src/routes/tags*.tsx`, the tags read, `tags` strings block | — | merged `bd362e4` | `feat/tag-browse` |
| [P1](P1.md) | Creator releases and packs | 4 | Rust (ingest) | G2, L3 | `lapidary-ingest/src/import.rs`, `lapidary-targets/src/bundle.rs`, the release rows | 0053 | open | `feat/release-import` |
| [P2](P2.md) | Browse by creator | 4 | Rust + web | W0, P1 | the creator read and facet, `web/src/routes/creators*.tsx`, `creators` strings block | — | open | `feat/creator-browse` |
| [P4](P4.md) | The explore landing | 5 | web | G3, G4, G5, P2, P3 | `web/src/routes/explore.tsx`, `explore` strings block | — | open | `feat/explore-landing` |
| [R1](R1.md) | The card shows a picture; the viewer loads the original | 4 | web (+ one db read) | — | the page query's card picture, `Card.tsx`, `Viewer.tsx`, the engine half of the frontend | — | merged `c96a5a1` | `feat/picture-first` |
| [R2](R2.md) | OCCT out, STEP and IGES refused by name | 4 | Rust + deploy | — | `sidecar/`, `lapidary-cad/src/occt.rs`, `deploy/Containerfile`, `xtask/src/{verify,deploy}.rs`, `kernel_for` | — | merged `43e7932` | `chore/remove-occt` |
| [R3](R3.md) | Production stops, and similarity goes with it | 4 | Rust | R1, R2 | `DerivativeKind`, the four producers, `shapes.rs`/`likeness.rs` and their api | 0054 | merged `76eec33` | `chore/stop-rendering` |
| [R4](R4.md) | Purge the rendered derivatives | 5 | Rust + ops | R3 | migration 0055, `cache_kinds`, `Storage.tsx` | 0055 | merged `4b9f856` | `chore/purge-renders` |
| [R5](R5.md) | Trim `lapidary-cad` to parsers and measurement | 5 | Rust | R2, R3 | `lapidary-cad/src/*`, the `Kernel` trait, the worker-router gate | — | merged `270309a` | `refactor/mesh-facts-only` |
| [R6](R6.md) | The rig, and the documents | 5 | tooling + lead | R1, R2, R3 | `scripts/e2e/`, `CLAUDE.md`, `PRODUCT.md`, `FEATURES.md`, `DATA.md`, `ARCHITECTURE.md` | — | merged `06b0d01` | `docs/visual-library` |

**Wave 1 is closed** (2026-09-26): G3 `f18e0f0`, G6 `4479e1b`, G2 `5fdd5af`, T1 `9818ad2`, each gated on its merged
tree. Phase 6's second exit is met and measured through containers.

**Wave 2 is closed** (2026-09-27): G4 `36a0ad2`, G1 `8b9f94c`, L1 `8602d52`, G5 `a20b3aa`. **Both Phase 6 exits are
met**, measured on one stack built from `main` — see the ROADMAP.

**Wave 3 is closed** (2026-09-28): L2 `bac30be`, L3 `3866a62`, P3 `bd362e4`, L4 `fd5c7dd`.

**Wave 4 — the pivot** (owner, 2026-10-03). Lapidary becomes a **visual library**: you look through pictures
and search metadata, and the 3D view is something you ask for. The render pipeline, the CAD kernel, the
measurement and assembly machinery and shape similarity all come out — about 10,000 lines and 250 tests. The
plan and the owner's decisions are in the goal files R1–R6; `docs/ROADMAP.md` carries the ledger.

**The ordering trap, read it before claiming R3:** the viewer must stop needing rungs (R1) *before* rung
production stops (R3). A part with no rung is **silently un-viewable** — `Viewer.tsx`'s rung effect returns
early on a null hash, with no error and no spinner.

R1 lane 1 and R2 lane 2 are claimed and disjoint. R3 waits for both. **P1, P2 and P4 stay open**, and R6
re-scopes P1 (it loses its dependency on shape profiles) and P4 (it loses the near-duplicate line). **Only one stack may be up at a time** — the machine has about 4 GiB of RAM free and `/` is at 9.6 GB, so G1 and
L1 ask the lead before `scripts/e2e/stack.sh up`. Phase 6's **first** exit (12 widgets in one round trip) is the lead's
to measure through the rig once G4 and G5 are both merged. Read [`W0.md`](W0.md)'s Record before building
against the contracts — it says where they differ from W0's stage text.

Merge order the lead follows: B0 → W0 → **G3 → G2 → G6** (T1 whenever it is ready — it owns only new files) → G4 → G1
→ G5 → L1 → L2 → L3 → L4 → P3 → P1 → P2 → P4 → close (W3).

**W3, the close (lead, after L3):** both Phase 6 exits measured on the merged `main` through a compose stack on a lane
port block; the ROADMAP Phase 6 ledger; FEATURES rows for similarity and near-duplicates and §8 ("per browser until
auth"); DATA.md §3.8 (`part_shape`, `part_link`, the profile version rule); ARCHITECTURE's layout and events rows.

**Migration numbers:** 0047–0048 W0 · 0049 G1 · 0050 spare (lead) · 0051 L3 · 0052 L2 · 0053 P1 · 0054 R3 · 0055 R4. Nobody takes the next free
number. `0047` restated `job_outcome_known` to add `profiled`: a later migration that restates it again keeps
`profiled` and every value before it.

## Slimming, and the creator library (planned 2026-10-16)

The owner kept Lapidary over Manyfold and PrintStash and narrowed it to **hobbyist models and creator libraries**:
pictures, search, a link to the file, and import from a creator's page. Container first — the app and Postgres; the
installer is a later phase. The research and the decisions are in the plan these goals were cut from; the lead copies
them into `docs/ROADMAP.md` with the first merge.

| Id | Goal | Wave | Kind | Depends on | Owns | Migrations | Status | Branch |
|---|---|---|---|---|---|---|---|---|
| [S1](S1.md) | One process, two containers | 6 | Rust + deploy | — | `bin/lapidary-server/src/main.rs`, `deploy/`, `xtask/src/{deploy,layers}.rs`, `scripts/e2e/stack.sh` | — | merged `958024f` | `chore/one-process` |
| [S2](S2.md) | Empty crates and pgvector | 6 | Rust (small) | — | the three placeholder crates, `deploy/db/` | — | merged `89961ab` | `chore/drop-placeholders` |
| [S3](S3.md) | Peer sharing out | 6 | Rust + web | S1 | `lapidary-peer`, the sharing modules in db/api/web, `repo.rs` (wave 6) | 0056 | merged `188a830` | `chore/remove-sharing` |
| [S4](S4.md) | Governance, locks and the desktop agent out | 6 | Rust + web | S3 | `lapidary-vcs`, `bin/lapidary`, locks/states/approvals in db/api/web | 0057 | merged `c6534a0` | `chore/remove-governance` |
| [F1](F1.md) | Pictures without rendering | 7 | Rust + web | S4 | `lapidary-cad/src/tmf.rs` thumbnail read, `images.rs::normalize`'s new home, `Viewer.tsx` snapshot | 0058 | merged `4fc86b9` | `feat/embedded-thumbnails` |
| [F2](F2.md) | A stable file link | 7 | Rust + web | S4 | `download.rs`, the copy-link action | — | merged `04a6087` | `feat/stable-file-link` |
| [F3](F3.md) | Import from a creator link (Printables, Thingiverse, MyMiniFactory) | 7 | Rust + web | S1, S4 | `fetch.rs`'s new home, the link-import job and route, the paste-a-link field, the settings page for site keys, the triangle cap | 0059 | merged `9de07b8` | `feat/link-import` |

**P1** (creator packs) follows F3 and keeps its own row above; its G2 and L3 dependencies are both merged.
**P2 and P4** wait for the owner. **S1 and S2 are disjoint** and may run at once; everything after is sequential
through `repo.rs` and `lib.rs`'s route list.

**Migration numbers:** 0056 S3 · 0057 S4 · 0058 F1 · 0059 F3.

## The MVP: a hobbyist library, dashboard and search first (planned 2026-10-17)

The owner's first look at the running install. The aim is an MVP whose main goal is **the dashboard and
efficient search** for a hobbyist models library. Decisions: files are read **in place** with an inbox for
uploads and imports; **delete removes from the library and never touches the file**, with no restore;
**PostgreSQL stays** for the MVP; a model's pictures are a **set**.

| Id | Goal | Wave | Kind | Depends on | Owns | Migrations | Status | Branch |
|---|---|---|---|---|---|---|---|---|
| [M1](M1.md) | What the owner saw first: stale card picture, overlapping part page, home link | 8 | web | — | `PartDetail.tsx` picture mutations and layout, `AppFrame.tsx` mark | — | merged `b421948` | `fix/first-look` |
| [M2](M2.md) | A user menu (Profile, Settings) and an MVP header | 8 | web + API | M1 | `AppFrame.tsx` places and menu, a profile route | 0060 | merged `ca87e42` | `feat/user-menu` |
| [M3](M3.md) | Delete is delete: no restore, purge or quarantine | 8 | Rust + web | M1 | `lifecycle.rs`, `removed.tsx`, `repo.rs` delete, the reaper's quarantine | 0061 | merged `fd6886a` | `chore/delete-is-final` |
| [M4](M4.md) | Your folders, read where they are, and an inbox | 9 | Rust + deploy | M3 | `lapidary-ingest` scan/handler, `lapidary-storage` source half, `download.rs`, `moves.rs`, compose mounts | 0062 | open | `feat/index-in-place` |
| [M5](M5.md) | A model's pictures, as a set | 9 | Rust + web | M1 | `link.rs` pictures, `Card.tsx` | — | open | `feat/picture-sets` |

**Migration numbers:** 0060 M2 · 0061 M3 · 0062 M4.

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
- **Five handlers log a database error outside the `internal_error` family** (`tags`, `filters`, `part_number`,
  `moves`'s rename, `blob`'s reachability sweep) and so still log a *cancelled* statement at error rather than warn —
  over-loud, never silent, and documented where it happens (`error.rs`'s `log_db_error`). L4 offered to wire `tags`
  alone; doing one of five would be less consistent than doing none. Wants one pass over all five.
- **A shared library's page has no pictures** (R4, 2026-10-04): `PgShares::catalogue` and `::thumbnail` read
  `kind = 'thumbnail'`, and `0055` deleted every such row. Nothing breaks — the part is listed with
  `thumbnail: false` and the route 404s — but **sharing should mirror `part_image`** for a remote catalogue to be
  worth browsing. New work, not a regression.
- **A blob a picture references can be quarantined by a future caller** (R4, 2026-10-04): `recount` and `reap` count
  `file` and `derivative` only, while `part_image.blake3` also references `blob`. Unreachable today; if it is ever
  reached, the reaper's foreign-key refusal rolls back **the whole hourly sweep, every hour, for ever**. Whoever adds a
  remove-a-picture route or widens `purge`'s doomed set meets it first.
- **Nothing writes a tag but a person and a bundle import** (P3, 2026-09-27): no migration seeds one, ingest reads none
  off a file, the rig writes none — so `/tags` is empty on any library nobody has hand-tagged, and the discovery half
  has nothing to discover until P1 carries tags in with a release. **A product decision for the owner**, and it shapes
  P1: tags from a pack's own structure, from folder names at ingest, or only ever typed.
- **Choosing a tag unmounts the grid** (P3, 2026-09-27): `/` and `/tags/{tag}` are two routes, so a bulk selection in
  progress and a scan the page started are lost when somebody picks a tag from the facet panel. A batch in the URL
  survives. Wants the selection lifted out of the component's local state.
- **A phone-width screenshot taken with `--window-size` is not one** (P3, confirmed by the lead 2026-09-28): headless
  Chrome floors its window at 500 CSS px, so `--window-size=390` renders at 500 and scales the image down — the file is
  390 wide and the page never was. `Emulation.setDeviceMetricsOverride` is the only way, and `scripts/e2e/cdp.mjs`'s
  `session()` already sets it. G5's `dashboard-390.png` was taken the wrong way, so **the dashboard's phone layout is
  unverified** and wants one capture through the rig at the next stack run.
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
