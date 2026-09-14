# Goal: Phase 4 slice 2 and Phase 5 bundles, on this Linux machine

Written 2026-09-15, from `main` at `4aaef44`. **This file is the goal's source of truth.** After any
context summary, re-read it together with ROADMAP's slice 2 record, which is the progress ledger.

## What this session can and cannot finish

Checked on this machine on 2026-09-15:
- `xdg-open` is installed. FreeCAD, Blender and slicers are not.
- Rust has one target, `x86_64-unknown-linux-gnu`.
- Docker is shared with a firmware project whose build container is running. The root disk has
  24 GB free, and that project holds 15 GB of build cache.

That rules out the following.
- **Phase 4's exit.** Opening a STEP in FreeCAD needs FreeCAD installed (sudo or Flatpak) and a real
  OCCT kernel (an image build). Both are stop conditions. Mesh parts on the mock kernel remain the
  path checked end to end.
- **macOS and Windows watchers.** Nothing here can run them.
- **The tiering job.**
  - DATA §1.3 recompresses source blobs.
  - Migration `0013` writes every source file raw into its model directory, so the owner sees a real
    file.
  - The owner decided on 2026-09-15: files stay raw. DATA §1.2 and §1.3 record the retirement, so
    there is nothing to build.
- **Other exclusions:**
  - `lapidary up` pulls images.
  - `lapidary worker` is Phase 8's lease protocol.
  - OpenGraph fetching is out by owner decision (FEATURES §6).
  - Face and edge deltas, and PMI in 3D, need OCCT.
  - Mass needs a density, and no density is stored.
  - Turkish search and custom fields are Phase 5 work unrelated to the round trip. They belong to
    the next goal.

## Facts already checked, so no stage checks them again

- **Older rungs are servable.** `derivative_is_reachable` (`repo.rs`) joins derivative → revision →
  part with no current-revision filter, so an older revision's rungs can already be fetched by hash.
- **Every revision's rungs are rebuilt.** `enqueue_stale_rungs` (`jobs.rs`) covers all revisions,
  not only the current one.
- **`PartRevision` has no tessellation hash** yet (`crates/lapidary-api/src/revisions.rs`).
- **The viewer never moves the mesh.** It frames the camera on the first rung's box (`Viewer.tsx`,
  `show`).
- **`touch_blob` writes `blob.last_accessed_at`** on each deliberate read.
- **Search uses the `simple` config.** `part.search` is a STORED generated column over
  `to_tsvector('simple', …)` (`0022`).
- **The grid already has bulk selection,** kept in the URL (`routes/index.tsx`).
- **Dependencies.**
  - `zip` 2.4.2 is in the workspace, with only the `deflate` feature.
  - `async-zip` and `notify` are named in ARCHITECTURE's dependency table but are not in
    `Cargo.lock`.
- **`part_source.license` exists** (`0015`).
- **The verify gate's `deny` step runs `cargo deny check`.**

## Stages

Each stage gets its own branch, merged before the next stage starts. Every stage is a coherent place
to stop.

Stages 1–5 are the round trip and its storage story, and they are the goal. Bundles, 6 and 7, run
only if 1–5 are merged with the session still going. Otherwise they move to stage 7 of
`2026-09-15-local-product-goal.md`.

ROADMAP's "Open points, 2026-09-15" section maps this goal beside the other two, and holds this
goal's record, which is the progress ledger.

### 0. Preflight (no branch)

- **Test database.** `docker ps` should list `lapidary-test-db`. If it doesn't, start it with the
  command under "How to work".
- **Advisories.** Run `cargo deny check` on `main`.
  - If it fails, the cause is a newly published advisory, not your work.
  - Fix it first on `fix/<crate>-advisory`, gate it, and merge it, the way `40d1bc4` did.

### 1. Spec: `docs/phase-4-slice-2-spec`

- **This file is already committed** (`docs/open-points`, 2026-09-15). Edit it on this branch if the
  spec changes a default.
- **Write the spec.** `docs/superpowers/specs/2026-09-15-phase-4-slice-2-design.md`, in the format of
  slice 1's spec.
  - It settles every decision below, so no later branch has to decide one again.
  - Where this file gives a default, take it.
  - Where it gives none, pick the boring option and list it under a "Decided without the owner"
    heading, so the report can put it in front of the owner.
