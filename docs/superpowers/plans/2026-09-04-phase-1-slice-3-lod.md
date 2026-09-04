# Phase 1 slice 3 — the LOD ladder, OBJ, one kernel output: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** every ingested mesh gets three glTF rungs alongside its thumbnail, stored by hash
and served over an authorized blob route, so Phase 3's viewer has something to open.

**Architecture:** one indexing function at three grid sizes produces the ladder — `L2` at
`measure.rs`'s existing 1e-4 mm quantisation is lossless de-duplication, `L1` and `L0` at
`bbox/96` and `bbox/32` are the lossy rungs. Output is hand-written uncompressed glTF 2.0
binary. `MeshOutput` is deleted, `KernelOutput` becomes the one output type, and the `Kernel`
trait starts taking bytes because the shipped code's refusal to re-read the file is right.

**Tech Stack:** Rust 1.95.0 edition 2024, no new dependencies. axum 0.8.9, sqlx 0.9.0
against PostgreSQL 18.

**Spec:** `docs/superpowers/specs/2026-09-04-phase-1-slice-3-lod-design.md` — read it first.
Every "why" below is argued there; this plan is the "how".

## Global Constraints

Copied from `CLAUDE.md` and the spec. Every task's requirements implicitly include this
section.

- **No new dependencies.** This slice adds none — not for clustering, not for glTF. If a
  task feels like it needs one, it is the wrong task; re-read spec §3.2.
- **No SQL outside `lapidary-db`.** The three derivative rows are no exception.
- **Layering, CI-enforced by `cargo xtask check-layers`:** `lapidary-api` may never depend
  on `lapidary-cad`. The blob route reads derivative rows and derivative blobs, never a
  source file and never the kernel.
- **`lapidary-api` must never name `SourceStore`** — `check-deploy`'s open-path rule greps
  for the literal.
- **We never delete user data implicitly.** The reap in task 9 removes only blobs the same
  call wrote, and only on the branch that wrote them.
- **Errors say what broke and what to do.** "Could not read this OBJ — the file has no
  faces. Re-export it from your CAD or slicing tool and retry." Not "parse failed (3)".
- **Rust:** `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside
  tests**; the workspace lint denies it.
- **`cargo xtask check-strings`** scans every new string literal for runs of three or more
  spaces. Write continuation strings with a real `\` and no alignment padding inside
  literals — pad at runtime if a column is wanted.
- **Real content in fixtures.** The OBJ fixture is a real part with a plausible number, not
  `cube.obj` with three vertices.
- **Commit messages** pass `cargo xtask check-commit-msg`: Conventional Commits, a closed
  type list, and no AI attribution trailer.
- **When unsure, prefer the boring option.**

## The verification bar

Exactly what `.github/workflows/ci.yml` runs. A task is not done until it passes. **Never
pipe these through `tail` or `grep` when the exit code matters** — that mistake was made
twice in slice 1 and reported success both times. Use `; echo "exit=$?"`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo xtask check-strings
cargo xtask export-bindings      # must exit 0 AND leave web/src/bindings/ unchanged
cargo xtask export-agents-md     # must exit 0 AND leave AGENTS.md unchanged
cargo test --workspace --all-features
cargo deny check
cd web && npm test && npm run typecheck && npm run build
```

Tests need a live PostgreSQL 18. Either runtime:

```sh
docker run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/postgres:18
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```

Check it is **reachable**, not merely up — a dead rootless port forwarder cost a session:

```sh
bash -c 'cat < /dev/null > /dev/tcp/127.0.0.1/55432' && echo reachable
```

Baseline at the start of this slice: **287 passed / 0 failed**, web 33 passed.

## File structure

