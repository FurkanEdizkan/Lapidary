# The overlay diff, `lapidary://`, render cache, and bundles

**Slice:** `2026-09-15-phase-4-slice-2`
**Goal file:** `docs/superpowers/plans/2026-09-15-phase-4-slice-2-goal.md`
**Closes:**
- the visual overlay (DATA §6.1);
- a link that opens a part in its tool;
- the storage view's missing figures and the render-cache action (DATA §1.5, §1.6);
- slice 1's two recorded debts;
- Phase 5's bundles, if this slice reaches them.

---

## 0. What this slice cannot reach

These are recorded in ROADMAP's "Open points, 2026-09-15" and are not in this slice:
- **Phase 4's exit.** It needs FreeCAD and the OCCT image.
- **macOS and Windows watchers.** There is no machine to run them on.
- **Source tiering.** It is retired: library files stay raw (DATA §1.3).

The checks here run on mesh parts, with the mock kernel, on Linux.

## 1. Facts this design stands on

**Revisions share one coordinate frame.** Two revisions' rungs come from the same millimetre model
coordinates as their source files.
- **The glb:** `glb.rs` writes one node with no transform. Positions are float32 in lossless
  meshopt `ATTRIBUTES` mode, with no quantization offset.
- **Clustering:** `cluster.rs` copies vertex positions unchanged.
- **The readers:**
  - STL and OBJ apply no transform at all.
  - 3MF applies the file's own build and component transforms, and its unit.
  - The OCCT bridge writes world coordinates in millimetres.
- **The consequence:** a ghost drawn at the same scene origin as the solid is drawn where the earlier
  file placed it. If a tool moved the model's origin between saves, the ghost shows that offset, and
  that offset is true.

**Rungs.**
- Ingest writes only L0.
- L1 and L2 are built on request, and only for the latest revision (`POST /api/parts/{id}/rungs/{level}`
  reads `latest_revision`).
- An earlier revision therefore often has only L0.
- The blob route (`GET /api/blob/{hash}`) serves a rung of any revision through
  `derivative_is_reachable`.

## 2. Overlay diff

**API.** `PartRevision` gains `tessellationL0` and `tessellationL1`, named as on `PartDetail`.
- The page draws L1, else L0, and knows from which of the two it drew whether the ghost is the coarse
  one.
- Read the same way `PgParts::detail` reads its rungs: a `LATERAL` per kind, bound from
  `DerivativeKind`.
- No new route is needed.

**The page.**
- Compare gains a "Show the earlier revision as a ghost" toggle. The ghost is Compare's `from`
  revision.
- The toggle reaches `Preview` → `Viewer` as a `ghost` prop: the hash, or `null`.
- Compare is keyed by part, so the toggle never survives a change of part.

**The viewer.**
- The ghost is its own scene object with its own material, never a child of `model`. `show` repaints
  and disposes `model`'s meshes, and `applyHidden` rewrites their materials, so the ghost has to stay
  out of both.
- **Its material:**
  - `MeshBasicMaterial`, amber (`--color-warn`), `transparent`, opacity 0.4, with `depthTest` and
    `depthWrite` both off, so it is drawn through the part;
  - amber rather than DATA §6.1's grey. The first browser check drew a grey ghost over the grey part
    on the near-black ground, and it was barely visible. A smaller earlier revision sits inside the
    current one, so it has to show through;
  - drawn after the solid (`renderOrder` 1);
  - the same `clippingPlanes` as the solid's while a cut is on, so a cut hides both halves alike.
- **It is never picked.** `pick` and `through` cast against `model` only.
- **The camera does not reframe** for the ghost: it stays framed on the current revision.
- **Disposal:** the ghost's geometry is disposed when it is switched off, when its hash changes, and
  when the view unmounts.

**Honesty.**
- **No rung.** When the `from` revision has no rung, the toggle is replaced by a line saying that
  revision has no mesh to show. The viewer is never handed an empty ghost.
- **Coarse ghost.** When the ghost is an L0 and the solid is finer, the page says the ghost is the
  coarse preview. A coarse outline next to a fine one must not read as a shape change.
- **Mock kernel.** Its rung bytes are markers, not glTF. The browser check therefore runs on real
  rungs, and the section's own tests work at the data level.

## 3. `lapidary://` and launching a tool

**Commands.**
- **`lapidary register`**
  - Writes `$XDG_DATA_HOME/applications/lapidary-url.desktop`, defaulting to
    `~/.local/share/applications`.
    - `Exec` runs this binary's absolute path as `open %u`.
    - `MimeType=x-scheme-handler/lapidary;` and `NoDisplay=true`.
    - An `X-Lapidary-Handler=1` marker, so `unregister` only ever removes a file it wrote.
  - Runs `xdg-mime default lapidary-url.desktop x-scheme-handler/lapidary`, which writes
    `$XDG_CONFIG_HOME/mimeapps.list`.
