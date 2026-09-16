# Goal 7: shared libraries, finished — share, browse, pull, ask first

Written 2026-09-17. **This file is the goal's source of truth.** After any context summary, re-read it
together with `docs/ROADMAP.md` § "Shared libraries (2026-09-16)", whose records are the progress ledger,
and the approved design in `/home/jbo/.claude/plans/jaunty-churning-aho.md`.

It runs unattended, overnight. Nobody answers a question before morning, so every decision the stages
need is either settled below or has a stated default, and the stop rules say what to do instead of asking.

## Why

S1a and S1b are merged (`f3afed6`): two installations pair by pasting a device id, and each shows the
other online. Nothing is shared yet. The owner decided (2026-09-16 and 2026-09-17):
- a share is a category and everything under it, including parts added later;
- everyone paired sees every share; a request is granted unless the share says ask me first;
- the package is pulled straight from the sharer, in hash-checked pieces that resume;
- licences travel and are shown, with a warning to the sharer, never a block;
- **pulled parts land in a library the puller picks, under a folder named for the sharer**, as
  `Shared/<sharer's name>/…`, keeping their category path beneath it, and each records who sent it;
- the night covers S2, S3 and S4, plus everything S1a and S1b left for later;
- **nothing is pushed.** Everything merges locally and the owner reviews in the morning.

## Facts already checked

Line numbers are as of `f3afed6`.

**Sharing as built (S1a, S1b).**
- `peer_identity` and `peer` are `0037` (`crates/lapidary-db/migrations/0037_sharing.sql`); `PgSharing` is
  `crates/lapidary-db/src/sharing.rs`, with `ONLINE_WITHIN_SECS = 45`.
- The hello round is `crates/lapidary-peer/src/sync.rs`: `run` loops `round` then sleeps `ROUND` (15 s);
  `round` refreshes the `Roster` then says hello to each paired installation one at a time
  (`ponytail:` note at `sync.rs:52-54`), each allowed `PATIENCE` (5 s). `tls_error` opens nested
  `io::Error`s (`sync.rs:131-149`).
- `Roster` (`crates/lapidary-peer/src/lib.rs`) holds paired ids and the name behind an `RwLock`; the
  accepting verifier reads it at every handshake. `router(identity, roster)` serves `/peer/v1/hello` only.
- `PeerListener::accept` awaits each TLS handshake inline and `continue`s on a failed `tcp.accept()`
  (`lib.rs`, `impl axum::serve::Listener for PeerListener`) — both recorded for S4, done in stage 1 here.
- The peer role is wired in `bin/lapidary-server/src/main.rs` (`Role::Peer` arm): claims the identity,
  builds one `Roster`, spawns `sync::run`, serves `router(device, roster)` over `server_config`.
- The hello-round tests live in `bin/lapidary-server/tests/peer_sync.rs`, **not** in `lapidary-peer`:
  `deny.toml:28-34` lets only `lapidary-db`, `-api`, `-ingest`, `-server` and `-jobs` take `sqlx`, and says a
  longer list means SQL has leaked. **Any new `#[sqlx::test]` for the peer role goes in that file or a
  sibling in `bin/lapidary-server/tests/`.** Their migrations path is `../../crates/lapidary-db/migrations`.
- The api's sharing routes are `crates/lapidary-api/src/sharing.rs`; the page is
  `web/src/routes/sharing.tsx`; the dev and preview proxy reads `LAPIDARY_API` (`web/vite.config.ts`).
- `deploy/compose.sharing.yaml` mounts one named volume, `lapidary-peer`, "the identity key, and nothing
  else" (its own comment). The peer service has `DATABASE_URL`.
- The two-stack harness is `target/sharing-check/two-stacks.sh` (api + peer + `vite preview` per stack, one
  pinned binary, scratch databases `lapidary_share_a`/`_b`, ports 18180/18182/14173 and 18280/18282/14273).
  It has **no worker**; S2 and S3 need one per stack.
