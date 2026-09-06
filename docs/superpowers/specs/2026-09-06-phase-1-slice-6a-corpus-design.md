# Phase 1 slice 6a — a real corpus survives, and files go in from a browser

**Status:** design, binding for slice 6a. **Branch:** `feat/corpus-and-upload`.

**Why this slice is cut out of slice 6.** `2026-09-05-phase-1-remaining-slices.md` gives
slice 6 four bullets, and slice 5's spec §5 quietly added two more (the detail card and the
image gallery). Two more arrived from the owner: a nested corpus cannot be scanned at all,
and ingest reads whole files into memory. Eight workstreams is not a slice. 6a is the half
that closes the pipeline's *input* end; 6b is Phase 1's exit criterion.

**Exit:** point Lapidary at a nested directory of STLs and every file lands, once, with no
flattening — and separately, drag a folder onto the grid and the same is true, with no
terminal in either case.

---

## 1. The walk becomes recursive

`crates/lapidary-ingest/src/scan.rs:125` is a single `read_dir`. Subdirectories are
silently ignored, which is why a real corpus has to be flattened before Lapidary can see
it. Silence is the problem: a directory of 4,000 STLs in 200 folders scans as "0 files".

**The payload's meaning changes, its type does not.** `JobPayload::IngestFile { path }`
carries `path.file_name()` today — a bare filename. It becomes a path **relative to
`ingest_dir`**, with `/` separators. Still a `String`, so pre-existing `pending` rows
deserialize and run exactly as before: a bare filename is a relative path of one segment.
`ingest_one` already does `self.ingest_dir.join(file_name)`, which is correct for both.

**Rules, none optional:**

- **Depth is capped at 16.** Real directories cannot cycle, and symlinked ones are not
  followed (below), so this is not a loop guard — it is a bound on pathological nesting
  and on bind mounts that can cycle. A tree deeper than 16 logs and stops descending
  rather than failing the scan, because the files above the cap are still real work.
- **Symlinked directories are not followed.** `DirEntry::file_type()` does not traverse a
  symlink, so `file_type().is_dir()` is already false for one — the guard is free, and it
  is what makes cycles impossible. Symlinked *files* are still ingested: `is_mesh_candidate`
  uses `Path::is_file`, which does follow, and an operator who symlinks an STL into their
  library meant it.
- **Dot-entries are skipped**, files and directories both. A `.git`, `.Trash` or
  `.DS_Store` inside someone's parts folder is not part of their library, and walking a
  `.git` on a large corpus is pure waste. Skipped silently, like any other non-candidate.
- **A relative path is refused if it escapes.** No absolute path, no `..` segment — the
  rule `DATA.md` §5.4 already states for archive entries, applied here because the payload
  is now multi-segment and `Path::join` will happily accept `../../etc/passwd`. Today the
  scan is the only producer of these payloads and produces none, so this guard protects a
  door nobody has opened yet; task 3 opens it, and the guard belongs on the door, not on
  the caller.
- **The order stays deterministic.** The whole relative path sorts, not the basename, so
  job ids follow the order a person reading the tree would expect.
- **Unreadable subdirectories are logged and skipped, not fatal.** Only the *root* being
  unreadable fails the job — that is the existing `ingest_dir_unreadable`, and the reason
  it is `Permanent` is unchanged. A single unreadable folder deep in a corpus must not
  cost the other 3,999 files.

## 2. Identity moves from the name to the path

**This is the part that makes recursion safe, and it must ship in the same commit.**

Recursion alone is silent data loss. `part_name` is the file stem
(`crates/lapidary-ingest/src/handler.rs:341`), so `brackets/bracket.stl` and
`plates/bracket.stl` both become the part `bracket`. The second one violates
`part_name_unique_per_library`, and `classify_write`
(`crates/lapidary-ingest/src/handler.rs:385`) maps that violation to `Outcome::Skipped` —
so the file is reported as *"already here"* and is never indexed. That is precisely the
failure `handler.rs:46` says the design refuses:

> A directory of files is a set of files, and a scan that quietly indexes only the first of
> two is the same shape of lie as the empty second library.

**`part.source_path` becomes the identity; `part.name` stays for humans.** A part number
convention does not exist yet, so the stem remains the label in the grid, duplicates and
all. Two parts called `bracket` in different folders is the truth, and the path is what
tells them apart.

Migration `0007_source_path.sql`:

- `part.source_path text`, the path relative to the library's ingest root, `/`-separated.
- Backfilled from existing rows, which are all flat by construction — the scan that
  created them could not descend — so the original filename is exactly
  `name || '.' || file.format`. A part with no source `file` row falls back to `name`
  alone, so the backfill cannot strand a row.
- `NOT NULL` after the backfill. Uploads have a filename too (§4), so there is no ingest
  route that lacks one.
- `part_name_unique_per_library` is **dropped** and replaced by
  `part_source_path_unique_per_library UNIQUE (library_id, source_path)`.

**Three things must move together, and `0003_jobs.sql:52-57` says why:**

> The key must agree with `PgBlobs::library_holds` […] If one of the two starts filtering
> and the other does not, `library_holds` returns false and this constraint throws on a
> path with no reason to expect it.

So all three change in one commit:

1. `PgBlobs::library_holds(library, part_name, hash)` → keyed on `source_path`. "The same
   file, seen again" becomes *same library, same path, same bytes*, which is what a re-scan
   actually is. It still does not filter `deleted_at`, for the reason recorded there.
2. `part_source_path_unique_per_library` is the constraint the race now trips.
3. `classify_write` matches the new constraint name.

The race-safety story is unchanged in shape: two workers racing the same file still have
one win and one receive a unique violation mapped to `Skipped`. Only the key differs.

**Rename detection is not in this slice.** The column is its prerequisite, and
`handler.rs:50` already schedules it that way — "closing that needs a source-path column
and the slice that owns incremental directory sync". Same bytes at a new path still
produces a second part sharing one blob (`ref_count` 2), which stays the documented
behaviour and the visible failure mode.

## 3. Streaming instead of buffering

`std::fs::read` at `handler.rs:132`, a `Vec<u8>` out of `SourceReader::get`, and a
`Vec<u8>` response body in `download.rs` — which says so itself: *"Fine at Phase 1 sizes
and wrong for a 2 GB STEP."* `DATA.md`'s own source range is 1 MB – 2 GB.

Stream the download body, and stop the store holding a second full copy of everything it
writes or reads. The mesh kernel still needs the whole mesh in memory to tessellate — that
is inherent and not this slice's fight — so **ingest** goes from two copies to one rather
than to none. The **download** path can go to constant memory, because it only forwards
bytes.

That asymmetry is the point, and the original wording of this section got it wrong: it
asked for ingest RSS not to track file size, which the kernel makes impossible. The two
honest criteria are:

- **Ingest:** peak resident memory is ~1× the file, not ~1.5×. `write_blob` compressed into
  a `Vec` and held it beside the caller's slice; on a real 380 MB STL, which the slice 5
  handoff measured compressing 2.05×, that is 380 MB + ~185 MB at once inside a worker
  capped at 2 GB running two jobs.
- **Download:** peak resident memory is constant in the file's size. This one is measurable
  directly, and `deploy/compose.yaml` caps `api` at **512 MB** — so a 380 MB file was not a
  slow download, it was one the container could not serve at all, let alone twice at once.

**Measured, in a fresh process per mode, on a synthetic 380 MB STL:** streaming peaks at
**5.7 MB**; buffering the same blob peaks at **385.8 MB**. Reverting the streaming decode is
behaviourally identical — no test can catch it — which is exactly why this criterion is a
measurement and is written down here.

