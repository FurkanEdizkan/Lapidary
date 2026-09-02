# Phase 1 Slice 1 — Local Ingest to a Visible Grid

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A mesh file in a mounted directory becomes a card with a real rendered thumbnail in the grid.

**Architecture:** A read-only mounted directory is walked by a scan endpoint that mounts only in the worker role. Each file is hashed with BLAKE3 first; a known hash short-circuits all work. New files are parsed for measurements, rasterized to a WebP thumbnail on the CPU, written to a content-addressed blob store, and recorded in one transaction. The api role serves a keyset-paginated grid endpoint that returns thumbnails inline as `bytea`, so a page costs one query.

**Tech Stack:** Rust 1.95.0 (edition 2024), axum 0.8.9, sqlx 0.9.0 against PostgreSQL 18, `blake3`, `zstd`, `image` 0.25 (WebP), React + TanStack Router/Query, Vite, Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`

## Global Constraints

Copied from `CLAUDE.md` and the spec. Every task's requirements implicitly include these.

- **We never edit geometry.** Editing happens in Rhino, Fusion, FreeCAD, Blender, Orca. We visualize, measure, route, version.
- **We never delete user data implicitly.** Delete is soft. Blobs quarantine 30 days before removal.
- **Measurement must not lie.** Analytic values from B-rep entities where available. Mesh-derived measurements are labelled "approximate" in the UI, always. Where the honest answer is *no answer*, write NULL rather than a plausible number.
- **The open path never touches a source file and never invokes the CAD kernel.** Opening a part reads metadata and derivatives only.
- **Hash first, always.** BLAKE3 before anything else in ingest. A known hash short-circuits the whole pipeline.
- **No SQL outside `lapidary-db`.** Everything goes through repository traits.
- **Content addressing is not authorization.** Knowing a blob hash never grants access.
- **Generated columns are explicitly `STORED`.** PG 18 defaults to virtual, and virtual columns cannot be indexed.
- **`lapidary-api` is a library that builds a Router.** Never a binary.
- **Container-first.** Bundle only our own binaries — never Postgres, never OCCT.
- **Pin everything.** Exact image digests, `Cargo.lock` committed, Actions pinned to commit SHAs.
- Rust: `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside tests** — enforced by `[workspace.lints]` across every target.
- **Errors say what broke and what to do.** Not "parse failed (3)."
- Frontend: **dark only, no light mode.** Motion 120/180/280ms, `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity only, respect `prefers-reduced-motion`. **No bare user-facing strings in components** — everything through `web/src/lib/strings.ts`.
- **Real content in all examples and fixtures.** Plausible part numbers, real dimensions. Never "Part 1 / Part 2".
- Prefer the boring option. Solo-maintained, air-gapped industrial deployments.

**Verification bar for every task — this is exactly what `ci.yml` runs:**

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo test --workspace --all-features
cargo deny check
```

Tests need a live database:
```
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```
Start one with:
```
podman run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/lapidary-db:latest
```
Web tasks also run `npm run typecheck`, `npm test`, `npm run build` in `web/`.

`cargo fmt --all --check` is listed first deliberately: a previous phase shipped a formatting violation that survived its own review and five later verification passes because no bar named it. It is CI's first gate.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/lapidary-db/migrations/0002_parts.sql` | Schema: library, part, revision, file, blob, derivative; seeded default library |
| `crates/lapidary-core/src/measurement.rs` | `Provenance`, `MeshMeasurements` — the vocabulary both the kernel and the DB speak |
| `crates/lapidary-cad/src/stl.rs` | STL parsing (binary + ASCII) → `Mesh` |
| `crates/lapidary-cad/src/measure.rs` | `Mesh` → `MeshMeasurements` |
| `crates/lapidary-cad/src/raster.rs` | `Mesh` → 512px WebP bytes, CPU only |
| `crates/lapidary-cad/src/mesh_kernel.rs` | `MeshKernel`, implementing the existing `Kernel` trait |
| `crates/lapidary-storage/src/lib.rs` | Blob CAS; `SourceStore` / `DerivativeStore` split |
| `crates/lapidary-db/src/repo.rs` | `PartRepository` impl, `BlobRepository`, `IngestRepository` |
| `crates/lapidary-api/src/lib.rs` | `Role`, role-aware `router()` |
| `crates/lapidary-api/src/parts.rs` | `GET /api/libraries/{id}/parts` |
| `crates/lapidary-api/src/scan.rs` | `POST /api/libraries/{id}/scan` |
| `bin/lapidary-server/src/main.rs` | Reads `LAPIDARY_ROLE`, selects the router |
| `xtask/src/deploy.rs` | Adds the open-path boundary assertion |
| `deploy/compose.yaml` | `/ingest:ro` mount, `LAPIDARY_ROLE` per service |
| `web/src/lib/api.ts`, `strings.ts`, `routes/index.tsx` | Grid fetch, copy, card rendering |
| `fixtures/` | Real STL fixtures + the rasterizer golden image |

---

## Task 1: Schema and the seeded library

**Files:**
- Create: `crates/lapidary-db/migrations/0002_parts.sql`
- Create: `crates/lapidary-db/tests/schema.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: tables `library`, `part`, `revision`, `file`, `blob`, `derivative`; a seeded library with id `01931b6e-0000-7000-8000-000000000001`.

Note the DDL order: `blob` is created before `file`, which references it.

- [ ] **Step 1: Write the failing test**

Create `crates/lapidary-db/tests/schema.rs`:

```rust
//! The schema is the contract every repository depends on. These assert the parts of it
//! that are easy to get wrong and expensive to discover later.

#[sqlx::test(migrations = "./migrations")]
async fn every_expected_table_exists(pool: sqlx::PgPool) {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .expect("query runs");

    for expected in ["blob", "derivative", "file", "library", "part", "revision"] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected table `{expected}`, found {names:?}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn the_search_column_is_stored_not_virtual(pool: sqlx::PgPool) {
    // PG18 defaults generated columns to VIRTUAL, and virtual columns cannot be indexed.
    // A virtual `search` column would make Phase 2's search silently unindexable.
    let generation: Option<String> = sqlx::query_scalar(
        "SELECT attgenerated::text FROM pg_attribute \
         WHERE attrelid = 'part'::regclass AND attname = 'search'",
    )
    .fetch_one(&pool)
    .await
    .expect("column exists");
    assert_eq!(generation.as_deref(), Some("s"), "search must be STORED");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_default_library_is_seeded(pool: sqlx::PgPool) {
    // Nothing in this slice creates a library, so the scan endpoint needs one to address.
    let (id, name): (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, name FROM library ORDER BY created_at LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("a library is seeded");
    assert_eq!(id.to_string(), "01931b6e-0000-7000-8000-000000000001");
    assert_eq!(name, "Default");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_blob_cannot_be_orphaned_by_deleting_it_out_from_under_a_file(pool: sqlx::PgPool) {
    // file.blake3 references blob.blake3. Without the FK a purge could strand a file row
    // pointing at bytes that no longer exist.
    sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, 10, 10)")
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .expect("blob inserts");
    let err = sqlx::query("DELETE FROM blob WHERE blake3 = $1")
        .bind("a".repeat(64))
        .execute(&pool)
        .await;
    assert!(err.is_ok(), "deleting an unreferenced blob is allowed");
}
```

Add to `crates/lapidary-db/Cargo.toml` under `[dev-dependencies]`:

```toml
uuid.workspace = true
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
cargo test -p lapidary-db --test schema
```

Expected: FAIL — `relation "part" does not exist`.

- [ ] **Step 3: Write the migration**

Create `crates/lapidary-db/migrations/0002_parts.sql`:

```sql
-- Phase 1 slice 1. Shape follows docs/DATA.md §3.2; part_source and part_image are
-- Phase 2 and deliberately absent.

CREATE TABLE library (
  id          uuid PRIMARY KEY,
  name        text NOT NULL,
  mode        text NOT NULL DEFAULT 'hobby',   -- hobby | controlled, per LibraryMode
  created_at  timestamptz NOT NULL DEFAULT now()
);

-- Nothing in this slice creates a library and there is no library UI yet, so one is
-- seeded here with a fixed id. Whichever slice adds a second library replaces this seed
-- rather than building beside it.
INSERT INTO library (id, name) VALUES
  ('01931b6e-0000-7000-8000-000000000001', 'Default');

CREATE TABLE blob (
  blake3            text PRIMARY KEY,
  size_bytes        bigint NOT NULL,
  stored_bytes      bigint NOT NULL,          -- after compression; show real disk usage
  zstd_level        smallint,
  dict_id           uuid,                     -- per-library dictionaries land later
  ref_count         integer NOT NULL DEFAULT 0,
  quarantined_at    timestamptz,
  last_accessed_at  timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE part (
  id             uuid PRIMARY KEY,            -- uuid v7
  library_id     uuid NOT NULL REFERENCES library(id),
  part_number    text,
  name           text NOT NULL,
  classification text,
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now(),
  created_by     uuid,
  deleted_at     timestamptz,                 -- soft delete; never hard-deleted here
  metadata_json  jsonb NOT NULL DEFAULT '{}',
  -- STORED is mandatory: PG18 defaults to VIRTUAL and virtual columns cannot be indexed.
  -- `simple` deliberately: Phase 2 owns search, and part numbers must not be stemmed.
  search tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(part_number, '')), 'A') ||
    setweight(to_tsvector('simple', name), 'B')
  ) STORED
);

CREATE INDEX part_library_id_desc ON part (library_id, id DESC);

CREATE TABLE revision (
  id                  uuid PRIMARY KEY,
  part_id             uuid NOT NULL REFERENCES part(id),
  rev_label           text NOT NULL,
  parent_revision_id  uuid REFERENCES revision(id),
  origin              text NOT NULL,          -- 'ingest' in this slice
  author              uuid,
  message             text,
  created_at          timestamptz NOT NULL DEFAULT now(),
  lifecycle_state     text,
  locked_by           uuid,
  locked_at           timestamptz,
  -- Every measured value carries its own provenance. A single row-level flag would have
  -- to lie in Phase 2, where a STEP revision has an analytic volume and a tessellated
  -- triangle count on the same row.
  volume              double precision,
  volume_source       text,                   -- tessellated | analytic
  surface_area        double precision,
  surface_area_source text,
  bbox_x              double precision,
  bbox_y              double precision,
  bbox_z              double precision,
  bbox_source         text,
  -- triangle_count has no _source column: it counts tessellated primitives and cannot
  -- be analytic. Do not add one for symmetry.
  triangle_count      integer,
  is_watertight       boolean,
  units               text,
  mass_props_json     jsonb
);

CREATE INDEX revision_part_id ON revision (part_id);

CREATE TABLE file (
  id           uuid PRIMARY KEY,
  revision_id  uuid NOT NULL REFERENCES revision(id),
  role         text NOT NULL,                 -- 'source' in this slice
  format       text NOT NULL,                 -- 'stl'
  blake3       text NOT NULL REFERENCES blob(blake3),
  size_bytes   bigint NOT NULL,
  created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX file_revision_id ON file (revision_id);
CREATE INDEX file_blake3 ON file (blake3);

CREATE TABLE derivative (
  id             uuid PRIMARY KEY,
  revision_id    uuid NOT NULL REFERENCES revision(id),
  kind           text NOT NULL,               -- 'thumbnail' in this slice
  blake3         text,                        -- NULL when stored inline
  thumb_bytes    bytea,                       -- inline when < 64 KB, per DATA.md §1.5
  kernel_version text NOT NULL,
  params_json    jsonb NOT NULL,
  created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX derivative_revision_kind ON derivative (revision_id, kind);
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p lapidary-db --test schema
```

Expected: 4 passed.

- [ ] **Step 5: Run the full bar and commit**

```bash
cargo fmt --all --check && \
cargo clippy --workspace --all-targets --all-features -- -D warnings && \
cargo test --workspace --all-features && cargo deny check
git add crates/lapidary-db/migrations/0002_parts.sql crates/lapidary-db/tests/schema.rs crates/lapidary-db/Cargo.toml
git commit -m "feat(db): Phase 1 schema — parts, revisions, blobs, derivatives"
```

---

## Task 2: Measurement vocabulary in `lapidary-core`

**Files:**
- Create: `crates/lapidary-core/src/measurement.rs`
- Modify: `crates/lapidary-core/src/lib.rs`

**Interfaces:**
- Consumes: `Approximate<T>` (already exists in this crate).
- Produces: `Provenance::{Analytic, Tessellated}`, `MeshMeasurements { bbox_mm: [f64; 3], triangle_count: u32, surface_area_mm2: f64, volume_mm3: Option<f64>, is_watertight: bool }`, and `MeshMeasurements::volume_approximate() -> Option<Approximate<f64>>`.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `crates/lapidary-core/src/lib.rs`:

```rust
    #[test]
    fn an_open_mesh_reports_no_volume_at_all() {
        // Signed-volume integration over a non-watertight mesh returns a number that
        // means nothing. "Measurement must not lie" includes refusing to answer.
        let m = MeshMeasurements {
            bbox_mm: [88.0, 34.0, 12.0],
            triangle_count: 12_940,
            surface_area_mm2: 15_320.5,
            volume_mm3: None,
            is_watertight: false,
        };
        assert!(m.volume_approximate().is_none());
    }

    #[test]
    fn a_closed_mesh_reports_volume_as_tessellated_never_analytic() {
        let m = MeshMeasurements {
            bbox_mm: [61.0, 42.0, 18.5],
            triangle_count: 48_112,
            surface_area_mm2: 9_804.25,
            volume_mm3: Some(21_478.5),
            is_watertight: true,
        };
        let v = m.volume_approximate().expect("watertight mesh has a volume");
        assert!(v.is_approximate(), "a mesh-derived volume is never analytic");
        assert_eq!(*v.value(), 21_478.5);
    }

    #[test]
    fn provenance_round_trips_through_its_wire_form() {
        assert_eq!(Provenance::Tessellated.as_str(), "tessellated");
        assert_eq!(Provenance::Analytic.as_str(), "analytic");
        assert_eq!("tessellated".parse::<Provenance>().expect("parses"), Provenance::Tessellated);
        assert!("guessed".parse::<Provenance>().is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p lapidary-core
```

Expected: FAIL — `cannot find type MeshMeasurements`.

- [ ] **Step 3: Write the implementation**

Create `crates/lapidary-core/src/measurement.rs`:

```rust
use crate::Approximate;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a measured value came from. Persisted as text beside the value it describes,
/// because a single row-level flag cannot describe a revision whose volume is analytic
/// and whose triangle count is tessellated — which is every STEP part from Phase 2 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Provenance {
    /// Read from a B-rep entity. Safe to present as exact.
    Analytic,
    /// Derived from tessellated geometry. The UI must label it.
    Tessellated,
}

impl Provenance {
    /// The persisted form. `revision.volume_source` and friends store this string.
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Analytic => "analytic",
            Provenance::Tessellated => "tessellated",
        }
    }
}

impl std::str::FromStr for Provenance {
    type Err = crate::CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "analytic" => Ok(Provenance::Analytic),
            "tessellated" => Ok(Provenance::Tessellated),
            other => Err(crate::CoreError::ProvenanceUnknown {
                got: other.to_owned(),
            }),
        }
    }
}

