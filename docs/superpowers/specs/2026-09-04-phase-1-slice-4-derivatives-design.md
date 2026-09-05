# Phase 1 slice 4 — derive less, and let a part carry a real picture

**Status:** design. Execution follows the plan built from this document.

Ingest currently produces four derivatives for every mesh, unconditionally. This slice
makes three of them optional or on-demand, and gives a part somewhere to keep a real
photograph.

---

## 1. Why this slice exists

Slice 3's exit run measured what ingest actually writes, across 151 real parts:

| Item | 151 parts | ~1,000 parts | Share of derivatives |
|---|---|---|---|
| L2 rungs | 52 MB | ~345 MB | 52% |
| L1 rungs | 41 MB | ~272 MB | 41% |
| L0 rungs | 7.3 MB | ~48 MB | 7% |
| Thumbnails (inline `bytea`) | ~4 MB | ~26 MB | 4% |

**L2 is a lossless re-encode of the source mesh.** It contains the same geometry the source
blob already holds, and source blobs are never deleted (`DATA.md` §1.5). Half of all
derivative storage is a duplicate of data the system keeps anyway.

The cost is not only disk. Every ingested file pays for a 512×512 software render — about
4 MB of transient buffers — plus three clustering passes and three glTF writes, multiplied
by worker concurrency. The owner's constraint, stated during slice 3b: *"not every computer
has this kind of free memory."* That machine has 15 GB, and the kernel's OOM killer fired
twice during slice 3b's development.

Separately, an auto-rendered grey three-quarter view is frequently not the picture a person
wants. A vendor photo, or the designer's own render, is often better, and there is nowhere
to put one.

**Outcome:** derivative storage drops roughly 93% (~665 MB → ~48 MB per 1,000 parts),
ingest stops rendering unless asked, and a part can carry an uploaded image that outranks
anything generated.

---

## 2. Scope

**In:**

- Ingest stores **L0 only**; L1 and L2 are built on demand
- `library.auto_thumbnail`, default true — a library may ingest without rendering
- On-demand derivation: one part, or a per-library sweep over everything missing one
- A `derive` job kind, and the generic enqueue the job queue has never had
- `blob.last_accessed_at` starts being written — the column exists (`0002_parts.sql:25`)
  and nothing has ever written it

**Out, with the trigger that brings each back:**

| Deferred | Trigger |
|---|---|
| **`part_image` upload** | Slice 5, alongside the rest of the image UI. The decisions are made here (§3.3, §3.4) because they were worked out here; the execution belongs beside the detail view and the upload control rather than in a slice that otherwise touches no UI |
| **Image by URL** | Its own slice. `DATA.md` §3.5 states the build order — "1. **User uploads a file.** Always works. 2. **User pastes an image URL.**" — `FEATURES.md:91-94` schedules URL plus SSRF controls at Phase 5, and there is no HTTP client in the workspace (`grep -c '^name = "reqwest"' Cargo.lock` → 0). Adding one touches `deny.toml`'s source allow-list and the air-gapped build claim, and deserves the review slice 3b got. The `origin` and `source_url` columns land now, so that slice is purely additive |
| Reclaiming L1/L2 rows already written | The render-cache eviction slice. An existing rung is not stale — it is a correct cache of a revision that still exists. Removing it is a space action, which `DATA.md` §1.5 already specifies with its own wording rules and its own quarantine machinery |
| A route listing a revision's derivatives | Phase 3. The viewer needs it regardless of this slice, and there is no viewer to test its shape against |
| Multiple images per part | No consumer. `unique (part_id)` now; a gallery is a schema change when something wants one |
| EXIF stripping, image orientation | Phase 5 with the rest of the image pipeline. Re-encoding through `image` already drops metadata as a side effect; relying on that deliberately is a separate decision |

**Explicitly not re-decided here:** the clustering algorithm, the glTF writer, the
derivative schema, `GET /api/blob/{blake3}` and the archive caps all shipped in slices 3
and 3b and this slice consumes them unchanged.

---

## 3. Decisions

### 3.1 Ingest stores L0 only

L0 is the grid's rung and the viewer's first paint. L1 and L2 are built when something asks
for them.

This **reverses `DATA.md` §2.1**, which says all rungs are generated at ingest and calls
lazy derivation "the tempting optimization that makes first open slow, which is the
impression that sticks". That ruling weighed first-open latency against nothing. It was
made before "must run on a modest workstation" was a stated constraint, and before the
measurement above existed.

