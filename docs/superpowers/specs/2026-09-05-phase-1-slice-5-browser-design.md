# Phase 1 slice 5 — drive it from a browser

Binding. Scan a folder, browse it, download a file and `diff` it against the one you
started with — without a terminal.

The plan budgeted an hour of spec for one blocker: source bytes cannot reach `Role::Api`.
That framing was wrong in two ways, and §1 records what is actually true before deciding
anything on top of it.

---

## 1. The source-access decision

### 1.1 What is actually blocking

Three things the plan asserted, checked against the tree:

| Asserted | Actually |
|---|---|
| The api cannot reach source bytes | `deploy/compose.yaml:79` mounts `lapidary-blobs:/var/lib/lapidary:Z` on **api**, read-write. The bytes are already on its filesystem |
| `SourceStore` and `DerivativeStore` are different stores | Same root, same `blob_path` layout. `DerivativeStore::get` and `SourceStore::get` read the *same file*; the only difference is the zstd decode flag |
| The browser cannot reach the worker at all | The worker binds `0.0.0.0:8081` and publishes `8081:8081` — the scan route lives there today. What is true is that the SPA **through Caddy** cannot: `deploy/web/Caddyfile` proxies `/api/*` to `api:8080` only |

So this was never an access problem. `WorkerRole` is a **naming and ownership** gate:
`xtask/src/deploy.rs`'s `check_open_path_boundary` greps `lapidary-api`'s sources for the
string `SourceStore` and nothing else. Its own module doc says as much — a green run
proves "nothing named `SourceStore` directly", not "no source bytes are reachable".

**The security boundary is elsewhere, and it holds.** A source hash 404s on
`GET /api/blob/{hash}` because `derivative_is_reachable` joins `derivative`, so a source
blob has no row to match — not because of a token. Whatever §1.2 decides, that query is
what must not weaken.

### 1.2 Decision: a read-only handle, and the gate stays

Add `SourceReader` to `lapidary-storage`. Read-only: `open(root)` with no proof, `get`,
no `put`, no `remove`. `lapidary-api` uses it; `lapidary-ingest` keeps `SourceStore` for
everything that writes.

Rejected, with reasons:

- **Relax the `SourceStore` gate.** Hands the api `put` and `remove` on source bytes to
  buy a read. The write surface is the half of that type worth gating.
- **A Caddy route to `worker:8081`.** Buys nothing — the api already has the bytes — and
  costs a second public origin, a second CORS surface, and a download path that dies
  whenever the worker is busy meshing. The worker's 2 GiB / 2 CPU ceiling exists because
  it meshes; serving downloads from under it is the wrong place to spend that.
- **Stage bytes into a derivative blob.** Doubles storage for a copy of bytes that are
  already there, and puts a job round-trip in front of a download.

`check_open_path_boundary`'s existing grep is untouched — `SourceStore` is not a substring
of `SourceReader`, so no false positive and no rule change. It gains one assertion:
**`lapidary-api` may name `SourceReader` only in `download.rs`.** Same helper shape, five
lines. Without it the exemption spreads by copy-paste, which is exactly how the original
mistake this gate catches gets made.

### 1.3 The product rule this amends

`CLAUDE.md`: *"The open path never touches a source file and never invokes the CAD kernel.
Opening a part reads metadata + derivatives only."*

That rule is about **opening** — the grid, the viewer, the detail card, the interactive
path whose latency budget is 80 ms and whose failure mode is parsing STEP to draw a
thumbnail. It stays exactly as strict: nothing in this slice adds a source read to any
route that renders a part.

**Download is not open.** It is a distinct, deliberately-named path that streams stored
bytes to a user who asked for those exact bytes, and it parses nothing. `CLAUDE.md`'s
other rule — *"`variant=original` returns byte-identical ingested bytes"* — cannot be
satisfied without it. Amend `docs/ARCHITECTURE.md` and `lapidary-storage`'s module doc to
say *open*, not *api*, so the next reader is not left reconciling two true sentences.

Kernel-linking is unaffected and independently enforced: `layers.rs`'s `FORBIDDEN_PAIRS`
forbids the `lapidary-api → lapidary-cad` edge, and `check_compose` keeps `SERVER_FEATURES`
off the api service. Neither depends on the `SourceStore` grep.

---

## 2. The download route

`GET /api/revisions/{id}/download?variant=original`, `Role::Api`, per `DATA.md` §5.1.

### 2.1 Scope

No library segment, matching `DATA.md`. `revision_source` takes `(library, revision)`
because ruling T7-C found a derive job could name another library's revision — there, a
second id arrives in the payload and cross-checking it is a real test. Here there is no
second id: resolving the library *from* the revision and then checking it against itself
is a no-op that reads like a check, which is worse than no check.

A new repo method instead, `source_for_download(revision)`, returning hash, format, part
name and `zstd_level` in one row. It filters **`part.deleted_at IS NULL`**: a deleted part
is not browsable, and a held URL must not outlive the delete. (`library_holds` deliberately
ignores `deleted_at` for the opposite reason — re-scanning must not resurrect. Different
question, different answer.)

The revision uuid is the capability, consistent with every other Phase 1 route. When auth
lands, the check becomes "is this revision's library reachable by this caller" — a real
check with a real subject. Write it then, not as a placeholder now.

### 2.2 `variant`

Exactly one legal value in this slice: `original`. Missing or unknown → **400**, naming
what to send. Never a silent fallback — a download that quietly returns something other
than what was asked for is the failure `DATA.md` §5.1 exists to forbid.