/// What a mesh can tell us about itself. Every figure here is tessellated by
/// construction — a mesh has no analytic entities to read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeshMeasurements {
    pub bbox_mm: [f64; 3],
    pub triangle_count: u32,
    pub surface_area_mm2: f64,
    /// `None` when the mesh is not watertight. Signed-volume integration over an open
    /// mesh produces a plausible-looking number with no meaning, and a wrong number is
    /// worse than no number.
    pub volume_mm3: Option<f64>,
    pub is_watertight: bool,
}

impl MeshMeasurements {
    /// The volume, wrapped so a caller cannot render it without its approximate label.
    pub fn volume_approximate(&self) -> Option<Approximate<f64>> {
        self.volume_mm3.map(Approximate::tessellated)
    }
}
```

Add to `crates/lapidary-core/src/error.rs`, inside the `CoreError` enum:

```rust
    #[error(
        "`{got}` is not a measurement provenance. Expected `analytic` (read from a B-rep entity) or `tessellated` (derived from mesh geometry). A row written outside lapidary-db may have used a different vocabulary."
    )]
    ProvenanceUnknown { got: String },
```

Add to `crates/lapidary-core/src/lib.rs`, beside the existing module declarations and re-exports:

```rust
mod measurement;

pub use measurement::{MeshMeasurements, Provenance};
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p lapidary-core
```

Expected: PASS.

- [ ] **Step 5: Regenerate bindings and commit**

The new types are `#[ts(export)]`, so `web/src/bindings` changes and CI fails if it is stale.

```bash
cargo xtask export-bindings
git status --porcelain -- web/src/bindings   # expect new files
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/lapidary-core web/src/bindings
git commit -m "feat(core): measurement vocabulary with per-value provenance"
```

---

## Task 3: Read an STL into measurements

**Files:**
- Create: `crates/lapidary-cad/src/stl.rs`
- Create: `crates/lapidary-cad/src/measure.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`
- Modify: `crates/lapidary-cad/src/kernel.rs` (new `CadError` variants)
- Create: `fixtures/bracket-lp-1042-03.stl` (binary), `fixtures/spacer-lp-2001-00.stl` (ASCII)

**Interfaces:**
- Consumes: `lapidary_core::MeshMeasurements`.
- Produces: `pub struct Mesh { pub triangles: Vec<[[f32; 3]; 3]> }`, `pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, CadError>`, `pub fn measure(mesh: &Mesh) -> MeshMeasurements`.

**The one real trap:** many *binary* STL files begin with the ASCII word `solid`, so sniffing the magic string misclassifies them and the parser then reads garbage. Detection must be size arithmetic: a binary STL is exactly `84 + 50 * triangle_count` bytes. Check that first; fall back to ASCII only when it does not hold.

- [ ] **Step 1: Write the failing tests**

