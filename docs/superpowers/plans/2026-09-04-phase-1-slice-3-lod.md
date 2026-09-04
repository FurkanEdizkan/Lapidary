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
| `crates/lapidary-cad/Cargo.toml` | **Modify.** Adds `serde_json`, already a workspace dependency used by four crates — nothing new enters `Cargo.lock` or the licence audit. See task 4. |
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

**One refinement of the spec.** Spec §7 declares `Tessellation::grid` as `u32`. Make it
`Option<u32>`: `L2` has no cell count — it quantises at a fixed 1e-4 mm — and writing `0`
there would put `{"grid": 0}` into `params_json`, which is a lie about how the derivative
was made. `None` is the honest value and `params_json` gets `null`.

- [ ] **Step 1: The module doc and the types**

```rust
//! Vertex clustering: one indexing function at three grid sizes.
//!
//! `measure.rs` already quantises vertices and hashes them, to rebuild the adjacency that
//! STL's per-facet vertex duplication destroys. Clustering is that same idea at a coarser
//! grid, and the ladder falls out of one function:
//!
//! | Rung | Cell size            | Effect                                    |
//! |------|----------------------|-------------------------------------------|
//! | `L2` | 1e-4 mm              | lossless de-duplication; nothing dropped  |
//! | `L1` | bounding box / 96    | lossy                                     |
//! | `L0` | bounding box / 32    | lossy                                     |
//!
//! The input is `Mesh`, an unindexed triangle soup. That is not a limitation here:
//! clustering does not need indexed input, it *produces* it, and an indexed mesh is what
//! glTF wants. Nothing about `Mesh` changes.
//!
//! Cell size is a fraction of the bounding box rather than an absolute length, so a 5 mm
//! screw and a 2 m gantry get comparable triangle counts. `docs/prototype-notes.md`
//! records that the deleted prototype clustered on a 48³ grid and that "the 48 constant
//! was tuned by eye and should become an L0/L1/L2 ladder".

use crate::glb::write_glb;
use crate::kernel::CadError;
use crate::stl::Mesh;
use std::collections::HashMap;

/// The finest cell this module will use, and the grid `L2` always uses. Matches
/// `measure.rs`'s quantisation: finer than any real mesh tolerance, coarse enough to
/// collapse the f32 representation noise at a shared corner.
const FINEST_MM: f64 = 1e-4;

/// How many times the budget retry may halve the grid before giving up and accepting an
/// over-budget rung. Two, like `raster.rs`'s two `FALLBACK_PX` steps: a third pass costs
/// another full clustering for a mesh that is already telling us it will not fit.
const MAX_RETRIES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    L0,
    L1,
    L2,
}

impl Lod {
    /// Ascending detail. The array shape is deliberate: `KernelOutput.tessellations` is
    /// `[Tessellation; 3]`, so a kernel that produced two rungs would not compile.
    pub const ALL: [Lod; 3] = [Lod::L0, Lod::L1, Lod::L2];

    /// Cells per axis across the bounding box. `None` means the fixed `FINEST_MM` grid.
    fn cells(self) -> Option<u32> {
        match self {
            Lod::L0 => Some(32),
            Lod::L1 => Some(96),
            Lod::L2 => None,
        }
    }

    /// Triangle budget from `DATA.md` §2.1. Approximate by design — the retry below is
    /// what keeps them approximately true across a corpus, which a fixed grid does not.
    fn budget(self) -> Option<u32> {
        match self {
            Lod::L0 => Some(5_000),
            Lod::L1 => Some(50_000),
            Lod::L2 => None,
        }
    }

    pub fn as_kind(self) -> &'static str {
        match self {
            Lod::L0 => "tessellation_l0",
            Lod::L1 => "tessellation_l1",
            Lod::L2 => "tessellation_l2",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tessellation {
    pub lod: Lod,
    /// glTF 2.0 binary, uncompressed — see the design doc, section 3.2.
    pub glb: Vec<u8>,
    pub triangle_count: u32,
    /// The grid actually used, after any budget retry, so `params_json` can say how the
    /// derivative was made. `None` for `L2`, which has no cell count.
    pub grid: Option<u32>,
}

/// An indexed mesh: shared positions plus a triangle index buffer. `glb.rs` writes one of
/// these; keeping it separate is what lets clustering be tested without a glTF parser and
/// the writer be tested without a mesh.
pub(crate) struct Indexed {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// A vertex's cell. Signed because a coordinate may sit below the bounding-box minimum by
/// a rounding step.
type Cell = (i64, i64, i64);
```