### 2.3 Headers

- `Content-Type: application/octet-stream`, always. Never `model/stl`: a browser that
  renders it inline is a download that did not download. (The blob route's hardcoded
  `model/gltf-binary` is correct for what it serves and is not copied here.)
- `Content-Disposition: attachment` with **both halves** — ASCII `filename=` fallback and
  RFC 5987 `filename*=UTF-8''…`. `DATA.md` §5.1 is explicit that Turkish part names carry
  ğ, ş, ı and a naive `filename=` mangles the download. A Turkish name in the fixture, not
  an ASCII one with a comment about Turkish.
- `ETag` is the blob hash, as the blob route does. No `immutable` cache header: the URL
  names a revision, not a hash, and a revision's source could in principle be re-pointed.

### 2.4 Filename

Synthesized: `{part.name}.{format}`. No new column. `part.name` is the ingested file's
stem (`handler.rs:337`), so the round-trip is exact for every file scanned today.

Sanitized: path separators, control characters and quotes stripped, length capped. A
renamed part downloads under its **new** name — intended. The byte-identity claim is about
bytes; the filename is a current label, and a download named after a name the user changed
last month would be the surprising behaviour.

If a later slice wants the original basename preserved across renames, that is a column on
`file`, and it is not free — it is a migration and a backfill for parts ingested before it.
Out of scope here.

### 2.5 Decompression, and verifying what we serve

The api decides from **`blob.zstd_level`**, read in the same row as the hash. Not from
`Compression::for_source_format` — that is ingest-time policy, and slice 7 is about to
change it; a reader that re-derives policy would start returning zstd frames the day the
policy moves.

`INSERT INTO blob … ON CONFLICT (blake3) DO NOTHING` is what makes the column trustworthy:
the row is written by the same call that created the file, and no later ingest overwrites
it. The same bytes arriving twice under different formats cannot leave the row disagreeing
with the disk.

**The route re-hashes the bytes it is about to serve and 500s on mismatch.** BLAKE3 at
~1 GB/s against a corpus whose largest file is single-digit megabytes is free next to the
transfer that follows. It makes the product claim — *"byte-identical, verifiable against
the stored BLAKE3, show the hash next to the button"* — actually true rather than asserted,
and it closes the one hole `DO NOTHING` leaves: a crash between `write_blob`'s rename and
the transaction commit can leave a file on disk with no row, and a later ingest of the same
bytes under a different format then writes a row whose level does not match those bytes.
Narrow, but silent, and this turns it into a loud 500.

Revisit when downloads stream rather than buffer. `read_blob` returns a `Vec<u8>` today, so
the whole file is already in memory; re-hashing adds no allocation. Streaming a 2 GB STEP
is a real concern and a later one — note it, do not build for it.

### 2.6 The warm signal

`touch_blob` on the `Ok` arm, after the bytes are in hand and the hash checks out, exactly
as `blob.rs` does — never on a 404, or a caller could move a timestamp by guessing.

**Consequence, recorded here so slice 7 does not inherit it as an accident:** this is the
only warm input for source blobs. A library browsed constantly but never downloaded stays
cold and gets compressed by the Phase D sweep. That is defensible — the sweep's question is
"are these bytes being handed out", not "is this part interesting" — but it must be a
decision, and slice 7 either adds a second warm input (a part-level `last_viewed_at` written
by the detail card) or writes down that browsing does not count.

Note also that `blob.last_accessed_at` is per-blob and deduplicated: two parts with
identical bytes share one timestamp. Correct for a compression decision, wrong as "this
part was used."

### 2.7 Forward constraint on slice 7

In-place recompression rewrites bytes and updates `zstd_level` with no transaction spanning
both. A reader landing between them must not misread. Two orderings, one safe:

- **Row first, then bytes** — the window shows level 3 over raw bytes. §2.5's hash check
  catches it; a decode of non-zstd bytes fails loudly.
- **Bytes first, then row** — the window shows level 0 over a zstd frame, and the route
  returns a zstd frame *as if it were the file*. §2.5's hash check catches this one too,
  but only because it exists.

Slice 7 writes the row first, or introduces frame-magic sniffing. Either is fine; picking
neither is not. One paragraph, no code, until that slice.

---

## 3. Scan from the UI

`POST /api/libraries/{id}/scan` on `Role::Api` as an enqueue, mirroring slice 4's `derive`
job exactly: the route validates and enqueues, the worker keeps the directory walk because
only it mounts `ingest_dir`. Returns the existing `ScanAccepted`.

The worker's own `:8081` scan route stays for now — it is what `README.md`'s first-run curl
documents. Whichever slice removes it owns updating that.

404 rather than 202 for a library id that matches no row, as the slice 4 trigger routes do.

## 4. Storage visibility

Per part: original size, size on disk, whether it is compressed. Per library: source total,
derivative total, and the ratio between them — the number that made slice 4's 92.5% drop
legible is the same number a user wants for their own library.

All of it is already on `blob` (`size_bytes`, `stored_bytes`, `zstd_level`). No migration.
This is an aggregate query and a panel, and it is the readout Phase D's tiering work will be
judged against, which is why it lands before the tiering rather than after.

---

## 5. Out of scope

Upload from the browser (Phase F), SSE (Phase F), the detail card and the image gallery
(slice 6), any compression policy change (slice 7), folder-mode libraries (slice 8).

`variant=3mf` and every other converted download: there is no converter on the open path
and adding one would invoke the kernel from the api. It belongs wherever format negotiation
lands, and it is a worker-side job producing a derivative, not a route.