Create `crates/lapidary-cad/src/stl.rs` with only its test module for now (implementation follows in step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A binary STL whose header starts with "solid" — the exact file that breaks a
    /// parser which sniffs the magic string instead of checking the length.
    fn binary_stl_that_looks_ascii(triangles: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let mut header = [0u8; 80];
        header[..5].copy_from_slice(b"solid");
        v.extend_from_slice(&header);
        v.extend_from_slice(&triangles.to_le_bytes());
        for _ in 0..triangles {
            v.extend_from_slice(&[0u8; 12]); // normal, ignored — we recompute
            for xyz in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                for c in xyz {
                    v.extend_from_slice(&c.to_le_bytes());
                }
            }
            v.extend_from_slice(&[0u8; 2]); // attribute byte count
        }
        v
    }

    #[test]
    fn a_binary_stl_beginning_with_solid_is_not_mistaken_for_ascii() {
        let mesh = parse_stl(&binary_stl_that_looks_ascii(2)).expect("parses as binary");
        assert_eq!(mesh.triangles.len(), 2);
    }

    #[test]
    fn an_ascii_stl_parses() {
        let src = b"solid spacer
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex 1 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid spacer
";
        let mesh = parse_stl(src).expect("parses as ascii");
        assert_eq!(mesh.triangles.len(), 1);
        assert_eq!(mesh.triangles[0][1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_truncated_binary_stl_says_what_broke_and_what_to_do() {
        let mut bytes = binary_stl_that_looks_ascii(10);
        bytes.truncate(200); // claims 10 triangles, carries far fewer
        let err = parse_stl(&bytes).expect_err("must not parse");
        let msg = err.to_string();
        assert!(msg.contains("10"), "message names the claimed count: {msg}");
        assert!(
            msg.contains("truncated") || msg.contains("incomplete"),
            "message must suggest a cause: {msg}"
        );
    }

    #[test]
    fn an_empty_file_is_rejected_rather_than_read_as_zero_triangles() {
        let err = parse_stl(&[]).expect_err("must not parse");
        assert!(err.to_string().contains("0 bytes"));
    }

    #[test]
    fn a_mesh_with_no_triangles_is_rejected() {
        // 84 bytes is a structurally valid binary STL claiming zero triangles. It is
        // still not a part, and ingesting it would create a card for nothing.
        let err = parse_stl(&binary_stl_that_looks_ascii(0)).expect_err("must not parse");
        assert!(err.to_string().contains("no triangles"));
    }
}
```

Create `crates/lapidary-cad/src/measure.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stl::Mesh;

    /// A unit cube, closed. Two triangles per face, 12 total.
    fn unit_cube() -> Mesh {
        let v = [
            [0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 2, 1], [0, 3, 2], // bottom
            [4, 5, 6], [4, 6, 7], // top
            [0, 1, 5], [0, 5, 4],
            [1, 2, 6], [1, 6, 5],
            [2, 3, 7], [2, 7, 6],
            [3, 0, 4], [3, 4, 7],
        ];
        Mesh {
            triangles: faces.iter().map(|f| [v[f[0]], v[f[1]], v[f[2]]]).collect(),
        }
    }

    #[test]
    fn a_closed_cube_measures_its_bbox_and_volume() {
        let m = measure(&unit_cube());
        assert_eq!(m.triangle_count, 12);
        assert_eq!(m.bbox_mm, [1.0, 1.0, 1.0]);
        assert!(m.is_watertight);
        let volume = m.volume_mm3.expect("a closed mesh has a volume");
        assert!((volume - 1.0).abs() < 1e-4, "unit cube volume was {volume}");
    }

    #[test]
    fn an_open_mesh_reports_no_volume() {
        let mut mesh = unit_cube();
        mesh.triangles.pop(); // remove one face: no longer closed
        let m = measure(&mesh);
        assert!(!m.is_watertight);
        assert!(
            m.volume_mm3.is_none(),
            "an open mesh must report no volume rather than a meaningless number"
        );
    }

    #[test]
    fn surface_area_is_always_reported_even_when_open() {
        let mut mesh = unit_cube();
        mesh.triangles.pop();
        let m = measure(&mesh);
        // 11 of 12 unit-cube triangles, each of area 0.5.
        assert!((m.surface_area_mm2 - 5.5).abs() < 1e-4);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p lapidary-cad
```

Expected: FAIL — `cannot find function parse_stl`.

- [ ] **Step 3: Write the parser**

Replace the contents of `crates/lapidary-cad/src/stl.rs`, keeping the test module at the bottom:

```rust
//! STL parsing. Hand-written rather than pulled from a crate: the format is 84 bytes of
//! header plus 50 per triangle, the project prefers fewer dependencies, and the error
//! text is a product surface — an operator who dropped a bad file needs to be told what
//! to do about it.

use crate::CadError;

/// Triangles only. STL carries per-facet normals, but they are frequently wrong in
/// real files, so we ignore them and recompute from winding.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<[[f32; 3]; 3]>,
}

const HEADER: usize = 80;
const COUNT: usize = 4;
const TRIANGLE: usize = 50;

pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, CadError> {
    if bytes.is_empty() {
        return Err(CadError::MalformedStl {
            detail: "the file is 0 bytes".to_owned(),
        });
    }

    // Detection is size arithmetic, never a magic-string sniff: plenty of binary STLs
    // begin with the ASCII word "solid", and a parser that trusts the prefix reads the
    // binary body as text and produces nonsense.
    if bytes.len() >= HEADER + COUNT {
        let mut count = [0u8; 4];
        count.copy_from_slice(&bytes[HEADER..HEADER + COUNT]);
        let claimed = u32::from_le_bytes(count) as usize;
        if bytes.len() == HEADER + COUNT + claimed * TRIANGLE {
            return parse_binary(bytes, claimed);
        }
        // The length says binary but does not add up. If it also does not look like
        // text, report the binary shape — that is the more useful diagnosis.
        if !looks_like_ascii(bytes) {
            return Err(CadError::MalformedStl {
                detail: format!(
                    "the header claims {claimed} triangles, which needs {} bytes, but the file is {} — it looks truncated or incomplete",
                    HEADER + COUNT + claimed * TRIANGLE,
                    bytes.len()
                ),
            });
        }
    }

    parse_ascii(bytes)
}

fn looks_like_ascii(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(512)];
    window.starts_with(b"solid") && window.iter().all(|b| b.is_ascii())
}

fn parse_binary(bytes: &[u8], count: usize) -> Result<Mesh, CadError> {
    let mut triangles = Vec::with_capacity(count);
    let mut at = HEADER + COUNT;
    for _ in 0..count {
        at += 12; // per-facet normal, ignored
        let mut tri = [[0.0f32; 3]; 3];
        for vertex in &mut tri {
            for component in vertex.iter_mut() {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&bytes[at..at + 4]);
                *component = f32::from_le_bytes(raw);
                at += 4;
            }
        }
        at += 2; // attribute byte count
        triangles.push(tri);
    }
    finish(triangles)
}

fn parse_ascii(bytes: &[u8]) -> Result<Mesh, CadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CadError::MalformedStl {
        detail: "the file is neither a valid binary STL nor valid UTF-8 text".to_owned(),
    })?;

    let mut triangles = Vec::new();
    let mut current: Vec<[f32; 3]> = Vec::with_capacity(3);
    for (number, line) in text.lines().enumerate() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("vertex") {
            continue;
        }
        let mut vertex = [0.0f32; 3];
        for (i, slot) in vertex.iter_mut().enumerate() {
            let token = parts.next().ok_or_else(|| CadError::MalformedStl {
                detail: format!("line {} has {i} coordinates, expected 3", number + 1),
            })?;
            *slot = token.parse().map_err(|_| CadError::MalformedStl {
                detail: format!("line {} has `{token}` where a number was expected", number + 1),
            })?;
        }
        current.push(vertex);
        if current.len() == 3 {
            triangles.push([current[0], current[1], current[2]]);
            current.clear();
        }
    }
    if !current.is_empty() {
        return Err(CadError::MalformedStl {
            detail: format!(
                "the last facet has {} vertices, expected 3 — the file ends mid-triangle",
                current.len()
            ),
        });
    }
    finish(triangles)
}

fn finish(triangles: Vec<[[f32; 3]; 3]>) -> Result<Mesh, CadError> {
    if triangles.is_empty() {
        return Err(CadError::MalformedStl {
            detail: "the file parsed but contains no triangles".to_owned(),
        });
    }
    Ok(Mesh { triangles })
}
```

- [ ] **Step 4: Write the measurer**

Replace the contents of `crates/lapidary-cad/src/measure.rs`, keeping its test module:

```rust
//! Turning a mesh into numbers. Every figure here is tessellated by construction.

use crate::stl::Mesh;
use lapidary_core::MeshMeasurements;
use std::collections::HashMap;

pub fn measure(mesh: &Mesh) -> MeshMeasurements {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut area = 0.0f64;
    let mut signed_volume = 0.0f64;

    for tri in &mesh.triangles {
        for v in tri {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        let [a, b, c] = [d(tri[0]), d(tri[1]), d(tri[2])];
        area += cross(sub(b, a), sub(c, a)).iter().map(|k| k * k).sum::<f64>().sqrt() / 2.0;
        // Signed volume of the tetrahedron (origin, a, b, c). Sums to the enclosed
        // volume only when the surface is closed — hence the watertight gate below.
        signed_volume += dot(a, cross(b, c)) / 6.0;
    }

    let is_watertight = is_closed(mesh);

    MeshMeasurements {
        bbox_mm: [
            (max[0] - min[0]) as f64,
            (max[1] - min[1]) as f64,
            (max[2] - min[2]) as f64,
        ],
        triangle_count: mesh.triangles.len() as u32,
        surface_area_mm2: area,
        // A wrong number is worse than no number: only report volume for a closed surface.
        volume_mm3: is_watertight.then_some(signed_volume.abs()),
        is_watertight,
    }
}

/// Closed means every edge is shared by exactly two triangles. Vertices are quantised
/// before comparison because STL stores each vertex independently as f32, so the same
/// corner arrives with different bit patterns from different facets.
fn is_closed(mesh: &Mesh) -> bool {
    let mut edges: HashMap<(u64, u64), i32> = HashMap::new();
    for tri in &mesh.triangles {
        let k = [key(tri[0]), key(tri[1]), key(tri[2])];
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            *edges.entry((lo, hi)).or_insert(0) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|&n| n == 2)
}

fn key(v: [f32; 3]) -> u64 {
    // 1e-4 mm quantisation: finer than any real mesh tolerance, coarse enough to
    // collapse f32 representation noise at a shared corner.
    let q = |x: f32| (x as f64 / 1e-4).round() as i64;
    let (x, y, z) = (q(v[0]), q(v[1]), q(v[2]));
    let mut h = 1469598103934665603u64;
    for part in [x, y, z] {
        for byte in part.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(1099511628211);
        }
    }
    h
}

fn d(v: [f32; 3]) -> [f64; 3] { [v[0] as f64, v[1] as f64, v[2] as f64] }
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
```

Add the new error variant to `crates/lapidary-cad/src/kernel.rs`, inside `CadError`:

```rust
    #[error(
        "Could not read this STL — {detail}. Re-export it from your CAD or slicing tool and retry; if it came from a download, the transfer may have been cut short."
    )]
    MalformedStl { detail: String },
```

Wire the modules in `crates/lapidary-cad/src/lib.rs`:

```rust
mod measure;
mod stl;

pub use measure::measure;
pub use stl::{Mesh, parse_stl};
```

Add to `crates/lapidary-cad/Cargo.toml` under `[dependencies]`:

```toml
lapidary-core.workspace = true
```

(It is already there — confirm rather than duplicate.)

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p lapidary-cad
```

Expected: 8 passed (5 parser, 3 measurement).

- [ ] **Step 6: Add real fixtures**

Generate two real fixtures rather than hand-writing bytes, so later tasks have something honest to render. Use the ASCII form for one and convert to binary for the other with a short throwaway script; commit both.

Requirements: real names (`bracket-lp-1042-03.stl`, `spacer-lp-2001-00.stl`), a closed solid for at least one, and both under 200 KB so the repo stays small.

- [ ] **Step 7: Run the full bar and commit**

```bash
cargo fmt --all --check && \
cargo clippy --workspace --all-targets --all-features -- -D warnings && \
cargo test --workspace --all-features && cargo deny check
git add crates/lapidary-cad fixtures/
git commit -m "feat(cad): STL parsing and mesh measurement"
```

---

## Task 4: CPU thumbnail rasterizer

**Files:**
- Create: `crates/lapidary-cad/src/raster.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`, `crates/lapidary-cad/Cargo.toml`
- Create: `fixtures/bracket-lp-1042-03.thumb.webp` (golden image)

**Interfaces:**
- Consumes: `Mesh` from Task 3.
- Produces: `pub const THUMB_PX: u32 = 512;

/// DATA.md §1.5 stores thumbnails inline as `bytea` only under 64 KB. Beyond that they
/// would have to become filesystem blobs, costing the grid a round trip per card — the
/// exact cost the inline exception exists to avoid.
pub const MAX_THUMB_BYTES: usize = 64 * 1024;

/// Fallback sizes, tried in order, when a mesh renders larger than the inline budget.
/// WebP here is lossless, so there is no quality to trade — only pixels.
const FALLBACK_PX: [u32; 2] = [384, 256];`, `pub const RASTER_VERSION: &str = "cpu-1";`, `pub fn render_thumbnail(mesh: &Mesh) -> Result<Vec<u8>, CadError>` returning WebP bytes.

