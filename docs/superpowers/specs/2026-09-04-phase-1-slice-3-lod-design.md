# Phase 1, slice 3 — the LOD ladder, OBJ, and one kernel output

**Status:** design. Binding authority for slice 3.
**Predecessors:** `2026-09-02-phase-1-slice-1-ingest-design.md`,
`2026-09-03-phase-1-slice-2-jobs-design.md`.
**Plan:** `docs/superpowers/plans/2026-09-04-phase-1-slice-3-lod.md`.

---

## 1. Why this slice exists

Slice 1 made a part visible: one 512 px thumbnail, inline as `bytea`. Slice 2 made ingest
survive a crash. Neither produced anything a viewer can open. `docs/DATA.md` §2.1 says the
ladder is generated **at ingest**, and says why:

> Deriving lazily on first open is the tempting optimization that makes first open slow,
> which is the impression that sticks.

So the ladder is this slice's job, and it is the last thing standing between the grid and
Phase 3's viewer.

Three long-open items close here, and they are not incidental — each is a place the
codebase currently lies about itself:

- **`KernelOutput` and `MeshOutput` are two unreconciled types.** The `Kernel` trait returns
  `KernelOutput`; production ingest returns `MeshOutput` and does not implement the trait.
  `docs/superpowers/plans/2026-09-01-phase-0a-followups.md` item 2 has recorded this as open
  since Phase 0a, and both prior specs name slice 3 as its closer.
- **`DerivativeStore` has never run.** `grep` finds it outside its own tests only in doc
  comments. Thumbnails go inline and never touch it, so the entire derivative-blob path —
  store → `derivative.blake3` → an endpoint that serves it — is unexercised. LOD meshes
  exceed the 64 KB inline budget by orders of magnitude, so this slice is what lights it up.
- **`/api/blob/{blake3}` does not exist.** `DATA.md` §2.3 specifies it, with immutable
  caching and — the part that matters — an authorization check, because
  `CLAUDE.md` is explicit that content addressing is not authorization.

---

## 2. Scope

**In:**

- Vertex-clustering LOD generation in `lapidary-cad`, producing `L0`/`L1`/`L2`
- Uncompressed glTF 2.0 binary (`.glb`) output, hand-written
- OBJ ingest alongside STL, with format dispatch and a `kernel_version` that names the parser
- One reconciled kernel output type; `MeshKernel` implements `Kernel`
- LOD derivatives written through `DerivativeStore` and referenced by `derivative.blake3`
- Migration `0004`: the `derivative` storage-discriminator CHECK and the missing foreign key
- `GET /api/blob/{blake3}` under `Role::Api`, with reachability checked
- Generation budgets, measured rather than asserted

**Out, with the slice that covers each:**

| Deferred | Slice | Trigger |
|---|---|---|
| **3MF ingest** | **3b** | Immediately after this slice. It needs the workspace's first ZIP and first XML dependencies plus `DATA.md` §5.4's zip-bomb caps — a different review problem from geometry, and the only part of the roadmap's mesh-format line that is not dependency-free |
| meshopt (`EXT_meshopt_compression`) encoding | Phase 3 | When a viewer exists to decode it — see §3.2 |
| The viewer itself, LOD streaming, prefetch | Phase 3 | — |
| Browser upload, `variant=original` download | 4 | — |
| SSE, virtualized grid, seed part | 5 | — |
| `structure.json` / `entities.json` in the kernel output | Phase 2 | Mesh input yields no analytic entities; these arrive with STEP |
| Derivative cache eviction | Phase 8 | `DATA.md` §1.5's "clear render cache" action needs a fleet to be worth having |

**Explicitly not a goal:** matching a triangle budget exactly, topology-preserving
decimation, normals or materials in the output, or any LOD chosen at read time. One
clustering pass per rung, decided at ingest, is the whole policy.

---

## 3. Decisions

### 3.1 One indexing function, three grid sizes — and the primitive already exists

`crates/lapidary-cad/src/measure.rs` already quantises vertices and hashes them, to rebuild
the adjacency that STL's per-facet vertex duplication destroys:

```rust
fn key(v: [f32; 3]) -> u64 {
    // 1e-4 mm quantisation: finer than any real mesh tolerance, coarse enough to
    // collapse f32 representation noise at a shared corner.
```