- [ ] **Step 2: Bounds and cell size**

```rust
fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for tri in &mesh.triangles {
        for v in tri {
            for axis in 0..3 {
                min[axis] = min[axis].min(v[axis]);
                max[axis] = max[axis].max(v[axis]);
            }
        }
    }
    (min, max)
}

/// Cell size per axis. Two guards, both reachable with real parts:
///
/// A **zero extent** — a flat plate, or a part whose bounding box is degenerate on one
/// axis — would divide by zero and produce a NaN cell index. Such an axis gets
/// `FINEST_MM`, which collapses nothing because every vertex is already at the same
/// coordinate.
///
/// A **cell finer than `FINEST_MM`** is pointless: it distinguishes vertices that
/// `measure.rs` already treats as the same corner, and a very fine cell over a large part
/// risks overflowing the `i64` cell index. Clamped.
fn cell_size(min: [f32; 3], max: [f32; 3], cells: Option<u32>) -> [f64; 3] {
    let Some(n) = cells else {
        return [FINEST_MM; 3];
    };
    std::array::from_fn(|axis| {
        let span = f64::from(max[axis]) - f64::from(min[axis]);
        if span <= 0.0 {
            FINEST_MM
        } else {
            (span / f64::from(n)).max(FINEST_MM)
        }
    })
}

fn cell_of(v: [f32; 3], min: [f32; 3], size: [f64; 3]) -> Cell {
    let q = |axis: usize| (((f64::from(v[axis]) - f64::from(min[axis])) / size[axis]).floor() as i64);
    (q(0), q(1), q(2))
}
```

- [ ] **Step 3: The indexing pass**

Two passes, and the reason for two rather than one is worth keeping in the code: a
one-pass version assigns a representative index the first time it sees a cell, including
for triangles that are then dropped, leaving positions in the buffer that no index
references. Deciding which triangles survive *first* means every position emitted is
referenced.

```rust
/// Index a mesh at a given cell size, dropping degenerate triangles.
///
/// A triangle whose corners fall into fewer than three distinct cells has collapsed to a
/// line or a point. Dropping it is the point of clustering rather than a loss: a zero-area
/// triangle contributes nothing to render and makes some viewers emit NaN normals.
///
/// Deterministic: the representative for a cell is the first surviving triangle's vertex
/// in that cell, in `mesh.triangles` order. The `HashMap`s are only ever looked up, never
/// iterated, so their ordering cannot leak into the output — which matters because
/// `kernel_version` + `params_json` are supposed to make regeneration reproduce the bytes.
fn index_at(mesh: &Mesh, min: [f32; 3], size: [f64; 3]) -> Indexed {
    // Pass 1: which triangles survive, keeping each one's cells and its vertices.
    let mut surviving: Vec<([Cell; 3], [[f32; 3]; 3])> = Vec::new();
    for tri in &mesh.triangles {
        let cells = [
            cell_of(tri[0], min, size),
            cell_of(tri[1], min, size),
            cell_of(tri[2], min, size),
        ];
        if cells[0] != cells[1] && cells[1] != cells[2] && cells[0] != cells[2] {
            surviving.push((cells, *tri));
        }
    }

    // Pass 2: assign indices in first-referenced order.
    let mut index_of: HashMap<Cell, u32> = HashMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(surviving.len() * 3);
    for (cells, vertices) in &surviving {
        for (cell, vertex) in cells.iter().zip(vertices) {
            let index = *index_of.entry(*cell).or_insert_with(|| {
                positions.push(*vertex);
                // The cast is safe for any mesh that fits in memory: one position per
                // cell, and a u32 index buffer caps a rung at 4 billion vertices anyway.
                (positions.len() - 1) as u32
            });
            indices.push(index);
        }
    }

    Indexed { positions, indices }
}
```

- [ ] **Step 4: The public entry point and the budget retry**