**One guarantee changes shape, deliberately.** The route used to hash the whole file and
compare it against `blob.blake3` before sending anything, so a corrupt blob was a 500 and
the user got nothing. Verifying before the first byte *is* buffering, so streaming cannot
keep that. The bytes are hashed incrementally and a mismatch closes the body early, with
`Content-Length` sent from `blob.size_bytes` so a short read is detectable and the strong
`ETag` still carrying the digest to check against. The promise moves from *"we never begin
sending unverified bytes"* to *"we never finish a download whose bytes did not verify, and
we always say how many bytes a complete one has"*. `crates/lapidary-api/src/download.rs`'s
header carries this too, because that is where someone will meet it.

## 4. Upload from the browser

`DATA.md` §5.2 states the contract — client-side BLAKE3, a probe, resumable chunks, a
server-side re-verification, `webkitdirectory` for folders — and is not re-derived here.
What it does not say is *where the uploaded bytes land*, and that is this task's whole
design problem: `check_open_path_boundary` forbids `lapidary-api` from naming
`SourceStore` at all, and the worker's `/ingest` mount is read-only. Neither process has
an obvious place to put a file.

### 4.1 The api writes the blob; the worker reads it back out

The shape this rejects is a shared staging volume the worker also mounts, with
`IngestFile` learning a second root to join against. That buys a mount, a payload
discriminator, and a new failure mode — the worker opening a file the api has not
finished writing — in order to move bytes the api is already sitting on.
`deploy/compose.yaml` mounts `lapidary-blobs` on `api` **read-write** today, with a
comment saying that is deliberate and that it is not a hole in the open-path rule,
because *the boundary is a type, not a mount flag*.

So: the api writes the source blob into the CAS and enqueues
`JobPayload::IngestBlob { blake3, source_path }`. The worker reads those bytes back with
the `SourceStore` it already holds, meshes them, and writes the rows. `job.kind` is the
discriminator column and carries no CHECK constraint — `0003_jobs.sql` constrains `state`
and `outcome` and nothing else — so a new kind needs no migration.

**`SourceWriter` is the mirror of slice 5's `SourceReader`, and inherits its rule.** A
handle over the source half of the CAS that can only *write*: no `get`, no `remove`, no
`WorkerRole` to construct. The reasoning is the same one recorded on `SourceReader` and
comes out the other way round: gating this behind `WorkerRole` would hand the api `get`
and `remove` on every source blob in order to buy a `put`, and read is the half of that
type worth spending a token on when the caller already holds the bytes in its own request
body. `check_open_path_boundary` gains one symmetric clause — `lapidary-api` may name
`SourceWriter` only in `crates/lapidary-api/src/upload.rs` — for the reason the
`SourceReader` clause exists: a write handle in one named route is a decision, the same
handle in six files is the mistake arrived at by copy-paste.

`SourceWriter::put_file` takes the staged file, the hash the client claimed, and streams
one into the other — hashing, compressing and writing in a single pass. **The
verification lives here, not in the route**, because "store these bytes and they must
hash to X" is the content-addressed store's own invariant, and because a route that
verified separately would read a 2 GB file twice. A mismatch writes nothing: the temp
file is removed and `StorageError::HashMismatch` names both digests.

### 4.2 The orphan this creates, and the row that closes it

`ingest_one` writes the source blob immediately before its transaction and reaps it if
that transaction fails — an ordering `handler.rs`'s module doc and
`docs/prototype-notes.md` both exist to protect, because the Node prototype's exact bug
was a successful blob write followed by a failed insert with no cleanup.

This split moves the two halves into different processes and puts a queue between them.
Between the api's commit and the worker's job there are bytes on disk that no `part` row
points at, and if the job fails permanently — an unparseable STL, a library deleted
underneath it — nothing ever reaps them. That is a new orphan class, and the reap-on-
failure trick cannot reach it: the failing process did not write the bytes and must not
assume it may delete them.