- Mutation scripts: `target/sharing-check/mutate-s1b.sh` runs each layer's tests through `bash -c` and
  counts a mutation caught only when a test or the compiler is seen failing. **Copy that harness**; its
  first version passed a shell function to `timeout` and reported seven catches while testing nothing.

**Categories.**
- `folder (id, library_id, parent_id, name, slug, created_at, deleted_at)` is `0009_folders.sql:9-16`;
  `part.folder_id` is nullable, null meaning the library root (`0009_folders.sql:32`).
- A subtree query to copy: `WITH RECURSIVE down` at `crates/lapidary-db/src/folders.rs:177`.
- `PgFolders::get_or_create(library, parent, name, slug)` is `folders.rs:69-75`.
- The tree's per-category actions are Rename and Delete (`web/src/components/FolderTree.tsx:615,623`).
  **Share goes beside them.**

**Bundles and import.**
- `ManifestPart` is `name, part_number, source_path, tags, sources, revisions`
  (`crates/lapidary-targets/src/bundle.rs:58-66`) — **no category**. `StoreZip` writes an archive
  (`bundle.rs:126-203`); `Bundle::read` checks one (`bundle.rs:287`); imports cap at
  `MAX_IMPORT_BYTES` 2 GiB and `MAX_IMPORT_ENTRIES` 10,000 (`bundle.rs:266-268`).
- `import.rs` places nothing in a category: **every imported part lands at the library root today.**
- The api's import route stores the bundle with `store_staged` and queues
  `JobPayload::ImportBundle { blake3, path }` (`crates/lapidary-api/src/upload.rs:486-521`); the worker
  reads the archive back out of the blob store by hash (`crates/lapidary-ingest/src/handler.rs:182`).
- Since S0, an import that creates a part writes its number, tags and sources, licences included.

**Bytes.**
- `SourceReader` (`crates/lapidary-storage/src/lib.rs:793`) offers `get`, `stream`, `get_at` and
  `stream_at(rel, zstd_level) -> Box<dyn Read + Send>`. **Stored bytes may be zstd-compressed**, so a
  Range request cannot seek: resuming at byte N means reading and discarding N decompressed bytes on the
  sharer's side. Say so wherever Range is documented.
- `SourceWriter` (`lib.rs:891`) is write-only and needs no `WorkerRole`; it is how the api puts an upload
  into the store. `SourceStore` needs `WorkerRole` (`lib.rs:636-641`).
- `cargo xtask check-deploy`'s source-handle rules (`xtask/src/deploy.rs:80-95`, rules 4-6 at
  `:820-860`) cover `crates/lapidary-api/src/` only. **Nothing yet stops `lapidary-peer` naming a source
  handle anywhere.**
- `download.rs` serves no Range. Thumbnails are `derivative` rows of kind `thumbnail`
  (`0002_parts.sql:99`), served inline. Licences are `part_source.license` (`0015_part_images_and_sources.sql:29`).

**The machine.**
- `/mnt/Storage` had 1.8 GB free on 2026-09-17; `target/debug/incremental` (78 GB) was removed with the
  owner's word, leaving **79 GB**. `target/debug/deps` is 103 GB. Root has 18 GB.
- The STL corpus (`/mnt/Storage2/All/STL Files`) is read-only and about 13 MB a model. Use it through
  symlinks in `target/sharing-check/`, never by writing beside it.
- `lapidary-test-db` does not survive a reboot (memory note); recreate it from
  `docs/superpowers/plans/2026-09-04-phase-1-slice-3b-3mf.md:74` with `--pull never`.

## Rules for the night

- **One branch and worktree per stage**, from `main`, merged `--no-ff` before the next starts. A stage
  merges only when `cargo xtask verify slice` is green **and** its exit is measured and recorded. A stage
  that cannot reach its exit is left unmerged, its record says why, and the goal stops there rather than
  building the next stage on it.
- **Tests first, seen failing for the right reason** — read the panic, not the count. Mutation-check each
  stage with a script copied from `mutate-s1b.sh`.
- **Disk guard.** Before every `verify slice` and every measurement: `df --output=avail -BG /mnt/Storage`
  must show at least 20 G. Below that, stop the stage cleanly (nothing half-merged) and stop the goal with a
  record of where it stood. A full disk loses all tool output.