- **Settle the overlay's coordinates here, before stage 2 is planned.**
  - Does the kernel bridge, the mock kernel or `derive.rs` translate a mesh to its box centre before
    writing a rung?
  - If it does, two revisions of different size are offset from each other. The overlay must then
    undo the translation from each revision's own box, or the stage does not ship.
- **Update DATA.** Put the decisions in §5.3 (bundles), §6 (overlay, `lapidary://`) and §1.5 (render
  cache).

### 2. Overlay diff: `feat/overlay-diff`

**What it adds.**
- `PartRevision` gains the revision's L1 rung hash, falling back to L0.
- The viewer draws an earlier revision as a grey ghost behind the current solid.
- The Compare section switches it on, and its "from" revision is the ghost.

**Defaults.**
- Both meshes use the same rung level.
- Picks and measurements hit only the solid. The ghost can't be picked.
- When the earlier revision has no rung, the page says so in words and draws nothing. An empty ghost
  would read as "nothing changed".
- Both meshes are drawn in the part's own model coordinates, as stage 1 settled. A ghost drawn at
  the wrong offset is a measurement that lies.

**Tests.**
- **API:**
  - the new field;
  - an older revision's rung is served;
  - a hash from another library is still a 404.
- **Web:** the toggle, and the message when there is no rung.

**Browser check** (SwiftShader, native stack).
1. Make two revisions of the flange: one scaled ×1.1 in X, one ×0.9.
2. Sample pixels in the band where only one of the two meshes is drawn, and show that the ghost and the
   solid are visibly different.
3. Save a screenshot to scratch.

### 3. `lapidary://` and launching a tool: `feat/lapidary-url`

**Commands.**
- **`lapidary register`**
  - Writes a `.desktop` handler for `x-scheme-handler/lapidary` under `$XDG_DATA_HOME` (default
    `~/.local/share`).
  - Makes it the default handler with `xdg-mime`, which writes to `$XDG_CONFIG_HOME/mimeapps.list`.
- **`lapidary unregister`** removes exactly what `register` wrote.
- **`lapidary open <uri>`**
  - Checks the part out, or reuses this machine's existing checkout of it.
  - When somebody else holds the part's lock, it refuses with the existing `PartCheckedOut` message
    and launches nothing. List this under "Decided without the owner".
  - Launches `xdg-open <file>` through `std::process::Command` with separate arguments, never through
    a shell.

**The URI is untrusted input.** Any web page can hand one to the handler.
- The only accepted shape is `lapidary://open?part=<uuid>`.
- The following are refused, and nothing is written:
  - any other host;
  - any other key, or a repeated key;
  - a `part` value that isn't a UUID.
- The server is always the agent's own configured server. A URI never names a server, a path, a
  command or a tool.

**Web.**
- An "Open in desktop app" control on the part page, for controlled libraries.
- The honesty line from DATA §6.3: which tools save back, and that Fusion and Onshape don't.
- Download stays beside it as the fallback when no agent is installed.
- Every string goes through `strings.ts`.