**So the api inserts the `blob` row, `ref_count = 0`, in the same commit that writes the
bytes.** Slice 7's reference-counted reaper is defined over exactly that: a `blob` row
whose count is zero is collectable, and an orphan with no row at all is invisible to it
forever. One INSERT now is the difference between a known orphan and a leak.

It also makes the worker's arm shorter rather than longer. `ingest_one` already branches
on `blobs.exists(&hash)`, and an uploaded blob's row makes that branch true — so the
existing `link_existing` path runs, which skips the blob insert, writes the part chain,
and on failure reaps **only the rungs it produced itself**, never the source blob. That
is precisely the correct behaviour here, reached without a special case.

### 4.3 The staging area is a file, and the hash is the session id

The client computes BLAKE3 before transferring, so it already holds the one identifier
both sides agree on. A chunk goes to `PUT /api/libraries/{id}/uploads/{blake3}?offset=N`
and appends to `<upload_dir>/<library>/<blake3>.part`.

This is what makes resumability cost nothing. There is no session table, no session id,
no expiry sweep and no in-memory map: **the staging file's length is the session state.**
Resuming is `offset = length`; a wrong offset is a `409` carrying the length the server
actually has, so a client that lost track is told, not guessed at. An api restart loses
nothing as long as `upload_dir` is a volume, and degrades to a re-transfer if it is not —
which is the same answer a session table would give with a schema attached.

`blake3` is parsed with `BlobHash::parse_hex` before it reaches the filesystem: 64
characters, lowercase, hex. There is no traversal to defend against because there is no
string from the client in that path at all.

The library scopes the staging directory, which is not a permission check — there is no
authentication in Phase 1 — but removes the question. Two libraries uploading the same
bytes cannot interleave into one another's partial file and fail each other's
verification.

Three routes, and that is the whole surface:

- `POST /api/libraries/{id}/uploads/probe` — `{ files: [{ path, blake3 }] }` answers
  `{ have, needRows, needBytes }`. `have` is `PgBlobs::library_holds` — this library
  already indexes these bytes at this path, so there is nothing to do. `needRows` is
  `PgBlobs::exists` — some library holds the bytes, so only the rows are missing and the
  transfer is skipped entirely. `needBytes` is the rest. Three lists rather than
  `DATA.md`'s two, because the second win is the larger one: re-importing a folder into a
  *new* library transfers nothing.
- `PUT /api/libraries/{id}/uploads/{blake3}?offset=N` — one chunk, raw body, answers the
  new length.
- `POST /api/libraries/{id}/uploads/commit` — `{ files: [{ path, blake3 }] }`, one batch
  for the whole folder, `202` with the batch id the grid already polls. Committing per
  file would give a folder of 500 parts 500 progress bars.

### 4.4 The path is checked at both doors, for two different reasons

`reject_escaping_path` guards `ingest_one` against a payload that would escape the
`ingest_dir` join. The upload path never joins anything — but a client-supplied
`source_path` goes straight into `part.source_path`, and from there into a
`Content-Disposition` filename on the download route. Different surface, same refusal.

The predicate moves to `lapidary-core` as `path_escapes`, and each caller keeps its own
error type and its own wording. One predicate, two doors: a guard on the caller rather
than on the door is the shape that leaves a sibling caller unprotected.

### 4.5 Not in this task

The browser half — WASM BLAKE3, the drop target, `webkitdirectory` — is the commit after
this one. The server half is testable with `curl` and is where the boundary decisions
live, so it lands first and alone.

A staging file with no commit behind it is never swept. It is a `.part` in a volume, it
holds no row, and no route reads it; slice 7 owns reclaiming disk, and a sweep written
now would be a second reaper to keep in step with the real one.

## 5. Out of scope

The detail card, the image gallery, SSE, the virtualized grid and the seeded example part
are **slice 6b**. Incremental directory sync, rename and deletion detection are the slice
that owns them (§2). Soft delete, purge, quarantine and the reference-counted reaper are
**slice 7**. STEP, IGES and anything requiring OCCT are **Phase 0b and Phase 2**.
