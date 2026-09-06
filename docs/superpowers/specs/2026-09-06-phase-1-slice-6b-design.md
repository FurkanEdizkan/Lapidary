# Phase 1 slice 6b — the part you clicked, and a grid that holds a thousand

**Status:** design, binding for slice 6b. **Branch:** `feat/detail-and-live-grid`.

**Exit (`ROADMAP.md`, verbatim):** drop a folder of 1,000 STLs, the grid is interactive
immediately, every part appears — with a thumbnail where the library renders them
automatically, and with "No preview yet" plus a working `POST
/api/libraries/{id}/thumbnails` where it does not — re-dropping the same folder completes
in seconds via the hash short-circuit, and grid page load is under 80 ms warm.

Slice 6a closed the pipeline's input end. This closes its output end, and it is Phase 1's
last slice.

---

## 0. What this slice is not, and why

The plan of 2026-09-06 gave 6b five workstreams. **The `part_image` gallery is not one of
them any more**, on the owner's decision, and this section exists so nobody re-derives it
from `2026-09-04-phase-1-slice-4-derivatives-design.md` §3.3.

That section decided a user image is stored as bounded inline WebP `bytea` — not by hash,
not in `DerivativeStore` — and the reasoning was sound at the time: source bytes need
`WorkerRole`, and derivatives are freely evictable, so user data in either is a data-loss
bug waiting for the "clear render cache" action. `2026-09-06-folder-tree-and-moves-design.md`
dissolves that argument by moving the whole store: a model becomes a real directory holding
its file, its metadata and an `images/` folder. **The gallery belongs to whichever slice
lands that layout**, where it is one directory listing rather than a migration, an upload
route and a UI that get rewritten weeks later.

So the ordering column, `part_image`, and every image route stay unbuilt, and Phase 1 exits
without an image gallery. `ROADMAP.md`'s Phase 1 list never mentioned one — it arrived from
slice 4's deferral table and slice 5's §5 — so the exit criterion is unaffected.

Also out: the viewer (Phase 3), search (Phase 2), and anything needing the CAD kernel.

## 1. The measurement that re-cut this slice

The plan asked for a **virtualized grid**, and the roadmap bullet says the same. Measured
first, on a seeded library of 1,000 parts with real 2.8 kB WebP thumbnails, against the
release binary:

| Request | Warm | Payload |
|---|---|---|
| `GET …/parts` (default limit 50) | **16 ms** | 214 kB |
| `GET …/parts?limit=100` (`MAX_LIMIT`) | **17 ms** | 428 kB |

The budget is 80 ms. The server is five times inside it, and nothing about the query
degrades with library size — it is keyset, one round trip, thumbnails inline.

**So the grid is not slow. It is incomplete.** `fetchParts` asks for one page and never
asks for another: `PartsPage.next` is returned by the server, read by exactly one component
(`PageExtent`, which renders a sentence about truncation), and never used to fetch
anything. A library of 1,000 parts shows 50, and the other 950 are unreachable from the UI
at all. `ROADMAP.md`'s "every part appears" is the failing half of the exit criterion, and
it fails for a reason no amount of virtualization addresses.

That inverts the task. **Paging is the requirement; virtualization is the optimization it
enables**, and it gets built only as far as a measurement justifies.

## 2. Paging, and the cheapest thing that makes a thousand cards interactive

**`useInfiniteQuery` over the cursor the server already returns.** `PartsPage.next` is a
`PartId` or `null`, which is exactly `getNextPageParam`'s contract, and the route already
takes `after`. Nothing on the server changes.

**More pages are fetched by an `IntersectionObserver` sentinel after the last card**, not by
a button. A grid is a scrolling surface and the gesture that means "show me more" is
scrolling to the end of it. The observer is ~15 lines and needs no dependency.

**Virtualization is `content-visibility: auto`, not a virtualizer.** The ladder, honestly
walked:

- A virtualizer (`@tanstack/react-virtual`) is a dependency, a measured scroll container, and
  a fight with the responsive `repeat(auto-fill, minmax(11rem, 1fr))` grid this UI uses —
  the column count changes with the viewport, so row height and items-per-row have to be
  measured and re-measured rather than known.
- `content-visibility: auto` with `contain-intrinsic-size` is **one CSS rule** that tells
  the browser to skip layout, paint and image decode for off-screen cards, and it keeps the
  grid a plain CSS grid. It is the platform feature for exactly this, and `CLAUDE.md` says
  to prefer the boring option.

The card keeps its own intrinsic size so the scrollbar does not jump as cards enter and
leave — that is what `contain-intrinsic-size` is for, and omitting it is the mistake that
makes `content-visibility` look broken.

**This is a decision to re-measure, not to trust.** §7 records the number it has to beat; if
1,000 cards are not interactive with the CSS rule alone, the virtualizer is the answer and
the rule was one line to try first.

**`MAX_LIMIT` stays 100.** Ten requests for a thousand parts at 428 kB each is a scroll that
never stalls, and a larger page buys a longer first paint for a screen that shows twenty
cards.

## 3. The rung hash stops being dead bytes

`PartCard` carries `source_hash` and no derivative hash, so the L0 glTF every ingest writes
is unreachable: `GET /api/blob/{blake3}` has no possible caller, and ~7.5 MB per 1,000 parts
is written, reference-counted and never read. One field closes it.