**Check.** Never touch the real `~/.local/share/applications` or `mimeapps.list`. Every run uses
throwaway `HOME`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` directories under `target/`.
1. Run `lapidary register`.
2. Register a stand-in editor for `model/stl` as a `.desktop` file. Its `Exec` runs the scale script
   (below) on the file it is given, the way an editor's save would.
3. Start `lapidary agent`, then run `xdg-open 'lapidary://open?part=<flange>'`. Expect the checkout
   folder, then revision 2 from the agent with origin `agent`.
4. Hostile URIs change nothing:
   - an extra `server=` key;
   - `part=../x`;
   - a value that isn't a UUID;
   - another host.
5. If `xdg-open` can't be driven outside a desktop session, record that. Then check `lapidary open`
   directly, and check the content of the `.desktop` file it wrote.

**The `Target` trait.**
- Build it in this stage only if both download and open need format negotiation.
- Otherwise, write in ROADMAP why a single caller doesn't justify a trait.

### 4. Storage view and render cache: `feat/storage-view`

**Already there: extend it, don't rebuild it.**
- `GET /api/storage` exists: `instance_storage` in `crates/lapidary-api/src/parts.rs`, and
  `InstanceStorage` in `crates/lapidary-db/src/repo.rs`.
- It already reports source, derivative and quarantined blob bytes.

**What it adds.**
- Nothing for quarantined model directories: the quarantined figure already includes
  `quarantined_file` (spec §4).
- How much render cache could be freed.
- A panel that shows every figure.
- A "free cache space" action, behind `Dialog`.

**Defaults.**
- **What counts as render cache:** L1 and L2 tessellation rungs of parts whose rungs haven't been read
  in 90 days.
- **What stays:** L0, thumbnails, structure, entities and PMI.
  - The open path still draws the part and asks for a finer rung, as it does today.
  - Structure, entities and PMI must stay in any case: nothing can re-derive them. See the `ponytail:`
    note in `crates/lapidary-ingest/src/derive.rs`.
- **How space comes back:**
  - Removal goes through the existing ref-count recompute and the 30-day quarantine.
  - The wording says the space comes back after quarantine, not immediately.

**Wording is a product rule** (DATA §1.5, and the boundary already written in `web/src/lib/strings.ts`
above the remove and purge strings).
- The action says "free cache space". It never says "delete".
- It shows how much render cache it removes, and says that source files are untouched.
- No number is labelled "freed": quarantined bytes leave 30 days later, not on the day.
- It must never read as remove or purge.

**Tests.**
- The figures, on a fixture.
- Eviction leaves source files and L0 alone.
- A part opened after eviction gets its rung back on request (mock kernel).

### 5. Slice 1's debts: `fix/slice-1-debts`

- **First-revision origin.** A new part's first revision records the route it came by: `upload` for
  bytes from the browser, `ingest` for a scan.
  - `insert_part_chain` takes the origin as a parameter.
  - The agent never creates a part, so it needs no case here.
- **Watcher tests.** The rename and still-writing tests must assert on the change poll itself.
  - Prove it by making `poll` hash at once and watching both tests fail.
- **ROADMAP.** Remove the matching lines from "Recorded rather than fixed", citing this branch's hash.

### 6. Only once 1–5 are merged: bundle export (`feat/bundle-export`)

**What it adds.** A ZIP of the grid's selection.
- It is streamed, never buffered.
- Every entry is stored uncompressed (STORE).

**Layout mirrors the library.**
- `<model>/<name>` holds the current revision.
- `<model>/revisions/<label>/<name>` holds each earlier revision.
- `manifest.json` sits at the root.

**The manifest records:**
- a format version, and the mode of the library the bundle came from;
- for each part: part number, name, tags, materials, and sources with their licences;
- for each revision, oldest first: label, parent label, origin, `created_at`, BLAKE3, size, format,
  and its path inside the ZIP.

**Integrity.**
- Each entry is hashed while it streams, and refused on a mismatch, as `download.rs` does.
- The download filename uses RFC 5987 encoding.

**Dependency.**
- Use `zip` 2.4.2 if its streaming writer can emit STORE entries to a body that can't seek.
- Otherwise add `async-zip`, which ARCHITECTURE names, pinned to an exact version.
- Record which one you chose, and why.

**Tests.**
- A two-revision part's entries are byte-identical to the stored files.
- Every entry is STORE.
- The manifest has the shape above.
- A part from another library is refused.

### 7. Only once 6 is merged: bundle import (`feat/bundle-import`)

**How a bundle arrives.**
- The ZIP comes in through the existing chunked upload.
- Import has its own route and its own job kind, so a bundle is never mistaken for a 3MF.

**Checked before anything is written** (DATA §5.4):
- limits on the number of entries and the total size;
- absolute paths and `..` are refused;
- every entry's BLAKE3 matches the manifest.

**What gets written.**
- **Controlled library:** each part with its whole chain. Labels, parent links, origins and
  `created_at` are kept, and the rows get new ids.
- **Hobby library:**
  - Only the newest revision is imported.
  - The others are counted as `unkept`, and the batch line says so.
- **A part whose `source_path` already exists:**
  - The same current bytes are `skipped`.
  - Anything else is refused, naming the path.
  - Grafting a chain onto an existing part is out of scope.

**Tests.**
- Export, then import into a second controlled library. Labels, parents, origins and hashes all match,
  and downloads are byte-identical.
- An entry with a traversal path writes nothing.
- A hash mismatch writes nothing.

**Check toward Phase 5's exit.**
1. Make 40 parts from the six examples (scaled copies), each with two revisions.
2. Export them, import them into a fresh library, and compare the lineage row by row.
3. Record that this was 40 mesh parts, not a 40-part STEP assembly, which needs OCCT.

## Before teardown

**Review.**
- Have a fresh reader, a subagent or the advisor, review the whole slice's diff (`4aaef44..main`),
  after telling it what shipped.
- Look hardest at state that outlives the thing it belongs to. In slice 1, Compare kept another part's
  revisions, and no test had the shape to catch it.
- Fix anything real with a test, on a short branch.

**Records.** Update after each merge, not at the end.
- **ROADMAP:**
  - a Phase 4 "Slice 2" record;
  - "Early" entries under Phase 5 for bundles;
  - commit hashes, the numbers measured, and what wasn't covered.
- **FEATURES:** the notes for §4 and §5.
- **DATA:** whatever the spec settled.

## How to work

**Worktree.**
- Your own, from local `main`: `git worktree add .claude/worktrees/<name> -b <branch> main`.
- Point `CARGO_TARGET_DIR` at the main checkout's `target/`.
- Run `npm ci` in the worktree's `web/`.
- From inside a worktree, never `cd` to the repo root; use `git -C` instead.
- Stay out of `.claude/worktrees/design-sync-import`.

**TDD.**
- See each test fail before making its change.
- A test written alongside its code gets a mutation check instead, and the report says which tests
  those were.

**Gates, on every branch.**
1. Run `cargo xtask export-bindings`, and commit the bindings.
2. Check `docker ps` for `lapidary-test-db`. If it is missing, start it:
   `docker run -d --rm --name lapidary-test-db -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary -p 55432:5432 docker.io/library/postgres:18`
3. Run the gate in the foreground, logging to `target/`:
   `CARGO_BUILD_JOBS=4 DATABASE_URL=postgres://lapidary:localdev@localhost:55432/lapidary cargo xtask verify slice > target/<branch>-verify.log 2>&1`