| File | Responsibility |
|---|---|
| `crates/lapidary-db/migrations/0004_derivative_storage.sql` | **Create.** The storage-exclusivity CHECK and the missing blob foreign key. |
| `crates/lapidary-cad/src/cluster.rs` | **Create.** `Lod`, `Tessellation`, `cluster` — one indexing function, three grids. |
| `crates/lapidary-cad/src/glb.rs` | **Create.** `write_glb` — glTF 2.0 binary, uncompressed. |
| `crates/lapidary-cad/src/obj.rs` | **Create.** `parse_obj`, sharing `stl.rs`'s `finish` gate. |
| `crates/lapidary-cad/src/kernel.rs` | **Modify.** `KernelOutput` reshaped, `Entity` added, `process` takes bytes, `MalformedStl` → `MalformedMesh`. |
| `crates/lapidary-cad/src/mesh_kernel.rs` | **Modify.** `MeshOutput` deleted; `MeshKernel` implements `Kernel`, dispatches on format, produces the ladder. |
| `crates/lapidary-cad/src/mock.rs` | **Modify.** Follows the trait's new signature and output. |
| `crates/lapidary-ingest/src/scan.rs` | **Modify.** `is_stl_candidate` → `is_mesh_candidate`. |
| `crates/lapidary-ingest/src/handler.rs` | **Modify.** Writes three derivative blobs, extends the reap, passes format and tessellations. |
| `crates/lapidary-db/src/repo.rs` | **Modify.** `IngestRequest` gains `format` and `tessellations`; `insert_part_chain` writes blob rows and three derivative rows. |
| `crates/lapidary-api/src/blob.rs` | **Create.** `GET /api/blob/{blake3}`. |
| `crates/lapidary-api/src/lib.rs` | **Modify.** Mount it under `Role::Api`. |
| `fixtures/idler-bracket-lp-2210-01.obj` | **Create.** A real OBJ part. |

---

### Task 1: Migration 0004 — what `derivative` should always have enforced

**Files:**
- Create: `crates/lapidary-db/migrations/0004_derivative_storage.sql`
- Test: `crates/lapidary-db/tests/repo.rs` (modify)

**Read first:** spec §6, and `0002_parts.sql:96-107` for the table as it stands.

Both constraints hold on existing data — every derivative written so far is a thumbnail with
`thumb_bytes` set and `blake3` null — but that must be *verified against a migrated
database*, not assumed, because a migration that fails on real data fails at deploy rather
than in CI.

- [ ] **Step 1: Write the migration**

```sql
-- A derivative is stored inline or by hash, never both and never neither. Both columns
-- have been nullable and independent since 0002, so a row with neither has been legal --
-- and a row with neither is a derivative that cannot be served, which the LOD ladder is
-- the first thing able to produce.
alter table derivative add constraint derivative_storage_is_exclusive
    check ((blake3 is null) <> (thumb_bytes is null));

-- file.blake3 has referenced blob(blake3) since 0002; derivative.blake3 never has. The
-- ladder is the first thing to write that column, so it is the first thing that could
-- write a dangling one.
alter table derivative add constraint derivative_blake3_references_blob
    foreign key (blake3) references blob(blake3);
```

- [ ] **Step 2: Write the failing tests**

In `crates/lapidary-db/tests/repo.rs`, four cases. Each inserts directly rather than through
`PgIngest`, because the point is what the *database* refuses:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_with_both_storage_columns_is_rejected(pool: PgPool) { /* ... */ }

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_with_neither_storage_column_is_rejected(pool: PgPool) { /* ... */ }

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_naming_no_blob_is_rejected(pool: PgPool) { /* ... */ }

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_naming_a_real_blob_is_accepted(pool: PgPool) { /* ... */ }
```

The fourth is not filler: without it, a CHECK written as `and` instead of `<>` would pass
the three negative cases and reject everything real.

- [ ] **Step 3: Prove it holds on data written the old way**

```sh
# migrate to 0003, ingest through the real path, then migrate to 0004
cargo test -p lapidary-ingest --test handler; echo "exit=$?"
cargo test -p lapidary-db; echo "exit=$?"
```

`#[sqlx::test]` runs every migration in order against a fresh database, so a constraint that
contradicted the rows `insert_part_chain` writes would fail every one of
`lapidary-ingest`'s handler tests. That is the check — it needs no separate harness.

- [ ] **Step 4: Verify**