```rust
/// One rung. Clusters, writes glTF, and retries at a coarser grid if the rung came out
/// over its triangle budget.
///
/// The retry mirrors `raster.rs`, which halves the thumbnail's pixel size when the encode
/// exceeds `MAX_THUMB_BYTES` — the same problem, the same shape of answer.
///
/// There is deliberately no retry in the other direction. A mesh that comes in under
/// budget has clustered to itself, which is correct; refining toward the budget would
/// make a 12-triangle bracket run extra passes to produce 12 triangles.
pub fn cluster(mesh: &Mesh, lod: Lod) -> Result<Tessellation, CadError> {
    let (min, max) = bounds(mesh);
    let mut cells = lod.cells();

    for attempt in 0..=MAX_RETRIES {
        let size = cell_size(min, max, cells);
        let indexed = index_at(mesh, min, size);
        let triangle_count = (indexed.indices.len() / 3) as u32;

        let over_budget = lod.budget().is_some_and(|budget| triangle_count > budget);
        let can_retry = cells.is_some() && attempt < MAX_RETRIES;
        if !over_budget || !can_retry {
            return Ok(Tessellation {
                lod,
                glb: write_glb(&indexed)?,
                triangle_count,
                grid: cells,
            });
        }
        // `max(1)` rather than allowing 0: a zero grid divides by zero in cell_size, and
        // one cell per axis is the coarsest meaningful clustering.
        cells = cells.map(|c| (c / 2).max(1));
    }

    // The loop always returns: `can_retry` is false on the final iteration.
    unreachable!("the budget retry loop returns on its last iteration")
}

/// All three rungs, ascending. The array shape is what makes a two-rung kernel a compile
/// error rather than a runtime surprise.
pub fn ladder(mesh: &Mesh) -> Result<[Tessellation; 3], CadError> {
    Ok([
        cluster(mesh, Lod::L0)?,
        cluster(mesh, Lod::L1)?,
        cluster(mesh, Lod::L2)?,
    ])
}
```

`unreachable!` is not an `unwrap` and does not trip the workspace lint, but it is still a
panic path — if the reviewer prefers, restructure as a `loop` with the final iteration
outside it. Either is acceptable; do not silence it with a default `Tessellation`.