**Chain gate, commit and merge with `&&`, never `;`.** In slice 1, a `;` let a failing `deny` gate
through to a merge.

**Commits and merges.**
- Use conventional commits with no AI attribution trailer of any kind. The commit-msg hook rejects them.
- `git merge --no-ff` into local `main`, then delete the branch.
- Never push.

**Native stack for browser checks.**
- Run `lapidary-server --features mock-kernel` twice:
  - `LAPIDARY_ROLE=api` on 127.0.0.1:8080;
  - `LAPIDARY_ROLE=worker` on 127.0.0.1:8081.
- Both use a scratch database created inside `lapidary-test-db`.
- `LAPIDARY_BLOB_ROOT`, `LAPIDARY_UPLOAD_DIR` and `LAPIDARY_INGEST_DIR` all point under `target/`.
- Serve the web app with `npx vite preview`.
- **The scale script.** Keep it in `target/`. It is about 20 lines of Python:
  - read a binary STL;
  - check that its length is `84 + 50 × count`;
  - multiply every vertex's X by the factor;
  - write the result to `<file>.saving`, fsync it, and `os.replace` it over the original, as editors
    save.
- Drive headless `google-chrome` with a throwaway profile over CDP, not the Chrome extension.
- Tear down after each check: stop the processes, drop the database, remove scratch files.

**Measurement must not lie.**
- Mark anything mesh-derived with ≈.
- A missing figure is never shown as zero.
- A missing mesh is said in words.

**Project rules.**
- Errors say what broke and what to do.
- No `unwrap()` outside tests.
- No SQL outside `lapidary-db`.
- The open path never touches a source file or the kernel.

## Stop and ask before

- Any Docker image build or pull, any Docker prune, or anything that uses more than 2 GB of the root
  disk.
- Anything that needs sudo, or installing FreeCAD or any other application.
- Pushing.
- Deleting or overwriting user data, `deploy/.env`, or the real home directory's desktop and MIME
  settings.
- A decision that would contradict a doc. Decisions the docs leave open don't need a stop.

## When done

- Remove your worktrees and merged branches.
- Report, per stage:
  - what shipped, with commit hashes;
  - what was checked, with numbers;
  - which tests failed first, and which were mutation-checked instead;
  - the "Decided without the owner" list.
- Then list what still needs the owner:
  - FreeCAD and OCCT, for Phase 4's exit;
  - a macOS machine and a Windows machine;
  - tiering versus raw source files.
