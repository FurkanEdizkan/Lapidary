# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Tauri is Phase 10 of `docs/ROADMAP.md` and bundles only our own binaries — never Postgres.
It is a later shell around this same web interface, not a second design language, so the
platform of record stays `web`.

## Users

**Primary: the maker with a large personal mesh library.** Hundreds to a few thousand STL
files on a NAS or a local disk, accumulated from Printables, Thingiverse, MakerWorld and
their own modelling, organised by folder and increasingly unfindable. They are looking for
"the bracket I printed last spring", they will decide from a picture in under a second, and
they open the result in Orca, Blender or FreeCAD.

**Secondary: the engineer with an industrial mesh library.** The same scanning job at a
different scale and under different rules — revisions, approvals, locks, air-gapped
deployment. Phase 8 and the `controlled` library mode exist for them. **A STEP or IGES file
is refused at ingest, by name** (owner, 2026-10-03): they export a mesh from the CAD tool
that owns the part, and the CAD file stays in the system that is built for it. That is a
real narrowing of this audience and it was taken deliberately — the kernel that read B-rep
cost 766 s of an 849 s build and existed to feed a renderer this product no longer has.

**The tie-break, decided 2026-09-08: the hobbyist leads.** Where the two audiences pull
apart on a design decision — density against delight, ceremony against speed,
approachability against precision — the maker wins and the engineer inherits a tool that
also happens to scale. This does not soften any correctness rule: "figures must not lie"
and the deletion guarantees are constraints, not register.

## Product Purpose

A visual index for 3D part libraries, with a return path. Scan hundreds of parts, decide if
one fits, hand it to the tool that actually edits it, and get the result back versioned.

Lapidary is explicitly **not** a CAD editor, not a slicer, and not a PDM system. It is the
layer that makes parts you already have findable, inspectable, and reachable from the tools
you already use. Success is a person finding a part they had lost and getting it into the
right application without thinking about the file.

## Positioning

Three things a neighbouring product cannot truthfully copy without becoming this one:

- **The return path.** Every other viewer is a dead end — you look, you download, and
  whatever happens next is untracked. Lapidary hands a part out and takes the edited result
  back as a versioned, content-addressed snapshot.
- **Meshes in, meshes out.** The library takes `stl`, `obj` and `3mf`, and `Target`
  negotiation hands a slicer the part's own file when it is already in a format the slicer
  reads, or a `*.lapidary.*` export written from its mesh when it is not. Nothing is
  silently converted: `variant=original` is byte-identical to what was ingested, verified
  against the stored BLAKE3 on the way out. *(Until 2026-10-03 this read "CAD gets STEP, the
  viewer gets glTF"; the kernel that made the first true and the pipeline that made the
  second are both gone.)*
- **Governance is opt-in per library.** Hobby libraries have no revisions, states or
  approvals at all. The same binary serves a folder of printed brackets and a controlled
  industrial catalogue, because the machinery is flipped on per library rather than sold as
  a different product.

The application is free and complete under AGPL-3.0 — no gated features in the app. Revenue
is the server, the worker fleet, support and cloud (`docs/ROADMAP.md` § Commercial model:
Local free, Team, Enterprise, Cloud).

## Operating Context

- **Container-first**, Podman and Docker. Three services split by port: web on 3000, api on
  8080, worker on 8081 — and peer on 8082, only with sharing switched on. A route only the
  worker serves is a route no browser can reach.
- **Air-gapped industrial deployments are a real target**, which is why the plain stack makes
  one outbound request, the image fetch a person pastes a URL into, and why uploading a picture
  always works without it. Sharing is the only other thing that reaches another machine, and it
  is off until switched on: the `peer` service ships as an overlay
  (`deploy/compose.sharing.yaml`), accepts only installations somebody paired by hand or accepted
  an introduction to, and talks only to those: a hello every 15 s, a read of what they share when
  it changes, the roster of each folder they share with this one, a question about who has a file
  before it is fetched, and the files a person asked to pull (owner's decisions, 2026-09-16 and
  2026-09-17). Every one of those goes to a machine on that list and to no other: there is no
  directory, no tracker and no discovery of machines nobody has paired with.
- **Parts arrive** either from a directory mounted into the worker or from a browser drop
  (client-side BLAKE3 first, then a probe, then resumable chunked upload).
- **Parts leave** to Rhino, Fusion, FreeCAD, Blender and Orca. Those are the tools the user
  already has open; Lapidary never competes with them.
- **The store is meant to be opened in a file manager.** One directory per model, a folder
  tree mirrored on disk, and a path the user can paste. That legibility is a product
  promise, not an implementation detail.
- PostgreSQL 18 is the only datastore.

## Capabilities and Constraints

Confirmed and load-bearing (`CLAUDE.md` holds the authoritative list):

- **Geometry is never edited.** Catalogue, picture, search, route, version.
- **Figures must not lie.** Every figure now comes from reading a mesh, so every one of them
  is approximate and **labelled so in the UI, always**, wherever it appears. There is no
  analytic half any more, which makes the rule simpler rather than weaker: if a number
  reaches a person without its `≈`, that is a bug.
- **Deletion is three steps.** Delete is soft, purge is separate and explicit, blobs
  quarantine 30 days before removal. Derivative cache eviction is a different action with
  different wording and must never read as data loss.
- **Downloads are never silently converted.** `variant=original` returns byte-identical
  ingested bytes; anything produced is named `*.lapidary.*`.