- [ ] **Step 5: The tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube as a triangle soup: 12 triangles, 36 vertices, 8 distinct corners.
    /// Written out rather than generated so the expected counts below are readable.
    fn cube() -> Mesh {
        let c = [
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6],
            [0, 4, 5], [0, 5, 1], [1, 5, 6], [1, 6, 2],
            [2, 6, 7], [2, 7, 3], [3, 7, 4], [3, 4, 0],
        ];
        Mesh {
            triangles: faces.iter().map(|f| [c[f[0]], c[f[1]], c[f[2]]]).collect(),
        }
    }

    fn bracket() -> Mesh {
        crate::parse_stl(include_bytes!("../../../fixtures/bracket-lp-1042-03.stl"))
            .expect("the fixture parses")
    }

    #[test]
    fn l2_keeps_every_triangle_and_deduplicates_to_eight_corners() {
        let t = cluster(&cube(), Lod::L2).expect("clusters");
        assert_eq!(t.triangle_count, 12, "L2 is lossless — nothing may be dropped");
        assert_eq!(t.grid, None, "L2 has no cell count, and must not claim one");
        // 36 soup vertices collapse to the cube's 8 real corners.
        let indexed = index_at(&cube(), bounds(&cube()).0, [FINEST_MM; 3]);
        assert_eq!(indexed.positions.len(), 8);
    }

    #[test]
    fn l0_of_a_real_part_has_strictly_fewer_triangles_than_l2() {
        let mesh = bracket();
        let l0 = cluster(&mesh, Lod::L0).expect("clusters");
        let l2 = cluster(&mesh, Lod::L2).expect("clusters");
        assert!(
            l0.triangle_count < l2.triangle_count,
            "L0 {} should be coarser than L2 {}",
            l0.triangle_count,
            l2.triangle_count
        );
    }

    #[test]
    fn a_mesh_under_the_budget_still_produces_all_three_rungs() {
        // Spec 3.6: a small part clusters to itself at every grid and is written anyway.
        // Content addressing makes three identical rungs one blob with ref_count 3.
        let rungs = ladder(&cube()).expect("ladder");
        assert_eq!(rungs.len(), 3);
        for rung in &rungs {
            assert!(rung.triangle_count > 0, "no rung may be empty");
        }
    }

    #[test]
    fn every_index_points_at_a_vertex_that_exists() {
        let mesh = bracket();
        for lod in Lod::ALL {
            let (min, max) = bounds(&mesh);
            let indexed = index_at(&mesh, min, cell_size(min, max, lod.cells()));
            assert!(
                indexed.indices.iter().all(|i| (*i as usize) < indexed.positions.len()),
                "{lod:?} emitted an index past the end of the position buffer"
            );
        }
    }

    #[test]
    fn no_position_is_left_unreferenced() {
        // The reason index_at is two passes. A one-pass version assigns a representative
        // for triangles it then drops, leaving positions nothing points at — legal glTF,
        // wasted bytes in the rung that most needs to be small.
        let mesh = bracket();
        let (min, max) = bounds(&mesh);
        let indexed = index_at(&mesh, min, cell_size(min, max, Lod::L0.cells()));
        let referenced: std::collections::HashSet<u32> = indexed.indices.iter().copied().collect();
        assert_eq!(referenced.len(), indexed.positions.len());
    }

    #[test]
    fn a_triangle_whose_corners_share_a_cell_is_dropped() {
        // Three vertices well inside one coarse cell: the triangle has collapsed and must
        // not reach the viewer, where a zero-area face yields a NaN normal.
        let tiny = Mesh {
            triangles: vec![[[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.001, 0.0]]],
        };
        let (min, max) = bounds(&tiny);
        // One cell across the whole part.
        let indexed = index_at(&tiny, min, cell_size(min, max, Some(1)));
        assert!(indexed.indices.is_empty());
    }

    #[test]
    fn a_flat_part_does_not_divide_by_zero() {
        // Every vertex at z = 0: the z extent is zero, and an unguarded cell size would
        // be 0.0 and every cell index NaN-then-garbage.
        let flat = Mesh {
            triangles: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]]],
        };
        let t = cluster(&flat, Lod::L0).expect("a flat part still clusters");
        assert_eq!(t.triangle_count, 1);
    }

    #[test]
    fn exceeding_the_budget_halves_the_grid_and_records_the_grid_it_used() {
        // A mesh dense enough that L0's 32³ grid overshoots 5 000 triangles, so the retry
        // must fire. Generated rather than a fixture so the density is explicit.
        let mesh = dense_sphere(20_000);
        let t = cluster(&mesh, Lod::L0).expect("clusters");
        assert!(
            t.grid.is_some_and(|g| g < 32),
            "the retry should have coarsened the grid below the starting 32, got {:?}",
            t.grid
        );
    }

    #[test]
    fn clustering_is_deterministic_for_the_same_input() {
        // kernel_version + params_json are supposed to make regeneration reproduce the
        // bytes. A HashMap iteration order leaking into the representative choice would
        // break that silently, and only for some meshes.
        let mesh = bracket();
        let a = cluster(&mesh, Lod::L0).expect("clusters");
        let b = cluster(&mesh, Lod::L0).expect("clusters");
        assert_eq!(a.glb, b.glb, "the same mesh must produce byte-identical output");
    }
}
```

`dense_sphere(n)` is a small test helper generating a triangulated sphere with roughly `n`
triangles. Write it in the test module; it exists so the budget-retry test states its own
density rather than depending on a fixture whose triangle count could change.

- [ ] **Step 6: Run**

```sh
cargo test -p lapidary-cad; echo "exit=$?"
cargo clippy -p lapidary-cad --all-targets --all-features -- -D warnings; echo "exit=$?"
```

- [ ] **Step 7: Verify — four mutations, each naming its test**

1. Delete the degenerate-triangle condition in `index_at` (accept every triangle);
   `a_triangle_whose_corners_share_a_cell_is_dropped` must fail.
2. Change `Lod::L2`'s `cells()` from `None` to `Some(1024)`;
   `l2_keeps_every_triangle_and_deduplicates_to_eight_corners` must fail **on
   `triangle_count`**, not merely on the position count — if it fails only on positions,
   the test is not pinning losslessness.
3. Remove the `span <= 0.0` guard in `cell_size`; `a_flat_part_does_not_divide_by_zero`
   must fail.
4. Collapse `index_at` to one pass (assign representatives during the survival check);
   `no_position_is_left_unreferenced` must fail while every other test still passes — that
   is what shows the two-pass structure is load-bearing rather than stylistic.

Revert each.

- [ ] **Step 8: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): cluster a mesh into an LOD ladder"
```