Change the CHECK to `((blake3 is null) or (thumb_bytes is null))`;
`a_derivative_with_neither_storage_column_is_rejected` must fail. Revert.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): make a derivative say which storage it uses"
```

---

### Task 2: The kernel's types and its signature

**Files:**
- Modify: `crates/lapidary-cad/src/kernel.rs`, `crates/lapidary-cad/src/mock.rs`,
  `crates/lapidary-cad/src/lib.rs`

**Read first:** spec §3.4, §3.5, §7.

This task changes the trait and leaves `MeshKernel` alone — `MeshOutput` still exists and
`handler.rs` still compiles against it. The workspace stays green because the only
implementor of `Kernel` is `MockKernel`.

- [ ] **Step 1: Reshape `KernelOutput` and add `Entity`**

```rust
/// Analytic B-rep entities. Empty for mesh input, and that emptiness is load-bearing: it
/// is what stops tessellated numbers being presented as exact. Phase 2's STEP ingest gives
/// this variants — axes, radii, normals — which is why it is a type rather than the
/// `Vec<String>` it was, where measurement would have had to parse
/// `"CYLINDRICAL_SURFACE:22.000"` to snap to anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Entity {}

pub struct KernelOutput {
    pub measurements: MeshMeasurements,
    pub thumbnail_webp: Vec<u8>,
    /// L0, L1, L2 in ascending detail. Always three — see the design doc, section 3.6.
    pub tessellations: [Tessellation; 3],
    pub entities: Vec<Entity>,
}
```

- [ ] **Step 2: Change the trait to take bytes**

```rust
    async fn process(&self, bytes: &[u8], params: &KernelParams) -> Result<KernelOutput, CadError>;
```

Carry the reasoning onto the trait itself, because the next person to read it will wonder
why a kernel does not take a path:

```rust
/// Bytes, not a path. Ingest has already read and hashed the file, and reading it twice
/// would be a second chance to read something different — the hash is committed before
/// the parse, so a kernel that re-opens the path can disagree with what was recorded.
/// Phase 0b's OCCT kernel writes the bytes to a scratch file inside the sidecar, which is
/// where that concern belongs: the sidecar already marshals across a process boundary.
```

- [ ] **Step 3: Rename the error variant**

`MalformedStl { detail }` → `MalformedMesh { format: String, detail: String }`, and the
message names the format it was given. `cargo xtask check-strings` scans the new literal.

- [ ] **Step 4: Update `MockKernel`**

It gains three fixture tessellations. They need not be valid glTF — this task has no writer
yet — but they must be distinguishable from each other so a later test cannot pass by
returning the same rung three times.

- [ ] **Step 5: Run**

```sh
cargo test -p lapidary-cad; echo "exit=$?"
cargo clippy -p lapidary-cad --all-targets --all-features -- -D warnings; echo "exit=$?"
```

- [ ] **Step 6: Verify**

`mock_kernel_reports_an_actionable_error_for_unknown_input` must still name the file it was
given. Make `MalformedMesh`'s message drop `{format}`; a test asserting the OBJ message says
OBJ (task 6) will fail later — note here that this variant's coverage arrives with task 6
rather than pretending it exists now.

- [ ] **Step 7: Commit**

```sh
git add crates/lapidary-cad
git commit -m "refactor(cad): give the kernel one output type and bytes to work from"
```

---

### Task 3: Clustering — one indexing function, three grids

**Files:**
- Create: `crates/lapidary-cad/src/cluster.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`

**Read first:** spec §3.1, and `measure.rs`'s `key`/`is_closed` for the quantisation this
generalises.

Pure geometry, no IO, fully unit-testable. This is the slice's core and the task most worth
getting right before anything depends on it.

- [ ] **Step 1: The types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod { L0, L1, L2 }

impl Lod {
    /// Cells per axis across the bounding box. Relative rather than absolute so a 5 mm
    /// screw and a 2 m gantry get comparable triangle counts — `prototype-notes.md`
    /// records that the prototype's 48 was tuned by eye on one corpus.
    ///
    /// `L2` is the sentinel for "quantise at 1e-4 mm", the same grid `measure.rs` already
    /// uses to rebuild adjacency: lossless de-duplication rather than decimation.
    fn cells(self) -> Option<u32> {
        match self { Lod::L0 => Some(32), Lod::L1 => Some(96), Lod::L2 => None }
    }

    /// Triangle budget from DATA.md 2.1. Approximate by design: the retry below keeps
    /// them approximately true across a corpus, which a fixed grid does not.
    fn budget(self) -> Option<u32> {
        match self { Lod::L0 => Some(5_000), Lod::L1 => Some(50_000), Lod::L2 => None }
    }
}

pub struct Tessellation {
    pub lod: Lod,
    pub glb: Vec<u8>,
    pub triangle_count: u32,
    pub grid: u32,
}
```