The principle it leans on is one the same document already states. `DATA.md` §1.5:
derivative blobs are "never compress, **evict freely**; `kernel_version` + `params_json`
make regeneration deterministic". A derivative that may be evicted and regenerated is a
derivative that need not exist yet.

§2.1's warning is not dismissed — it is accepted as the price. First open of a part that
has never been opened pays for L1 and L2. The mitigation is that L0 is already there, so
the viewer paints immediately and refines.

### 3.2 Auto-thumbnail becomes a per-library setting, default on

`library` already carries `mode text NOT NULL DEFAULT 'hobby'` (`0002_parts.sql:7`), so
per-library behaviour is an established shape, and `CLAUDE.md` makes governance opt-in per
library.

Default **on**, because the alternative fails the product. Drop a folder of 1,000 STLs into
a library with rendering off and you get 1,000 blank cards — and a visual index whose
cards are blank is not one. Off is for the case the owner named: a large or low-powered
library where the render cost is not worth paying up front.

### 3.3 A user image is stored as bounded inline WebP, not by hash

*Decided here, executed in slice 5 — see §2.*

A user's photograph is never re-derivable, which puts it in `DATA.md` §1.1's **Source**
class. Source bytes live in `SourceStore`, which requires `WorkerRole::assume()` — and the
worker is unreachable from the browser (§3.5). `DerivativeStore` is not an escape hatch
either: `DATA.md` §1.5 says derivatives are evicted freely, so putting user data there
plants a data-loss bug inside the future "clear render cache" action that same section
promises.

So: re-encode to WebP under `MAX_THUMB_BYTES` and store inline as `bytea`, on exactly the
grounds `DATA.md` §1.5 already uses for thumbnails — "so they arrive in the same query as
the grid page instead of costing 100 filesystem round trips per scroll".

This is a deviation from `DATA.md` §3.2's schema sketch, which gives `part_image` a `blake3` column, and the spec says
so rather than quietly differing. It also costs nothing downstream: `PartCard.thumbnail`
stays a `data:image/webp;base64,…` string, so `parts.rs`'s conversion and the generated
TypeScript bindings do not change at all.

**The original upload is not retained**, only the bounded re-encode. Refusing to store
something at the boundary, and saying so, is not implicit deletion.

### 3.4 The URL is provenance, never a fetch target

*Decided here, executed in slice 5 — see §2.*

`DATA.md` §3.5 is explicit: *"User pastes an image URL. **Fetch once, store as a blob.
Never hotlink** — hosts rotate URLs and hotlinking leaks a referrer on every grid scroll."*

The `origin` and `source_url` columns land in this slice so the URL slice is additive, but
nothing in this slice fetches. `source_url` records where an image came from; it is never
what an `<img src>` points at.

### 3.5 Every trigger route is on `Role::Api`, and that is necessity, not preference

`deploy/web/Caddyfile` proxies `/api/*` to `api:8080` only, and `web/vite.config.ts` does
the same in development. There is no route from the browser to the worker. A route mounted
under `Role::Worker` is **unreachable from the UI** — the existing `POST /scan` is
documented as a `curl` from the host for exactly that reason.

Enqueueing a job is a database write, so a trigger route needs neither the kernel nor
`ingest_dir` and may live on the api side without touching the open-path boundary. The
rendering itself still happens in the worker, where `lapidary-cad` is linked;
`xtask check-layers` and `check-deploy` continue to forbid the alternative.

### 3.6 One job kind, discriminated by what to produce

`derive`, with payload `{ revision, produce }` where `produce` is a `DerivativeKind`.

Not two kinds. "Render a thumbnail for this revision" and "build L2 for this revision" are
the same procedure — resolve the source blob, parse, produce, upsert — differing only in
the last step. And `derivative_kind_unique_per_revision` (`0003_jobs.sql:62`) means the
write is an upsert keyed on `(revision_id, kind)`, so the discriminator *is* the derivative
kind. Two job kinds would need a second mapping between them.

`job.payload` becomes a typed enum in `lapidary-core`, which closes a ledger item slice 2
opened with precisely this trigger: *"`payload` is untyped `jsonb` | **The second job
kind.**"*

**That ledger item said `#[serde(tag = "kind")]`, and it is wrong.** Every job row written
so far holds a bare `{"path": …}` with no `kind` key, because `kind` is a separate
*column*. An internally-tagged enum fails on all of them with `missing field 'kind'` —
tested against a real row before this was written — so on upgrade every pending job would
become undeserialisable and the queue would stop draining.

The **column stays the discriminator**. The payload carries only the variant's fields, and
`to_json` for `IngestFile` emits exactly what `enqueue_scan` has always written, so old and
new rows are indistinguishable and no migration is needed.