---

### Task 4: The glTF writer

**Files:**
- Create: `crates/lapidary-cad/src/glb.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`, `crates/lapidary-cad/Cargo.toml`

**Read first:** spec §3.2. The reference is the glTF 2.0 specification's binary-container
section; the subset needed is one buffer, two bufferViews, two accessors, one mesh, one
primitive, one node, one scene.

**On `serde_json`.** This task adds `serde_json` to `lapidary-cad`'s manifest. That is not
a new dependency in the sense spec §3.2 forbids: it is already in
`[workspace.dependencies]` and already used by four crates, so nothing new enters
`Cargo.lock` or the licence audit. Hand-formatting the JSON was considered and rejected —
the document is fixed-shape, but `min`/`max` are f32 values and getting float-to-JSON
formatting right (precision, `-0.0`, the fact that a non-finite value is not valid JSON at
all) is exactly the kind of detail a `format!` string gets wrong once and silently.

- [ ] **Step 1: The module doc and the constants**

```rust
//! glTF 2.0 binary output, uncompressed.
//!
//! `DATA.md` §2.2 chose meshopt as the codec and that stands — but its decoder is Phase 3's
//! viewer, and the Rust binding wraps C, which would put a C toolchain into the worker
//! image against the offline-build constraint `docs/prototype-notes.md` calls worth
//! preserving. Derivatives are designed to be evicted and regenerated (`DATA.md` §1.5), so
//! Phase 3 re-encodes and the cost is one pass over disposable data.
//!
//! What is written: one buffer, two bufferViews, two accessors, one mesh with one
//! primitive, one node, one scene. No materials and no normals — the viewer computes
//! normals from winding, exactly as `raster.rs` already does, and a normal buffer would
//! double the file for data the consumer regenerates anyway.

use crate::cluster::Indexed;
use crate::kernel::CadError;

/// Bumped whenever a change alters output bytes, and carried in `kernel_version` beside
/// the parser and the rasterizer. A regenerated rung must be distinguishable from a stale
/// one — the same rule `raster.rs`'s `RASTER_VERSION` exists for.
pub const GLB_VERSION: &str = "glb-1";

const MAGIC: u32 = 0x4654_6C67; // "glTF"
const VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

/// glTF component types.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// glTF bufferView targets.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
```

- [ ] **Step 2: The writer**