- [ ] **Step 2: The indexed intermediate**

`cluster` produces an indexed mesh, which `glb.rs` then writes. Keep them separate: the
clustering is testable without a glTF parser, and the writer is testable without a mesh.

```rust
pub(crate) struct Indexed {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}
```

- [ ] **Step 3: The algorithm**

One pass. For each vertex compute its cell; keep the first vertex seen in each cell as the
representative; rewrite each triangle's three corners to their cells' representative
indices; **drop the triangle if two or more corners landed in the same cell**, because a
triangle with a repeated corner has zero area and is a degenerate the viewer must never see.

At `Lod::L2` the cell size is 1e-4 mm, so nothing collapses that was not already the same
point, and no triangle is dropped.

- [ ] **Step 4: The budget retry**

If `triangle_count` exceeds the rung's budget, halve `cells` and cluster again, at most
twice. Record the grid actually used in `Tessellation::grid`. This mirrors `raster.rs`'s
`FALLBACK_PX` retry against `MAX_THUMB_BYTES` — the same problem, the same shape of answer.

Do **not** retry upward when a mesh comes in under budget: a small part clustering to itself
is correct, and refining toward a budget would make a 12-triangle bracket produce a
12-triangle L0 with extra passes to prove it.

- [ ] **Step 5: Tests**

```rust
#[test] fn l2_of_a_cube_keeps_every_triangle_and_deduplicates_to_eight_vertices()
#[test] fn l0_of_the_bracket_has_strictly_fewer_triangles_than_l2()
#[test] fn a_mesh_under_the_budget_still_produces_all_three_rungs()
#[test] fn every_index_points_at_a_vertex_that_exists()
#[test] fn a_triangle_whose_corners_share_a_cell_is_dropped()
#[test] fn exceeding_the_budget_halves_the_grid_and_records_the_grid_it_used()
#[test] fn clustering_is_deterministic_for_the_same_input()
```

The last matters more than it looks: `kernel_version` + `params_json` are supposed to make
regeneration deterministic, and a `HashMap` iteration order leaking into the representative
choice would quietly break that.

- [ ] **Step 6: Verify**

Remove the degenerate-triangle drop; `a_triangle_whose_corners_share_a_cell_is_dropped`
must fail. Then change `Lod::L2`'s cells from `None` to `Some(1024)`;
`l2_of_a_cube_keeps_every_triangle...` must fail on the triangle count, not merely on the
vertex count. Revert both.

- [ ] **Step 7: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): cluster a mesh into an LOD ladder"
```

---

### Task 4: The glTF writer

**Files:**
- Create: `crates/lapidary-cad/src/glb.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`

**Read first:** spec §3.2. The glTF 2.0 spec's binary container section is the reference;
the subset needed here is one buffer, one bufferView pair, two accessors, one mesh, one
primitive, one node, one scene.

- [ ] **Step 1: `write_glb(indexed: &Indexed) -> Vec<u8>`**

A 12-byte header (`glTF`, version 2, total length), then a JSON chunk padded with spaces to
a 4-byte boundary, then a BIN chunk padded with zeros. Positions as `VEC3`/`FLOAT`, indices
as `SCALAR`/`UNSIGNED_INT`.

`POSITION` accessors **must** carry `min` and `max` — the spec requires it, and a viewer that
computes bounds from them will frame the part wrongly if they are absent or stale.

No materials, no normals. The viewer computes normals from winding, exactly as `raster.rs`
already does, and a normal buffer would double the file for data the consumer regenerates.

- [ ] **Step 2: Tests**

```rust
#[test] fn the_header_says_gltf_version_two_and_the_real_length()
#[test] fn both_chunks_are_four_byte_aligned_with_the_right_padding()
#[test] fn the_position_accessor_carries_the_meshs_real_bounds()
#[test] fn the_index_accessor_count_is_three_times_the_triangle_count()
#[test] fn a_round_trip_through_an_independent_reader_recovers_the_triangles()
```

The last one cannot use our own writer's assumptions. Parse the bytes back with a
hand-written reader in the test module that walks the container by the spec's rules rather
than by ours — a self-consistent writer passes its own reader every time, which is exactly
the failure this test exists to catch.

- [ ] **Step 3: Verify**

Pad the JSON chunk with zeros instead of spaces (the spec requires spaces for JSON, zeros
for BIN); `both_chunks_are_four_byte_aligned_with_the_right_padding` must fail. Then drop
`min`/`max` from the accessor; `the_position_accessor_carries_the_meshs_real_bounds` must
fail. Revert both.

- [ ] **Step 4: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): write an LOD rung as uncompressed glTF binary"
```