- **Versioning is Perforce-shaped, not Git-shaped.** Immutable content-addressed snapshots,
  lineage DAG, pessimistic locks. No merge, no branches, no textual diff. A revision's
  **figures** are compared; the geometric diff that compared shapes went with the render
  pipeline.
- **Opening a part reads metadata and pictures, never a model file.** The one exception is
  deliberate and asked for: pressing **view** hands the browser the original mesh to draw,
  which is the whole point of the button. Nothing loads a model because a page opened or a
  pointer moved.
- Phase 1 ships ingest, folder tree, grid, search, images and source links. Phases 2–10 are
  sequenced in `docs/ROADMAP.md`.

**Open product decisions, recorded rather than invented:**

- **Multi-user is committed but not built** (decided 2026-09-08). There is no auth, no user
  table and no session table today. Several shipped decisions were taken *because* of that
  absence and are now explicitly interim rather than final — grid page size and card
  density live in per-viewer browser storage (`web/src/lib/preferences.ts`), and
  `docs/FEATURES.md` records that row as "per viewer, per library". Future design should
  anticipate identity: shared preferences, attribution, permissions and locks. The existing
  code is not wrong; it is honest about a gap that is now scheduled to close.
- `part` has no tags or materials columns. Search covers names and part numbers; the
  user-defined fields Phase 5 owns are where tags and materials would live.
- **Settled, and the other way round:** a grid card shows the part's own first image, framed
  as its row says, and a part nobody has photographed names its format rather than
  apologising for a picture it does not have. The grid carries only inline bytes, so an image
  over `INLINE_LIMIT` arrives as a hash and the card emits `/api/blob/{hash}` — one immutable
  image load, never a second API request.
- **Nothing produces a picture.** Every one is something a person added, which makes an
  unphotographed library a real state of the product rather than a transitional one. Whether
  that wants a bulk way in — a folder of images matched to parts by name, say — is open.

## Brand Commitments

- **Name:** Lapidary. Licence AGPL-3.0.
- **Dark only. No light mode.** A binding constraint, not a default.
- **Motion is mechanical** and respects `prefers-reduced-motion`; the exact durations and
  curve are pinned in `CLAUDE.md` § Style.
- **No bare user-facing strings.** Every string routes through `web/src/lib/strings.ts`,
  enforced by an AST gate (`web/src/no-bare-strings.test.ts`). English only today; Turkish
  is the planned second locale, so nothing may assume English word order or length.
- **Errors say what broke and what to do.** "Lapidary is a visual library of meshes and does
  not read STEP files. Export the part as STL, OBJ or 3MF from your CAD tool and add it
  again; the same file is refused the same way every time." Never "parse failed (3)."

- **Real content in every example and fixture.** Plausible part numbers, real dimensions.
  Never "Part 1 / Part 2".
- **Prefer the boring option.** Solo-maintained, and destined for air-gapped industrial
  environments: fewer dependencies, fewer moving parts, more explicit failure modes.

## Evidence on Hand

Real, and usable without fabrication:

- **A measured Phase 1 exit criterion** (`docs/ROADMAP.md`, measured 2026-09-07) against the
  owner's own library — 1,095 STL files, 15.67 GB: ingest 130.3 s (8.4 files/s, ~120 MB/s);
  re-drop 40.1 s with 1,094 skipped and 0 re-ingested; warm grid page load 5.3 ms median at
  page size 50 and 54.7 ms at 500; 1,095 of 1,095 parts visible after one fix.
- **A 156-part development corpus** on the running stack, and a 320 GB / 1,703-mesh test
  corpus for scale work.
- **Three design prototypes** in `design/` (`Lapidary.dc.html`, `v1`, `v2 (mono)`) with
  `design/DESIGN_README.md`, plus `docs/prototype-notes.md` recording what the discarded
  Node/Fastify prototype established.
- **A knowledge graph of the codebase** at `graphify-out/` — `wiki/index.md` is the
  agent-crawlable entry point, 2,807 nodes across 125 named communities.

Absences future work must **not** fill in: the product is **pre-alpha** and has no users, no
customers, no testimonials, no press, no case studies, no pricing in market, and no
benchmarks other than the measured criterion above. There is no logo or brand mark. Nothing
may claim deployment, adoption, or a security audit.

## Product Principles

1. **The picture is the interface.** A person decides from a picture in under a second;
   everything else on a card is there to confirm that decision, not to compete with it. This
   is the principle the 2026-10-03 pivot was decided on: a preview we render is complexity
   bought for a picture we can simply store.
2. **Never lie about what is on disk.** Figures carry their provenance, totals name what
   they exclude, and no wording lets an eviction read as data loss.
3. **The store stays legible.** A user can open the storage root in a file manager and
   understand it. Any change that trades that away for internal convenience is the wrong
   trade.
4. **Hand off, don't hold on.** Lapidary's job ends where a real tool's begins, and starts
   again when the result comes back. It never grows into the editor.
5. **Free and complete, honestly.** No feature is withheld from the local application to
   sell a tier. What is sold is scale and operation.

## Accessibility & Inclusion

**WCAG 2.2 AA is the stated target** (decided 2026-09-08). Future surfaces are measured
against it rather than against the current craft floor.

Already in place and not to be regressed: focus-trapped, portalled, Escape-dismissable
dialogs with a11y tests; `prefers-reduced-motion` honoured in `web/src/styles.css`; real
semantics over ARIA patches — a card's name is a real link so it survives the keyboard,
middle-click and a screen reader, and the file picker is a real input behind a labelled
button. Dark-only means contrast has one theme to satisfy rather than two, which removes an
excuse rather than a requirement.