```rust
/// Round up to the next 4-byte boundary. Both chunks and both bufferViews must be aligned:
/// the container requires it of chunks, and an accessor whose byteOffset is not a multiple
/// of its component size is invalid even when a lenient loader accepts it.
fn pad_to_four(n: usize) -> usize {
    n.div_ceil(4) * 4
}

pub(crate) fn write_glb(indexed: &Indexed) -> Result<Vec<u8>, CadError> {
    if indexed.indices.is_empty() {
        return Err(CadError::Unrenderable {
            detail: "the mesh has no triangles left after clustering".to_owned(),
        });
    }

    let positions_len = indexed.positions.len() * 12;
    let indices_offset = pad_to_four(positions_len);
    let indices_len = indexed.indices.len() * 4;
    let buffer_len = indices_offset + indices_len;

    let (min, max) = position_bounds(indexed);

    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": format!("lapidary-cad {GLB_VERSION}") },
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0 } ],
        "meshes": [ {
            "primitives": [ { "attributes": { "POSITION": 0 }, "indices": 1 } ]
        } ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": FLOAT,
                "count": indexed.positions.len(),
                "type": "VEC3",
                // Required by the spec on POSITION, and not decoration: a viewer frames
                // the part from these, so absent or stale bounds put the camera wrong.
                "min": min,
                "max": max
            },
            {
                "bufferView": 1,
                "componentType": UNSIGNED_INT,
                "count": indexed.indices.len(),
                "type": "SCALAR"
            }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": positions_len,
              "target": ARRAY_BUFFER },
            { "buffer": 0, "byteOffset": indices_offset, "byteLength": indices_len,
              "target": ELEMENT_ARRAY_BUFFER }
        ],
        "buffers": [ { "byteLength": buffer_len } ]
    });

    let mut json_chunk = serde_json::to_vec(&json).map_err(|source| CadError::Unrenderable {
        detail: format!("could not encode the glTF document: {source}"),
    })?;
    // JSON pads with spaces, BIN pads with zeros. The spec is explicit, and a loader that
    // reads the JSON chunk as a string will choke on a trailing NUL.
    json_chunk.resize(pad_to_four(json_chunk.len()), b' ');

    let mut bin_chunk = Vec::with_capacity(buffer_len);
    for position in &indexed.positions {
        for axis in position {
            bin_chunk.extend_from_slice(&axis.to_le_bytes());
        }
    }
    bin_chunk.resize(indices_offset, 0);
    for index in &indexed.indices {
        bin_chunk.extend_from_slice(&index.to_le_bytes());
    }
    bin_chunk.resize(pad_to_four(bin_chunk.len()), 0);

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&bin_chunk);
    Ok(out)
}

fn position_bounds(indexed: &Indexed) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in &indexed.positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}
```

- [ ] **Step 3: An independent reader, in the test module**