---

### Task 5: `MeshKernel` produces the ladder

**Files:**
- Modify: `crates/lapidary-cad/src/mesh_kernel.rs`, `crates/lapidary-ingest/src/handler.rs`

**Read first:** spec §3.5, §4.3.

`MeshOutput` is deleted here, so `handler.rs` changes in the same commit — leaving it broken
would be a red workspace. The handler adapts to the new type but still persists only the
thumbnail; persisting the rungs is tasks 8 and 9.

- [ ] **Step 1: `MeshKernel` implements `Kernel`**

`process` parses, measures, rasterizes, clusters three rungs, and returns `KernelOutput` with
`entities: Vec::new()`. The inherent `ingest` method goes away; there is one entry point.

- [ ] **Step 2: Adapt the handler**

`output.thumbnail_webp` and `output.measurements` are unchanged in meaning.
`output.tessellations` is carried but not yet persisted. Add a `// task 8 persists these`
marker rather than a silent drop, so an intermediate reviewer can see it is deliberate.

- [ ] **Step 3: Run**

```sh
cargo test --workspace --all-features; echo "exit=$?"
```

- [ ] **Step 4: Verify**

Make `process` return two tessellations instead of three; it must fail to compile, because
the field is `[Tessellation; 3]` rather than a `Vec`. That is the point of the array type
and this is where it is confirmed rather than assumed.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-cad crates/lapidary-ingest
git commit -m "feat(cad): produce three tessellations from one kernel call"
```

---

### Task 6: OBJ

**Files:**
- Create: `crates/lapidary-cad/src/obj.rs`, `fixtures/idler-bracket-lp-2210-01.obj`
- Modify: `crates/lapidary-cad/src/lib.rs`

**Read first:** `stl.rs`, especially its module doc on why it is hand-written and its
`finish` gate.

- [ ] **Step 1: `parse_obj(bytes: &[u8]) -> Result<Mesh, CadError>`**

Handle `v` and `f`; ignore `vt`, `vn`, `o`, `g`, `s`, `usemtl`, `mtllib` and comments. Faces
may be triangles or larger polygons — fan-triangulate anything with more than three
vertices. Index references may be `v`, `v/vt`, `v//vn` or `v/vt/vn`; only the first
component matters here. Indices are 1-based, and **negative indices count back from the end
of the vertex list**, which is the feature most hand-rolled OBJ parsers forget and which
real exporters do emit.

Reject non-finite coordinates, and route the empty case through the same `finish` gate
`parse_stl` uses so both parsers give the same answer for a file with no geometry.

- [ ] **Step 2: The fixture**

A real part with plausible geometry and a plausible number — `idler-bracket-lp-2210-01.obj`.
Generate it from a real solid rather than typing eight vertices; `example/parts/generate.py`
is the precedent for how this project makes fixtures. It must contain at least one quad face
so triangulation is exercised by the end-to-end tests and not only by unit tests.

- [ ] **Step 3: Tests**

```rust
#[test] fn a_triangle_face_parses()
#[test] fn a_quad_face_is_triangulated_into_two_triangles()
#[test] fn negative_indices_count_back_from_the_end()
#[test] fn texture_and_normal_components_are_ignored()
#[test] fn comments_blank_lines_and_crlf_are_tolerated()
#[test] fn a_file_with_no_faces_fails_with_an_actionable_message()
#[test] fn a_non_finite_coordinate_is_rejected()
#[test] fn the_real_fixture_parses_with_its_real_triangle_count()
```

- [ ] **Step 4: Verify**