- **The server and workspace are fixed when you register.**
  - A handler started by a browser does not get the shell's environment, so `Exec` carries
    `LAPIDARY_SERVER` and `LAPIDARY_WORKSPACE` itself: `env LAPIDARY_SERVER=… LAPIDARY_WORKSPACE=…`.
  - Arguments are written unquoted. `register` refuses a server, workspace or install path holding
    anything but letters, digits and `/ . _ - : = @ + ,`.
    - `xdg-open`'s own launcher, the one used outside GNOME and KDE, splits `Exec` on spaces and
      keeps the quotes.
    - The check caught it: a spec-quoted `Exec` handed `env` the literal text
      `"LAPIDARY_SERVER=…"`.
  - This is also the security property: nothing in a URI can change either value. Re-run `register`
    after changing either one.
- **`lapidary unregister`**
  - Removes the `.desktop` file, only when it carries the marker.
  - Removes the exact `x-scheme-handler/lapidary=lapidary-url.desktop` line from `mimeapps.list`, and
    nothing else.
- **`lapidary open <uri>`**
  1. Parse the URI. Any refusal writes nothing and launches nothing.
  2. Find this workspace's checkout of the part, by the `part` in `.lapidary-checkout.json` and the
     same server. If there is one, reuse it.
  3. Otherwise check the part out exactly as `lapidary checkout` does, so a lock held by somebody else
     is refused with the server's own `checkedOut` message.
  4. Launch `xdg-open <file>` through `std::process::Command` with a single argument, never a shell.

**The URI is untrusted.** Any web page can hand one to the handler.
- Parsed by hand, with no URL crate: the prefix is exactly `lapidary://open?`, followed by exactly one
  pair, `part=<uuid>`.
- **Refused:**
  - another host, a path, a port or user info;
  - a fragment;
  - a second pair, a repeated key or an unknown key;
  - percent-encoding;
  - anything that does not parse as a UUID.
- The refusal names what was wrong.

**Where a failure shows.**
- A handler has no terminal. A refusal goes to stderr, and to `notify-send` when that command is on
  `PATH`, with the same text.
- The desktop notification is a convenience. Nothing depends on it.

**Web.**
- An "Open in desktop app" link (`lapidary://open?part=<id>`) beside Download on the part page, in a
  controlled library only. A hobby library refuses a checkout, so the link would only lead to a
  refusal there.
- Below it, DATA §6.3's honesty line:
  - it needs `lapidary register` and `lapidary agent` on this computer;
  - Rhino, FreeCAD and Blender save back;
  - Fusion 360 and Onshape do not;
  - Download is the way without the agent.

**The `Target` trait is not built.**
- `open` always hands out `variant=original`, and download names its variant explicitly. Neither
  negotiates a format, so one trait with no second caller would be an abstraction with nothing behind
  it.
- It arrives with the first target that needs a format the source is not in, which needs derivative
  exports (OCCT).

## 4. Storage view and the render cache

**What already exists.** `GET /api/storage` (`instance_storage`, `PgParts::instance_storage`) reports
source, derivative, inline-preview, removed and quarantined bytes.
- The quarantined figure already sums both `quarantined_file` (model directories) and quarantined
  blobs.
- So the goal file's "quarantined model directories" are already counted. This slice adds no second
  figure for them.

**What it adds.**
- **`renderCacheBytes`** in the response. It counts the bytes the action below would put into
  quarantine: blobs that are referenced *only* by evictable rung rows.
  - A small part's L0, L1 and L2 are often one shared blob (`0004`). Removing L1 and L2 frees nothing
    while L0 still points at it, so such a blob is not counted.
- **`POST /api/storage/render-cache`** removes every evictable rung row. It then recomputes the
  affected blobs' `ref_count` and quarantines those that reach zero.
  - This is the same recompute purge runs, lifted into one shared function, so the two cannot drift.
  - It answers with how many rungs it removed and how many bytes entered quarantine.
- **An evictable rung row is:**
  - an L1 or L2 `derivative` row;
  - whose blob's `coalesce(last_accessed_at, created_at)` is more than 90 days old.
  - Blobs never read since access tracking began count from when they were written.
- **Never touched:** L0, thumbnails, structure, entities, PMI, images, and every source file.
  Structure, entities and PMI could not be rebuilt (`derive.rs`). L0 keeps the open path drawing.
