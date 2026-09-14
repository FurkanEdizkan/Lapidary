# Goal: finish the local product: custom fields, Turkish search, saved filters, a capped cut, watched folders

Written 2026-09-15. **This file is the goal's source of truth.** After any context summary, re-read it
together with ROADMAP's "Open points, 2026-09-15" section and this goal's record there, which is the
progress ledger.

Run it last of the three, after `2026-09-15-phase-4-slice-2-goal.md` and
`2026-09-15-correctness-debt-goal.md`.

## Why

These are the Phase 4 and Phase 5 features still missing that this Linux machine can build and check,
with the mock kernel and no image build. Phase 5's exit is not reachable here:
- Its URL clause needs OpenGraph fetching, which is out by owner decision. That contradiction is
  recorded in ROADMAP for the owner.
- A 40-part STEP assembly needs OCCT.

## Facts already checked

- **Custom fields.**
  - `part.metadata_json` is `jsonb NOT NULL DEFAULT '{}'` (`0002`). The `cad` key inside it is taken
    (`0021` reads `metadata_json->'cad'->'materials'`).
  - No `custom_field` table exists anywhere.
- **Search.**
  - `part.search` is a STORED generated column over `to_tsvector('simple', …)`, including tags and
    materials through `lapidary_words` (`0022`).
  - `postgres:18` ships the `turkish` config (ROADMAP "Open items", checked 2026-09-14).
  - Libraries are created by `create_library` (`crates/lapidary-api/src/derive.rs` and
    `crates/lapidary-db/src/repo.rs`), which takes a name and a mode.
- **Saved filters.**
  - `saved_filter` (`0023`) and `crates/lapidary-api/src/filters.rs`, routed at
    `/api/libraries/{id}/filters`.
  - The API keeps only the keys the grid's URL carries, and refuses any other.
  - There is no rename and no order.
  - A filter on a deleted category opens an empty grid (ROADMAP, Phase 5, saved filters).
- **Section cut.**
  - `sectionPlane` in `web/src/lib/viewer-math.ts`, and `SectionBar`.
  - The cut is open. A cap needs the stencil buffer.
  - The first cut compiles a program, and nobody has timed it (ROADMAP, Phase 5, section plane).
- **The agent.**
  - `bin/lapidary/src/watch.rs` is a pure poll → settle → hash state machine.
  - `bin/lapidary/src/main.rs` already has the probe → 8 MiB chunks → commit upload, and follows the
    batch.
- **The stretch corpus.** The STL test corpus at `/mnt/Storage2/All/STL Files` holds 1,703 loose meshes
  at depths 2–9. Read it only; never write to it.

## Stages

Each stage gets its own branch, merged before the next starts. Every stage is a coherent place to stop.

### 0. Preflight (no branch)

The same as the slice 2 goal's stage 0.

### 1. Spec: `docs/local-product-spec`

- **The spec itself.** `docs/superpowers/specs/<date>-local-product-design.md`, in slice 1's spec
  format.
  - It settles every stage below.
  - It takes this file's defaults.
  - It lists anything else under "Decided without the owner".
- **DATA.** Record the decisions in §3.3 (Turkish), §3.5 (custom fields), §3.6 (saved filters) and §6.2
  (watched folders).

### 2. Custom fields: `feat/custom-fields`

**Schema.**
- `custom_field(id, library_id, key, label, type, options_json, indexed)`, following DATA §3.5.
- Values live under `part.metadata_json->'custom'->'<key>'`, beside `cad` and never mixed into it.

**Defaults.**
- **Types:** `text`, `number`, and `choice`, whose options are in `options_json`.
- **Keys:** a slug, `[a-z0-9_]{1,40}`, unique per library, and never renamed. The label can change.
- **Indexing amends DATA §3.5, on the owner's behalf.**
  - One GIN index over `(metadata_json->'custom') jsonb_path_ops`, with filters written as `@>`.
  - This replaces one expression index per field, because that would need DDL built from a key a user
    typed.
  - `indexed` then means "offered as a grid filter", still capped at 8 per library.
  - Record the amendment, with that reason, in DATA §3.5.
- **Removing a field** removes its definition only. Values stay in `metadata_json`, since user data is
  never deleted implicitly. The spec says whether they show as orphaned.

**UI.**
- Define fields in the library's settings.
- Edit a part's values on the part page, where the part can be edited.
- Filter the grid by an indexed field. The field travels in the URL, and saved filters accept it.
- Every string goes through `strings.ts`.

**Tests.**
- The cap of 8.
- A duplicate key is refused.
- The value type is validated: a number field refuses "twelve".
- The `@>` filter.
- A removed field leaves its values untouched.

**Browser check.**
1. On the six example parts, define "Supplier" (choice) and "Stock count" (number).
2. Set them on two parts.
3. Filter the grid by Supplier, save the filter, and reopen it.

### 3. Turkish search: `feat/turkish-search`

**Defaults.**
- `library.language text not null default 'simple'`, checked to `simple` or `turkish`, and chosen at
  creation.