**Vertex clustering is that same function at a coarser grid.** Map each vertex to a cell,
keep one representative per cell, rewrite each triangle's three corners to their cells, and
drop the triangles whose corners collapsed together. That gives the whole ladder from one
function:

| Rung | Grid | Effect |
|---|---|---|
| `L2` | 1e-4 mm, i.e. `measure.rs`'s existing quantisation | Lossless de-duplication. No triangle is dropped; the soup becomes indexed |
| `L1` | `bbox / 96` per axis | Lossy. Targets `DATA.md`'s ~50 k |
| `L0` | `bbox / 32` per axis | Lossy. Targets ~5 k |

Two consequences worth stating because they remove hazards rather than create them:

**`Mesh` stays an unindexed triangle soup.** Clustering does not need indexed input — it
*produces* it. The concern that a triangle soup is the wrong shape for decimation dissolves:
the soup is the input, an indexed mesh is the output, and glTF wants exactly that output.
No change to `Mesh`, no re-parse, no adjacency structure.

**The grid is relative to the bounding box, not absolute.** `prototype-notes.md` records
that `rust-mesh` clustered on a 48³ grid and that "the 48 constant was tuned by eye and
should become an L0/L1/L2 ladder". Sizing cells as a fraction of the bbox makes the ladder
scale-invariant: a 5 mm screw and a 2 m gantry get comparable triangle counts.

**Budgets are enforced by retry, following a pattern already in this crate.**
`raster.rs` retries the thumbnail at 384 px then 256 px when it exceeds
`MAX_THUMB_BYTES`. LOD generation does the same: if a rung's output exceeds its triangle
budget, halve the grid and retry, up to twice. `DATA.md`'s "~5 k" and "~50 k" are
approximate and the retry keeps them approximately true across a corpus where a fixed grid
would not. The final grid actually used is recorded in `params_json`, so the derivative says
how it was made.

### 3.2 Uncompressed GLB now; meshopt when there is something to decode it

`DATA.md` §2.2 chose meshopt over Draco and that choice is not reopened here:

> Draco compresses ~15–20% smaller; **meshopt decodes roughly an order of magnitude
> faster** and is SIMD-friendly. Decode latency dominates for visual triage.

But the decoder is Phase 3's viewer, and `EXT_meshopt_compression` is a real codec whose
Rust binding wraps C — which would put a C toolchain into the worker image and break the
constraint `prototype-notes.md` calls worth preserving:

> `rust-mesh` was dependency-free so it compiled offline in the container build — a
> constraint worth preserving.

Derivatives are designed to be thrown away. `DATA.md` §1.5: "never compress, **evict
freely**; `kernel_version` + `params_json` make regeneration deterministic." So this slice
writes plain glTF 2.0 binary and Phase 3 re-encodes. The cost is one regeneration pass over
data that is explicitly disposable; the saving is that the first geometry code in the tree
carries no third-party dependency.

glTF 2.0 binary is a 12-byte header, a JSON chunk and a binary chunk. One mesh, one
primitive, `POSITION` and indices, no materials or normals — the viewer computes normals
from winding, exactly as `raster.rs` already does. That is a few hundred lines of
hand-written code in a crate whose STL parser exists for the same reason.

### 3.3 `DATA.md`'s `kind` vocabulary wins

Code and docs currently disagree: `DATA.md` §3.2 names
`tessellation_l0|l1|l2|structure|entities|thumbnail`, while `db/tests/repo.rs` inserts a
`'lod0'` row. The docs win, because they are the ones Phase 2's `structure` and `entities`
rows have to fit alongside. The three new kinds are `tessellation_l0`, `tessellation_l1`,
`tessellation_l2`; `thumbnail` is unchanged.

`crates/lapidary-db/tests/repo.rs:450` inserts a `lod0` derivative and asserts the grid card
still shows the thumbnail — it exists to guard the LATERAL in `PgParts::page` against
fanning out when a revision has several derivatives. **That test predates this slice and
anticipates it.** It is updated to the real kind name and otherwise left alone; it is not a
mistake to be corrected.

### 3.4 The trait's `&Path` is the defect, not the code

`MeshKernel` does not implement `Kernel`. `grep -rn 'impl Kernel for'` returns exactly one
hit, `mock.rs`. Slice 1's plan said it would implement the trait "by reading the path and
delegating"; the shipped code refuses, and says why:

> Bytes rather than a path: ingest has already read and hashed the file, and reading it
> twice would be a second chance to read something different.

That reasoning is correct and the trait signature is what is wrong. `docs/README.md`: "Where
a spec and the code disagree, one of them is a defect — say which, in the spec." **The
defect is `Kernel::process(&self, src: &Path, …)`.** It becomes:

```rust
async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError>;
```

Hashing before parsing is not an optimization, it is the ordering the whole ingest pipeline
rests on — `handler.rs` hashes, short-circuits on `library_holds`, and only then parses. A
kernel that re-opens the path can disagree with the hash that was already committed.

Phase 0b's OCCT kernel is the one caller that might genuinely want a path, since OCCT reads
files itself. It writes the bytes to a scratch file inside the sidecar boundary, which is
the honest place for that concern: the sidecar already marshals across a process boundary.

### 3.5 One output type, carrying blob references

`ARCHITECTURE.md` already specifies the target shape, and `sidecar/occt-bridge/README.md`
says the current one is a placeholder:

> `KernelOutput`'s fields are not stable: Phase 0a's `{ triangle_count, bbox_mm, entities:
> Vec<String> }` is a placeholder, and Phase 0b will replace it with the richer shape
> `docs/ARCHITECTURE.md` already specifies — `{ tessellation_l0/l1/l2.glb, structure.json,
> entities.json }` — carrying blob references for the LOD ladder.

`MeshOutput` is deleted and `KernelOutput` becomes that shape, minus the two fields only a
B-rep kernel can fill:

```rust
pub struct KernelOutput {
    pub measurements: MeshMeasurements,
    pub thumbnail_webp: Vec<u8>,
    /// L0, L1, L2 as .glb bytes. Always three, in ascending detail.
    pub tessellations: [Tessellation; 3],
    /// Analytic B-rep entities. Empty for mesh input, and that emptiness is load-bearing:
    /// it is what stops tessellated numbers being presented as exact.
    pub entities: Vec<Entity>,
}
```

`entities` stays `Vec<Entity>` rather than `Vec<String>` so Phase 3's measurement can snap
to axes and radii instead of parsing `"CYLINDRICAL_SURFACE:22.000"`. `Entity` is defined in
Phase 2 when there is something to put in it; slice 3 declares it as an empty enum so the
field's type is right and the mesh-empty invariant is expressible today.

### 3.6 A small mesh still gets three rungs

A source under the L0 budget clusters to itself at every grid: the ladder is written anyway,
three rows, three blobs, possibly byte-identical. The alternative — write one row and let
the viewer fall back — puts a conditional into every consumer to save a few kilobytes on the
smallest parts in the library.

Content addressing makes the waste nearly free: three identical `.glb` files are one blob
with `ref_count` 3. The rows differ, the bytes do not.

### 3.7 `kernel_version` names the parser, and this is a correctness rule

`MeshKernel::version()` currently returns `format!("stl-1+{RASTER_VERSION}")`. Its own test
carries the reason:

```rust
// derivative.kernel_version must change when output bytes could change, or a
// regenerated thumbnail is indistinguishable from a stale one.
```

Once OBJ is ingested, a version that says `stl-1` for an OBJ-derived thumbnail is a lie of
exactly the kind that comment forbids. The version becomes
`{parser}-1+glb-1+{RASTER_VERSION}` — `stl-1+glb-1+cpu-1`, `obj-1+glb-1+cpu-1` — so it names
the parser, the tessellation writer and the rasterizer, each of which can change output
bytes independently.

### 3.8 Format dispatch is on the extension, and the walk decides

`is_stl_candidate` becomes `is_mesh_candidate`, matching `.stl` and `.obj`
case-insensitively. Dispatch happens once, in `lapidary-cad`, on the extension the scan
already used to select the file — not by sniffing bytes.

Sniffing is tempting and wrong here: OBJ is plain text with no magic number, and a
"sniffing" implementation reduces to guessing from the first non-comment line. The extension
is what the walk filtered on, so it is what the parser is told. A file whose extension lies
fails to parse and becomes a per-file `Permanent` failure with a message that says so, which
is the same treatment a corrupt STL already gets.

`file.format` stops being the SQL literal `'stl'`. `IngestRequest` gains a `format: &'a str`
field, and `insert_part_chain` binds it.