- **Rows, not only files.** The row goes too. The blob route rebuilds a rung whose *bytes* are missing,
  but a row pointing at nothing would be served as a 404 forever.
- **After eviction:** a part opened later draws L0 and asks for L1, exactly as a freshly ingested part
  does. The derive job rebuilds it.
- **The bytes leave through the existing 30-day quarantine and hourly sweep** (`reap`). Nothing here
  deletes a file.

**Wording** (the eviction boundary in `strings.ts`, and DATA §1.5):
- The action is "Free cache space", and it sits behind `Dialog`.
- The dialog says:
  - it removes rendered previews Lapidary can rebuild, and never a source file;
  - how much would enter quarantine;
  - that the space returns when quarantine ends, 30 days later.
- The result line says how many previews were removed and how much space returns after quarantine. It
  never says "freed" or "deleted".
- The figure and action sit in the existing instance storage block (`InstanceStorage`,
  `routes/index.tsx`).

**Tests.**
- **The figure:**
  - an L1 blob that no other row references counts;
  - an L1 shared with L0 does not;
  - an L1 read yesterday does not.
- **The action:**
  - only old L1/L2 rows go;
  - the source file, L0 and the thumbnail stay;
  - blobs are quarantined, not deleted;
  - `ref_count` equals what points at each blob.
- **Rebuild:** after eviction, `POST /api/parts/{id}/rungs/l1` queues a rebuild.

## 5. Slice 1's debts

**A new part's first revision says how it arrived.**
- `IngestRequest` gains `origin`, and `insert_part_chain` writes it.
- `index` already receives `origin` from both routes: `ingest` for a scan, and `upload` or `agent` for
  a commit.
- The agent never creates a part, since a lock names an existing one. So in practice a new part is
  `ingest` or `upload`.

**The watcher tests assert every verdict.**
- `a_write_still_going_restarts_the_wait` and `a_file_mid_rename_is_hashed_only_once_it_is_back_and_still`
  assert `Wait` on each change poll.
- Checked by making `poll` answer `Hash` on a change: both tests must fail.

## 6. Bundle export (only once 1–5 are merged)

**Routes.**
- `POST /api/libraries/{id}/bundle/plan` takes JSON `{ parts }`. It makes every check below and answers
  with parts, revisions and exact bytes, so the page can show a refusal before any download starts.
- `POST /api/libraries/{id}/bundle` is a form with one field, `parts`: comma-separated part ids, at
  most 500.
- **Why a form post:** a browser form post keeps the response's `Content-Disposition` filename, which a
  `fetch` into a blob URL would lose. A GET that carries 500 ids would be an 18 KB URL.
- **Where it lives:** in `crates/lapidary-api/src/download.rs`. The deploy gate allows `SourceReader`
  in that file only.

**Refused before a byte is sent,** each with a message saying what to do:
- a part from another library, or one that is deleted;
- more than 500 parts;
- two parts whose ZIP paths collide;
- a bundle whose sources total 4 GiB or more.

**The ZIP is written by hand: STORE entries with data descriptors.**
- **Why:** `zip` 2.4.2 and 4.0.0 both need `Write + Seek`, because they seek back to patch sizes and
  CRCs. A streamed body cannot seek.
- **How:**
  - Each entry is a local header with flag bit 3 and zero sizes, then the bytes, then a data
    descriptor.
  - The central directory at the end carries each entry's real CRC32 and sizes.
  - `crc32fast`, already in `Cargo.lock` through `flate2`, becomes a direct dependency at the locked
    version.
- **No ZIP64.** The 4 GiB refusal is the ceiling, marked `ponytail:`. ZIP64 records arrive with the
  first bundle that needs them.
- **Streaming:** the body goes out through `stream_verified`'s channel pattern. Each entry is
  BLAKE3-hashed as it streams, and a mismatch ends the body early, exactly as a download does.
- **Filename:** `<library slug>-bundle.lapidary.zip`, through `content_disposition`. The archive is
  something Lapidary produced, so it carries the `.lapidary.` infix.

**Layout.** It follows each part's `source_path`, so the tree reads like the library it came from and
imports back to the same paths:
- `<source_path>` holds the current revision;
- `<dir of source_path>/revisions/<label>/<file name>` holds each earlier revision;
- `manifest.json` sits at the root, written last. Its entry is small and built in memory.

**`manifest.json`.**
- **Bundle level:** `{ "format": "lapidary-bundle", "version": 1, "library": {name, mode}, "parts": [...] }`.
- **Per part:** `name`, `partNumber`, `sourcePath`, `tags` and `sources` (url, vendor, externalId,
  title, licence), plus `revisions`, oldest first.
  - Materials are left out. Import runs the kernel on every revision, and it reads them from the file
    again.