Make negative indices resolve as if positive; `negative_indices_count_back_from_the_end`
must fail with wrong coordinates rather than an error — a silently wrong mesh is the failure
mode this test is for. Revert.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-cad fixtures
git commit -m "feat(cad): parse OBJ alongside STL"
```

---

### Task 7: Format dispatch, and a version string that names the parser

**Files:**
- Modify: `crates/lapidary-cad/src/mesh_kernel.rs`, `crates/lapidary-ingest/src/scan.rs`,
  `crates/lapidary-ingest/src/handler.rs`

**Read first:** spec §3.7, §3.8.

- [ ] **Step 1: `is_mesh_candidate`**

`is_stl_candidate` becomes `is_mesh_candidate`, matching `.stl` and `.obj`
case-insensitively. Its doc comment says why dispatch is on the extension rather than a byte
sniff: OBJ has no magic number, so sniffing reduces to guessing from the first non-comment
line, and the extension is what the walk already filtered on.

- [ ] **Step 2: Dispatch**

`MeshKernel::process` takes the format alongside the bytes. `KernelParams` gains
`format: String` rather than adding a positional argument — the trait's signature is
`(bytes, params)` and the format is a parameter of the job, not a second payload.

- [ ] **Step 3: The version string**

`{parser}-1+glb-1+{RASTER_VERSION}` — `stl-1+glb-1+cpu-1`, `obj-1+glb-1+cpu-1`. Extend
`the_reported_version_pins_both_the_parser_and_the_rasterizer` to assert the parser and the
writer are both named, and add a test that two formats give two version strings.

- [ ] **Step 4: Verify**

Make `version()` ignore the format and always say `stl-1`; the new test must fail. This is
the correctness case from spec §3.7, not a cosmetic one: a regenerated thumbnail must be
distinguishable from a stale one, and a version that lies about the parser makes an
OBJ-derived derivative indistinguishable from an STL-derived one.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-cad crates/lapidary-ingest
git commit -m "feat(ingest): scan and dispatch OBJ as well as STL"
```

---

### Task 8: Persisting the ladder

**Files:**
- Modify: `crates/lapidary-db/src/repo.rs`, `crates/lapidary-db/tests/repo.rs`

**Read first:** spec §6, §7, and `insert_part_chain` in full.

- [ ] **Step 1: Widen `IngestRequest`**

Add `format: &'a str` and `tessellations: &'a [(String, BlobHash)]`. Bind `format` where the
SQL literal `'stl'` is today.

- [ ] **Step 2: Write the blob rows and the derivative rows**

Each rung needs a `blob` row before its `derivative` row, because task 1's foreign key
requires one. Reuse the pattern already in `insert_part_chain` for the source blob:
`INSERT … ON CONFLICT DO NOTHING`, then `UPDATE blob SET ref_count = ref_count + 1`. A rung
whose bytes match another revision's costs a `ref_count` bump and no second file, which is
what makes spec §3.6's three-identical-rungs case cheap.

`zstd_level` is NULL and `stored_bytes = size_bytes`: derivatives are never compressed.

- [ ] **Step 3: Rename the anticipating test**

`crates/lapidary-db/tests/repo.rs:450` inserts a `'lod0'` derivative to prove `PgParts::page`
does not fan out. Rename the kind to `tessellation_l0`. **Do not otherwise touch it** — it
predates this slice and was written for it. It uses `thumb_bytes` rather than `blake3`, so
task 1's CHECK leaves it valid; do not "fix" that either.

- [ ] **Step 4: Tests**

```rust
#[test] fn three_tessellations_and_a_thumbnail_coexist_on_one_revision()
#[test] fn a_rung_shared_between_two_revisions_is_one_blob_with_ref_count_two()
#[test] fn the_file_row_records_the_format_it_was_given()
```

- [ ] **Step 5: Verify**