### 3.7 The payload names a revision, not a part

A part-scoped payload would make the handler resolve "the latest revision" independently of
`PgParts::page`'s LATERAL, which resolves it too. Two resolutions that must agree is a bug
waiting for a second revision to exist: render revision B, show revision A's row.

Revision-scoped also gives Phase 3 the shape it needs — the viewer wants L2 *of the
revision it is displaying*, not of whatever is latest by the time the job runs.

### 3.8 `GET /api/blob/{blake3}` is untouched

A rung that has not been built has no hash, so "fetch, and build on a miss" is not
undesirable but unexpressible — there is nothing to put in the URL. Nor does opening a part
trigger a build: that would be a GET with a side effect, and it would put derivation policy
inside the crate whose whole purpose is not having any.

The client learns a rung is absent from a listing route, and asks for it with the same
enqueue route the thumbnail button uses. This slice ships the enqueue half; the listing
route is Phase 3's, because there is no viewer to test its shape against.

### 3.9 Nothing already written is deleted

The migration removes no row. An existing L1 or L2 is not stale — it is a correct cache of
a revision that still exists, and `CLAUDE.md` forbids implicit deletion. Reclaiming that
space is the "clear render cache" action `DATA.md` §1.5 already specifies, which needs
quarantine machinery that does not exist.

What this slice does is make that action *safe to write*: slice 3's ledger records "a
referenced derivative missing from disk logs but has no regeneration path". This slice
builds the regeneration path.

### 3.10 Access is tracked; the cold tier is not built

`blob.last_accessed_at` has existed since `0002_parts.sql:25` and nothing writes it. Every
age-based storage feature depends on it — the render-cache eviction `DATA.md` §1.5
promises, and the cold-compression tier §1.2 specifies. It starts being written here
because this is the slice that touches the read path anyway, and because a column that has
never been populated is worth nothing at the moment you first want it.

**The cold tier itself is deliberately not built.** `DATA.md` §1.2 says "zstd -3 at ingest
→ -19 when cold". Measured on 143 MB of the project's real STL corpus:

| Level | Compress | Size | Saved | Decompress |
|---|---|---|---|---|
| `-3` (today) | 0.72 s | 70.0 MB | 51% | 0.11 s |
| `-19` | 27.7 s | 64.0 MB | 55% | 0.14 s |

Thirty-eight times the compression CPU for four percentage points. §1.2's 6–10× figure is
for STEP, which is text; its own binary-STL estimate of ~2–2.5× is what the measurement
confirms (2.05× at -3, 2.24× at -19). Nearly all of the available win is already taken at
ingest. The tier waits for Phase 2's STEP ingest, where it may pay.

The measurement also settles a UX question before it is asked: **decompression is
level-independent and effectively free.** 143 MB in 0.11 s means a single 1 MB part
decompresses in about a millisecond, so opening a compressed part is indistinguishable
from opening a stored one and no "decompressing…" affordance is needed.

---

## 4. Architecture

### 4.1 Where each piece lives

| Piece | Crate | Why there |
|---|---|---|
| `DerivativeKind`, `JobPayload`, `Outcome::Rendered` | `lapidary-core` | The payload enum is the wire format; both the queue and the handler need it |
| `KernelParams.produce` | `lapidary-cad` | The kernel is told what to make |
| `part_image`, `upsert_derivative`, `revisions_missing` | `lapidary-db` | No SQL outside it |
| The `derive` handler arm | `lapidary-ingest` | It holds the `WorkerRole` proof and links the kernel |
| Trigger routes, image upload | `lapidary-api` | Enqueue is a DB write; upload never touches the blob store — §3.5 |

### 4.2 Routes by role

| Route | Role | Notes |
|---|---|---|
| `GET /api/libraries/{id}` | `Api` | New. Answers the `PATCH` body's own type. Added by ruling T9-B: without it the toggle renders §3.2's default rather than the library's setting, so a library already switched off shows on. `404` for a library that does not exist, as `PATCH` is |
| `PATCH /api/libraries/{id}` | `Api` | New. `{ autoThumbnail }` |
| `POST /api/parts/{id}/thumbnail` | `Api` | New. Enqueues one `derive`, returns `ScanAccepted` |
| `POST /api/libraries/{id}/thumbnails` | `Api` | New. Sweep; `queued: 0` is a success |
| `POST /api/parts/{id}/image` | `Api` | New. Multipart upload |
| `DELETE /api/parts/{id}/image` | `Api` | New. Explicit removal — `CLAUDE.md`'s carve-out from "never delete implicitly" |
| `GET /api/blob/{blake3}` | `Api` | Unchanged — §3.8 |
| `POST /api/libraries/{id}/scan` | `Worker` | Unchanged; it needs `ingest_dir` |