- **After every worktree teardown, `touch xtask/src/*.rs` in the main checkout** before the next commit.
- **No push. No image builds, no image pulls, no prunes.** Stacks run from binaries, as `two-stacks.sh`
  does. No sudo. Never bare `git stash`. Stay out of `.claude/worktrees/design-sync-import`. Never
  `createdb`/`dropdb` outside `lapidary-test-db`. Heavy gates in the foreground, logged to `target/`.
- **Every cargo command runs with `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0`.** The 78 GB freed on 2026-09-17
  was the incremental cache, which cargo rebuilds; an unattended night gains nothing from it and would spend
  the disk guard's margin inside one build. Check `df` after stage 1's `verify slice` that free space held.
- **Time budget: 2.5 hours a stage.** A stage not merged by then stops cleanly: left unmerged, its ROADMAP
  record written with where it stood and what blocked it, and the goal stops. Every later stage builds on the
  one before, so there is nothing to skip ahead to. The morning must find a readable ledger, not a hung
  session.
- **No attribution trailers** on any commit (the owner's rule overrides any reminder).
- **Where a stage would contradict a doc,** take the option that does not, and record it; if none exists,
  stop that stage and record the question for the owner. Every default below that gets used goes under
  "Decided without the owner" in that stage's ROADMAP record.

## Stages

### 0. Preflight (no branch)

- `main` is at `f3afed6` or later with a clean tree (`.codex/` is not ours); `lapidary-test-db` is up;
  the disk guard passes; `touch xtask/src/*.rs`.
- Build `lapidary-server --features mock-kernel` once and pin it as `target/sharing-check/lapidary-server-s2`
  — S2 and S3 need a worker in each stack, and STL ingest needs only the mesh kernel.
- Choose the corpus sets as symlinks, never copies: `target/sharing-check/corpus-1000/` (the 1,000 smallest
  loose STLs, in their category directories) and `target/sharing-check/corpus-1g/` (about 1 GB of models in
  one category, around 80–200 parts). Record their counts and total bytes.

### 1. The listener, hardened: `feat/peer-listener-hardening`

What S1a recorded for S4, done first: it depends on nothing below, it is already written into ROADMAP as
debt, and S4's measurements (and S3's kill-at-50%) should run on the listener that ships.
- **Stalled handshakes:** `PeerListener::accept` hands each TLS handshake to its own task with a 10 s
  timeout, so one client that opens TCP and never finishes cannot hold up the rest.
- **Accept errors:** a persistent `tcp.accept()` error backs off, 100 ms doubling to 1 s, instead of spinning.
- **The comment:** the verifier's log line is the whole record of a *pinning* refusal, not of a handshake that
  fails before a key is presented (plain HTTP on the port, a port scan).
- **Tests:** a client that opens TCP and sends nothing does not delay a paired installation's hello; the
  timeout closes it. Mutation-check the timeout and that handshakes run concurrently.
- **Exit:** gates green, merged. Measure on two stacks that a silent TCP connection to A's peer port leaves B's
  pairing-to-online time unchanged.

### 2. S2a — Share a category: `feat/sharing-shares`