`PartSummary.tessellation_l0: Option<BlobHash>`, carried onto `PartCard` the way
`source_hash` is, from a fourth LATERAL in `PgParts::page` keyed on
`DerivativeKind::TessellationL0` — the same shape as the thumbnail LATERAL beside it,
including why it is a LATERAL and not a join.

**It is a hash, not a URL, and holding it is not authorization.** `blob::by_hash` already
asks `PgBlobs::derivative_is_reachable` before serving anything, and that check is what
makes the field safe to hand out; this slice adds no route and weakens no check.

**Why now, with no viewer to use it.** Because the alternative is a slice that writes bytes
nothing can address, and because the detail route (§4) is the first honest caller: it shows
the rung's hash and size next to the source's, which is what tells a user their part has a
usable derivative at all.

## 4. The detail route

`web/` has one route. This is the second: `/parts/$partId`.

**One API call, one new route.** `GET /api/parts/{id}` answering a `PartDetail` — every
figure the card carries, plus what the card has no room for: bounding box, volume, surface
area, watertightness, measurement provenance per figure, revision label, format, kernel
version, and the source path §2 of slice 6a made the part's identity.

**`Approximate` is the type that carries provenance, and it is already in `lapidary-core`.**
Every mesh-derived figure goes over the wire as `{ value, provenance }` rather than as a
bare number with a badge computed beside it — `CLAUDE.md` makes the label non-negotiable
("Mesh-derived measurements are labelled 'approximate' in the UI, always"), and a bare
number with a separate boolean is how that label gets lost when B-rep figures arrive on the
same part in Phase 2.

**An open mesh reports no volume at all.** `MeshMeasurements::volume_approximate` already
returns `None` for a non-watertight mesh, because signed-volume integration over one means
nothing. The detail card says so in words rather than rendering a number or a blank.

**A 404 covers a deleted part and an id that names nothing**, undistinguished, exactly as
`derive.rs`'s `no_such_part` already does.

## 5. SSE, and the freeze it kills

`2026-09-05-phase-1-slice-5-HANDOFF.md` records the risk: *"The progress line freezes on a
hidden tab — react-query does not poll a hidden document. Slice 6's SSE work is where this
dies."* A user drops 1,000 files, switches tabs to do something else, and comes back to a
progress line stopped where they left it.

`GET /api/libraries/{library}/jobs/{batch}/events` streams `BatchStatus` as
`text/event-stream`. The browser keeps an `EventSource` open on a hidden tab, so the line
keeps moving and the grid keeps filling.

**It is `LISTEN/NOTIFY`, not a polling loop wearing an SSE costume.** `lapidary-db` already
re-exports `PgListener` for the worker loop, and slice 2's queue already `NOTIFY`s on the
`JOB_CHANNEL`. The stream wakes on the notification, re-reads `batch_status`, and sends the
row.

**With a floor, because a missed notification must not hang a progress bar forever.** The
same argument slice 2 wrote down for `LAPIDARY_JOB_POLL_SECS`: NOTIFY makes delivery fast,
the poll makes it correct. The stream re-reads on a timer as well as on a notification, and
the timer is what a dropped connection or a notification sent while reconnecting cannot
defeat.

**The stream ends itself when the batch does.** `finished_at` is set, one last event goes
out, and the response completes — the server closes rather than waiting for the client to
notice, because `EventSource` reconnects automatically and a stream that merely goes quiet
is one the browser will re-open forever. This is the same hazard `refetchInterval`
returning `false` closed for the poll, one layer down, and it gets the same test.

**The poll is not deleted.** `EventSource` has no `Authorization` header and no way to
signal a fatal error to the page, and a proxy that buffers `text/event-stream` breaks it
silently. The query stays as the fallback, armed when the stream errors, which also keeps
every existing progress test meaningful.

## 6. A first run is never an empty grid

`ROADMAP.md`: *"First run seeds a bundled licence-clean example part — never an empty
grid."*

The six parts in `example/parts/` are already licence-clean and already generated by
`example/parts/generate.py` — committed so the STLs have a source rather than being opaque
binaries. What is missing is that they only appear if someone runs a scan.

**One of them is seeded by migration, not by the worker.** A migration cannot mesh, so the
seeded part carries its measurements and its thumbnail as literals, generated once and
committed beside the STL. The part is real: a DN40 flange with the dimensions
`generate.py` produced.

**It is a normal part, not a special one.** No `is_example` column, no filter that hides it,
no first-run flag. A user who does not want it deletes it, which is a thing they can already
do to any part — and the row this writes is the same shape ingest writes, so nothing
downstream needs to know it arrived differently.

## 7. Acceptance

Numbers, because §1 shows this slice's premise was wrong once already:

- **Grid page under 80 ms warm** with 1,000 parts. Baseline 16 ms; the paging work must not
  regress it.
- **All 1,000 parts reachable** by scrolling, and the count on screen agrees with the
  library.
- **1,000 cards in the DOM stay interactive.** Measured in a real browser, not asserted.
  If `content-visibility: auto` does not carry it, §2 says the virtualizer is the answer.
- **The progress line moves on a hidden tab**, which is the one thing the poll cannot do
  and the reason SSE is in this slice.
- **`GET /api/blob/{blake3}` has a caller** — the detail route shows the L0 rung.