Every new route returns a `BatchId` where it enqueues, so the existing
`GET /api/libraries/{lib}/jobs/{batch}` polling works with no change.

---

## 5. Data flow

```
ingest a file
  │
  ├─ read library.auto_thumbnail
  │
  └─ kernel.process(bytes, KernelParams { format, produce })
                                            │
        produce = [TessellationL0]          │  auto_thumbnail = false
        produce = [Thumbnail, TessellationL0]  auto_thumbnail = true
                    │
                    ▼
        one or two derivatives written, never four


derive on demand   (POST a route, or the sweep)
  │
  └─ job { kind: "derive", payload: { revision, produce } }
        │
        ├─ latest source for that revision: file.blake3 + file.format
        ├─ SourceStore::get(hash, Compression::for_source_format(format))
        ├─ kernel.process(bytes, KernelParams { format, produce: [what] })
        └─ upsert derivative ON CONFLICT (revision_id, kind)
                    │
                    ▼
              Outcome::Rendered


the grid reads
  COALESCE(part_image.image_webp, derivative.thumb_bytes)   -- user image wins
```

---

## 6. Schema — migration `0005`

Three related facts, one migration, following `0004`'s precedent.

```sql
alter table library add column auto_thumbnail boolean not null default true;

-- PostgreSQL cannot modify a CHECK in place.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in ('ingested', 'skipped', 'rendered'));
```

**`part_image` is not created here.** Its shape is decided in §3.3, but the table lands in
slice 5's migration alongside the routes that write it — a migration should arrive with its
use, not four tasks ahead of it. `0001_init.sql`'s "Phase 2 and deliberately absent"
comment is therefore amended by slice 5, not this one.

**No row is deleted.** A test asserts that a database carrying pre-existing
`tessellation_l1`/`l2` rows still has them after `0005`.

---

## 7. Domain types

```rust
// lapidary-core
pub enum DerivativeKind { Thumbnail, TessellationL0, TessellationL1, TessellationL2 }
impl DerivativeKind { pub fn as_str(&self) -> &'static str; }   // the strings derivative.kind already holds

/// The `job.kind` COLUMN is the discriminator, not a key inside the payload — see §3.6.
pub enum JobPayload {
    IngestFile { path: String },
    Derive { revision: RevisionId, produce: DerivativeKind },
}
impl JobPayload {
    pub fn kind(&self) -> &'static str;                 // the column's value
    pub fn to_json(&self) -> serde_json::Value;         // the payload column's value
    pub fn from_row(kind: &str, payload: &serde_json::Value) -> Result<Self, CoreError>;
}

pub enum Outcome { Ingested, Skipped, Rendered }

// lapidary-cad
pub struct KernelParams { pub linear_deflection_mm: Option<f64>, pub format: String,
                          pub produce: Vec<DerivativeKind> }   // no Default — see below
pub struct KernelOutput { pub measurements: MeshMeasurements,
                          pub thumbnail_webp: Option<Vec<u8>>,
                          pub tessellations: Vec<Tessellation>,
                          pub entities: Vec<Entity> }
```

`KernelParams` loses `#[derive(Default)]` deliberately. A default `produce` is either
"everything", which is silently expensive, or "nothing", which silently produces no
derivatives. Removing it turns every construction site into a compile error that forces the
decision — which is the entire point of the field.

`tessellations` becomes a `Vec`. Slice 3 typed it `[Tessellation; 3]` so consumers would
index rather than search; that guarantee is spent the moment a call may ask for one rung.
`ladder()` is deleted with it — `cluster(mesh, lod)` is already public and is what both
callers want.

---

## 8. Error handling

| Condition | Result |
|---|---|
| A `derive` job for a revision with no source file | `Permanent` — the revision cannot be re-derived, and retrying will not change that |
| Source blob missing from disk | `Transient` — the store may be a mount that is not ready |
| Parse failure during re-derivation | `Permanent`, same as ingest: the bytes are immutable |
| Unknown `job.kind` | `Permanent`, **naming the kind**. Today the handler answers any unrecognised job with "This job has no file path in its payload", which is simply false |
| Upload larger than 10 MB | 413 before the body is read |
| Upload that is not PNG/JPEG/WebP by magic bytes | 415 |
| Upload that decodes past `image::Limits` | 413 — a decompression bomb, which `DATA.md` §4.1's control list names specifically |

---

## 9. Testing