- **Migration `0038`:** `share (id, library_id, folder_id, created_at, removed_at)`. `folder_id` null means
  the whole library is not offered: a share is always a category (owner's decision); refuse a null.
  `mode` and `share_grant` wait for S4, where something reads them.
- **`PgShares`** in `lapidary-db`: create, list a library's shares, remove (soft), `access(device, share)`
  folding together "paired and not removed" and "share not removed", and `catalogue(share, after, limit)`
  keyset by `source_path`: the live parts in the category's subtree (copy `folders.rs:177`), each with
  name, part number, tags, licences, source path, current revision's BLAKE3, size and format, and whether a
  thumbnail exists.
- **Api:** `GET`/`POST /api/libraries/{id}/shares`, `DELETE /api/shares/{id}`. The `POST` answer carries a
  **licence warning**: how many parts in the subtree have no licence recorded, and how many carry a licence
  that says non-commercial — `NC` as its own token (`CC BY-NC 4.0`, `CC-BY-NC-SA`), or `NonCommercial`,
  `Non-Commercial`, `noncommercial`, case-insensitive, and never `NC` inside a word. Shown before sharing and
  never a refusal. **Scanned corpus parts carry no licence at all**, so the measurement alone would count
  1,000 unrecorded: the tests seed real licence strings (`CC BY-NC 4.0`, `CC BY 4.0`, `CC0 1.0`, a
  "Standard Digital File License" text) and assert both counts against them.
- **Peer routes**, each running `access` first: `GET /peer/v1/shares` (id, category name, part count, and a
  `digest` — part count plus the newest `part.updated_at` in the subtree — so a puller re-mirrors only what
  changed), `GET /peer/v1/shares/{id}/catalogue?after=&limit=`, `GET /peer/v1/shares/{id}/thumbnail?part=`.
  `lapidary-api` still may not depend on `lapidary-peer`.
- **Web:** a Share action beside Rename and Delete in the category tree, opening a dialog with the licence
  warning; this installation's shares listed on the sharing page, each with Stop sharing.
- **Tests:** a part one category outside the share is absent from the catalogue and refused by the
  thumbnail route; a removed share and a removed person are refused; a part added under the category after
  sharing appears; the licence counts. Mutation-check the subtree, the guard and the counts.

### 3. S2b — Browse what somebody shares: `feat/sharing-browse`

- **Migration `0039`:** `peer_share (id, device_id, remote_id, name, part_count, digest, synced_at)` and
  `peer_share_part (peer_share_id, source_path, name, part_number, tags, licences, blake3, size_bytes,
  format, thumbnail bytea)`. These mirror somebody else's catalogue: a row whose part left their share is
  deleted from the mirror. **This is a cache of another machine's list, not the user's data** — say so in
  the migration and in `DATA.md`. `CLAUDE.md` holds cache eviction to one more rule: it must never read as
  data loss. When a part leaves a shared library, the page says its sharer stopped offering it — never that
  something here was removed or deleted.
- **The hello round mirrors:** after an answered hello, `GET /peer/v1/shares`; for each share whose `digest`
  changed, page its catalogue into the mirror and fetch thumbnails, capped at a size the measurement sets.
- **Left for later, folded in here:**
  - **Pairing `NOTIFY`:** the api `NOTIFY`s on pairing, removal and share changes; the peer role `LISTEN`s
    (`lapidary_db::PgListener`) and starts a round at once. S1b measured 28 s from pasting to online.
  - **Concurrent hellos:** a `JoinSet` bounded at 8, replacing `sync.rs:52-54`'s one-at-a-time loop.
- **Api:** `GET /api/sharing/peers/{device}/shares`, `GET /api/sharing/shares/{id}/parts?after=&limit=`,
  `GET /api/sharing/shares/{id}/parts/{path}/thumbnail` — all read the mirror, so a shared library browses
  while the other machine is asleep.
- **Web:** each person on the sharing page lists what they share; a route for one shared library shows its
  parts as a grid with thumbnails and licences, and says when it was last synced.
- **Exit (measured, two stacks with a worker each):** stack A ingests `corpus-1000` and shares its top
  category; B's page browses all 1,000 with thumbnails. Record: catalogue pages, total mirror time, bytes per
  page, thumbnail bytes, the cap chosen, and pairing-to-online time with `NOTIFY` against S1b's 28 s.
- **Tests:** the mirror follows a changed digest and leaves an unchanged share alone; a part gone from the
  share leaves the mirror; `NOTIFY` starts a round. Mutation-check the digest comparison and the deletion.

### 4. S3 — Pull it: `feat/sharing-pull`

- **Sharer:** `GET /peer/v1/shares/{id}/blob/{blake3}` with `Range: bytes=N-`. `access` first, then
  **reachability**: the hash must be a revision's file of a live part inside the share's subtree — content
  addressing is not authorization. Streams through `SourceReader::stream_at`, discarding N decompressed
  bytes to resume.
- **`check-deploy` learns `lapidary-peer`:** it may name `SourceReader` only in its blob module and
  `SourceWriter` only in its pull module, and never `SourceStore`. Test the rule the way `deploy.rs`'s other
  rules are tested.
- **Puller:** `POST /api/sharing/shares/{id}/pull { library, parts }` (a selection, or all) records the pull
  and `NOTIFY`s. The peer role pulls: each file staged as `<blake3>.part` in a **second named volume,
  `lapidary-peer-staging`** (the key volume holds the key and nothing else), resumed from the staged length,
  whole-file BLAKE3 checked at the end; then bundles of at most 64 MiB assembled with `StoreZip`, written with
  `SourceWriter`, and queued as `ImportBundle` into one batch the page follows.
- **Categories travel:** `ManifestPart` gains an optional `category: Vec<String>` (the path of category names
  from the library root, `#[serde(default)]` so older bundles still read); export writes it; import places a
  part along it with `get_or_create`. `ImportBundle` gains an optional destination folder, and a pull's is
  `Shared/<sharer's name>`. Plain bundle import and export keep categories from here on — record it.
- **Migration `0040`:** `part_provenance (part_id, device_id, sharer_name, pulled_at)`. The part page says
  "From <sharer>".
- **The overlay** mounts the storage volume the api mounts, and the staging volume.
- **Web:** Pull on the shared library page, with the destination library, reusing the batch progress; pulled
  parts show their licence and the sharer.
- **Exit (measured):** A shares `corpus-1g`; B pulls all of it cold. Record bytes moved against the
  `parts × size` prediction and the time. Kill B's peer role at about 50% and restart it: only the remainder
  moves. Add one revision on A and pull again: only that file moves, and no part is duplicated. Parts land in
  B's chosen library under `Shared/<A's name>/…` with licences shown and the sharer named.
- **Tests:** a hash outside the share is refused by the blob route; Range resumes from the staged length; a
  bad BLAKE3 is refused and the staged file dropped; categories round-trip through a bundle; a re-pull
  duplicates nothing. Mutation-check reachability, the resume offset, the hash check and category placement.

### 5. S4 — Ask me first, and taking it back: `feat/sharing-consent`

- **Migration `0041`:** `share.mode` (`open` default, `ask`), and `share_grant (share_id, device_id, state,
  decided_at, decided_by)` with `decided_by` nullable — the seam Phase 8 fills.
- **Ask mode end to end:** `POST /peer/v1/shares/{id}/request` records a request; the sharer's page lists
  requests with Grant and Deny; the puller's pull waits, saying whose permission it waits for, and continues
  when granted without re-transferring what is staged. Deny says so on the puller's page.
- **Taking it back:** stopping a share, or removing a person, refuses the next request with a message naming
  the share; parts already pulled stay. Every peer route checks `access` per request, so a connection opened
  before a removal is refused at its next request — **this closes S1b's "an open connection outlives a
  removal"**; record it.
- **Pause** on the puller's side.
- **Limits:** at most 2 blob streams per device and 8 in all; a refusal past them says to try again.
- **Exit (measured):** an ask-first share makes B's pull wait; A grants; the pull continues with no staged
  file moved twice. A stops sharing mid-pull: B's next request is refused naming the share, and B's already
  pulled parts stay.
- **Tests:** each of those, plus the limits. Mutation-check the grant states, the per-request guard and the
  limits.

### 6. Close (on the last stage's branch, before its merge)

- `DATA.md` §7 for every new table; `FEATURES.md` §10 rows marked done; `ARCHITECTURE.md` for the staging
  volume and the peer's source handles; the jaunty plan's S2–S4 marked done with the merge hashes.
- One ROADMAP record per stage, as S1a and S1b have: what was built, bugs the tests found, mutation counts,
  the measured exit, decisions taken without the owner, what is left.

## Done when

The listener hardening, S2a, S2b, S3 and S4 are each merged `--no-ff` into `main` with `cargo xtask verify
slice` green on the merged tree, mutation-checked, and their exits measured on two stacks and recorded in
`docs/ROADMAP.md`. Nothing is pushed.