### 3.9 LOD blobs go to `DerivativeStore`, and the schema learns which storage a row uses

Thumbnails stay inline as `bytea` — `DATA.md` §1.5 is explicit that this is worth it. LOD
meshes cannot: they exceed `MAX_THUMB_BYTES` by orders of magnitude and belong in the
content-addressed store, which is exactly what `DerivativeStore` is for and has never done.

`derivative` currently permits nonsense. `blake3` and `thumb_bytes` are both nullable and
independent, so a row with neither, or both, is legal; and `blake3` is not a foreign key to
`blob`, though `file.blake3` is. Migration `0004` closes both — see §6.

---

## 4. Architecture

### 4.1 Where each piece lives

| Piece | Crate | Why there |
|---|---|---|
| `cluster`, the LOD generator | `lapidary-cad` | Geometry. Behind the kernel boundary, so the open path structurally cannot reach it |
| `glb`, the glTF writer | `lapidary-cad` | Same |
| `parse_obj` | `lapidary-cad` | Beside `parse_stl`, sharing its `finish` gate |
| Writing LOD blobs | `lapidary-ingest` | It already holds the `WorkerRole` proof and the blob roots |
| Three `derivative` rows | `lapidary-db` | No SQL outside `lapidary-db` |
| `GET /api/blob/{blake3}` | `lapidary-api` | The open path: it reads a derivative, never a source file, never the kernel |

`lapidary-api` reads derivative blobs, which is why `DerivativeStore` is readable by both
roles while `SourceStore` requires `WorkerRole::assume()`. `xtask check-deploy` already
fails if `lapidary-api` so much as names `SourceStore`.

### 4.2 Routes by role

| Route | Role | Notes |
|---|---|---|
| `GET /api/blob/{blake3}` | `Api` | New. Immutable caching, and a reachability check |
| `GET /api/libraries/{id}/parts` | `Api` | Unchanged |
| `GET /api/libraries/{lib}/jobs/{batch}` | `Api` | Unchanged |
| `POST /api/libraries/{id}/scan` | `Worker` | Unchanged |

The blob route's authorization is the point, not a formality. `DATA.md` §2.3:

> **But content addressing is not authorization.** Every blob request must verify the
> principal has access to a part referencing that blob in their tenant. Hashes leak into
> manifests, logs, bundles and support tickets — knowing one must never be a capability.

There is no auth in Phase 1, so "the principal has access" reduces to *the blob is
referenced by a `derivative` row of a part in a library that exists*. An unreferenced hash
is a 404 whether or not the bytes are on disk. That check is written now, while it is one
join, rather than retrofitted in Phase 8 when it is a security fix.

### 4.3 The ingest pipeline gains one step

Slice 1's ordering is unchanged through step 4. The kernel now returns tessellations, and a
new step writes them before the transaction:

1. read bytes
2. BLAKE3 — hash first, always
3. `blobs.library_holds(library, name, hash)`? yes → `Skipped`, no further work
4. `kernel.process(bytes, params)` — parse, measure, rasterize, **and cluster three rungs**
5. **`derivatives.put(glb)` × 3**, before the transaction, like `source.put` already is
6. link-or-put the source blob, then `ingest.record(...)` writing part, revision, file,
   thumbnail row and three tessellation rows in one transaction

Step 5 is outside the transaction for the same reason step 6's source write is: a filesystem
write cannot be rolled back by Postgres. The existing reap on failure extends to cover them —
`source.remove(hash)` gains three `derivatives.remove(hash)` calls on the error path, and
the same rule holds that a reap runs only on the branch that wrote.

Derivative blobs are content-addressed, so the reap must not remove a blob another revision
references. `ref_count` guards source blobs today; §6 records that derivatives get the same
guard, and §11 records the consequence of getting it wrong.

### 4.4 What `lapidary-cad` exposes afterwards

```rust
pub use cluster::{Lod, Tessellation, cluster};
pub use glb::write_glb;
pub use kernel::{CadError, Entity, Kernel, KernelOutput, KernelParams, KernelVersion};
pub use measure::measure;
pub use mesh_kernel::MeshKernel;
pub use obj::parse_obj;
pub use raster::{MAX_THUMB_BYTES, RASTER_VERSION, THUMB_PX, render_thumbnail};
pub use stl::{Mesh, parse_stl};
```