- **Per revision:** `revLabel`, `parentLabel`, `origin`, `createdAt`, `blake3`, `sizeBytes`, `format`,
  `path` (inside the ZIP).

**Web.** "Export bundle" in `SelectionBar`, beside Move and Remove. It submits a hidden form for the
current selection, which lives in component state, not the URL.

**Tests.**
- A two-revision part's entries are byte-identical to the stored files.
- Every entry is STORE.
- `zip::ZipArchive` reads the archive back, and its CRCs check out.
- The manifest has the shape above.
- Each refusal above.

## 7. Bundle import (only once 6 is merged)

**How the bundle arrives.**
1. The ZIP arrives through the existing chunked upload, as one blob of at most 2 GiB (the upload cap).
2. `POST /api/libraries/{id}/imports { blake3 }` validates the bundle before anything is queued.
3. It then enqueues one `ImportPart { bundle, index }` job per part, in one batch.

**Validation, all before any write** (DATA §5.4):
- at most 10,000 entries and 2 GiB in total;
- every name is refused if it is absolute or contains `..`, by `tmf.rs`'s rule;
- `manifest.json` parses, and its `format` and `version` are known;
- every revision's path is present, its size matches, and its BLAKE3 matches the manifest;
- every revision's label, parent and origin are known;
- no two parts share a `sourcePath`.

**One part's job.**
- **Controlled library:** `ImportPart` runs the ingest pipeline's `index` once per revision, oldest
  first, with that revision's bytes, `sourcePath` and origin.
- **Hobby library:** it runs `index` once, with the newest revision only, because a hobby library keeps
  no history. The job's line says how many earlier revisions were left out.

**What each call does.**
- **The first call** makes the part: `ingested`, or `skipped` when the library already holds those
  bytes at that path.
- **Each later call** meets a changed file at a known path, so the existing rules decide: a controlled
  library records the next revision, and the same current bytes are `skipped`.
- **Labels follow import order.** Labels were numeric and gapless in the source, so they come out the
  same.
- **Parents follow the chain.**
- **Kernel runs.** The kernel runs per revision, as ingest does, so every figure and rung is this
  instance's own.
- **Grafting is refused.** A part whose `sourcePath` already exists holding *different* bytes than the
  bundle's first revision would graft onto history it does not share. That job fails `Permanent`,
  naming the path.
- **The job's outcome** is its part's final outcome. The batch line counts parts, not revisions.

**Not preserved: `created_at`.**
- Revisions are stamped when they are imported.
- The bundle's own times stay in its manifest.
- Order is preserved, and order is what "current" reads.

**Web.** "Import bundle" beside the library's upload control. It uploads through the existing chunked
path, then calls the import route and follows the batch as any upload does.

**Tests.**
- Export, then import into a second controlled library: labels, parent labels, origins and hashes all
  match, and every revision's original download is byte-identical.
- Into a hobby library: only the newest revision's bytes arrive, and the line counts the ones left
  out.
- A traversal entry, a hash mismatch and a missing manifest each queue nothing.
- A graft is refused.

## Decided without the owner

1. **The ghost's rung.** The ghost is the earlier revision's L1, else its L0. It is not the same rung
   level as the solid, because older revisions usually have only L0. The page says so when the ghost
   is coarser.
2. **Reusing a checkout.** `lapidary open` reuses this machine's checkout of the part, and refuses when
   somebody else holds the lock.
3. **Fixed at registration.** Server and workspace are baked into the handler by `register`.
4. **Handler refusals** go to stderr and, where available, `notify-send`.
5. **Controlled libraries only.** "Open in desktop app" shows only there.
6. **No `Target` trait** in this slice.
7. **What counts as render cache.**
   - L1 and L2 rows whose blob was last read more than 90 days ago.
   - A blob never read since tracking began counts from when it was written.
   - Only blobs no surviving row references are counted, or put into quarantine.
8. **Eviction reports quarantine, not "freed".** It reports bytes entering quarantine, and says they
   return 30 days later.
9. **Export limits.**
   - It is a form post of at most 500 part ids.
   - It refuses a bundle whose sources total 4 GiB or more, rather than writing ZIP64.
   - The ZIP writer is written by hand, since no crate here streams one.
10. **One import job per part.** The batch counts parts, not revisions.
11. **Import stamps `created_at`.** Imported revisions get the time of import. Their order and the
    manifest's own times are kept.
12. **Import refuses grafts.** A `sourcePath` that already holds different bytes is refused, never
    grafted onto.