Skip the `ref_count` increment for tessellations;
`a_rung_shared_between_two_revisions_is_one_blob_with_ref_count_two` must fail. Revert.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): record a revision's tessellations beside its thumbnail"
```

---

### Task 9: Writing the blobs, and reaping them when the transaction does not land

**Files:**
- Modify: `crates/lapidary-ingest/src/handler.rs`

**Read first:** spec §4.3, and `handler.rs`'s existing reap on the `record` error arm.

- [ ] **Step 1: Open a `DerivativeStore`**

This is its first production use. `AppState` already carries `blob_root`; the store opens
under it beside `SourceStore`, and needs no `WorkerRole` proof because derivatives are
readable by both roles.

- [ ] **Step 2: Write the three rungs before the transaction**

Same reason the source blob is written first: a filesystem write cannot be rolled back by
Postgres.

- [ ] **Step 3: Extend the reap**

On the `record` error arm, remove the three derivative blobs alongside the source blob.
**Only on the branch that wrote them** — the `link_existing` branch wrote no source blob and
must not reap one, and the same asymmetry now applies three more times.

The prototype shipped an orphan-blob bug of exactly this shape;
`docs/prototype-notes.md` records it and slice 1 fixed it for source blobs.

- [ ] **Step 4: Tests**

```rust
#[test] fn a_real_stl_writes_three_tessellation_blobs_and_rows()
#[test] fn a_failure_after_the_rungs_are_written_leaves_no_orphan_blob()
```

The second extends `a_failure_after_the_blob_write_leaves_no_orphan_blob_on_disk`, which
already exists and already checks the filesystem rather than the returned error — because
the error looks identical whether or not the reap ran.

- [ ] **Step 5: Verify**

Delete the derivative reap; `a_failure_after_the_rungs_are_written_leaves_no_orphan_blob`
must fail on files left under `blob_root`. Revert.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-ingest
git commit -m "feat(ingest): store the LOD ladder as derivative blobs"
```

---

### Task 10: `GET /api/blob/{blake3}`

**Files:**
- Create: `crates/lapidary-api/src/blob.rs`, `crates/lapidary-api/tests/blob.rs`
- Modify: `crates/lapidary-api/src/lib.rs`

**Read first:** spec §4.2, `DATA.md` §2.3, and `crates/lapidary-api/src/parts.rs` for shape.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] async fn a_referenced_blob_is_served_with_immutable_caching_and_an_etag()
#[test] async fn a_blob_on_disk_that_no_derivative_references_is_not_found()
#[test] async fn an_unknown_hash_is_not_found_with_the_same_body()
#[test] async fn the_worker_role_does_not_serve_blobs()
```

The second and third are the security pair, and they must be indistinguishable from outside:
a different status or body for "exists but unreferenced" confirms the blob exists, which is
precisely the capability `CLAUDE.md` says knowing a hash must not grant.

- [ ] **Step 2: Implement**

Look the hash up through a `lapidary-db` query joining `derivative` to `revision` to `part`
to `library`. Unreferenced or unknown → the same 404. Referenced → stream the bytes from
`DerivativeStore` with:

```
Cache-Control: public, max-age=31536000, immutable
ETag: "{blake3}"
```

There is no auth in Phase 1, so "the principal has access" reduces to *reachable from a
library that exists*. Write the join now, while it is one query, rather than retrofitting it
in Phase 8 when it is a security fix.

- [ ] **Step 3: Mount under `Role::Api` only**

- [ ] **Step 4: Verify**

Drop the reachability join and serve any hash present on disk;
`a_blob_on_disk_that_no_derivative_references_is_not_found` must fail. Then move the route
to the unconditional `shared` router; `the_worker_role_does_not_serve_blobs` must fail.
Revert both.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-api
git commit -m "feat(api): serve derivative blobs by hash, to callers that can reach them"
```

---

### Task 11: End to end

**Files:**
- Modify: `crates/lapidary-ingest/tests/handler.rs`

**Read first:** the existing four handler tests; these follow their shape.

- [ ] **Step 1: The tests**

```rust
#[test] async fn a_real_stl_yields_a_thumbnail_and_three_tessellation_rows()
#[test] async fn a_real_obj_yields_the_same_with_its_format_recorded()
#[test] async fn the_kernel_version_differs_between_an_stl_and_an_obj_ingest()
#[test] async fn each_rung_is_valid_gltf_and_l0_is_smaller_than_l2()
```

The fourth parses the stored bytes back rather than trusting the writer, for the reason task
4 gives: a self-consistent writer passes its own reader.

- [ ] **Step 2: Verify**

Make `process` return the L2 tessellation three times;
`each_rung_is_valid_gltf_and_l0_is_smaller_than_l2` must fail. This is the mutation that
catches a ladder wired up but not actually laddered — the shape of bug that passes every
row-count assertion.

- [ ] **Step 3: Commit**

```sh
git add crates/lapidary-ingest
git commit -m "test(ingest): pin that both formats produce a real ladder"
```

---

### Task 12: The exit criterion, measured