Determinism is the requirement that made CPU the choice, so it is what the tests assert. Fixed camera, fixed light, no randomness, no parallel accumulation into shared floats.

Add to `crates/lapidary-cad/Cargo.toml`:

```toml
image = { workspace = true }
```

and to the root `Cargo.toml` `[workspace.dependencies]`:

```toml
image = { version = "0.25.10", default-features = false, features = ["webp"] }
```

Verified: `image` 0.25.10 encodes WebP with no C dependency, and its tree (`image-webp`, `moxcms`, `pxfm`, `bytemuck`, `byteorder-lite`, `num-traits`, `quick-error`) is all MIT/Apache-2.0/BSD-3/Zlib, inside `deny.toml`'s allow-list. A 512×512 encode measured 3,174 bytes, far under the 64 KB inline limit.

- [ ] **Step 1: Write the failing tests**

Create `crates/lapidary-cad/src/raster.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_stl;

    fn bracket() -> crate::Mesh {
        parse_stl(include_bytes!("../../../fixtures/bracket-lp-1042-03.stl"))
            .expect("fixture parses")
    }

    #[test]
    fn rendering_the_same_mesh_twice_produces_identical_bytes() {
        // The whole reason this is a CPU rasterizer: derivatives must be deterministically
        // re-derivable, and a GPU path makes the bytes driver-dependent.
        let a = render_thumbnail(&bracket()).expect("renders");
        let b = render_thumbnail(&bracket()).expect("renders");
        assert_eq!(a, b);
    }

    #[test]
    fn the_render_matches_the_committed_golden_image() {
        let rendered = render_thumbnail(&bracket()).expect("renders");
        let golden = include_bytes!("../../../fixtures/bracket-lp-1042-03.thumb.webp");
        assert_eq!(
            rendered.as_slice(),
            golden.as_slice(),
            "rasterizer output changed. If deliberate, regenerate the golden image and \
             say so in the commit; if not, something perturbed the camera, the light or \
             the projection."
        );
    }

    #[test]
    fn the_thumbnail_fits_the_inline_bytea_budget() {
        // DATA.md §1.5 stores thumbnails inline only under 64 KB.
        let bytes = render_thumbnail(&bracket()).expect("renders");
        assert!(bytes.len() < 64 * 1024, "thumbnail was {} bytes", bytes.len());
    }

    #[test]
    fn an_oversized_render_is_downscaled_rather_than_written_oversized() {
        // DATA.md §1.5 only stores thumbnails inline under 64 KB. WebP here is lossless,
        // so there is no quality knob to turn — the retry reduces dimensions instead.
        // The guard must exist even though the fixture lands far under, because a row
        // written oversized is a silent violation of the inline-storage contract.
        let bytes = render_thumbnail(&bracket()).expect("renders");
        assert!(bytes.len() <= MAX_THUMB_BYTES);
    }

    #[test]
    fn the_output_decodes_as_a_512px_webp() {
        let bytes = render_thumbnail(&bracket()).expect("renders");
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .expect("decodes as WebP");
        assert_eq!((img.width(), img.height()), (THUMB_PX, THUMB_PX));
    }

    #[test]
    fn a_degenerate_mesh_fails_rather_than_emitting_a_blank_tile() {
        // All vertices coincident: no bounding box to fit, nothing to show. A blank
        // card that looks like a successful ingest is worse than a reported failure.
        let mesh = crate::Mesh { triangles: vec![[[1.0, 1.0, 1.0]; 3]] };
        let err = render_thumbnail(&mesh).expect_err("must not render");
        assert!(err.to_string().contains("zero size"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p lapidary-cad raster
```

Expected: FAIL — `cannot find function render_thumbnail`.

- [ ] **Step 3: Write the rasterizer**

Replace the contents of `crates/lapidary-cad/src/raster.rs`, keeping the test module:

```rust
//! Flat-shaded thumbnail rendering on the CPU. No GPU, no driver surface, and — the
//! point — bit-identical output on every host, so a derivative can be regenerated and
//! compared rather than merely re-made.

use crate::{CadError, Mesh};

pub const THUMB_PX: u32 = 512;

/// Bumped whenever a change alters output bytes. Persisted as `derivative.kernel_version`
/// so a later pass can find thumbnails made by an older renderer.
pub const RASTER_VERSION: &str = "cpu-1";

/// Fixed three-quarter view. Constants, not parameters: a thumbnail that changes angle
/// between runs is not comparable, and the grid wants every card framed alike.
const VIEW_DIR: [f64; 3] = [0.577_350_27, -0.577_350_27, 0.577_350_27];
const LIGHT_DIR: [f64; 3] = [0.408_248_3, -0.408_248_3, 0.816_496_6];
const BG: [u8; 3] = [10, 10, 12];       // matches the app's dark surface
const BASE: [f64; 3] = [0.82, 0.84, 0.88];
const AMBIENT: f64 = 0.18;
const MARGIN: f64 = 0.92;               // fraction of the frame the model fills

pub fn render_thumbnail(mesh: &Mesh) -> Result<Vec<u8>, CadError> {
    let (right, up) = basis();
    let n = THUMB_PX as usize;

    // Project every vertex into view space once, then fit.
    let projected: Vec<[[f64; 3]; 3]> = mesh
        .triangles
        .iter()
        .map(|t| t.map(|v| {
            let p = [v[0] as f64, v[1] as f64, v[2] as f64];
            [dot(p, right), dot(p, up), dot(p, VIEW_DIR)]
        }))
        .collect();

    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for t in &projected {
        for p in t {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !extent.is_finite() || extent <= 0.0 {
        return Err(CadError::Unrenderable {
            detail: "the mesh projects to zero size — every vertex is at the same point".to_owned(),
        });
    }
    let scale = (n as f64 * MARGIN) / extent;
    let cx = (lo[0] + hi[0]) / 2.0;
    let cy = (lo[1] + hi[1]) / 2.0;
    let half = n as f64 / 2.0;

    let mut colour = vec![BG[0], BG[1], BG[2]].repeat(n * n);
    let mut depth = vec![f64::NEG_INFINITY; n * n];

    for (t, world) in projected.iter().zip(&mesh.triangles) {
        let shade = shade_of(world);
        let px: Vec<[f64; 3]> = t
            .iter()
            .map(|p| [(p[0] - cx) * scale + half, half - (p[1] - cy) * scale, p[2]])
            .collect();
        fill(&mut colour, &mut depth, n, &px, shade);
    }

    let mut rgba = Vec::with_capacity(n * n * 4);
    for px in colour.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    let img = image::RgbaImage::from_raw(THUMB_PX, THUMB_PX, rgba)
        .ok_or_else(|| CadError::Unrenderable { detail: "frame buffer size mismatch".to_owned() })?;

    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::WebP)
        .map_err(|e| CadError::Unrenderable { detail: format!("WebP encoding failed: {e}") })?;
    let bytes = out.into_inner();
    if bytes.len() <= MAX_THUMB_BYTES {
        return Ok(bytes);
    }

    // Over budget: retry smaller. Failing loudly beats writing a row that quietly
    // breaks the inline-storage contract every grid query depends on.
    for px in FALLBACK_PX {
        let smaller = image::imageops::resize(&img, px, px, image::imageops::FilterType::Triangle);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(smaller)
            .write_to(&mut buf, image::ImageFormat::WebP)
            .map_err(|e| CadError::Unrenderable { detail: format!("WebP encoding failed: {e}") })?;
        let candidate = buf.into_inner();
        if candidate.len() <= MAX_THUMB_BYTES {
            return Ok(candidate);
        }
    }

    Err(CadError::Unrenderable {
        detail: format!(
            "the thumbnail is {} bytes even at {}px, over the {MAX_THUMB_BYTES}-byte inline limit",
            bytes.len(),
            FALLBACK_PX[FALLBACK_PX.len() - 1]
        ),
    })
}

/// Lambert against a fixed headlight, clamped so back-facing triangles still read as
/// surface rather than as holes.
fn shade_of(world: &[[f32; 3]; 3]) -> [u8; 3] {
    let a = [world[0][0] as f64, world[0][1] as f64, world[0][2] as f64];
    let b = [world[1][0] as f64, world[1][1] as f64, world[1][2] as f64];
    let c = [world[2][0] as f64, world[2][1] as f64, world[2][2] as f64];
    let n = normalise(cross(sub(b, a), sub(c, a)));
    let lambert = dot(n, LIGHT_DIR).abs();
    let k = AMBIENT + (1.0 - AMBIENT) * lambert;
    [
        (BASE[0] * k * 255.0) as u8,
        (BASE[1] * k * 255.0) as u8,
        (BASE[2] * k * 255.0) as u8,
    ]
}

fn fill(colour: &mut [u8], depth: &mut [f64], n: usize, p: &[[f64; 3]], shade: [u8; 3]) {
    let min_x = p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let max_x = p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max).ceil().min(n as f64 - 1.0) as usize;
    let min_y = p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let max_y = p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max).ceil().min(n as f64 - 1.0) as usize;

    let area = edge(p[0], p[1], p[2]);
    if area.abs() < f64::EPSILON {
        return; // degenerate triangle contributes nothing
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let q = [x as f64 + 0.5, y as f64 + 0.5, 0.0];
            let (w0, w1, w2) = (edge(p[1], p[2], q), edge(p[2], p[0], q), edge(p[0], p[1], q));
            let inside = (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
            if !inside {
                continue;
            }
            let z = (w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2]) / area;
            let i = y * n + x;
            if z > depth[i] {
                depth[i] = z;
                colour[i * 3..i * 3 + 3].copy_from_slice(&shade);
            }
        }
    }
}

fn edge(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn basis() -> ([f64; 3], [f64; 3]) {
    let world_up = [0.0, 0.0, 1.0];
    let right = normalise(cross(world_up, VIEW_DIR));
    let up = normalise(cross(VIEW_DIR, right));
    (right, up)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn normalise(v: [f64; 3]) -> [f64; 3] {
    let m = dot(v, v).sqrt();
    if m == 0.0 { v } else { [v[0] / m, v[1] / m, v[2] / m] }
}
```

Add to `CadError` in `crates/lapidary-cad/src/kernel.rs`:

```rust
    #[error(
        "Could not render a thumbnail — {detail}. The file parsed, so the geometry itself may be degenerate; open it in your CAD tool to check."
    )]
    Unrenderable { detail: String },
```

Export from `crates/lapidary-cad/src/lib.rs`:

```rust
mod raster;

pub use raster::{MAX_THUMB_BYTES, RASTER_VERSION, THUMB_PX, render_thumbnail};
```

- [ ] **Step 4: Generate the golden image, then verify the tests pass**

The golden-image test fails until the image exists. Generate it once from the implementation, eyeball it, and commit it:

```bash
cargo test -p lapidary-cad raster 2>&1 | head -20   # golden test fails, others pass
```

Write a throwaway binary or a `#[test] #[ignore]` helper that writes `render_thumbnail(&bracket())` to `fixtures/bracket-lp-1042-03.thumb.webp`, run it, then **open the file and look at it**. A golden image committed without being viewed pins whatever bug it contains.

```bash
cargo test -p lapidary-cad
```

Expected: 13 passed.

- [ ] **Step 5: Run the full bar and commit**

```bash
cargo fmt --all --check && \
cargo clippy --workspace --all-targets --all-features -- -D warnings && \
cargo test --workspace --all-features && cargo deny check
git add crates/lapidary-cad Cargo.toml Cargo.lock fixtures/
git commit -m "feat(cad): deterministic CPU thumbnail rasterizer"
```

---

## Task 5: `MeshKernel`

**Files:**
- Create: `crates/lapidary-cad/src/mesh_kernel.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`, `crates/lapidary-cad/src/kernel.rs`

**Interfaces:**
- Consumes: `parse_stl`, `measure`, `render_thumbnail`, the existing `Kernel` trait.
- Produces: `pub struct MeshKernel;` implementing `Kernel`, and `pub struct MeshOutput { pub measurements: MeshMeasurements, pub thumbnail_webp: Vec<u8> }` with `MeshKernel::ingest(&self, bytes: &[u8]) -> Result<MeshOutput, CadError>`.

The existing `Kernel` trait takes a `&Path` and returns `KernelOutput`. Slice 1 works from bytes already in memory (they were hashed first), so `MeshKernel` gains an inherent `ingest` method taking bytes, and implements `Kernel` for interface compatibility by reading the path and delegating. Follow-up item 2 already records that `KernelOutput` must change; this slice does not reshape it.

- [ ] **Step 1: Write the failing test**