The property most at risk is not "does it work" but "does ingest still write what we think".
Every assertion below names a specific number or string, because slice 3b produced five
tests that passed for the wrong reason and every one of them asserted only that an error
occurred or that a value came back.

- `auto_thumbnail = false` ingests and writes **zero** rows of `kind = 'thumbnail'`, and
  the part still appears in the grid — the `LEFT JOIN LATERAL` is what guarantees that, so
  it is asserted rather than assumed
- Ingest writes exactly **one** tessellation row, and it is `tessellation_l0`
- A `derive` job then writes the missing thumbnail and returns `Outcome::Rendered`
- Upserting twice with different bytes leaves **one** row carrying the second bytes
- `revisions_missing` returns exactly the revisions lacking a generated thumbnail
- Reading a blob updates `last_accessed_at`; reading it again moves the timestamp forward
- The trigger routes are **absent** under `Role::Worker`
- A sweep whose jobs all return `rendered` triggers exactly one grid refetch

---

## 10. Exit criterion

Ingest a library with `auto_thumbnail = false`: every part appears in the grid showing "No
preview yet", with zero thumbnail rows and exactly one tessellation row each. Derivative
bytes for the corpus drop from slice 3's measured ~100 MB per 151 parts to under 10 MB.

`POST /api/libraries/{id}/thumbnails` then fills them: the returned batch drains to
`finishedAt`, `rendered` equals the part count, and the grid refetches without a manual
reload.

An on-demand L2 for one part produces a rung a third-party glTF validator accepts. That is
an **exit-run step, not a suite step**, performed with Khronos `gltf-validator` as an
external tool exactly as slices 3 and 3b performed it. No validator exists in this repo,
and `triangles_in_glb` in `crates/lapidary-ingest/tests/handler.rs` is a *structural*
check — magic, container version, declared-length agreement, JSON chunk parse — so a rung
with a valid header and an invalid mesh passes it. Its absence from the suite is
deliberate; it is not a missing test.

Byte-identity is asked of **L0**, not L2 — a lazily-built rung must not differ from an
eager one, and L0 is the only rung built both ways. Ingest builds no L2 at all, which is
the point of this slice, so there is no eager L2 to compare a lazy one against and the
clause cannot be checked as it was originally worded.
`a_derived_l0_is_byte_identical_to_the_one_ingest_wrote` pins the version that can be: a
`derive` of L0 over an ingest-built L0 yields the same BLAKE3, adds no `blob` row, does not
inflate `ref_count`, and records the same `kernel_version`. The last of those is not
incidental — ingest asks the kernel for `[Thumbnail, L0]` and the derive asks for `[L0]`,
and `kernel.version()` names the build rather than the run, so it must not vary with
`produce`.

Ingest throughput is **measured and recorded**, not asserted. It should improve — two
clustering passes, two glTF writes and one render per file are gone — but the slice-3b
ceilings are still in force, so the comparison is against that run's 42.8 files/s, not
slice 3's 89.4.

---

## 11. Risks

**First open gets slower, and §2.1 warned about exactly this.** Accepted with eyes open.
The mitigation is that L0 exists, so the viewer has something to paint immediately. If
Phase 3 finds the refine step objectionable, the answer is a background sweep after
ingest — the machinery this slice builds — not a return to eager generation.

**Two derivative-producing paths must not drift.** Ingest and the `derive` handler both
call `kernel.process`. They differ only in `produce`, and §10 requires a lazily-built **L0**
to be byte-identical to an eager one, which is the test that catches drift. It has to be
L0: ingest builds no L2, so an L2 has no eager counterpart to be compared against.

**The sweep can enqueue thousands of jobs.** A library of 10,000 parts missing thumbnails
produces 10,000 jobs in one `INSERT ... SELECT`. That is the same shape `enqueue_scan`
already has, and the queue is designed for it — but the sweep should be the first thing
looked at if the queue ever misbehaves at scale.

**An image upload is the first route that accepts bytes from a browser.** Everything before
it was a read or an enqueue. The decode limits, the magic-byte check and the size cap are
not optional, and the slice-3b lesson applies: the test for a cap must be built so that
removing the cap fails it.

---

## 12. What this unblocks

Phase 3's viewer gets a system where a rung it needs may be absent, and a documented way to
ask for it. That is a better starting point than one where every rung exists and the
question never arose.

The render-cache eviction action `DATA.md` §1.5 promises becomes safe to write for the
first time, because the regeneration path it depends on now exists.

And the image-by-URL slice becomes purely additive: the table, the `origin` vocabulary and
the precedence rule all land here, so that slice adds one route and one job arm.