**Files:** none — this task produces the handoff's numbers.

**Read first:** spec §10.

- [ ] **Step 1: Bring the stack up**

```sh
docker compose -f deploy/compose.yaml up -d --build
```

- [ ] **Step 2: Scan the fixtures plus the OBJ, and check the rows**

Every part has four derivative rows: one `thumbnail` inline, three `tessellation_l*` by
hash. Re-scanning drains to `skipped` and writes no new blobs.

- [ ] **Step 3: Check the route**

```sh
curl -sI "http://localhost:8080/api/blob/<hash>"   # 200, ETag, immutable
curl -sI "http://localhost:8080/api/blob/$(printf 'a%.0s' {1..64})"  # 404
```

- [ ] **Step 4: Validate the glTF independently**

Run each stored `.glb` through a glTF validator that is not ours. This is the one check that
cannot be replaced by a test in this repository, because everything here shares our reading
of the spec.

- [ ] **Step 5: Measure against the real corpus**

150 real STLs, as slice 2's exit run used. Record throughput against slice 2's measured 74
files/s — spec §10 allows 3× — and the warm grid page against `DATA.md` §2.5's 80 ms. The
grid reads only the thumbnail, so the ladder must not slow it at all.

- [ ] **Step 6: Write the handoff**

`docs/superpowers/plans/2026-09-04-phase-1-slice-3-HANDOFF.md`, following slice 2's: what
landed, the measured numbers, what the plan got wrong, and the ledger below.

- [ ] **Step 7: Commit**

```sh
git add docs/superpowers/plans
git commit -m "docs(plan): record what the slice 3 exit run showed"
```

---

## Ledger items this slice closes or opens

**Closes:** Phase 0a follow-up item 2 — `KernelOutput`'s shape, open since Phase 0a.

**Opens, with triggers:**

| Item | Trigger |
|---|---|
| meshopt encoding | Phase 3, when a viewer exists to decode it |
| `params_json` holds two shapes (`{px}` and `{grid, budget}`) | A third shape. Then it becomes a tagged enum in `lapidary-core`, as `job.payload` is scheduled to |
| Derivative cache eviction | Phase 8. `DATA.md` §1.5's "clear render cache" needs a fleet to be worth having |
| Clustering damages thin features | A user report, or Phase 3 showing it. Quadric error metrics are the upgrade, and they need the topology clustering deliberately does not build |
| `Entity` has no variants | Phase 2's STEP ingest fills it |
| Three derivative writes lengthen a job | Lease heartbeats, already on slice 2's ledger. This slice moves that trigger closer without reaching it |

## Self-review

Checked against the spec, section by section:

- §3.1 clustering → tasks 3, 5. §3.2 GLB → task 4. §3.3 `kind` vocabulary → tasks 8 (rename)
  and 9 (writes). §3.4 the trait's defect → task 2. §3.5 one output type → tasks 2, 5.
  §3.6 small meshes → task 3 step 5. §3.7 `kernel_version` → task 7. §3.8 dispatch → task 7.
  §3.9 derivative storage → tasks 1, 8, 9.
- §4.1 crate placement → every task's Files block. §4.2 routes → task 10. §4.3 the pipeline's
  new step → task 9.
- §5 data flow → tasks 5 (kernel), 9 (blobs), 8 (rows), 10 (serving).
- §6 schema → task 1, verbatim. §7 domain types → tasks 2, 3, 8.
- §8 error handling → task 2 (the rename), task 6 (the OBJ message), task 9 (the
  transient/permanent split for a store failure).
- §9 testing → every test named there appears in a task, each with its mutation.
- §10 exit criterion → task 12.
- §11 risks → the ledger above.

Type consistency: `Lod`, `Tessellation`, `Indexed`, `Entity`, `KernelOutput`, `write_glb`,
`cluster`, `parse_obj`, `is_mesh_candidate` are each defined in exactly one task and
referenced by the same name everywhere after.

**Known gap, stated rather than hidden.** Every mutation check in this plan is *specified*
but none has been *run* — the plan is written before the code exists. Slice 2's execution
found two mutations that did not bite, one because the fixture could not distinguish the
cases and one because the guard it targeted was unreachable on that path. Expect the same
rate here, and when a mutation does not bite, record why in the handoff rather than
weakening the test until it passes.
