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

Hash and store the source in chunks, and stream the download body. The mesh kernel still
needs the whole mesh in memory to tessellate — that is inherent and not this slice's fight;
what changes is that reading, hashing, compressing and serving stop each holding their own
full copy.

**Acceptance is a measurement, not an assertion:** resident memory during ingest of the
largest file in the corpus must not track file size.

## 4. Upload from the browser

`DATA.md` §5.2 is already binding and is not re-derived here:

- BLAKE3 in WASM on the client, **before** transferring.
- `POST /api/uploads/probe { hashes: [...] } → { have, need }`, so re-importing a library
  transfers only what is new. `PgBlobs::library_holds` already answers this per file.
- **Resumable chunked transfer is mandatory.** Chunk-with-offset; a 2 GB file over a VPN
  as one POST fails at 90%.
- **The server re-verifies the assembled BLAKE3 against the client's claim before
  committing.** Never trust the client hash — it is the dedup key, and a wrong one
  silently corrupts another user's part.
- Folder upload via `webkitdirectory` and `DataTransferItem.webkitGetAsEntry()`. The
  relative path the browser reports is the `source_path` of §2, which is what makes an
  uploaded folder and a scanned folder the same thing in the database.

The upload route lands on `Role::Api`; it writes bytes and enqueues, and it never invokes
the kernel, so the open-path rule and `check-deploy`'s boundary are untouched.

## 5. Out of scope

The detail card, the image gallery, SSE, the virtualized grid and the seeded example part
are **slice 6b**. Incremental directory sync, rename and deletion detection are the slice
that owns them (§2). Soft delete, purge, quarantine and the reference-counted reaper are
**slice 7**. STEP, IGES and anything requiring OCCT are **Phase 0b and Phase 2**.