`MeshOutput` is gone.

---

## 5. Data flow

```
scan walks the ingest dir
  └─ is_mesh_candidate: *.stl, *.obj  (case-insensitive)
      └─ enqueue_scan  →  job rows, one per file          [slice 2, unchanged]

worker leases a job
  └─ IngestHandler::ingest_one
      1. fs::read                                          [unchanged]
      2. BLAKE3                                            [unchanged]
      3. library_holds? ── yes ──▶ Skipped                 [unchanged]
      4. MeshKernel::process(bytes, params)
           ├─ dispatch on extension → parse_stl | parse_obj
           ├─ measure(&mesh)          → MeshMeasurements
           ├─ render_thumbnail(&mesh) → WebP, ≤ 64 KB
           └─ cluster(&mesh, rung)    → Tessellation × 3
                └─ write_glb          → .glb bytes
      5. DerivativeStore::put × 3      → BlobHash × 3
      6. link_existing | (source.put → record)
           └─ one transaction: part, revision, file,
              derivative(thumbnail, inline bytea),
              derivative(tessellation_l0|l1|l2, blake3)

grid reads                                                 [unchanged]
  └─ PgParts::page — LATERAL picks kind='thumbnail' only

viewer reads                                               [Phase 3]
  └─ GET /api/blob/{blake3} — immutable, ETag, reachability-checked
```

---

## 6. Schema — migration `0004_derivative_storage.sql`

Two constraints the table should always have had, plus nothing else. No new tables and no
new columns: three tessellations are three rows of the existing shape.

```sql
-- A derivative is stored inline or by hash, never both and never neither. Both columns
-- have been nullable and independent since 0002, so a row with neither has been legal —
-- and a row with neither is a derivative that cannot be served, which is the failure this
-- slice would otherwise be the first to be able to produce.
alter table derivative add constraint derivative_storage_is_exclusive
    check ((blake3 is null) <> (thumb_bytes is null));

-- file.blake3 has referenced blob(blake3) since 0002; derivative.blake3 never has. The
-- LOD ladder is the first thing to write it, so it is the first thing that could write a
-- dangling one.
alter table derivative add constraint derivative_blake3_references_blob
    foreign key (blake3) references blob(blake3);
```

Both are `alter table` on a table whose only existing rows are thumbnails with
`thumb_bytes` set and `blake3` null — so both hold on existing data. The plan verifies that
against a database migrated from slice 2 rather than assuming it.

**The foreign key means each rung needs a `blob` row**, not just a file on disk. That is the
point rather than a side effect: it is what makes `ref_count` cover derivatives, and
`ref_count` is what makes §3.6's three-identical-rungs case cost one blob instead of three.
`insert_part_chain` writes them the way it already writes the source blob —
`INSERT … ON CONFLICT DO NOTHING` then `UPDATE blob SET ref_count = ref_count + 1`
(`repo.rs:113` and `repo.rs:200`).

A derivative blob row carries `zstd_level = NULL` and `stored_bytes = size_bytes`, because
`DATA.md` §1.2 says derivatives are never compressed. The column is already nullable, so
this needs no schema change — but it does mean `stored_bytes` is not evidence of
compression for every row, and the "show real disk usage" comment on it stays true only
because uncompressed rows report their real size.

---

## 7. Domain types

In `lapidary-cad`:

```rust
/// A rung of the ladder. Ordinal, not a triangle count: the count is an outcome of the
/// grid and the mesh, and DATA.md §2.1's figures are targets rather than guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod { L0, L1, L2 }

/// One rung's output.
#[derive(Debug, Clone, PartialEq)]
pub struct Tessellation {
    pub lod: Lod,
    /// glTF 2.0 binary. Uncompressed — see the design doc, section 3.2.
    pub glb: Vec<u8>,
    pub triangle_count: u32,
    /// The grid actually used, after any budget retry. Recorded in params_json so the
    /// derivative says how it was made and can be regenerated identically.
    pub grid: u32,
}

/// Analytic B-rep entities. Empty for mesh input, which is the invariant that stops
/// tessellated numbers being presented as exact. Phase 2 gives this variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Entity {}
```

In `lapidary-db`, `IngestRequest` gains two fields:

```rust
pub struct IngestRequest<'a> {
    pub library: LibraryId,
    pub name: &'a str,
    /// 'stl' or 'obj'. Was a SQL literal until slice 3.
    pub format: &'a str,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub thumbnail_webp: &'a [u8],
    /// (kind, hash) per rung, written as three derivative rows.
    pub tessellations: &'a [(String, BlobHash)],
    pub kernel_version: &'a str,
}
```

No new `lapidary-core` types and no new ts-rs bindings: nothing here reaches the wire in
this slice. The blob route returns bytes, not JSON.

---

## 8. Error handling

`CadError::MalformedStl { detail }` is renamed `MalformedMesh { format, detail }`. The
variant name and its user-facing message both say STL today, and an OBJ file failing to
parse must not be told to re-export an STL. The message keeps its shape — what broke, then
what to do:

> Could not read this OBJ — the file has no faces. Re-export it from your CAD or slicing
> tool and retry; if it came from a download, the transfer may have been cut short.

`cargo xtask check-strings` polices these literals, so the rewrite happens in one place.

A rung that cannot be written is `Permanent`, like a parse failure: the same bytes will
cluster the same way. A `DerivativeStore` write failure is `Transient` — the file is fine
and the disk was not. This matches `handler.rs`'s existing split, whose module doc states
the rule: kernel errors are permanent because content-addressed bytes reproduce, and I/O
errors are transient because something else was wrong.

Where genuinely unsure, `Transient`: a retried permanent failure costs one wasted parse; a
non-retried transient failure costs the user a file.

---

## 9. Testing

Every test below names the mutation it catches, because a test whose failure mode is not
known is a test nobody can trust.

**Clustering — `crates/lapidary-cad/src/cluster.rs`**

| Test | Mutation it catches |
|---|---|
| L2 of a cube is the same solid, de-duplicated to 8 vertices | Indexing that drops or duplicates a corner |
| L0 of the bracket fixture has strictly fewer triangles than L2 | A grid that is not actually coarser |
| A mesh under the L0 budget still yields three rungs | §3.6 collapsing to one |
| Every rung's triangles reference vertices that exist | An index buffer built from the wrong cell map |
| A degenerate triangle, all three corners in one cell, is dropped | Emitting zero-area triangles into the viewer |
| The budget retry halves the grid and records the grid it used | A `params_json` that describes a grid the output did not come from |

**glTF — `crates/lapidary-cad/src/glb.rs`**

| Test | Mutation it catches |
|---|---|
| The header is `glTF`, version 2, and `length` equals the buffer | A container no loader will open |
| Chunk padding is 4-byte aligned, JSON with spaces and BIN with zeros | Misalignment that only some loaders forgive |
| Accessor `min`/`max` equal the mesh's real bounds | Omitting the fields the spec requires on `POSITION` |
| A round trip through a parser recovers the triangle count | A writer that is self-consistently wrong |

**OBJ — `crates/lapidary-cad/src/obj.rs`**

| Test | Mutation it catches |
|---|---|
| A quad face is triangulated into two triangles | Dropping polygon faces silently |
| Negative (relative) vertex indices resolve | The most common real-file feature to forget |
| `v`/`f` with `vt`/`vn` present parses, ignoring the extras | A parser that chokes on textured exports |
| Comments, blank lines and CRLF are tolerated | Failing on files from Windows tools |
| A file with no faces fails with an actionable message | An empty `Mesh` reaching the rasterizer |

**Storage and schema — `crates/lapidary-db`**

| Test | Mutation it catches |
|---|---|
| A derivative with both `blake3` and `thumb_bytes` is rejected | The CHECK not applied |
| A derivative with neither is rejected | The CHECK written as `or` rather than `<>` |
| A `blake3` naming no blob is rejected | The FK not applied |
| Three tessellation rows plus a thumbnail coexist on one revision | The `(revision_id, kind)` unique constraint mis-scoped |
| `PgParts::page` returns one row per part with four derivatives present | The LATERAL fanning out — the existing `repo.rs:450` test, updated |

**The blob route — `crates/lapidary-api/tests/blob.rs`**

| Test | Mutation it catches |
|---|---|
| A referenced hash returns the bytes with `ETag` and `immutable` | Caching headers omitted, costing every repeat open a transfer |
| A hash on disk but referenced by nothing is 404 | §4.2's reachability check removed — the security case |
| An unknown hash is 404 with the same body as the previous case | A response that confirms a blob exists |
| `Role::Worker` does not serve it | The route mounted unconditionally |