This is the part that makes task 4's tests worth anything. Walk the container by the
specification's rules, not by the writer's — a self-consistent writer passes a reader built
from its own assumptions every time.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal GLB reader written from the specification, deliberately not sharing any
    /// helper with the writer above. It re-derives every offset from the bytes rather than
    /// recomputing them the way `write_glb` did.
    struct Parsed {
        json: serde_json::Value,
        bin: Vec<u8>,
    }

    fn read_glb(bytes: &[u8]) -> Parsed {
        let u32_at = |offset: usize| -> u32 {
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[offset..offset + 4]);
            u32::from_le_bytes(b)
        };
        assert_eq!(u32_at(0), MAGIC, "magic");
        assert_eq!(u32_at(4), 2, "version");
        assert_eq!(u32_at(8) as usize, bytes.len(), "declared length is the real length");

        let json_len = u32_at(12) as usize;
        assert_eq!(u32_at(16), CHUNK_JSON);
        let json_start = 20;
        let json: serde_json::Value =
            serde_json::from_slice(&bytes[json_start..json_start + json_len])
                .expect("the JSON chunk parses");

        let bin_header = json_start + json_len;
        let bin_len = u32_at(bin_header) as usize;
        assert_eq!(u32_at(bin_header + 4), CHUNK_BIN);
        let bin_start = bin_header + 8;
        Parsed { json, bin: bytes[bin_start..bin_start + bin_len].to_vec() }
    }

    fn a_triangle() -> Indexed {
        Indexed {
            positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn the_header_declares_gltf_two_and_the_real_length() {
        let bytes = write_glb(&a_triangle()).expect("writes");
        let parsed = read_glb(&bytes); // its asserts cover magic, version and length
        assert_eq!(parsed.json["asset"]["version"], "2.0");
    }

    #[test]
    fn both_chunks_are_four_byte_aligned_and_padded_with_the_right_filler() {
        let bytes = write_glb(&a_triangle()).expect("writes");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("4 bytes")) as usize;
        assert_eq!(json_len % 4, 0, "the JSON chunk must be 4-byte aligned");
        // JSON pads with spaces. A NUL here breaks loaders that read the chunk as a string.
        assert_eq!(bytes[20 + json_len - 1], b' ');
        let bin_len =
            u32::from_le_bytes(bytes[20 + json_len..24 + json_len].try_into().expect("4 bytes"))
                as usize;
        assert_eq!(bin_len % 4, 0, "the BIN chunk must be 4-byte aligned");
    }

    #[test]
    fn the_position_accessor_carries_the_meshs_real_bounds() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        let accessor = &parsed.json["accessors"][0];
        assert_eq!(accessor["min"], serde_json::json!([0.0, 0.0, 0.0]));
        assert_eq!(accessor["max"], serde_json::json!([2.0, 3.0, 0.0]));
    }

    #[test]
    fn the_index_accessor_count_is_three_per_triangle() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        assert_eq!(parsed.json["accessors"][1]["count"], 3);
        assert_eq!(parsed.json["accessors"][1]["componentType"], UNSIGNED_INT);
    }

    #[test]
    fn the_buffer_views_do_not_overlap_and_fit_the_buffer() {
        let parsed = read_glb(&write_glb(&a_triangle()).expect("writes"));
        let views = parsed.json["bufferViews"].as_array().expect("two views");
        let end = |v: &serde_json::Value| {
            v["byteOffset"].as_u64().unwrap_or(0) + v["byteLength"].as_u64().unwrap_or(0)
        };
        assert!(end(&views[0]) <= views[1]["byteOffset"].as_u64().expect("offset"));
        assert!(end(&views[1]) <= parsed.json["buffers"][0]["byteLength"].as_u64().expect("len"));
    }

    #[test]
    fn a_round_trip_through_the_independent_reader_recovers_the_vertices() {
        let indexed = a_triangle();
        let parsed = read_glb(&write_glb(&indexed).expect("writes"));
        let view = &parsed.json["bufferViews"][0];
        let offset = view["byteOffset"].as_u64().expect("offset") as usize;
        let recovered: Vec<f32> = parsed.bin[offset..offset + 36]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().expect("4 bytes")))
            .collect();
        assert_eq!(recovered, vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0]);
    }

    #[test]
    fn an_empty_mesh_is_an_error_rather_than_an_unopenable_file() {
        let empty = Indexed { positions: vec![], indices: vec![] };
        write_glb(&empty).expect_err("a rung with no triangles is not a glTF file");
    }
}
```

- [ ] **Step 4: Run**

```sh
cargo test -p lapidary-cad; echo "exit=$?"
```

- [ ] **Step 5: Verify — four mutations**

1. Pad the JSON chunk with `0` instead of `b' '`;
   `both_chunks_are_four_byte_aligned_and_padded_with_the_right_filler` must fail.
2. Drop `min`/`max` from the position accessor;
   `the_position_accessor_carries_the_meshs_real_bounds` must fail.
3. Write `total` as the buffer length rather than the whole file length; `read_glb`'s
   length assertion must fail — this is the mutation that proves the independent reader is
   actually independent, since the writer would still be self-consistent.
4. Swap `ARRAY_BUFFER` and `ELEMENT_ARRAY_BUFFER`;
   `the_buffer_views_do_not_overlap_and_fit_the_buffer` will **not** catch it. Note that
   plainly rather than adding a test that asserts a constant against itself — the targets
   are checked by the external validator in task 12, which is the right place for
   "conforms to a specification we did not write".

Revert each.

- [ ] **Step 6: Commit**

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
`cluster`, `ladder`, `parse_obj`, `is_mesh_candidate` are each defined in exactly one task
and referenced by the same name everywhere after.

**Where this plan refines the spec.** Two places, both recorded here so a reader comparing
the documents finds the difference stated rather than discovers it:

- **`Tessellation::grid` is `Option<u32>`, not `u32`** (spec §7). `L2` has no cell count —
  it quantises at a fixed 1e-4 mm — and writing `0` would put `{"grid": 0}` into
  `params_json`, a lie about how the derivative was made. `None` serialises as `null`.
- **Task 4 adds `serde_json` to `lapidary-cad`'s manifest**, which spec §3.2's "no new
  dependencies" does not forbid: it is already in `[workspace.dependencies]` and used by
  four crates, so nothing new enters `Cargo.lock` or the licence audit. Hand-formatting the
  JSON was considered and rejected — `min`/`max` are f32 values, and float-to-JSON
  formatting is exactly what a `format!` string gets wrong once and silently.

**Known gap, stated rather than hidden.** Every mutation check in this plan is *specified*
but none has been *run* — the plan is written before the code exists. Slice 2's execution
found two mutations that did not bite, one because the fixture could not distinguish the
cases and one because the guard it targeted was unreachable on that path. Expect the same
rate here, and when a mutation does not bite, record why in the handoff rather than
weakening the test until it passes.