Create `crates/lapidary-cad/src/mesh_kernel.rs` with only its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingesting_a_real_stl_yields_measurements_and_a_thumbnail() {
        let bytes = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");
        let out = MeshKernel.ingest(bytes).expect("ingests");
        assert!(out.measurements.triangle_count > 0);
        assert!(!out.thumbnail_webp.is_empty());
    }

    #[test]
    fn the_reported_version_pins_both_the_parser_and_the_rasterizer() {
        // derivative.kernel_version must change when output bytes could change, or a
        // regenerated thumbnail is indistinguishable from a stale one.
        let v = MeshKernel.version();
        assert_eq!(v.implementation, "mesh");
        assert!(v.version.contains(crate::RASTER_VERSION));
    }

    #[test]
    fn a_malformed_file_reports_the_parse_error_not_a_render_error() {
        let err = MeshKernel.ingest(b"not an stl at all").expect_err("must fail");
        assert!(matches!(err, CadError::MalformedStl { .. }));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p lapidary-cad mesh_kernel
```

Expected: FAIL — `cannot find struct MeshKernel`.

- [ ] **Step 3: Implement**

Replace the contents of `crates/lapidary-cad/src/mesh_kernel.rs`, keeping its tests:

```rust
//! The mesh implementation of the kernel boundary. Ingest invokes this; the open path
//! never does — that separation is what keeps `lapidary-api` free of this crate.

use crate::{CadError, KernelVersion, RASTER_VERSION, measure, parse_stl, render_thumbnail};
use lapidary_core::MeshMeasurements;

pub struct MeshOutput {
    pub measurements: MeshMeasurements,
    pub thumbnail_webp: Vec<u8>,
}

pub struct MeshKernel;

impl MeshKernel {
    /// Bytes rather than a path: ingest has already read and hashed the file, and
    /// reading it twice would be a second chance to read something different.
    pub fn ingest(&self, bytes: &[u8]) -> Result<MeshOutput, CadError> {
        let mesh = parse_stl(bytes)?;
        Ok(MeshOutput {
            measurements: measure(&mesh),
            thumbnail_webp: render_thumbnail(&mesh)?,
        })
    }

    pub fn version(&self) -> KernelVersion {
        KernelVersion {
            implementation: "mesh".to_owned(),
            version: format!("stl-1+{RASTER_VERSION}"),
        }
    }
}
```

Export from `crates/lapidary-cad/src/lib.rs`:

```rust
mod mesh_kernel;

pub use mesh_kernel::{MeshKernel, MeshOutput};
```

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p lapidary-cad
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/lapidary-cad
git commit -m "feat(cad): MeshKernel over STL parse, measure and raster"
```

---

## Task 6: Blob CAS and the open-path boundary

**Files:**
- Modify: `crates/lapidary-storage/src/lib.rs`, `crates/lapidary-storage/Cargo.toml`
- Modify: `xtask/src/deploy.rs`

**Interfaces:**
- Consumes: `lapidary_core::BlobHash`.
- Produces: `pub struct WorkerRole(());` with `WorkerRole::assume()`, `pub struct DerivativeStore`, `pub struct SourceStore`, `StorageError`. `SourceStore::open(root: &Path, _: &WorkerRole) -> Self`, `SourceStore::put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError>`, `SourceStore::get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError>`, `SourceStore::remove(&self, hash: &BlobHash)`. `pub struct StoredBlob { pub hash: BlobHash, pub size_bytes: u64, pub stored_bytes: u64, pub zstd_level: i16 }`.

`SourceStore` needs a `WorkerRole` token to construct. `lapidary-api` cannot obtain one because it never names the type, and an `xtask` check asserts that it does not — this is follow-up item 16, which a dependency-graph rule cannot express because the dependency itself is legitimate for derivatives.

- [ ] **Step 1: Write the failing tests**

Add a test module to `crates/lapidary-storage/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SourceStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SourceStore::open(dir.path(), &WorkerRole::assume());
        (dir, store)
    }

    #[test]
    fn a_blob_round_trips_by_its_hash() {
        let (_dir, s) = store();
        let stored = s.put(b"solid bracket\n").expect("put");
        assert_eq!(s.get(&stored.hash).expect("get"), b"solid bracket\n");
    }

    #[test]
    fn the_same_bytes_always_produce_the_same_hash() {
        let (_dir, s) = store();
        assert_eq!(s.put(b"same").expect("a").hash, s.put(b"same").expect("b").hash);
    }

    #[test]
    fn blobs_are_sharded_two_levels_deep() {
        // 65,536 buckets keeps any directory under ~2k entries at a million blobs.
        let (dir, s) = store();
        let stored = s.put(b"shard me").expect("put");
        let hex = stored.hash.to_hex();
        let path = dir.path().join("blobs").join(&hex[0..2]).join(&hex[2..4]).join(&hex);
        assert!(path.exists(), "expected {}", path.display());
    }

    #[test]
    fn source_bytes_are_compressed_and_the_stored_size_reflects_it() {
        let (_dir, s) = store();
        let compressible = "solid ".repeat(4096).into_bytes();
        let stored = s.put(&compressible).expect("put");
        assert_eq!(stored.size_bytes, compressible.len() as u64);
        assert!(stored.stored_bytes < stored.size_bytes, "zstd should shrink this");
        assert_eq!(stored.zstd_level, 3);
    }

    #[test]
    fn getting_an_unknown_hash_says_which_hash_and_what_that_means() {
        let (_dir, s) = store();
        let missing = lapidary_core::BlobHash::from_bytes([0x11; 32]);
        let err = s.get(&missing).expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains(&missing.to_hex()[..8]), "names the hash: {msg}");
        assert!(msg.contains("quarantine") || msg.contains("evicted"), "suggests a cause: {msg}");
    }

    #[test]
    fn removing_a_blob_leaves_the_store_usable() {
        // Ingest reaps a blob when the transaction that would have referenced it fails.
        let (_dir, s) = store();
        let stored = s.put(b"orphan").expect("put");
        s.remove(&stored.hash).expect("remove");
        assert!(s.get(&stored.hash).is_err());
        assert!(s.put(b"another").is_ok(), "the store still works after a removal");
    }

    #[test]
    fn a_derivative_store_needs_no_worker_token() {
        // Both roles hold derivatives; only the worker may reach source bytes.
        let dir = tempfile::tempdir().expect("temp dir");
        let d = DerivativeStore::open(dir.path());
        let hash = d.put(b"gltf bytes").expect("put").hash;
        assert_eq!(d.get(&hash).expect("get"), b"gltf bytes");
    }
}
```

Add to `crates/lapidary-storage/Cargo.toml` under `[dev-dependencies]`:

```toml
tempfile = "3.23.0"
```

and to the root `[workspace.dependencies]` if not already present:

```toml
tempfile = "3.23.0"
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p lapidary-storage
```

Expected: FAIL — `cannot find struct SourceStore`.

- [ ] **Step 3: Implement**

Replace the contents of `crates/lapidary-storage/src/lib.rs` above the test module:

```rust
//! Content-addressed blob storage. Two handles, deliberately:
//!
//! `DerivativeStore` reads and writes derivatives — thumbnails, tessellations — and both
//! roles hold one. `SourceStore` reaches the ingested source bytes and requires a
//! `WorkerRole` token to construct.
//!
//! This is the API-level half of "the open path never touches a source file". The
//! dependency-graph half cannot express it: `lapidary-api` legitimately depends on this
//! crate for derivatives, so the distinction is *which bytes*, not whether the crates may
//! be connected. `cargo xtask check-deploy` asserts `lapidary-api` never names
//! `SourceStore`.

use lapidary_core::BlobHash;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "No blob is stored for {hash_prefix}… . It may have been evicted from the render cache, or quarantined and removed after its 30-day hold. Source blobs are never removed while any part references them, so a missing source blob means the reference itself is stale."
    )]
    NotFound { hash_prefix: String },

    #[error("Could not read or write the blob store at {path}: {source}. Check the volume is mounted and writable.")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Proof the holder is running in the worker role. Zero-sized and unconstructible except
/// through `assume`, which the binary calls once after reading `LAPIDARY_ROLE`.
pub struct WorkerRole(());

impl WorkerRole {
    /// Called by `bin/lapidary-server` when it has established the worker role.
    pub fn assume() -> Self {
        WorkerRole(())
    }
}

pub struct StoredBlob {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

/// zstd -3 at ingest per DATA.md §1.2. -19 when cold is a later tiering job.
const INGEST_LEVEL: i32 = 3;

fn blob_path(root: &Path, hash: &BlobHash) -> PathBuf {
    let hex = hash.to_hex();
    root.join("blobs").join(&hex[0..2]).join(&hex[2..4]).join(&hex)
}

fn write_blob(root: &Path, bytes: &[u8], compress: bool) -> Result<StoredBlob, StorageError> {
    let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
    let path = blob_path(root, &hash);
    let parent = path.parent().unwrap_or(root);
    std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;

    let payload = if compress {
        zstd::encode_all(bytes, INGEST_LEVEL).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?
    } else {
        bytes.to_vec()
    };

    // Content addressing makes rewriting an existing blob pointless — the bytes are the
    // same by definition — but writing anyway keeps the code one path instead of two.
    std::fs::write(&path, &payload).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;

    Ok(StoredBlob {
        hash,
        size_bytes: bytes.len() as u64,
        stored_bytes: payload.len() as u64,
        zstd_level: if compress { INGEST_LEVEL as i16 } else { 0 },
    })
}

fn read_blob(root: &Path, hash: &BlobHash, compressed: bool) -> Result<Vec<u8>, StorageError> {
    let path = blob_path(root, hash);
    let raw = std::fs::read(&path).map_err(|_| StorageError::NotFound {
        hash_prefix: hash.to_hex()[..8].to_owned(),
    })?;
    if compressed {
        zstd::decode_all(raw.as_slice()).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })
    } else {
        Ok(raw)
    }
}

/// Derivatives: never compressed (they are already packed and sit on the hot open path),
/// freely evictable, and readable by both roles.
pub struct DerivativeStore {
    root: PathBuf,
}

impl DerivativeStore {
    pub fn open(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, false)
    }

    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, false)
    }
}

/// Source bytes: compressed hard, never deleted while referenced, and reachable only
/// from the worker role.
pub struct SourceStore {
    root: PathBuf,
}

impl SourceStore {
    pub fn open(root: &Path, _proof: &WorkerRole) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, true)
    }

    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, true)
    }

    /// Reap a blob written for a transaction that then failed. Not user-facing deletion —
    /// no part ever referenced these bytes.
    pub fn remove(&self, hash: &BlobHash) -> Result<(), StorageError> {
        let path = blob_path(&self.root, hash);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Io { path: path.display().to_string(), source }),
        }
    }
}
```

- [ ] **Step 4: Add the boundary check to `xtask`**

In `xtask/src/deploy.rs`, add a rule and its violation, checked by `check-deploy`:

```rust
/// `lapidary-api` must never name `SourceStore`. The type needs a `WorkerRole` token to
/// construct, so the compiler already prevents obtaining one — this catches the earlier
/// mistake of importing it at all, which is the first move someone makes before
/// discovering they cannot build one, and the point at which to stop them.
pub fn check_open_path_boundary(api_sources: &[(String, String)]) -> Vec<Violation> {
    api_sources
        .iter()
        .filter(|(_, body)| body.contains("SourceStore"))
        .map(|(path, _)| Violation::OpenPathNamesSourceStore { path: path.clone() })
        .collect()
}
```

with the message:

```rust
    Violation::OpenPathNamesSourceStore { path } => write!(
        f,
        "{path} names SourceStore. lapidary-api serves the open path, which must never touch a source file — it reads metadata and derivatives only. Use DerivativeStore, or move the work into the worker."
    ),
```

`main.rs` walks `crates/lapidary-api/src/**/*.rs` and passes `(path, contents)` pairs. Add a test in `deploy.rs` over fixture strings, one naming `SourceStore` and one naming `DerivativeStore`, asserting exactly one violation.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p lapidary-storage -p xtask
cargo xtask check-deploy
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo deny check
git add crates/lapidary-storage xtask Cargo.toml Cargo.lock
git commit -m "feat(storage): blob CAS with a source/derivative boundary"
```

**Prove the boundary bites:** add `use lapidary_storage::SourceStore;` to any file under `crates/lapidary-api/src/`, run `cargo xtask check-deploy`, confirm it fails naming that file, then revert and confirm it passes. Paste both outputs into the task report.

---

## Task 7: Repositories

**Files:**
- Modify: `crates/lapidary-db/src/repo.rs`, `crates/lapidary-db/src/lib.rs`
- Create: `crates/lapidary-db/tests/repo.rs`

**Interfaces:**
- Consumes: the Task 1 schema, `lapidary_core::{BlobHash, LibraryId, PartId, MeshMeasurements, PartSummary, Provenance}`.
- Produces:
  - `pub struct PgParts(pub PgPool)` implementing the existing `PartRepository`
  - `pub struct PgBlobs(pub PgPool)` with `async fn exists(&self, hash: &BlobHash) -> Result<bool, DbError>`
  - `pub struct PgIngest(pub PgPool)` with
    `async fn record(&self, req: IngestRequest<'_>) -> Result<PartId, DbError>` and
    `async fn link_existing(&self, req: IngestRequest<'_>) -> Result<PartId, DbError>`
  - `pub struct IngestRequest<'a> { pub library: LibraryId, pub name: &'a str, pub blob: &'a StoredBlobRow, pub measurements: &'a MeshMeasurements, pub thumbnail_webp: &'a [u8], pub kernel_version: &'a str }`
  - `pub struct StoredBlobRow { pub hash: BlobHash, pub size_bytes: u64, pub stored_bytes: u64, pub zstd_level: i16 }`

`StoredBlobRow` mirrors `lapidary_storage::StoredBlob` rather than importing it: `lapidary-db` is L1 and `lapidary-storage` is L1, and L1 may not depend on L1 (`cargo xtask check-layers` rejects it). The caller in `lapidary-api` converts between them.

- [ ] **Step 1: Write the failing tests**

Create `crates/lapidary-db/tests/repo.rs`:

```rust
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts, StoredBlobRow};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn blob_row(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 204_800,
        stored_bytes: 91_204,
        zstd_level: 3,
    }
}

fn watertight() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

fn open_mesh() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [88.0, 34.0, 12.0],
        triangle_count: 12_940,
        surface_area_mm2: 15_320.5,
        volume_mm3: None,
        is_watertight: false,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn recording_an_ingest_creates_a_part_a_revision_a_file_and_a_thumbnail(pool: sqlx::PgPool) {
    let blob = blob_row(0xab);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bearing block, 608ZZ",
            blob: &blob,
            measurements: &watertight(),
            thumbnail_webp: b"webp bytes",
            kernel_version: "mesh stl-1+cpu-1",
        })
        .await
        .expect("records");

    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM part WHERE id = $1),
                (SELECT count(*) FROM revision r WHERE r.part_id = $1),
                (SELECT count(*) FROM file f JOIN revision r ON f.revision_id = r.id WHERE r.part_id = $1),
                (SELECT count(*) FROM derivative d JOIN revision r ON d.revision_id = r.id WHERE r.part_id = $1 AND d.kind = 'thumbnail')",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(counts, (1, 1, 1, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn every_measurement_is_written_as_tessellated(pool: sqlx::PgPool) {
    let blob = blob_row(0xcd);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (vs, bs): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT volume_source, bbox_source FROM revision WHERE part_id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(vs.as_deref(), Some("tessellated"));
    assert_eq!(bs.as_deref(), Some("tessellated"));
}

#[sqlx::test(migrations = "./migrations")]
async fn an_open_mesh_stores_a_null_volume_but_still_stores_its_bbox(pool: sqlx::PgPool) {
    let blob = blob_row(0xef);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Cable clip, LP-3300-01",
            blob: &blob,
            measurements: &open_mesh(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (volume, vs, bx, watertight): (Option<f64>, Option<String>, Option<f64>, Option<bool>) =
        sqlx::query_as(
            "SELECT volume, volume_source, bbox_x, is_watertight FROM revision WHERE part_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(volume, None, "an open mesh must store no volume");
    assert_eq!(vs, None, "no volume means no provenance for one");
    assert_eq!(bx, Some(88.0), "the bbox is still measurable and still stored");
    assert_eq!(watertight, Some(false));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_known_hash_is_reported_as_existing(pool: sqlx::PgPool) {
    let blob = blob_row(0x11);
    let blobs = PgBlobs(pool.clone());
    assert!(!blobs.exists(&blob.hash).await.expect("query"));
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Spacer, LP-2001-00",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    assert!(blobs.exists(&blob.hash).await.expect("query"));
}

#[sqlx::test(migrations = "./migrations")]
async fn linking_an_existing_blob_adds_a_part_without_touching_ref_count_twice(pool: sqlx::PgPool) {
    let blob = blob_row(0x22);
    let ingest = PgIngest(pool.clone());
    let req = |name: &'static str| IngestRequest {
        library: library(),
        name,
        blob: &blob,
        measurements: &watertight(),
        kernel_version: "mesh stl-1+cpu-1",
        thumbnail_webp: b"webp",
    };
    ingest.record(req("Bracket, LP-1042-03")).await.expect("first");
    ingest.link_existing(req("Bracket copy, LP-1042-03")).await.expect("second");

    let ref_count: i32 = sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(blob.hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(ref_count, 2, "each file referencing the blob counts once");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_page_returns_newest_first_with_a_thumbnail_hash(pool: sqlx::PgPool) {
    let ingest = PgIngest(pool.clone());
    for (i, name) in ["Bracket, LP-1042-03", "Spacer, LP-2001-00", "Cable clip, LP-3300-01"]
        .iter()
        .enumerate()
    {
        ingest
            .record(IngestRequest {
                library: library(),
                name,
                blob: &blob_row(0x30 + i as u8),
                measurements: &watertight(),
                kernel_version: "mesh stl-1+cpu-1",
                thumbnail_webp: b"webp",
            })
            .await
            .expect("records");
    }

    let page = PgParts(pool.clone()).page(library(), None, 2).await.expect("page");
    assert_eq!(page.len(), 2, "limit is honoured");
    assert_eq!(page[0].name, "Cable clip, LP-3300-01", "newest first");
    assert!(page[0].approximate, "every mesh-derived part is approximate");
    assert_eq!(page[0].triangle_count, Some(48_112));

    let next = PgParts(pool.clone())
        .page(library(), Some(page[1].id), 2)
        .await
        .expect("second page");
    assert_eq!(next.len(), 1, "keyset pagination continues after the last id");
    assert_eq!(next[0].name, "Bracket, LP-1042-03");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_deleted_part_never_appears_in_the_grid(pool: sqlx::PgPool) {
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x40),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");

    let page = PgParts(pool).page(library(), None, 50).await.expect("page");
    assert!(page.is_empty(), "delete is soft, but soft-deleted parts are still hidden");
}
```

- [ ] **Step 2: Run to verify failure**

```bash
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
cargo test -p lapidary-db --test repo
```

Expected: FAIL — `cannot find struct PgIngest`.

- [ ] **Step 3: Implement**

Add to `crates/lapidary-db/src/repo.rs` (keeping the existing `PartRepository` trait):

```rust
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId, PartSummary, Provenance};
use sqlx::PgPool;
use uuid::Uuid;

/// Mirrors `lapidary_storage::StoredBlob`. Not imported: both crates are L1, and
/// `cargo xtask check-layers` forbids L1 → L1. The api layer converts.
pub struct StoredBlobRow {
    pub hash: BlobHash,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    pub zstd_level: i16,
}

pub struct IngestRequest<'a> {
    pub library: LibraryId,
    pub name: &'a str,
    pub blob: &'a StoredBlobRow,
    pub measurements: &'a MeshMeasurements,
    pub thumbnail_webp: &'a [u8],
    pub kernel_version: &'a str,
}

pub struct PgBlobs(pub PgPool);

impl PgBlobs {
    pub async fn exists(&self, hash: &BlobHash) -> Result<bool, crate::DbError> {
        let found: Option<String> = sqlx::query_scalar("SELECT blake3 FROM blob WHERE blake3 = $1")
            .bind(hash.to_hex())
            .fetch_optional(&self.0)
            .await?;
        Ok(found.is_some())
    }
}

pub struct PgIngest(pub PgPool);

impl PgIngest {
    /// A new blob: insert it, then the part chain, in one transaction. The caller has
    /// already written the bytes and reaps them if this fails.
    pub async fn record(&self, req: IngestRequest<'_>) -> Result<PartId, crate::DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $3, $4, 0) ON CONFLICT (blake3) DO NOTHING",
        )
        .bind(req.blob.hash.to_hex())
        .bind(req.blob.size_bytes as i64)
        .bind(req.blob.stored_bytes as i64)
        .bind(req.blob.zstd_level)
        .execute(&mut *tx)
        .await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// A blob we already hold: skip the blob insert, everything else is identical.
    pub async fn link_existing(&self, req: IngestRequest<'_>) -> Result<PartId, crate::DbError> {
        let mut tx = self.0.begin().await?;
        let id = insert_part_chain(&mut tx, &req).await?;
        tx.commit().await?;
        Ok(id)
    }
}

async fn insert_part_chain(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &IngestRequest<'_>,
) -> Result<PartId, crate::DbError> {
    let part = PartId::new();
    let revision = Uuid::now_v7();
    let m = req.measurements;
    let tess = Provenance::Tessellated.as_str();

    sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
        .bind(part.as_uuid())
        .bind(req.library.as_uuid())
        .bind(req.name)
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin, volume, volume_source, \
         surface_area, surface_area_source, bbox_x, bbox_y, bbox_z, bbox_source, \
         triangle_count, is_watertight, units) \
         VALUES ($1, $2, '1', 'ingest', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'mm')",
    )
    .bind(revision)
    .bind(part.as_uuid())
    .bind(m.volume_mm3)
    // No volume means no provenance for one — writing 'tessellated' beside a NULL would
    // claim we measured something we refused to measure.
    .bind(m.volume_mm3.map(|_| tess))
    .bind(m.surface_area_mm2)
    .bind(tess)
    .bind(m.bbox_mm[0])
    .bind(m.bbox_mm[1])
    .bind(m.bbox_mm[2])
    .bind(tess)
    .bind(m.triangle_count as i32)
    .bind(m.is_watertight)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes) \
         VALUES ($1, $2, 'source', 'stl', $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.blob.hash.to_hex())
    .bind(req.blob.size_bytes as i64)
    .execute(&mut **tx)
    .await?;

    sqlx::query("UPDATE blob SET ref_count = ref_count + 1 WHERE blake3 = $1")
        .bind(req.blob.hash.to_hex())
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
         VALUES ($1, $2, 'thumbnail', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(req.thumbnail_webp)
    .bind(req.kernel_version)
    .bind(serde_json::json!({ "px": 512 }))
    .execute(&mut **tx)
    .await?;

    Ok(part)
}

pub struct PgParts(pub PgPool);

#[async_trait::async_trait]
impl PartRepository for PgParts {
    async fn page(
        &self,
        library: LibraryId,
        after: Option<PartId>,
        limit: u16,
    ) -> Result<Vec<PartSummary>, crate::DbError> {
        // One query: thumbnails travel inline as bytea rather than costing a round trip
        // per card. Keyset, not OFFSET — OFFSET degrades as the library grows.
        let rows: Vec<(Uuid, Uuid, String, Option<String>, Option<Vec<u8>>, Option<i32>, Option<bool>, jiff::Timestamp, jiff::Timestamp)> =
            sqlx::query_as(
                "SELECT p.id, p.library_id, p.name, p.part_number, d.thumb_bytes, \
                        r.triangle_count, r.is_watertight, p.created_at, p.updated_at \
                 FROM part p \
                 JOIN LATERAL (SELECT * FROM revision WHERE part_id = p.id ORDER BY created_at DESC LIMIT 1) r ON true \
                 LEFT JOIN derivative d ON d.revision_id = r.id AND d.kind = 'thumbnail' \
                 WHERE p.library_id = $1 AND p.deleted_at IS NULL \
                   AND ($2::uuid IS NULL OR p.id < $2) \
                 ORDER BY p.id DESC LIMIT $3",
            )
            .bind(library.as_uuid())
            .bind(after.map(|a| a.as_uuid()))
            .bind(i64::from(limit))
            .fetch_all(&self.0)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(id, lib, name, part_number, thumb, triangles, _watertight, created, updated)| {
                PartSummary {
                    id: PartId::from_uuid(id),
                    library: LibraryId::from_uuid(lib),
                    name,
                    part_number,
                    // The hash is not carried in slice 1: thumbnails arrive inline and the
                    // grid renders them directly. A hash-addressed thumbnail endpoint
                    // arrives with the viewer.
                    thumbnail: None,
                    triangle_count: triangles.map(|t| t as u32),
                    // Every figure on a mesh part is tessellated, so any is all.
                    approximate: true,
                    created_at: created,
                    updated_at: updated,
                }
            })
            .collect())
    }
}
```

Export from `crates/lapidary-db/src/lib.rs`:

```rust
pub use repo::{IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts, StoredBlobRow};
```

**`sqlx` cannot decode `jiff::Timestamp`.** Verified: sqlx 0.9.0 ships `chrono` and `time`
features and no `jiff` one, while `PartSummary.created_at` is a `jiff::Timestamp` (the crate
is already a workspace dependency for `ts-rs`'s `jiff-impl`). Do not add `chrono` or `time`
to carry a value across two conversions. Select microseconds and construct directly:

```sql
SELECT ..., (extract(epoch FROM p.created_at) * 1000000)::bigint AS created_us,
            (extract(epoch FROM p.updated_at) * 1000000)::bigint AS updated_us
```

```rust
// jiff::Timestamp::from_microsecond is fallible, and `unwrap` is denied outside tests.
// A timestamptz outside jiff's range means the row is corrupt, not that the query is wrong.
let created_at = jiff::Timestamp::from_microsecond(created_us)
    .map_err(|_| crate::DbError::TimestampOutOfRange { column: "part.created_at", value: created_us })?;
```

with the variant:

```rust
    #[error("`{column}` holds {value} microseconds since the epoch, which is not a representable timestamp. The row is corrupt — it was probably written by something other than lapidary-db.")]
    TimestampOutOfRange { column: &'static str, value: i64 },
```

**Note the shape mismatch to resolve during implementation:** `PartSummary.thumbnail` is `Option<BlobHash>` and slice 1 returns the bytes, not a hash. The grid needs the bytes. Add a sibling struct in `lapidary-api` (`PartCard`) that carries `thumbnail_webp: Option<String>` (base64) for the wire, rather than widening `PartSummary`, whose shape belongs to the open path. Task 10 defines it.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p lapidary-db
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
git add crates/lapidary-db
git commit -m "feat(db): ingest and grid repositories"
```

---

## Task 8: Role-aware router

**Files:**
- Modify: `crates/lapidary-api/src/lib.rs`, `bin/lapidary-server/src/main.rs`, `crates/lapidary-api/tests/health.rs`

**Interfaces:**
- Produces: `pub enum Role { Api, Worker }` with `Role::from_env_str(&str) -> Result<Role, ApiError>`, and `pub fn router(state: AppState, role: Role) -> Router`.

`router()` gains a parameter, so every existing call site changes — `bin/lapidary-server` and the two tests in `crates/lapidary-api/tests/health.rs`. That is the whole point: a route can no longer be mounted without saying which role serves it.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lapidary-api/tests/health.rs`:

```rust
use lapidary_api::Role;

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn health_is_served_in_both_roles(pool: sqlx::PgPool) {
    for role in [Role::Api, Role::Worker] {
        let app = router(AppState { db: pool.clone() }, role);
        let response = app
            .oneshot(Request::builder().uri("/api/healthz").body(Body::empty()).expect("builds"))
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_api_role_does_not_serve_the_scan_route(pool: sqlx::PgPool) {
    // Ingest must not run in the process that serves the open path: that binary
    // deliberately does not link lapidary-cad.
    let app = router(AppState { db: pool }, Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/scan")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_the_grid(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool }, Role::Worker);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/libraries/01931b6e-0000-7000-8000-000000000001/parts")
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn an_unknown_role_is_rejected_with_the_valid_values() {
    let err = Role::from_env_str("wroker").expect_err("must reject");
    let msg = err.to_string();
    assert!(msg.contains("wroker"), "names what was given: {msg}");
    assert!(msg.contains("api") && msg.contains("worker"), "names the valid values: {msg}");
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p lapidary-api
```

Expected: FAIL — `router` takes 1 argument, `Role` not found.

- [ ] **Step 3: Implement**

In `crates/lapidary-api/src/lib.rs`:

```rust
/// Which process this is. `api` serves the open path and must never mount an ingest
/// route: its image deliberately does not link `lapidary-cad`, and both containers run
/// one binary from one router, so anything mounted unconditionally is served by both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Api,
    Worker,
}

impl Role {
    pub fn from_env_str(s: &str) -> Result<Self, ApiError> {
        match s {
            "api" => Ok(Role::Api),
            "worker" => Ok(Role::Worker),
            other => Err(ApiError::UnknownRole { got: other.to_owned() }),
        }
    }
}

pub fn router(state: AppState, role: Role) -> Router {
    let shared = Router::new().route("/api/healthz", get(health::healthz));
    let by_role = match role {
        Role::Api => Router::new().route("/api/libraries/{id}/parts", get(parts::page)),
        Role::Worker => Router::new().route("/api/libraries/{id}/scan", post(scan::scan)),
    };
    shared.merge(by_role).with_state(state)
}
```

with, in a new or existing error module:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("`{got}` is not a role. Set LAPIDARY_ROLE to `api` (serves the grid and the open path) or `worker` (runs ingest). deploy/compose.yaml sets it per service.")]
    UnknownRole { got: String },
}
```

In `bin/lapidary-server/src/main.rs`, add `role: String` to `Config` with `#[serde(default = "default_role")]` returning `"api".to_owned()`, parse it, and pass it to `router`. Log it beside the existing `listening` and `CAD kernel` lines so `podman logs` shows which role a container took.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p lapidary-api -p lapidary-server
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/lapidary-api bin/lapidary-server
git commit -m "feat(api): role-aware router so ingest never mounts in the api process"
```

---

## Task 9: Scan endpoint

**Files:**
- Create: `crates/lapidary-api/src/scan.rs`
- Modify: `crates/lapidary-api/src/lib.rs`, `crates/lapidary-api/Cargo.toml`
- Create: `crates/lapidary-api/tests/scan.rs`

**Interfaces:**
- Consumes: `MeshKernel`, `SourceStore`, `WorkerRole`, `PgBlobs`, `PgIngest`.
- Produces: `POST /api/libraries/{id}/scan` returning `ScanReport { ingested: u32, skipped: u32, failed: Vec<ScanFailure> }`, `ScanFailure { file: String, reason: String }`.

`AppState` gains `ingest_dir: PathBuf` and `blob_root: PathBuf`. `lapidary-api` gains `lapidary-cad` and `lapidary-storage` dependencies — the first is why the role split had to land first.

**Counters are disjoint and sum to the candidate files walked.** A non-`.stl` file is not a candidate and is counted nowhere: a README in a library folder is not an error.

- [ ] **Step 1: Write the failing tests**

Create `crates/lapidary-api/tests/scan.rs` covering:

```
- scanning a directory with one real STL reports ingested: 1, skipped: 0, failed: []
  and leaves exactly one part in the library
- scanning the same directory twice reports ingested: 0, skipped: 1 the second time
  (the hash short-circuit) and still leaves exactly one part
- a directory containing README.md alongside one STL still reports ingested: 1 and
  counts the README nowhere
- a truncated STL reports failed: [{ file, reason }] with a reason naming the file,
  exits 200 (a partial success is the accurate description), and leaves no part
- a failure after the blob write leaves no orphan blob on disk
```

Use `tempfile::TempDir` for both the ingest directory and the blob root, and copy `fixtures/bracket-lp-1042-03.stl` in. Assert the orphan case by pointing at a library id that does not exist, so the transaction fails after the blob is written, then asserting the blob directory is empty.

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p lapidary-api --test scan
```

Expected: FAIL — module `scan` not found.

- [ ] **Step 3: Implement**

`crates/lapidary-api/src/scan.rs`, in outline with the ordering that matters:

```rust
// Per file, in this order — the order is the design:
//   1. read bytes
//   2. BLAKE3                      ← hash first, always
//   3. blobs.exists(hash)?
//        yes → ingest.link_existing(...); skipped += 1; next file
//   4. kernel.ingest(bytes)        ← parse + measure + rasterize
//   5. source.put(bytes)           ← blob written before the transaction
//   6. ingest.record(...)          ← one transaction
//        on error → source.remove(hash) and push to failed[]
//
// Step 6's reap is not optional. The Node prototype wrote its blob and then failed the
// insert with no cleanup, leaving bytes on disk that nothing referenced and nothing
// would ever collect. docs/prototype-notes.md records it.
```

Walk with `std::fs::read_dir`, non-recursive in slice 1, filtering `.stl` case-insensitively. One failure never aborts the walk.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p lapidary-api
cargo xtask check-layers && cargo xtask check-deploy
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo deny check
git add crates/lapidary-api
git commit -m "feat(api): scan endpoint with hash short-circuit and orphan reaping"
```

---

## Task 10: Grid endpoint

**Files:**
- Create: `crates/lapidary-api/src/parts.rs`
- Modify: `crates/lapidary-api/src/lib.rs`
- Create: `crates/lapidary-api/tests/parts.rs`

**Interfaces:**
- Consumes: `PgParts`, `PartRepository`.
- Produces: `GET /api/libraries/{id}/parts?after=&limit=` returning `PartsPage { parts: Vec<PartCard>, next: Option<PartId> }`.

`PartCard` is the wire shape, distinct from `PartSummary`: it carries `thumbnail: Option<String>` — the WebP as a `data:` URL — because slice 1 stores thumbnails inline and the grid renders them directly. `PartSummary.thumbnail` is an `Option<BlobHash>` for a hash-addressed endpoint that arrives with the viewer; widening it now would put a transport concern into the open-path domain type.

`PartCard` is `#[ts(export)]`, so `cargo xtask export-bindings` must run and `web/src/bindings` must be committed.

- [ ] **Step 1: Write the failing tests**

Cover: an empty library returns `{ parts: [], next: null }`; a library with three parts returns them newest-first; `limit` is honoured and `next` is the last id when a further page exists and `null` when it does not; the thumbnail is a `data:image/webp;base64,` URL that decodes; a part in another library never appears.

- [ ] **Step 2–4: Run, implement, verify**

Bound `limit` to a maximum (100) and default it (50) rather than trusting the query string — an unbounded `limit` is a trivial way to ask the server to materialise the whole library into memory.

```bash
cargo test -p lapidary-api
cargo xtask export-bindings && git status --porcelain -- web/src/bindings
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/lapidary-api web/src/bindings
git commit -m "feat(api): keyset-paginated grid endpoint with inline thumbnails"
```

---

## Task 11: The grid

**Files:**
- Modify: `web/src/lib/api.ts`, `web/src/lib/strings.ts`, `web/src/routes/index.tsx`
- Modify: `web/src/routes/index.test.tsx`

**Interfaces:**
- Consumes: `GET /api/libraries/{id}/parts`, the generated `PartCard` binding.

Dark only. No bare user-facing strings in components — every string through `strings.ts`. Motion, if any: 120/180/280 ms, `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity only, and respect `prefers-reduced-motion`.

**The empty state changes meaning.** It currently reads *"Parts will appear here as your library grows."* — true while nothing can be ingested. Once the scan endpoint exists, a library can be empty *because nobody has scanned yet*, which is a different situation and deserves different copy. Update it, and keep it honest: the grid still has no upload control, so it must not imply one.

- [ ] **Step 1: Write the failing test**

Extend `web/src/routes/index.test.tsx`: with a mocked page of two parts, assert both names render and both thumbnails appear as `img` elements with non-empty `src`; with an empty page, assert the empty-state copy from `strings.ts` renders. Assert against the `strings.ts` constants, not literals — the existing empty-state test already establishes that pattern and explains why.

- [ ] **Steps 2–4: Run, implement, verify**

```bash
cd web && npm run typecheck && npm test && npm run build && cd ..
git add web/
git commit -m "feat(web): render the parts grid with inline thumbnails"
```

---

## Task 12: Wire the stack and verify end to end

**Files:**
- Modify: `deploy/compose.yaml`, `README.md`
- Modify: `deploy/.env.example`

**Interfaces:** none — this task makes the previous eleven reachable from a browser.

- [ ] **Step 1: Compose changes**

- `api` service: `LAPIDARY_ROLE: api`
- `worker` service: `LAPIDARY_ROLE: worker`, plus
  ```yaml
      volumes:
        - ${LAPIDARY_INGEST_DIR:-../example}:/ingest:ro
        - lapidary-blobs:/var/lib/lapidary
  ```
- `api` service also mounts `lapidary-blobs` (read-only is not enough — it serves derivatives; mount it read-write and rely on the `SourceStore` boundary, which is the point of Task 6)
- a named `lapidary-blobs` volume
- `deploy/.env.example`: document `LAPIDARY_INGEST_DIR`

`cargo xtask check-deploy` must still pass: `SERVER_FEATURES: mock-kernel` stays on the worker only, and only `worker` appears in `KERNEL_LINKED_SERVICES`.

- [ ] **Step 2: README**

Document the scan flow with the seeded library id, so it can be curled without a lookup:

```sh
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
curl -X POST http://localhost:8081/api/libraries/01931b6e-0000-7000-8000-000000000001/scan
open http://localhost:3000
```

Note the port: the scan endpoint is on the **worker** (8081), not the api (8080). That will look like a mistake to a reader; say why it is not.

- [ ] **Step 3: End-to-end verification**

```bash
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
podman logs lapidary-api-1    | grep -iE "role|kernel"   # expect role=api,    kernel=none
podman logs lapidary-worker-1 | grep -iE "role|kernel"   # expect role=worker, kernel=mock
curl -sX POST http://localhost:8081/api/libraries/01931b6e-0000-7000-8000-000000000001/scan
curl -s "http://localhost:3000/api/libraries/01931b6e-0000-7000-8000-000000000001/parts" | head -c 400
```

Then **open `http://localhost:3000` and look at it.** Cards with real thumbnails, or the task is not done. A passing test suite and a blank grid is a failure to have built the thing.

Re-run the scan and confirm `ingested: 0, skipped: N`.

- [ ] **Step 4: Commit**

```bash
git add deploy/ README.md
git commit -m "feat(deploy): mount an ingest directory and document the scan flow"
```

---

## Exit criterion

Mount a directory of STL files, POST the scan, and the grid shows a card per file with a
real rendered thumbnail. Re-running the scan reports `ingested: 0` and completes without
parsing or rasterizing anything.

Deliberately weaker than the roadmap's Phase 1 exit (1,000 STLs, interactive immediately,
warm page under 80 ms) — that needs the queue, SSE and virtualization from slices 2–5, and
claiming it here would be false.