**End to end — `crates/lapidary-ingest/tests/handler.rs`**

| Test | Mutation it catches |
|---|---|
| A real STL yields a thumbnail row and three tessellation rows | Rungs computed but not persisted |
| A real OBJ yields the same, with `format = 'obj'` | The `'stl'` literal surviving |
| `kernel_version` differs between an STL and an OBJ ingest | §3.7's version rule reduced to cosmetics |
| A failure after the rungs are written leaves no orphan blob | The reap not extended to derivatives |

---

## 10. Exit criterion

Scan a directory holding the six repository fixtures plus one OBJ. Every part has four
derivative rows: one `thumbnail` inline, three `tessellation_l*` by hash. `GET
/api/blob/{blake3}` returns each `.glb` with an `ETag` and an immutable `Cache-Control`,
and returns 404 for a hash that is on disk but referenced by nothing.

Each `.glb` opens in an independent glTF validator. `L0` has fewer triangles than `L1`,
which has fewer than or equal to `L2`, and `L2`'s count equals the source mesh's.

Re-scanning the directory drains to `skipped`, writing no new blobs — the slice-2
short-circuit still holding with four times the derivative work behind it.

Against 150 real STLs on the live stack: ingest throughput stays within **3×** of slice 2's
measured 74 files/s, and the grid page stays under `DATA.md` §2.5's 80 ms warm — the grid
reads only the thumbnail, so the ladder must not slow it at all. Both are measured and
recorded in the handoff, not asserted.

---

## 11. Risks

**The ladder is written for a consumer that does not exist.** Phase 3's viewer is the first
thing to open a `.glb` this slice produces, and it is two phases away. Mitigated by
validating against an independent glTF validator rather than a round trip through our own
writer — a self-consistent writer that is wrong passes its own reader every time. Accepted:
some shape of the output will be wrong for the viewer, and derivatives are regenerable
precisely so that is cheap.

**Vertex clustering damages thin features.** A 1 mm wall in a 200 mm part vanishes at the
L0 grid. This is inherent to clustering and is why `prototype-notes.md` calls it "the right
*first* LOD algorithm" — it degrades gracefully on malformed meshes, which real libraries
are full of, at the cost of fidelity on thin ones. L0 is a hover preview; L2 is what
measurement reads, and L2 is lossless. Accepted, recorded here so the next person does not
rediscover it as a bug.

**Three derivative writes per file, on the ingest path.** At slice 2's measured 74 files/s
this is the first change to make a job meaningfully longer. The 60 s lease has wide headroom,
but slice 2's ledger already records that lease heartbeats arrive when a job can outlive its
lease, and this slice moves that trigger closer. Measured at the exit criterion rather than
predicted.

**The reap now spans two stores.** A failure between writing three derivative blobs and
committing the transaction must remove exactly what it wrote and nothing else. The prototype
shipped an orphan-blob bug of this shape — `docs/prototype-notes.md` records it — and slice
1 fixed it for source blobs. `ref_count` is what makes the derivative case safe when two
revisions share a rung's bytes, which §3.6 makes routine rather than rare.

**`params_json` grows a meaning.** It is `{"px": 512}` for thumbnails today and becomes
`{"grid": 32, "budget": 5000}` for tessellations. Two shapes in one untyped column is the
same weak typing slice 2 recorded for `job.payload`, with the same trigger: the moment a
third shape appears, it becomes a tagged enum in `lapidary-core`.

---

## 12. What this unblocks

Slice 3b adds 3MF, reusing this slice's format dispatch and `MalformedMesh` — the seam is
the only thing 3MF needs that this slice builds.

Phase 3's viewer has something to open. `GET /api/blob/{blake3}` is the endpoint
`PartSummary.thumbnail`'s doc comment has been waiting for since slice 1, and the LOD rows
are what `DATA.md` §2.4's prefetch-on-intent reads.

Phase 2's STEP ingest inherits a `KernelOutput` already shaped for it: `entities` is a real
type with no variants rather than a `Vec<String>` to be replaced, and `tessellations` is
where OCCT's output lands unchanged. Phase 0a follow-up item 2 closes here.