- **Out of scope:** changing a library's language later.
- `part.search_config regconfig`, copied from the library when the part is inserted.
- The generated STORED column becomes `to_tsvector(part.search_config, …)`.
  - `to_tsvector(regconfig, text)` is immutable, so a generated column may use it. That makes the
    column boring rather than a trigger.
  - Confirm this on `postgres:18` before building on it.
- **Queries** build their `tsquery` with the library's own config.
- **Trigram search** is unchanged.

**Tests.**
- In a `turkish` library, a part named with ğ, ş and ı is found by an inflected form of its name. For
  example, "bağlantı" finds "bağlantılar".
- A `simple` library returns exactly what it does today.
- A moved part keeps its config. Moves stay within a library, so there is nothing to recompute.

**UI.** A language choice in the create-library dialog.

### 4. Saved filters, finished: `feat/saved-filter-followups`

- **Rename:** `PATCH`. The name stays unique within the library.
- **Reorder:** a `position` column and a move up/down control. Drag is not needed.
- **A filter whose category was deleted:**
  - The list marks it.
  - Opening it says the category is gone, and offers the same filter without the category.
  - It never opens an empty grid silently.
- **Tests** for each of the three, and a browser check of all three.

### 5. A capped section cut: `feat/section-cap`

**Defaults.**
- **Visibility.** The cap is drawn where the cut crosses solid material, using the stencil buffer. It
  is visibly distinct from the part's surface.
- **Picking.** The cap is not pickable, so wall thickness and picks behave exactly as today.
- **Reduced motion** is not relevant: nothing animates.
- **Not closed solids.** A mesh that is not watertight has no inside, so no cap can be honest.
  - The bar says so, reading `isWatertight`, which the part detail already carries.
  - It never draws a guessed cap.

**Check** (SwiftShader, native stack).
1. The flange, cut at Z = 8 mm.
2. Sample pixels inside the cut section: cap colour. Sample through the bore: not cap colour.
3. Time the first cut's program compile with CDP. Record it, and that turning the cut off and on again
   costs no second compile.

### 6. Watched-folder ingest through the agent: `feat/agent-watch-folder`

**The command.** `lapidary watch <folder> --library <id>`.

**Defaults.**
- **Detection.**
  - Every file under the folder is polled by directory listing and `stat`, every 2 s.
  - Settle and hash as `watch.rs` does, with BLAKE3 before believing a change.
  - DATA §6.2's ignore list applies in full, because a whole folder is watched here.
- **Uploads.**
  - A new or changed file uploads through the existing probe, chunks and commit, with its path
    relative to the watched folder as its source path.
  - Origin is `upload`, since no lock is involved.
  - The server decides the outcome, as for any upload: ingested, skipped, revised or unkept.
- **Deletions.** A file deleted locally does nothing on the server. User data is never deleted
  implicitly. The agent prints that it noticed.
- **State.**
  - `$XDG_STATE_HOME/lapidary/watch-<library>.json` holds the known hashes.
  - Nothing is written inside the watched folder.
- **Ceiling, marked `ponytail:`.** Polling a tree costs one `stat` per file per interval. Measure a
  poll over the STL corpus's tree, listing only, and record the ceiling. Move to `notify` (inotify),
  which ARCHITECTURE names, when that cost matters.

**Tests** (unit): the ignore list, relative source paths, and a deletion that sends nothing.

**Check** (native stack).
1. Watch a scratch copy of `example/parts` in `target/`.
2. Add a file: it is ingested.
3. Change a file in a controlled library: a revision.
4. Write a `.tmp` file: nothing.
5. Delete a file: nothing on the server.

### 7. Bundles, only if slice 2 did not ship them

Slice 2's stages 6 and 7, unchanged, from `2026-09-15-phase-4-slice-2-goal.md`.

## Before teardown

- A fresh reader reviews this goal's diff: a subagent, or the advisor.
- Look hardest at state that outlives the thing it belongs to.
- Update the ROADMAP record, FEATURES notes and DATA after each merge.

## How to work, stop conditions, when done

Exactly as `2026-09-15-phase-4-slice-2-goal.md` states in "How to work", "Stop and ask before" and
"When done". The essentials:
- **Worktree:** your own, from local `main`.
- **TDD:** see each test fail first.
- **Gates:** `cargo xtask export-bindings`, then
  `CARGO_BUILD_JOBS=4 DATABASE_URL=postgres://lapidary:localdev@localhost:55432/lapidary cargo xtask verify slice`
  in the foreground, logging to `target/`.
- **Chain gate, commit and merge with `&&`.**
- **Commits:** conventional, with no AI attribution trailer.
- **Merges:** `git merge --no-ff` into local `main`. Never push.
- **Browser checks:** headless Chrome with a throwaway profile.
- **Stop and ask before:**
  - any Docker image build, pull or prune;
  - anything that needs sudo;
  - pushing;
  - touching user data, `deploy/.env` or the STL corpus;
  - a decision that contradicts a doc.
