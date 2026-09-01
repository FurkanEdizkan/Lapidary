# Phase 0a Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the deleted Node prototype with a working Rust workspace, a TanStack SPA, and a four-service container stack that serves a health-checked page — with the crate layering rule enforced in CI from the first commit.

**Architecture:** A modular monolith. Eleven crates in four layers (L0 domain types → L1 db/storage → L2 feature crates → L3 api/enterprise), where L2 crates may never depend on each other; a custom `xtask` enforces this by parsing `cargo metadata`. `lapidary-api` is a library that builds an axum `Router`; only `bin/` produces executables. The CAD kernel sits behind a `Kernel` trait with an in-process `MockKernel` — the real OCCT sidecar is Phase 0b and nothing here depends on it.

**Tech Stack:** Rust 1.95.0 (edition 2024), axum 0.8.9, sqlx 0.9.0, ts-rs 12.0.1, PostgreSQL 18, React 19.2.8, Vite 8.2.2, TanStack Router 1.170.32 / Query 5.102.8, Tailwind 4.3.3, TypeScript 7.0.2, Podman/Docker.

**Spec:** `docs/superpowers/specs/2026-09-01-phase-0a-foundation-design.md`

## Global Constraints

Every task's requirements implicitly include this section.

- **No SQL outside `lapidary-db`.** Enforced by `deny.toml` wrappers, not review.
- **`lapidary-api` is a library.** Never a binary. It must never gain a `src/main.rs` or a `[[bin]]` section.
- **No `unwrap()` outside tests.** Enforced by `#![deny(clippy::unwrap_used)]` at crate roots.
- **`thiserror` in libraries, `anyhow` at binary edges.** `anyhow` may appear only in `bin/` and `xtask/`.
- **L2 crates depend only on L0 and L1.** Never on each other, never on L3.
- **Errors say what broke and what to do.** "Could not reach the database at …. Check that the `db` service is running." Never "connection refused (3)".
- **Frontend is dark only.** No light mode, no theme toggle.
- **No bare user-facing strings in components.** Every string goes through `web/src/lib/strings.ts`. English only.
- **Motion is mechanical.** 120/180/280ms, `cubic-bezier(0.2, 0, 0, 1)`, transform and opacity only, `prefers-reduced-motion` respected.
- **Real content in all examples and fixtures.** Plausible part numbers and real dimensions. Never "Part 1 / Part 2".
- **Generated columns are explicitly `STORED`.** PG 18 defaults to virtual and virtual columns cannot be indexed.
- **Pin everything.** Container images by digest (values given in Task 10). GitHub Actions by commit SHA (values given in Task 11). `Cargo.lock` and `web/package-lock.json` committed.
- **Commit trailers.** Every commit in this plan ends with the session's standard `Co-Authored-By` and `Claude-Session` trailers.
- **Run `cargo fmt --all` before every Rust commit.** The code blocks below are written for readability, not for rustfmt's exact output — several lines run past 100 columns. `cargo fmt --all --check` gates CI from Task 11 onward, so formatting drift becomes a red build.

**Pinning mechanism, deliberately split:** `Cargo.toml` declares the versions below as ordinary caret requirements and `Cargo.lock` is the pin — this is idiomatic Cargo and what `cargo deny` checks. `package.json` uses exact versions with no caret **and** commits `package-lock.json`, because npm's resolution is looser. The spec's "no carets" rule applies to npm.

**Pinned versions** — resolved against crates.io and npm on 2026-09-01:

| Rust | | Web | |
|---|---|---|---|
| `axum` | 0.8.9 | `react`, `react-dom` | 19.2.8 |
| `tokio` | 1.53.1 | `vite` | 8.2.2 |
| `sqlx` | 0.9.0 | `@vitejs/plugin-react` | 6.1.1 |
| `serde` | 1.0.229 | `typescript` | 7.0.2 |
| `serde_json` | 1.0.151 | `@tanstack/react-router` | 1.170.32 |
| `ts-rs` | 12.0.1 | `@tanstack/router-plugin` | 1.168.35 |
| `thiserror` | 2.0.20 | `@tanstack/react-query` | 5.102.8 |
| `anyhow` | 1.0.104 | `@testing-library/dom` | 10.4.1 |
| `blake3` | 1.8.7 | `tailwindcss` | 4.3.3 |
| `zstd` | 0.13.3 | `@tailwindcss/vite` | 4.3.3 |
| `object_store` | 0.14.1 | `vitest` | 4.1.11 |
| `tower` | 0.5.3 | | |
| `tower-http` | 0.7.1 | | |
| `tracing` | 0.1.44 | | |
| `tracing-subscriber` | 0.3.23 | | |
| `uuid` | 1.26.0 | | |
| `jiff` | 0.2.35 | | |
| `clap` | 4.6.6 | | |
| `figment` | 0.10.19 | | |
| `async-trait` | 0.1.92 | | |

---

## File Structure

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Workspace members, shared `[workspace.package]` and `[workspace.dependencies]` |
| `rust-toolchain.toml` | Pins 1.95.0 so every machine and CI agree |
| `deny.toml` | Source allow-list; `sqlx` wrapper rule confining SQL to `lapidary-db` |
| `LICENSE` | AGPL-3.0-only, full text |
| `crates/lapidary-core/src/ids.rs` | Newtype ids: `LibraryId`, `PartId`, `RevisionId`, `BlobHash` |
| `crates/lapidary-core/src/part.rs` | `PartSummary`, `LibraryMode` |
| `crates/lapidary-core/src/approximate.rs` | `Approximate<T>` — makes the measurement rule a type, not a convention |
| `crates/lapidary-core/src/error.rs` | `CoreError` |
| `crates/lapidary-db/src/repo.rs` | Repository traits — the only crate that may name `sqlx` |
| `crates/lapidary-db/migrations/` | `sqlx migrate` SQL |
| `crates/lapidary-cad/src/kernel.rs` | `Kernel` trait, `KernelVersion`, `KernelParams`, `KernelOutput` |
| `crates/lapidary-cad/src/mock.rs` | `MockKernel`, feature-gated |
| `crates/lapidary-api/src/lib.rs` | `router()` — library only |
| `crates/lapidary-api/src/health.rs` | `/api/healthz` handler |
| `bin/lapidary-server/src/main.rs` | Container entrypoint |
| `bin/lapidary/src/main.rs` | `agent | worker | up` subcommand stubs |
| `xtask/src/layers.rs` | The layering rule, pure and unit-testable |
| `xtask/src/main.rs` | `check-layers`, `export-bindings` |
| `web/src/lib/strings.ts` | Every user-facing string |
| `web/src/bindings/` | ts-rs output, **committed** so CI can detect staleness |
| `deploy/Containerfile` | Rust multi-stage → `lapidary-server` |
| `deploy/db/Containerfile` | `postgres:18` + pgvector |
| `deploy/web/Containerfile` | node build → caddy static serve |
| `deploy/compose.yaml` | `web`, `api`, `worker`, `db`. No Redis, no broker. |
| `.github/workflows/ci.yml` | fmt, clippy, test, deny, layers, bindings, web |
| `.github/workflows/containers.yml` | Image builds — manual and on tags only |

---

### Task 1: Record the prototype, then delete it

The prototype carries domain knowledge that must survive its deletion. Capture it first, in the same commit that removes it, so the notes and the removal are reviewable together.

**Files:**
- Create: `docs/prototype-notes.md`
- Modify: `README.md`, `docs/README.md`, `.gitignore`
- Delete: `server/`, `rust-mesh/`, `package.json`, `package-lock.json`, `Dockerfile`, `compose.yaml`, `.dockerignore`, `.env.example`, `MIGRATION.md`, `docs/MIGRATION.md`, `lapidary-docs.zip`, `web/`

**Interfaces:**
- Consumes: nothing.
- Produces: `docs/prototype-notes.md` — referenced by Tasks 5, 6 and 10 for domain shape.

- [ ] **Step 1: Write the prototype notes**

Create `docs/prototype-notes.md`:

```markdown
# What the Node prototype knew

The prototype (Fastify + SQLite, ~1,000 LOC of services) is deleted. It remains on
`main` and in git history. This records what it established, so the knowledge outlives
the code.

## Domain shape that survived contact with real files

`model.service.ts` settled on a row shape worth carrying into `lapidary-core`:
identity (`id`, `name`, `creator`), classification (`type`, `mesh_kind`, `format`),
geometry (`bbox_x/y/z`, `triangle_count`, `file_size_bytes`), provenance
(`created_date`, `added_date`, `original_path`), and derivative presence flags
(`thumbnail_path`, `lod_path` → `hasThumbnail`, `hasLod`, `hasOriginal`).

The three many-to-many attachments — tags, groups, printer types — were each a join
table resolved to a sorted name list. Lapidary keeps that shape; the join resolution
moves behind repository traits in `lapidary-db`.

**Note the bug we are not carrying forward:** `listModels` did `SELECT * FROM models`
and filtered in TypeScript. Phase 1 filters and paginates in SQL with keyset
pagination.

## Search behaviour worth reproducing

`search.service.ts` returned three result classes from one query — matching models,
creators with counts, and tags drawn from matched models — plus a header that changes
between `POPULAR TAGS` (empty query) and `TAGS & SUGGESTIONS` (non-empty). Caps were
5 models, 4 creators, 8 tags, 9 popular tags, 12 rail tags.

Tag counts were computed by a full `GROUP BY` on every keystroke. `lapidary-index`
replaces this with `tsvector` + `pg_trgm` and the 10k exact-count threshold from
`docs/DATA.md`, but the *shape* of the suggestion payload is right.

## LOD approach

`rust-mesh` was dependency-free so it compiled offline in the container build — a
constraint worth preserving. It computed an exact bounding box in mm and a triangle
count, and generated LOD by **vertex clustering on a 48³ grid**, writing binary STL.

Vertex clustering is the right first LOD algorithm for `lapidary-cad`: single pass,
no topology required, degrades gracefully on the malformed meshes real libraries
contain. The 48 constant was tuned by eye and should become an L0/L1/L2 ladder.

## Explicitly not carried forward

- **`cache.service.ts`** — Redis. The cache and the job queue are both Postgres.
- **Per-model procedural sample shapes** in `seed.ts`. Phase 1 seeds one real
  licence-clean example part.
- **`npm run dev` as the primary run path.** `podman compose up` is the entry point.
```

- [ ] **Step 2: Delete the prototype**

```bash
git rm -r --quiet server rust-mesh web
git rm --quiet package.json package-lock.json Dockerfile compose.yaml .dockerignore .env.example docs/MIGRATION.md
rm -f MIGRATION.md lapidary-docs.zip
```

`MIGRATION.md` and `lapidary-docs.zip` at the repo root are untracked, so `rm` rather than `git rm`.

- [ ] **Step 3: Drop the MIGRATION row from the doc map**

In `docs/README.md`, delete the entire table row beginning `| [`MIGRATION.md`](MIGRATION.md) |`. Add this row to the same table:

```markdown
| [`prototype-notes.md`](prototype-notes.md) | What the deleted Node prototype established: domain shape, search payload, LOD approach | Designing `lapidary-core` types, `lapidary-index` search, or `lapidary-cad` LOD |
```

Then fix the root `README.md`, which is the other document still pointing at the withdrawn
plan. Replace its **Status** section with:

```markdown
## Status

**Pre-alpha.** `main` holds the Node/Fastify prototype that validated the product idea. It
is a reference implementation to read, not a base to build on. The Rust implementation
described in `docs/` is being built fresh on `rust-rewrite`, and that is the only thing
that will ship. There is no runnable application on `rust-rewrite` until Phase 1.
```

Its **Running it** section is also stale — the compose file moved into `deploy/`. Change
`podman compose up` to `podman compose -f deploy/compose.yaml up`. The **Licence** section
is wrong too, but it cannot be corrected until `LICENSE` exists; Task 2 Step 3 handles it.

- [ ] **Step 4: Fix the stale gitignore entry**

`.gitignore` ignores `web/src/generated/`, which nothing writes. Bindings live in
`web/src/bindings/` and are committed. Delete just that one line:

```
web/src/generated/
```

Keep the `# Generated` heading and `sidecar/occt-bridge/build/` underneath it — Task 5
creates the directory that entry refers to. Nothing else in `.gitignore` changes:
`/target/`, `node_modules/`, `web/dist/` and `.env` are already present and correct. Note
that the `.env` rule has no leading slash, so it matches at any depth and `deploy/.env`
is already covered.

- [ ] **Step 5: Verify nothing still references the deleted tree**

Run:

```bash
grep -rn --exclude-dir=.git -E "better-sqlite3|fastify|REDIS_URL|npm run dev|rust-mesh|MIGRATION\.md" . || echo "CLEAN"
```

Expected: `CLEAN` — but only once Step 3 has run. Before Step 3 this grep hits
`README.md:17` and `docs/README.md:10`, both genuine references to the withdrawn
`MIGRATION.md`, and both fixed by Step 3. If `docs/ROADMAP.md` or `CLAUDE.md` match, they
are describing future state or rules; inspect each hit and only fix genuine references to
deleted files.

- [ ] **Step 6: Verify the working tree is otherwise intact**

```bash
ls design fixtures docs .claude/skills && test -f CLAUDE.md && echo "KEPT OK"
```

Expected: `KEPT OK`, with `design/`, `fixtures/`, `docs/` and the three skills listed.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat!: delete the Node prototype

Fresh start rather than staged cutover. docs/prototype-notes.md records
the domain shape, search payload and LOD approach the prototype
established, so the knowledge outlives the code. The prototype remains
on main and in history.

BREAKING CHANGE: no runnable application on this branch until Phase 1."
```

---

### Task 2: Cargo workspace, licence, and `lapidary-core`

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `LICENSE`
- Create: `crates/lapidary-core/Cargo.toml`, `src/lib.rs`, `src/ids.rs`, `src/part.rs`, `src/approximate.rs`, `src/error.rs`
- Modify: `README.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md` — the licence is decided here, and all four still record it as open

**Interfaces:**
- Consumes: nothing.
- Produces: `LibraryId`, `PartId`, `RevisionId`, `BlobHash`, `PartSummary`, `LibraryMode`, `Approximate<T>`, `CoreError`. Every later task depends on these names.

- [ ] **Step 1: Create the workspace root**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
# Every member listed here MUST already exist on disk — cargo errors with
# "failed to load manifest for workspace member" otherwise. Each task below adds
# its own crates to this list. Unused [workspace.dependencies] path entries
# pointing at not-yet-created directories are tolerated, so that block is
# complete from the start.
members = [
    "crates/lapidary-core",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "AGPL-3.0-only"
repository = "https://github.com/FurkanEdizkan/Lapidary"

[workspace.dependencies]
lapidary-core = { path = "crates/lapidary-core" }
lapidary-db = { path = "crates/lapidary-db" }
lapidary-storage = { path = "crates/lapidary-storage" }
lapidary-cad = { path = "crates/lapidary-cad" }
lapidary-jobs = { path = "crates/lapidary-jobs" }
lapidary-index = { path = "crates/lapidary-index" }
lapidary-vcs = { path = "crates/lapidary-vcs" }
lapidary-build = { path = "crates/lapidary-build" }
lapidary-targets = { path = "crates/lapidary-targets" }
lapidary-api = { path = "crates/lapidary-api" }
lapidary-enterprise = { path = "crates/lapidary-enterprise" }

anyhow = "1.0.104"
async-trait = "0.1.92"
axum = "0.8.9"
blake3 = "1.8.7"
clap = { version = "4.6.6", features = ["derive"] }
figment = { version = "0.10.19", features = ["env", "toml"] }
jiff = { version = "0.2.35", features = ["serde"] }
object_store = "0.14.1"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
sqlx = { version = "0.9.0", features = ["postgres", "runtime-tokio", "macros", "migrate", "uuid"] }
thiserror = "2.0.20"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "signal"] }
tower = "0.5.3"
tower-http = { version = "0.7.1", features = ["trace", "cors"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
# uuid-impl and jiff-impl are not optional extras. Without them ts-rs has no TS impl
# for Uuid or jiff::Timestamp, and lapidary-core derives TS on types built from both —
# the crate does not compile.
ts-rs = { version = "12.0.1", features = ["uuid-impl", "jiff-impl"] }
uuid = { version = "1.26.0", features = ["v7", "serde"] }
zstd = "0.13.3"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
```

- [ ] **Step 2: Pin the toolchain**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.95.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 3: Add the licence**

```bash
curl -fsSL https://www.gnu.org/licenses/agpl-3.0.txt -o LICENSE
wc -l LICENSE
```

Expected: ~661 lines. If the fetch fails, copy the AGPL-3.0 text from
<https://www.gnu.org/licenses/agpl-3.0.txt> by hand. Do not substitute a summary or a
different licence — `ARCHITECTURE.md` records AGPL-3.0-only as a decision.

Four documents still say the licence is undecided. A `LICENSE` file that contradicts the
prose around it is worse than no file at all, so they are corrected in this same commit:

**`README.md`** — replace the whole **Licence** section with:

```markdown
## Licence

**AGPL-3.0-only**, for the entire workspace including `lapidary-enterprise`. The Ed25519
licence file gates fleet size and support entitlement as a contractual boundary, not as
technical DRM. Contributions are taken under the DCO; there is no CLA.
```

**`CONTRIBUTING.md`** — line 3 opens "Not yet open for contributions. The licence has not
been decided". The licence is decided. Rewrite that sentence to state AGPL-3.0-only and
DCO, and keep whatever it says about the project not yet accepting outside work if that
is still true — those are two separate claims and only the licensing one has changed.

**`docs/ARCHITECTURE.md`** — retitle `## Licensing — decision required` to
`## Licensing — decided: AGPL-3.0-only`. Keep option 1 as the recorded decision with its
full reasoning, and reduce option 2 to a single line noting it was considered and
rejected. Do not delete the reasoning: it is the argument that keeps the decision from
being relitigated, and `ARCHITECTURE.md` is where structural decisions live.

**`docs/ROADMAP.md`** — delete the **Licensing conflict** bullet from `# Open items`. It
is no longer open.

- [ ] **Step 4: Write the failing tests for `lapidary-core`**

`crates/lapidary-core/Cargo.toml`:

```toml
[package]
name = "lapidary-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true
ts-rs.workspace = true
uuid.workspace = true
jiff.workspace = true

[dev-dependencies]
# Only the serialisation tests need it. Keeping it out of [dependencies] stops the domain
# crate acquiring a JSON representation it does not actually own.
serde_json.workspace = true
```

`crates/lapidary-core/src/lib.rs`:

```rust
#![deny(clippy::unwrap_used)]
//! Domain types shared by every Lapidary crate. Depends on no other Lapidary crate.

mod approximate;
mod error;
mod ids;
mod part;

pub use approximate::Approximate;
pub use error::CoreError;
pub use ids::{BlobHash, LibraryId, PartId, RevisionId};
pub use part::{LibraryMode, PartSummary};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_hash_round_trips_through_hex() {
        let hash = BlobHash::from_bytes([0xab; 32]);
        let hex = hash.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(BlobHash::parse_hex(&hex).expect("valid hex"), hash);
    }

    #[test]
    fn blob_hash_rejects_wrong_length() {
        assert!(BlobHash::parse_hex("abcd").is_err());
    }

    #[test]
    fn part_summary_serialises_camel_case() {
        let now = jiff::Timestamp::now();
        let summary = PartSummary {
            id: PartId::new(),
            library: LibraryId::new(),
            name: "Bearing block, 608ZZ".to_owned(),
            part_number: Some("LP-1042-03".to_owned()),
            thumbnail: Some(BlobHash::from_bytes([0x11; 32])),
            triangle_count: Some(48_112),
            approximate: true,
            created_at: now,
            updated_at: now,
        };
        let json = serde_json::to_value(&summary).expect("serialises");
        assert!(json.get("partNumber").is_some(), "expected camelCase keys");
        assert!(json.get("part_number").is_none());
        assert!(json.get("createdAt").is_some());
        assert_eq!(
            json["thumbnail"],
            "1111111111111111111111111111111111111111111111111111111111111111",
            "a blob hash must go over the wire as hex, never as a byte array"
        );
    }

    #[test]
    fn approximate_marks_mesh_derived_values() {
        let from_brep = Approximate::analytic(20.0_f64);
        let from_mesh = Approximate::tessellated(19.987_f64);
        assert!(!from_brep.is_approximate());
        assert!(from_mesh.is_approximate());
        assert_eq!(*from_mesh.value(), 19.987);
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test -p lapidary-core`
Expected: FAIL — `unresolved module` / `cannot find` for `approximate`, `error`, `ids`, `part`.

- [ ] **Step 6: Implement the modules**

`crates/lapidary-core/src/ids.rs`:

```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

macro_rules! uuid_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a fresh, time-ordered id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

uuid_newtype!(LibraryId, "Identifies a library. Governance is opt-in per library.");
uuid_newtype!(PartId, "Identifies a part across all of its revisions.");
uuid_newtype!(RevisionId, "Identifies one immutable revision of a part.");

/// A BLAKE3 content hash. Content addressing is not authorization — holding one of
/// these never implies the right to read the blob it names.
///
/// Serialises as a 64-character hex string, not as a byte array, so the wire format
/// and the generated TypeScript type agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TS)]
#[ts(export, as = "String")]
pub struct BlobHash([u8; 32]);

impl Serialize for BlobHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for BlobHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        Self::parse_hex(&hex).map_err(serde::de::Error::custom)
    }
}

impl BlobHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parse a 64-character lowercase hex digest.
    pub fn parse_hex(hex: &str) -> Result<Self, crate::CoreError> {
        if hex.len() != 64 {
            return Err(crate::CoreError::BlobHashLength { got: hex.len() });
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let pair = hex.get(i * 2..i * 2 + 2).ok_or(crate::CoreError::BlobHashLength { got: hex.len() })?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| crate::CoreError::BlobHashHex)?;
        }
        Ok(Self(bytes))
    }
}
```

`crates/lapidary-core/src/approximate.rs`:

```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Wraps a measured value with whether it came from an analytic B-rep entity or from
/// tessellated geometry.
///
/// This is a type rather than a UI convention because `CLAUDE.md` makes it
/// non-negotiable: mesh-derived measurements are labelled "approximate" in the UI,
/// always. Making the flag inseparable from the value means a caller cannot render one
/// without the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Approximate<T> {
    value: T,
    approximate: bool,
}

impl<T> Approximate<T> {
    /// A value read from an analytic B-rep entity. Safe to present as exact.
    pub fn analytic(value: T) -> Self {
        Self { value, approximate: false }
    }

    /// A value derived from tessellated geometry. The UI must label it.
    ///
    /// Named for its provenance rather than `approximate`, which would collide with the
    /// type name and trip `clippy::self_named_constructors` under `-D warnings`. The
    /// provenance is the more useful name at the call site anyway.
    pub fn tessellated(value: T) -> Self {
        Self { value, approximate: true }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn is_approximate(&self) -> bool {
        self.approximate
    }
}
```

`crates/lapidary-core/src/part.rs`:

```rust
use crate::{BlobHash, LibraryId, PartId};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Governance is opt-in per library. Hobby libraries have no revisions, states or
/// approvals; flipping a library to `Controlled` turns that machinery on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryMode {
    Hobby,
    Controlled,
}

/// The grid row, in the shape the spec calls for: identity, part number, thumbnail
/// reference, approximate flag, timestamps. Deliberately narrow — the open path reads
/// metadata and derivatives only, never a source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartSummary {
    pub id: PartId,
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    /// The thumbnail derivative's content hash, not a URL. Holding it is not
    /// authorization to read it — the API still checks tenant and part reachability.
    pub thumbnail: Option<BlobHash>,
    pub triangle_count: Option<u32>,
    /// True when every geometric figure on this part is mesh-derived.
    pub approximate: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

`crates/lapidary-core/src/error.rs`:

```rust
use thiserror::Error;

/// Errors say what broke and what to do about it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("A blob hash must be 64 hex characters (a 32-byte BLAKE3 digest); got {got}. Copy the full hash from the part's detail panel.")]
    BlobHashLength { got: usize },

    #[error("A blob hash must contain only the characters 0-9 and a-f. Copy the full hash from the part's detail panel rather than retyping it.")]
    BlobHashHex,
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p lapidary-core`
Expected: 11 tests PASS — the 4 written above, plus the 7 `export_bindings_*` tests that
`#[ts(export)]` generates, one per exported type. Those seven are what Task 8 drives to
produce `web/src/bindings/`.

- [ ] **Step 8: Ignore ts-rs's default output location**

Running the tests just wrote `crates/lapidary-core/bindings/*.ts`. That is ts-rs's default
export directory when `TS_RS_EXPORT_DIR` is unset, and it is not where the bindings belong
— Task 8 sets that variable and puts the canonical, committed copy in `web/src/bindings/`.
Committing this second copy would leave seven generated files that nothing regenerates and
nothing checks, which is the exact drift the Task 11 staleness job exists to prevent.

Add one line to `.gitignore`, under the existing `# Generated` heading:

```
crates/*/bindings/
```

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml LICENSE crates/lapidary-core .gitignore
git add README.md CONTRIBUTING.md docs/ARCHITECTURE.md docs/README.md docs/ROADMAP.md
git status --porcelain   # expect no stray crates/lapidary-core/bindings/ entries
git commit -m "feat(core): cargo workspace, AGPL licence, and lapidary-core domain types

Approximate<T> makes the measurement rule a type rather than a UI
convention: a caller cannot render a mesh-derived value without its
approximate flag."
```

---

### Task 3: `xtask check-layers`

The layering rule is what keeps the monolith from congealing, so it is enforced before there is anything to enforce it against. The rule lives in a pure function tested against a synthetic graph — a test asserting only that the real graph passes today would still pass if the rule were `return Ok(())`.

**Files:**
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/layers.rs`
- Create: `.cargo/config.toml`

**Interfaces:**
- Consumes: the workspace member list from Task 2.
- Produces: `cargo xtask check-layers`. Task 9 adds `export-bindings` to the same binary. `layers::check(&Graph) -> Result<(), Vec<Violation>>`.

- [ ] **Step 1: Write the failing tests**

`xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dependencies]
anyhow.workspace = true
serde_json.workspace = true
```

`xtask/src/layers.rs`:

```rust
//! The CI-enforced layering rule from docs/ARCHITECTURE.md.
//!
//! L0 depends on no workspace crate. L1 depends only on L0. L2 depends only on L0 and
//! L1 — never on another L2, never on L3. L3 may depend on anything below it.
//!
//! If two L2 crates need to share something, it moves to lapidary-core.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    /// Binaries and xtask. Outside the rule; may depend on anything.
    Bin,
}

/// The authoritative layer assignment. Adding a crate to the workspace without adding
/// it here is itself a failure — see `check`.
pub fn layer_of(crate_name: &str) -> Option<Layer> {
    Some(match crate_name {
        "lapidary-core" => Layer::L0,
        "lapidary-db" | "lapidary-storage" => Layer::L1,
        "lapidary-cad" | "lapidary-jobs" | "lapidary-index" | "lapidary-vcs"
        | "lapidary-build" | "lapidary-targets" => Layer::L2,
        "lapidary-api" | "lapidary-enterprise" => Layer::L3,
        "lapidary-server" | "lapidary" | "xtask" => Layer::Bin,
        _ => return None,
    })
}

/// Workspace crate name -> the workspace crates it depends on.
pub type Graph = BTreeMap<String, Vec<String>>;

#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    /// A dependency edge the layering rule forbids.
    ForbiddenEdge { from: String, from_layer: Layer, to: String, to_layer: Layer },
    /// A workspace member with no entry in `layer_of`.
    UnknownCrate { name: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::ForbiddenEdge { from, from_layer, to, to_layer } => write!(
                f,
                "{from} ({from_layer:?}) -> {to} ({to_layer:?}) is forbidden. \
                 L2 crates may depend only on L0 and L1. If these two need to share \
                 something, move it into lapidary-core."
            ),
            Violation::UnknownCrate { name } => write!(
                f,
                "{name} is a workspace member but has no layer. Add it to layer_of() in \
                 xtask/src/layers.rs, choosing its layer from docs/ARCHITECTURE.md."
            ),
        }
    }
}

/// True when a crate at `from` may depend on a crate at `to`.
fn edge_allowed(from: Layer, to: Layer) -> bool {
    match from {
        Layer::L0 => false,
        Layer::L1 => to == Layer::L0,
        Layer::L2 => to == Layer::L0 || to == Layer::L1,
        Layer::L3 => to != Layer::Bin,
        Layer::Bin => true,
    }
}

pub fn check(graph: &Graph) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();

    for (name, deps) in graph {
        let Some(from_layer) = layer_of(name) else {
            violations.push(Violation::UnknownCrate { name: name.clone() });
            continue;
        };
        for dep in deps {
            let Some(to_layer) = layer_of(dep) else {
                violations.push(Violation::UnknownCrate { name: dep.clone() });
                continue;
            };
            if !edge_allowed(from_layer, to_layer) {
                violations.push(Violation::ForbiddenEdge {
                    from: name.clone(),
                    from_layer,
                    to: dep.clone(),
                    to_layer,
                });
            }
        }
    }

    if violations.is_empty() { Ok(()) } else { Err(violations) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        edges
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
            .collect()
    }

    #[test]
    fn accepts_a_legal_graph() {
        let g = graph(&[
            ("lapidary-core", &[]),
            ("lapidary-db", &["lapidary-core"]),
            ("lapidary-index", &["lapidary-core", "lapidary-db"]),
            ("lapidary-api", &["lapidary-core", "lapidary-index"]),
        ]);
        assert!(check(&g).is_ok());
    }

    #[test]
    fn rejects_l2_depending_on_l2() {
        let g = graph(&[
            ("lapidary-core", &[]),
            ("lapidary-vcs", &["lapidary-index"]),
            ("lapidary-index", &["lapidary-core"]),
        ]);
        let violations = check(&g).expect_err("L2 -> L2 must be rejected");
        assert_eq!(
            violations,
            vec![Violation::ForbiddenEdge {
                from: "lapidary-vcs".to_owned(),
                from_layer: Layer::L2,
                to: "lapidary-index".to_owned(),
                to_layer: Layer::L2,
            }]
        );
    }

    #[test]
    fn rejects_l2_depending_on_l3() {
        let g = graph(&[("lapidary-jobs", &["lapidary-enterprise"])]);
        let violations = check(&g).expect_err("L2 -> L3 must be rejected");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn rejects_l0_depending_on_anything() {
        let g = graph(&[("lapidary-core", &["lapidary-db"])]);
        assert!(check(&g).is_err(), "L0 must depend on no workspace crate");
    }

    #[test]
    fn rejects_a_member_with_no_layer() {
        let g = graph(&[("lapidary-mystery", &[])]);
        let violations = check(&g).expect_err("unlayered members must be rejected");
        assert_eq!(violations, vec![Violation::UnknownCrate { name: "lapidary-mystery".to_owned() }]);
    }

    #[test]
    fn allows_bins_to_depend_on_everything() {
        let g = graph(&[("lapidary-server", &["lapidary-api", "lapidary-core"])]);
        assert!(check(&g).is_ok());
    }

    #[test]
    fn violation_message_names_the_edge_and_the_remedy() {
        let v = Violation::ForbiddenEdge {
            from: "lapidary-vcs".to_owned(),
            from_layer: Layer::L2,
            to: "lapidary-index".to_owned(),
            to_layer: Layer::L2,
        };
        let msg = v.to_string();
        assert!(msg.contains("lapidary-vcs"));
        assert!(msg.contains("lapidary-index"));
        assert!(msg.contains("lapidary-core"), "message must state the remedy");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p xtask`
Expected: FAIL — `xtask` has no `src/main.rs`, so the target does not build.

- [ ] **Step 3: Implement the binary**

`xtask/src/main.rs`:

```rust
//! Workspace automation. Run via the `cargo xtask` alias in .cargo/config.toml.

mod layers;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::process::Command;

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("check-layers") => check_layers(),
        Some(other) => bail!("Unknown xtask '{other}'. Available: check-layers"),
        None => bail!("Usage: cargo xtask <check-layers>"),
    }
}

/// Read the workspace graph from `cargo metadata` and apply the layering rule.
fn check_layers() -> Result<()> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .context("Could not run `cargo metadata`. Is cargo on PATH?")?;

    if !output.status.success() {
        bail!(
            "`cargo metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("`cargo metadata` returned invalid JSON")?;

    let packages = meta["packages"]
        .as_array()
        .context("`cargo metadata` output had no packages array")?;

    let member_names: std::collections::BTreeSet<String> = packages
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();

    let mut graph: layers::Graph = BTreeMap::new();
    for pkg in packages {
        let Some(name) = pkg["name"].as_str() else { continue };
        let deps: Vec<String> = pkg["dependencies"]
            .as_array()
            .map(|ds| {
                ds.iter()
                    .filter(|d| d["kind"].is_null()) // normal deps only; dev-deps are exempt
                    .filter_map(|d| d["name"].as_str())
                    .filter(|d| member_names.contains(*d))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        graph.insert(name.to_owned(), deps);
    }

    match layers::check(&graph) {
        Ok(()) => {
            println!("layering OK — {} workspace crates checked", graph.len());
            Ok(())
        }
        Err(violations) => {
            eprintln!("Layering rule violated ({} problem(s)):\n", violations.len());
            for v in &violations {
                eprintln!("  {v}");
            }
            eprintln!("\nThe rule is in docs/ARCHITECTURE.md: L2 crates may depend on L0 and L1, never on each other or on L3.");
            bail!("layering check failed")
        }
    }
}
```

Note `.filter(|d| d["kind"].is_null())`: dev-dependencies are deliberately exempt, so an L2 crate may use another L2 crate as a test fixture without breaking the rule.

- [ ] **Step 4: Add the cargo alias**

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --quiet --package xtask --"
```

Add `xtask` to the workspace `members` array in the root `Cargo.toml`:

```toml
members = [
    "crates/lapidary-core",
    "xtask",
]
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p xtask`
Expected: 7 tests PASS.

- [ ] **Step 6: Verify it works against the real graph**

Run: `cargo xtask check-layers`
Expected: `layering OK — 2 workspace crates checked` — `lapidary-core` (L0) and `xtask` (Bin) are the only members so far.

The injected-violation proof runs in Task 4 Step 6, once there are two L2 crates to point at each other.

- [ ] **Step 7: Commit**

```bash
git add xtask .cargo/config.toml Cargo.toml Cargo.lock
git commit -m "feat(xtask): CI-enforced crate layering check

The rule lives in a pure function tested against a synthetic graph,
including violation cases — a test asserting only that the real graph
passes today would still pass if the rule were a no-op.

Dev-dependencies are exempt so an L2 crate may use another as a test
fixture. The injected-violation proof runs in the next commit, once
every crate exists."
```

---

### Task 4: The next eight crates

Each crate gets its error type and public trait surface. No bodies — those are Phases 1 onward. The deliverable is a workspace that builds, plus proof the layering check bites.

**Files:**
- Create: `crates/{lapidary-db,lapidary-storage,lapidary-jobs,lapidary-index,lapidary-vcs,lapidary-build,lapidary-targets,lapidary-enterprise}/{Cargo.toml,src/lib.rs}`
- Create: `crates/lapidary-db/migrations/0001_init.sql`
- Note: `lapidary-cad` is Task 5, `lapidary-api` is Task 6.

**Interfaces:**
- Consumes: `lapidary-core` types from Task 2; `cargo xtask check-layers` from Task 3.
- Produces: `DbError`, `StorageError`, `JobsError`, `IndexError`, `VcsError`, `BuildError`, `TargetsError`, `EnterpriseError`; `PartRepository` trait; `PgPool` re-export from `lapidary-db`.

- [ ] **Step 1: Create the eight crates**

For each of `lapidary-storage`, `lapidary-jobs`, `lapidary-index`, `lapidary-vcs`, `lapidary-build`, `lapidary-targets`, `lapidary-enterprise`, create `crates/<name>/Cargo.toml`:

```toml
[package]
name = "<name>"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
lapidary-core.workspace = true
thiserror.workspace = true
```

`lapidary-storage` additionally gets `blake3.workspace = true`, `zstd.workspace = true`, `object_store.workspace = true`.
`lapidary-jobs` additionally gets `lapidary-db.workspace = true`, `tokio.workspace = true`.
`lapidary-index` additionally gets `lapidary-db.workspace = true`.
`lapidary-vcs` and `lapidary-build` additionally get `lapidary-db.workspace = true`.
`lapidary-targets` additionally gets `serde.workspace = true`.
`lapidary-enterprise` additionally gets `lapidary-db.workspace = true`.

- [ ] **Step 2: Write each lib.rs**

Pattern — `crates/lapidary-storage/src/lib.rs`:

```rust
#![deny(clippy::unwrap_used)]
//! Blob content-addressed storage: BLAKE3 addressing, zstd compression, tiering and
//! quarantine. Implementation lands in Phase 1; see docs/DATA.md.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("No blob is stored under hash {hash}. It may have been purged after its 30-day quarantine, or the hash may be from a different library.")]
    NotFound { hash: String },

    #[error("Could not write to the blob store at {path}. Check that the volume is mounted and writable.")]
    WriteFailed { path: String },
}
```

Write the equivalent for each remaining crate, with two error variants apiece phrased per the Global Constraints — say what broke and what to do. Use these variant names so later phases have stable targets:

| Crate | Error type | Variants |
|---|---|---|
| `lapidary-jobs` | `JobsError` | `LeaseExpired { job_id: String }`, `QueueUnavailable` |
| `lapidary-index` | `IndexError` | `ExtractionFailed { stage: u8 }`, `SearchConfigMissing { config: String }` |
| `lapidary-vcs` | `VcsError` | `PartLocked { by: String }`, `RevisionNotFound { revision: String }` |
| `lapidary-build` | `BuildError` | `CycleRejected { at: String }`, `ProcessTypeUnknown { name: String }` |
| `lapidary-targets` | `TargetsError` | `NoFormatMatch { target: String, available: String }`, `ExportFailed { reason: String }` |
| `lapidary-enterprise` | `EnterpriseError` | `LicenceExpired { grace_days: u32 }`, `WorkerLimitReached { max: u32 }` |

`NoFormatMatch` is the type-level expression of "never hand a mesh to someone who needed B-rep".

- [ ] **Step 3: Create `lapidary-db`**

`crates/lapidary-db/Cargo.toml`:

```toml
[package]
name = "lapidary-db"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
lapidary-core.workspace = true
async-trait.workspace = true
sqlx.workspace = true
thiserror.workspace = true
```

`crates/lapidary-db/src/lib.rs`:

```rust
#![deny(clippy::unwrap_used)]
//! Every SQL statement in Lapidary lives in this crate. Other crates go through the
//! repository traits below.

mod repo;

pub use repo::PartRepository;
pub use sqlx::PgPool;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Could not reach the database at {target}. Check that the `db` service is running and that DATABASE_URL in your .env matches it.")]
    Unreachable { target: String },

    #[error("The database is PostgreSQL {found}, but Lapidary requires 18 or newer. Generated columns must be STORED, which earlier versions do not support.")]
    UnsupportedVersion { found: String },

    #[error("A database query failed: {0}")]
    Query(#[from] sqlx::Error),

    #[error("Could not bring the database schema up to date: {0}. If the database is at a newer schema version than this binary, check that the api and worker images are the same version.")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Strip credentials from a connection URL so it is safe to put in an error or a log.
/// `postgres://user:pw@host:5432/db` becomes `postgres://host:5432/db`.
///
/// This matters because `main` returns `anyhow::Result`, and anyhow prints the whole
/// source chain on exit. Without redaction the connection string — password included —
/// lands in `podman logs` the first time a container cannot reach its database.
/// Splits on the LAST `@` so a password that itself contains `@` is still removed.
pub(crate) fn redact_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "the configured database".to_owned();
    };
    // Credentials live in the authority, which ends at the first '/', '?' or '#'. Scoping
    // the split here matters: searching the whole remainder for the last '@' breaks on a
    // query string that contains one, e.g. `?options=foo@bar`, which would report the
    // host as `bar`.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let host = authority.rsplit_once('@').map_or(authority, |(_creds, host)| host);
    // Drop the query and fragment. libpq connection URIs accept `?password=...`, so
    // keeping them would reintroduce exactly the leak this function exists to prevent.
    let path = tail.split(['?', '#']).next().unwrap_or("");
    format!("{scheme}://{host}{path}")
}

#[cfg(test)]
mod tests {
    use super::redact_credentials;

    #[test]
    fn redaction_removes_the_password() {
        let out = redact_credentials("postgres://lapidary:sup3rs3cret@db:5432/lapidary");
        assert_eq!(out, "postgres://db:5432/lapidary");
        assert!(!out.contains("sup3rs3cret"));
    }

    #[test]
    fn redaction_handles_a_password_containing_an_at_sign() {
        let out = redact_credentials("postgres://lapidary:p@ss@db:5432/lapidary");
        assert!(!out.contains("p@ss"), "must split on the last @, got {out}");
        assert_eq!(out, "postgres://db:5432/lapidary");
    }

    #[test]
    fn redaction_is_scoped_to_the_authority_not_the_query_string() {
        // The last '@' here is inside the query string. Splitting on it would report the
        // host as "bar".
        let out = redact_credentials("postgres://user:pass@host:5432/db?options=foo@bar");
        assert_eq!(out, "postgres://host:5432/db");
    }

    #[test]
    fn redaction_drops_a_password_carried_in_the_query_string() {
        // libpq URIs accept ?password=... . Keeping the query would leak it even though
        // the authority had no credentials to strip.
        let out = redact_credentials("postgres://host:5432/db?password=hunter2");
        assert!(!out.contains("hunter2"), "query-string password must not survive, got {out}");
        assert_eq!(out, "postgres://host:5432/db");
    }

    #[test]
    fn redaction_keeps_a_bracketed_ipv6_host() {
        assert_eq!(
            redact_credentials("postgres://user:pw@[::1]:5432/db"),
            "postgres://[::1]:5432/db"
        );
    }

    #[test]
    fn redaction_passes_through_a_url_with_no_credentials() {
        assert_eq!(
            redact_credentials("postgres://db:5432/lapidary"),
            "postgres://db:5432/lapidary"
        );
    }
}

/// Connect and verify the server is PostgreSQL 18 or newer.
pub async fn connect(url: &str) -> Result<PgPool, DbError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await
        .map_err(|_| DbError::Unreachable { target: redact_credentials(url) })?;

    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&pool)
        .await?;

    if version < 180_000 {
        return Err(DbError::UnsupportedVersion { found: version.to_string() });
    }

    Ok(pool)
}

/// Apply every migration in `crates/lapidary-db/migrations`. `sqlx::migrate!` embeds them
/// at compile time, so an image carries its own schema and an air-gapped operator needs no
/// migration tooling on the host.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
```

`sqlx::migrate!` resolves its path against this crate's manifest directory, which is why
the call lives here and not in the binary — the same reason all the SQL does. The
migrations directory must be non-empty at compile time or the macro fails; `0001_init.sql`
below satisfies that.

`crates/lapidary-db/src/repo.rs`:

```rust
use crate::DbError;
use lapidary_core::{LibraryId, PartSummary};

/// Reading parts for the grid. The open path reads metadata and derivatives only and
/// never touches a source file.
#[async_trait::async_trait]
pub trait PartRepository: Send + Sync {
    /// One keyset page of grid rows, newest first. `after` is the previous page's last
    /// id. Implementation lands in Phase 1.
    async fn page(
        &self,
        library: LibraryId,
        after: Option<lapidary_core::PartId>,
        limit: u16,
    ) -> Result<Vec<PartSummary>, DbError>;
}
```

`crates/lapidary-db/migrations/0001_init.sql`:

```sql
-- Phase 0a establishes only what the health check and later phases need to exist.
-- Schema proper arrives in Phase 1; see docs/DATA.md.

-- Trigram search, used by lapidary-index from Phase 2. Ships with the postgres image.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
```

- [ ] **Step 4: Add the eight crates to the workspace**

The `members` array in the root `Cargo.toml` becomes:

```toml
members = [
    "crates/lapidary-core",
    "crates/lapidary-db",
    "crates/lapidary-storage",
    "crates/lapidary-jobs",
    "crates/lapidary-index",
    "crates/lapidary-vcs",
    "crates/lapidary-build",
    "crates/lapidary-targets",
    "crates/lapidary-enterprise",
    "xtask",
]
```

- [ ] **Step 5: Build the workspace**

Run: `cargo build --workspace`
Expected: SUCCESS, all ten members compile.

- [ ] **Step 5b: Verify layering passes**

Run: `cargo xtask check-layers`
Expected: `layering OK — 10 workspace crates checked`

- [ ] **Step 6: Prove the check catches an injected violation**

Add to `crates/lapidary-vcs/Cargo.toml` under `[dependencies]`:

```toml
lapidary-index.workspace = true
```

Run: `cargo xtask check-layers`
Expected: FAIL, printing `lapidary-vcs (L2) -> lapidary-index (L2) is forbidden. L2 crates may depend only on L0 and L1. If these two need to share something, move it into lapidary-core.`

Now **revert that line** and re-run:

Run: `cargo xtask check-layers`
Expected: `layering OK`

- [ ] **Step 7: Commit**

```bash
git add crates Cargo.toml Cargo.lock
git commit -m "feat: scaffold the remaining crates with error surfaces

Each crate carries its thiserror enum and public traits; bodies arrive
in later phases. lapidary-db is the only crate naming sqlx, and verifies
PostgreSQL 18 on connect because generated columns must be STORED.

Verified the layering check rejects an injected lapidary-vcs ->
lapidary-index edge before reverting it."
```

---

### Task 5: `lapidary-cad` — Kernel trait and MockKernel

**Files:**
- Create: `crates/lapidary-cad/Cargo.toml`, `src/lib.rs`, `src/kernel.rs`, `src/mock.rs`
- Create: `sidecar/occt-bridge/README.md` — the 0b placeholder the spec's repo end state calls for

**Interfaces:**
- Consumes: `lapidary-core` types.
- Produces: `Kernel` trait, `KernelVersion`, `KernelParams`, `KernelOutput`, `CadError`, `MockKernel`. Phase 0b adds `OcctKernel` implementing the same trait without changing it.

- [ ] **Step 1: Write the failing test**

`crates/lapidary-cad/Cargo.toml`:

```toml
[package]
name = "lapidary-cad"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
mock-kernel = []

[dependencies]
lapidary-core.workspace = true
async-trait.workspace = true
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt"] }
```

`crates/lapidary-cad/src/lib.rs`:

```rust
#![deny(clippy::unwrap_used)]
//! The CAD kernel boundary. One shipped implementation (OCCT, native, in the worker
//! container) plus a test double. The open path never invokes this crate.

mod kernel;
#[cfg(feature = "mock-kernel")]
mod mock;

pub use kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
#[cfg(feature = "mock-kernel")]
pub use mock::MockKernel;

#[cfg(all(test, feature = "mock-kernel"))]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn mock_kernel_reports_a_pinned_version() {
        let kernel = MockKernel::new();
        assert_eq!(kernel.version().implementation, "mock");
    }

    #[tokio::test]
    async fn mock_kernel_returns_fixture_output_for_a_known_part() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(Path::new("bearing-block-608zz.step"), &KernelParams::default())
            .await
            .expect("mock kernel processes the known fixture");
        assert_eq!(out.triangle_count, 48_112);
        assert_eq!(out.bbox_mm, [61.0, 42.0, 18.5]);
        assert!(!out.entities.is_empty(), "STEP input must yield B-rep entities");
    }

    /// The measurement invariant, locked. `CLAUDE.md` requires that mesh-derived values
    /// are labelled approximate, always — which downstream code decides by asking whether
    /// the kernel returned any analytic entities. If this ever returns a non-empty vec for
    /// mesh input, tessellated numbers start being presented as exact.
    #[tokio::test]
    async fn mesh_input_yields_no_analytic_entities() {
        let kernel = MockKernel::new();
        let out = kernel
            .process(Path::new("bracket-lp-1042-03.stl"), &KernelParams::default())
            .await
            .expect("mock kernel processes the known mesh fixture");
        assert_eq!(out.triangle_count, 12_940);
        assert_eq!(out.bbox_mm, [88.0, 34.0, 12.0]);
        assert!(
            out.entities.is_empty(),
            "mesh input must yield no analytic entities — every measurement taken from it \
             is approximate, and an entity list is what tells callers otherwise"
        );
    }

    #[tokio::test]
    async fn mock_kernel_reports_an_actionable_error_for_unknown_input() {
        let kernel = MockKernel::new();
        let err = kernel
            .process(Path::new("nonexistent.step"), &KernelParams::default())
            .await
            .expect_err("unknown fixture must fail");
        let msg = err.to_string();
        assert!(msg.contains("nonexistent.step"));
        // Assert the remedy clause, not just the word "fixture" — deleting the advice and
        // leaving "No fixture is registered for {path}." must fail this test.
        assert!(msg.contains("add an arm"), "error must say what to do, not just what broke");
    }
}
```

- [ ] **Step 2: Add the crate to the workspace, then run the test to verify it fails**

Add `"crates/lapidary-cad"` to the `members` array in the root `Cargo.toml`.

Run: `cargo test -p lapidary-cad --features mock-kernel`
Expected: FAIL — `kernel` and `mock` modules do not exist.

- [ ] **Step 3: Implement the kernel boundary**

`crates/lapidary-cad/src/kernel.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Pinned across the fleet — a worker running a different kernel version must not
/// produce derivatives that are cached as equivalent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelVersion {
    pub implementation: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelParams {
    /// Linear deflection in mm for tessellation. None means the kernel's default.
    pub linear_deflection_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelOutput {
    pub triangle_count: u32,
    pub bbox_mm: [f64; 3],
    /// Analytic B-rep entities, empty for mesh input. Measurement snaps to these.
    pub entities: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CadError {
    #[error("Could not read {path} — it may use an unsupported AP schema. Re-export from your CAD tool as AP214 or AP242 and retry.")]
    UnsupportedSchema { path: String },

    #[error("No fixture is registered for {path}. MockKernel answers only for the part names matched in crates/lapidary-cad/src/mock.rs; add an arm there, or run against the real kernel.")]
    NoFixture { path: String },

    #[error("The CAD kernel did not respond within {seconds}s while processing {path}. The file may be unusually large; raise LAPIDARY_KERNEL_TIMEOUT or split the assembly.")]
    Timeout { path: String, seconds: u64 },
}

/// One shipped implementation. The trait exists so tests have a double.
#[async_trait::async_trait]
pub trait Kernel: Send + Sync {
    fn version(&self) -> KernelVersion;

    async fn process(&self, src: &Path, params: &KernelParams) -> Result<KernelOutput, CadError>;
}
```

`crates/lapidary-cad/src/mock.rs`:

```rust
use crate::kernel::{CadError, Kernel, KernelOutput, KernelParams, KernelVersion};
use std::path::Path;

/// Returns canned output for known fixture names. Phase 0b replaces this in production
/// with OcctKernel; this stays for tests.
pub struct MockKernel;

impl MockKernel {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Kernel for MockKernel {
    fn version(&self) -> KernelVersion {
        KernelVersion { implementation: "mock".to_owned(), version: "0a".to_owned() }
    }

    async fn process(&self, src: &Path, _params: &KernelParams) -> Result<KernelOutput, CadError> {
        let name = src.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        match name {
            "bearing-block-608zz.step" => Ok(KernelOutput {
                triangle_count: 48_112,
                bbox_mm: [61.0, 42.0, 18.5],
                entities: vec![
                    "CYLINDRICAL_SURFACE:22.000".to_owned(),
                    "PLANE:top".to_owned(),
                    "CYLINDRICAL_SURFACE:8.000".to_owned(),
                ],
            }),
            "bracket-lp-1042-03.stl" => Ok(KernelOutput {
                triangle_count: 12_940,
                bbox_mm: [88.0, 34.0, 12.0],
                entities: Vec::new(), // mesh input: no analytic entities, values are approximate
            }),
            _ => Err(CadError::NoFixture { path: src.display().to_string() }),
        }
    }
}
```

The `unwrap_or_default()` on the file name is intentional and not an `unwrap()` — it yields an empty name that falls through to `NoFixture`.

The canned outputs live in the match arms rather than in fixture files on disk. A test
double that reads and parses JSON has its own failure modes, and this one exists so tests
never touch the filesystem or a kernel. That is why `serde_json` is not a dependency of
this crate and why `CadError::NoFixture` names `src/mock.rs` rather than a directory.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p lapidary-cad --features mock-kernel`
Expected: 4 tests PASS.

- [ ] **Step 5: Verify the crate builds without the feature**

Run: `cargo build -p lapidary-cad`
Expected: SUCCESS with no `MockKernel` compiled in.

- [ ] **Step 6: Verify layering still passes**

Run: `cargo xtask check-layers`
Expected: `layering OK`

- [ ] **Step 7: Create the 0b sidecar placeholder**

The spec's repo end state lists `sidecar/occt-bridge/` as existing in 0a with a README and
nothing else, and `.gitignore` already carries `sidecar/occt-bridge/build/` — an entry
pointing at a directory that does not exist. Create `sidecar/occt-bridge/README.md`:

```markdown
# occt-bridge

Empty until Phase 0b.

This directory will hold the C++ sidecar wrapping Open CASCADE: STEP and IGES reading,
tessellation, analytic B-rep entity extraction for measurement, and format conversion. It
is a separate process rather than a linked library, so an OCCT crash takes down one job
instead of the worker.

In Phase 0a the `Kernel` trait in `crates/lapidary-cad` has exactly one implementation,
`MockKernel`, returning canned output for named parts. Phase 0b adds `OcctKernel` behind
the same trait and builds OCCT from source in the worker image. The trait does not change.

Nothing in Phase 0a depends on this directory.
```

- [ ] **Step 8: Commit**

```bash
git add crates/lapidary-cad sidecar Cargo.toml Cargo.lock
git commit -m "feat(cad): Kernel trait and feature-gated MockKernel

Phase 0b adds OcctKernel behind the same trait without changing it.
Mesh fixtures return empty entities so callers cannot accidentally
present tessellated values as analytic."
```

---

### Task 6: `lapidary-api` and the health endpoint

**Files:**
- Create: `crates/lapidary-api/Cargo.toml`, `src/lib.rs`, `src/health.rs`
- Create: `crates/lapidary-api/tests/health.rs`

**Interfaces:**
- Consumes: `lapidary_db::{connect, PgPool}`.
- Produces: `lapidary_api::router(state: AppState) -> axum::Router`, `lapidary_api::AppState`. Task 7's binary calls `router`.

- [ ] **Step 1: Write the failing test**

`crates/lapidary-api/Cargo.toml` — note there is no `[[bin]]` and no `src/main.rs`; this crate is a library:

```toml
[package]
name = "lapidary-api"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
lapidary-core.workspace = true
lapidary-db.workspace = true
lapidary-storage.workspace = true
lapidary-jobs.workspace = true
lapidary-index.workspace = true
lapidary-vcs.workspace = true
lapidary-build.workspace = true
lapidary-targets.workspace = true
lapidary-cad.workspace = true
axum.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tower-http.workspace = true
tracing.workspace = true

[dev-dependencies]
sqlx.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
tower = { workspace = true, features = ["util"] }
```

`crates/lapidary-api/tests/health.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, router};
use tower::ServiceExt;

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_reports_ok_and_the_postgres_major_version(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool });

    let response = app
        .oneshot(Request::builder().uri("/api/healthz").body(Body::empty()).expect("request builds"))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");

    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"]["major"], 18);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn healthz_says_what_broke_and_what_to_do_when_the_database_is_gone(pool: sqlx::PgPool) {
    // Closing the pool is the cheapest reliable way to make server_version_num fail.
    // This test is also what gives the success test above its meaning: a handler that
    // hardcoded {"status":"ok","database":{"major":18}} and never touched the pool would
    // pass that one, and fail this one.
    pool.close().await;
    let app = router(AppState { db: pool });

    let response = app
        .oneshot(Request::builder().uri("/api/healthz").body(Body::empty()).expect("request builds"))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body reads");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body is JSON");
    assert_eq!(json["status"], "unavailable");

    // "Errors say what broke and what to do." Assert the remedy, not just the failure:
    // deleting the advice and leaving "Could not reach the database." must fail here.
    let message = json["message"].as_str().expect("message is a string");
    assert!(message.contains("`db` service"), "must name the service to check");
    assert!(message.contains("DATABASE_URL"), "must name the setting to check");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn unknown_routes_are_not_found(pool: sqlx::PgPool) {
    let app = router(AppState { db: pool });
    let response = app
        .oneshot(Request::builder().uri("/api/nope").body(Body::empty()).expect("request builds"))
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

`#[sqlx::test]` provisions a throwaway database per test from `DATABASE_URL`.

The explicit `migrations = "../lapidary-db/migrations"` is **required and not cosmetic**.
Left off, sqlx infers `./migrations` relative to *this* crate's manifest directory, finds
no such directory, and emits no migrator at all — no warning, no error, tests simply run
against an empty schema. That is invisible for a health check that only reads
`server_version_num`, and would silently invalidate every Phase 1 repository test.

It needs a live Postgres — Task 11 supplies one in CI. Locally: `podman run --rm -d -p 5432:5432 -e POSTGRES_PASSWORD=lapidary --name lapidary-test-db postgres:18` then `export DATABASE_URL=postgres://postgres:lapidary@localhost:5432/postgres`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p lapidary-api`
Expected: FAIL — `lapidary_api::router` and `AppState` do not exist.

- [ ] **Step 3: Implement the router**

`crates/lapidary-api/src/lib.rs`:

```rust
#![deny(clippy::unwrap_used)]
//! The HTTP surface. This crate is a LIBRARY that builds a Router — never a binary,
//! and never forked per distribution.

mod health;

use axum::Router;
use axum::routing::get;
use lapidary_db::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

/// Build the application router. Callers own the listener.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/healthz", get(health::healthz))
        .with_state(state)
}
```

`crates/lapidary-api/src/health.rs`:

First add the query to `lapidary-db` — `crates/lapidary-db/src/lib.rs`:

```rust
/// The PostgreSQL `server_version_num` (e.g. 180002). Lives here because no SQL may
/// appear outside this crate.
pub async fn server_version_num(pool: &PgPool) -> Result<i32, DbError> {
    let num = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(pool)
        .await?;
    Ok(num)
}
```

Then `crates/lapidary-api/src/health.rs` — note it names `sqlx` nowhere:

```rust
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    status: &'static str,
    database: Database,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Database {
    major: i32,
    reachable: bool,
}

/// Proves the whole path: HTTP in, a real query against Postgres, JSON out.
pub async fn healthz(State(state): State<AppState>) -> Response {
    match lapidary_db::server_version_num(&state.db).await {
        Ok(num) => axum::Json(Health {
            status: "ok",
            database: Database { major: num / 10_000, reachable: true },
        })
        .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "status": "unavailable",
                "message": "Could not reach the database. Check that the `db` service is running and that DATABASE_URL matches it."
            })),
        )
            .into_response(),
    }
}
```

`sqlx` appears in `lapidary-api`'s `[dev-dependencies]` only, for the `#[sqlx::test]` attribute. It must never appear in `[dependencies]` — `deny.toml` in Task 11 enforces that.

Add `crates/lapidary-api` to the workspace `members` array.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p lapidary-api`
Expected: 3 tests PASS.

- [ ] **Step 5: Verify the crate produces no binary**

Run: `cargo build -p lapidary-api && ls target/debug/lapidary-api 2>/dev/null && echo "FAIL: binary exists" || echo "OK: library only"`
Expected: `OK: library only`

- [ ] **Step 6: Commit**

```bash
git add crates/lapidary-api crates/lapidary-db Cargo.toml Cargo.lock
git commit -m "feat(api): axum router with a database-backed health endpoint

lapidary-api is a library that builds a Router and depends on every L2
crate. The version query lives in lapidary-db so no SQL leaks out of it."
```

---

### Task 7: The two binaries

**Files:**
- Create: `bin/lapidary-server/Cargo.toml`, `src/main.rs`
- Create: `bin/lapidary/Cargo.toml`, `src/main.rs`

**Interfaces:**
- Consumes: `lapidary_api::{router, AppState}`, `lapidary_db::connect`.
- Produces: the `lapidary-server` and `lapidary` executables.

- [ ] **Step 1: Write the server binary**

`bin/lapidary-server/Cargo.toml`:

```toml
[package]
name = "lapidary-server"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
lapidary-api.workspace = true
lapidary-db.workspace = true
anyhow.workspace = true
# This binary owns the listener, so it calls axum::serve directly. lapidary-api builds
# the Router and does not re-export axum. Both resolve to the one version pinned in
# [workspace.dependencies], so there is no skew between the Router's axum and this one.
axum.workspace = true
figment.workspace = true
serde.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

`bin/lapidary-server/src/main.rs`:

```rust
//! Container entrypoint: the API, and optionally an in-process worker.

use anyhow::{Context, Result};
use figment::Figment;
use figment::providers::Env;
use lapidary_api::{AppState, router};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    database_url: String,
    #[serde(default = "default_bind")]
    bind: String,
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_owned()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        // from_env() would default to ERROR, which silences the "listening" line below —
        // a container-first product that prints nothing on a successful start is not
        // operable. Default to INFO; LAPIDARY_LOG still overrides.
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .with_env_var("LAPIDARY_LOG")
                .from_env_lossy(),
        )
        .init();

    let config: Config = Figment::new()
        // Order matters: figment's later merge wins, so the namespaced variable is merged
        // LAST and takes precedence. sqlx projects routinely have a bare DATABASE_URL in
        // the environment for compile-time query checking, and it must not silently
        // override an operator's deliberate LAPIDARY_DATABASE_URL.
        .merge(Env::raw().only(&["DATABASE_URL"]))
        .merge(Env::prefixed("LAPIDARY_"))
        .extract()
        .context("Configuration is incomplete. Set LAPIDARY_DATABASE_URL (preferred — it wins if both are set) or DATABASE_URL; see deploy/.env.example.")?;

    let db = lapidary_db::connect(&config.database_url)
        .await
        .context("Could not start: the database is unreachable. Check that the `db` service is running.")?;

    lapidary_db::migrate(&db)
        .await
        .context("Could not start: the database schema could not be brought up to date.")?;

    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("Could not bind {}. Another process may already hold that port.", config.bind))?;

    tracing::info!(bind = %config.bind, "lapidary-server listening");
    axum::serve(listener, router(AppState { db }))
        .await
        .context("The HTTP server stopped unexpectedly")?;

    Ok(())
}
```

Migrations run at startup rather than as a separate deploy step: the image carries its
own schema, which is what an air-gapped operator needs. `api` and `worker` are the same
binary and start together, so both will call `migrate` — sqlx takes a Postgres advisory
lock around the migrator, so the second waits rather than colliding with the first.

- [ ] **Step 2: Write the desktop binary with subcommand stubs**

`bin/lapidary/Cargo.toml`:

```toml
[package]
name = "lapidary"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
anyhow.workspace = true
clap.workspace = true
```

`bin/lapidary/src/main.rs`:

```rust
//! Desktop binary. Ships properly in Phase 4; the subcommands are declared here so the
//! CLI surface is stable from the start.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lapidary", version, about = "Lapidary — a visual index for 3D part libraries")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch a workspace directory and return changed files as new revisions.
    Agent,
    /// Run a job worker against a Lapidary server.
    Worker,
    /// Start a local Lapidary stack.
    Up,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let name = match cli.command {
        Commands::Agent => "agent",
        Commands::Worker => "worker",
        Commands::Up => "up",
    };
    bail!("`lapidary {name}` arrives in Phase 4. Until then, run the stack with `podman compose -f deploy/compose.yaml up`.")
}
```

- [ ] **Step 3: Add both binaries to the workspace, then verify they build**

Add `"bin/lapidary-server"` and `"bin/lapidary"` to the `members` array in the root
`Cargo.toml`. It is now complete and matches the fourteen members listed in Task 2's
File Structure.

Run:

```bash
cargo build --workspace
cargo run --quiet -p lapidary -- --help
```

Expected: build SUCCESS; help lists `agent`, `worker`, `up`.

- [ ] **Step 4: Verify the stub exits with a useful message**

Run: `cargo run --quiet -p lapidary -- agent; echo "exit=$?"`
Expected: the Phase 4 message on stderr, `exit=1`.

- [ ] **Step 5: Verify layering**

Run: `cargo xtask check-layers`
Expected: `layering OK — 14 workspace crates checked` — eleven `lapidary-*` crates, two binaries and `xtask`.

- [ ] **Step 6: Commit**

```bash
git add bin Cargo.toml Cargo.lock
git commit -m "feat(bin): lapidary-server entrypoint and lapidary CLI stubs

The CLI surface (agent | worker | up) is declared now so it is stable
before Phase 4 fills it in; each subcommand exits with the reason and
the alternative."
```

---

### Task 8: ts-rs export pipeline

**Files:**
- Modify: `xtask/src/main.rs`
- Create: `web/src/bindings/` (generated, committed)

**Interfaces:**
- Consumes: the `#[ts(export)]` derives from Task 2.
- Produces: `cargo xtask export-bindings`; `web/src/bindings/*.ts` consumed by Task 9.

- [ ] **Step 1: Add the subcommand**

In `xtask/src/main.rs`, extend the match in `main`:

```rust
        Some("export-bindings") => export_bindings(),
        Some(other) => bail!("Unknown xtask '{other}'. Available: check-layers, export-bindings"),
```

and add:

```rust
/// Regenerate the TypeScript bindings from #[ts(export)] types in lapidary-core.
fn export_bindings() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must live one level below the workspace root")?
        .to_path_buf();
    let out = root.join("web/src/bindings");

    // ts-rs writes on test run; clear first so removed types do not linger.
    if out.exists() {
        std::fs::remove_dir_all(&out).context("Could not clear web/src/bindings")?;
    }
    std::fs::create_dir_all(&out).context("Could not create web/src/bindings")?;

    let status = Command::new(env!("CARGO"))
        .args(["test", "-p", "lapidary-core", "export_bindings"])
        .env("TS_RS_EXPORT_DIR", &out)
        .status()
        .context("Could not run the ts-rs export tests")?;

    if !status.success() {
        bail!("ts-rs export failed. Run `cargo test -p lapidary-core` to see which type could not be exported.");
    }

    println!("bindings written to {}", out.display());
    Ok(())
}
```

- [ ] **Step 2: Generate the bindings**

Run: `cargo xtask export-bindings`
Expected: `bindings written to …/web/src/bindings`

- [ ] **Step 3: Verify the expected files exist**

Run: `ls web/src/bindings/`
Expected: `Approximate.ts`, `BlobHash.ts`, `LibraryId.ts`, `LibraryMode.ts`, `PartId.ts`, `PartSummary.ts`, `RevisionId.ts`.

There is deliberately no `Timestamp.ts`: with the `jiff-impl` feature, `jiff::Timestamp` maps straight onto the TypeScript `string` primitive, so `PartSummary.ts` carries `createdAt: string` and no separate file is emitted. `BlobHash` does get its own file — `#[ts(as = "String")]` still exports the alias, as `export type BlobHash = string`.

If a file is missing, the corresponding type is missing `#[ts(export)]` — add it in `lapidary-core` and re-run.

- [ ] **Step 4: Verify the staleness check works**

Run:

```bash
cargo xtask export-bindings && git status --porcelain web/src/bindings
```

Expected: no output the second time — regenerating produces byte-identical files. This is exactly what CI asserts in Task 12.

- [ ] **Step 5: Commit**

```bash
git add xtask/src/main.rs web/src/bindings
git commit -m "feat(xtask): ts-rs binding export

Bindings are committed rather than gitignored so CI can detect drift
between Rust types and the frontend by regenerating and diffing."
```

---

### Task 9: The web application

**Files:**
- Create: `web/package.json`, `web/vite.config.ts`, `web/tsconfig.json`, `web/index.html`
- Create: `web/src/main.tsx`, `web/src/styles.css`, `web/src/lib/strings.ts`, `web/src/lib/api.ts`, `web/src/lib/types.ts`
- Create: `web/src/routes/__root.tsx`, `web/src/routes/index.tsx`
- Create: `web/src/routes/index.test.tsx`, `web/vitest.config.ts`
- Generate and **commit**: `web/src/routeTree.gen.ts`

**Interfaces:**
- Consumes: `web/src/bindings/*.ts` from Task 8; `GET /api/healthz` from Task 6.
- Produces: a static bundle in `web/dist/`, consumed by Task 11's container.

- [ ] **Step 1: Create the package manifest**

`web/package.json` — exact versions, no carets:

```json
{
  "name": "lapidary-web",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview",
    "typecheck": "tsc --noEmit",
    "test": "vitest run"
  },
  "dependencies": {
    "@tanstack/react-query": "5.102.8",
    "@tanstack/react-router": "1.170.32",
    "react": "19.2.8",
    "react-dom": "19.2.8"
  },
  "devDependencies": {
    "@tailwindcss/vite": "4.3.3",
    "@tanstack/router-plugin": "1.168.35",
    "@testing-library/dom": "10.4.1",
    "@testing-library/react": "16.3.0",
    "@types/react": "19.2.2",
    "@types/react-dom": "19.2.2",
    "@vitejs/plugin-react": "6.1.1",
    "jsdom": "27.0.0",
    "tailwindcss": "4.3.3",
    "typescript": "7.0.2",
    "vite": "8.2.2",
    "vitest": "4.1.11"
  },
  "engines": {
    "node": ">=24"
  }
}
```

Install with `cd web && npm install`.

`@testing-library/dom` is listed explicitly because `@testing-library/react` 16 declares it
as a required peer. npm would install it silently, but an unlisted direct dependency is
exactly what breaks under a different npm version or a stricter installer.

`@tanstack/react-query-devtools` is deliberately absent. It was pinned in an earlier draft
and never imported; Phase 1 can add it when there are queries worth inspecting.

`@tanstack/router-plugin` 1.168.35 declares a peer of `@tanstack/react-router` `^1.170.32`
— exactly the pin above. Move them together, never one alone.

If `@types/react`, `@testing-library/*` or `jsdom` resolve to different current versions, pin whatever `npm view <pkg> version` reports and record it — those are test-only and not load-bearing on the spec.

- [ ] **Step 2: Configure Vite**

`web/vite.config.ts` — the router plugin must come before the React plugin:

```ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { tanstackRouter } from '@tanstack/router-plugin/vite'

export default defineConfig({
  plugins: [
    tanstackRouter({
      target: 'react',
      autoCodeSplitting: true,
      // index.test.tsx sits beside the route it tests. Without this the plugin scans it
      // as a route file and warns "does not export a Route" on every single build, CI
      // included. The pattern excludes any test file next to a route, not just this one.
      routeFileIgnorePattern: '\\.test\\.tsx?$',
    }),
    react(),
    tailwindcss(),
  ],
  server: {
    proxy: {
      '/api': { target: 'http://localhost:8080', changeOrigin: true },
    },
  },
})
```

`web/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "verbatimModuleSyntax": true,
    "noUncheckedIndexedAccess": true,
    "types": ["vite/client"]
  },
  "include": ["src"]
}
```

`web/vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: { environment: 'jsdom', globals: true },
})
```

- [ ] **Step 3: Write the strings module**

`web/src/lib/strings.ts` — no bare user-facing strings anywhere else:

```ts
/**
 * Every user-facing string. English only; Turkish is the planned second locale, which
 * is why nothing is inlined in a component.
 */
export const strings = {
  appName: 'Lapidary',
  health: {
    checking: 'Checking the server…',
    ok: (major: number) => `Connected — PostgreSQL ${major}`,
    failed: 'Could not reach the server. Check that the api and db services are running.',
  },
  emptyLibrary: {
    title: 'No parts yet',
    body: 'Drop a folder of STL or STEP files to begin.',
  },
} as const
```

- [ ] **Step 4: Write the dark-only stylesheet**

`web/src/styles.css` — Tailwind v4 configures theme in CSS; there is no `tailwind.config.js`:

```css
@import "tailwindcss";

@theme {
  --color-bg: #0b0c0e;
  --color-surface: #131519;
  --color-border: #24272d;
  --color-text: #e6e8ec;
  --color-muted: #9aa1ac;
  --color-accent: #6ea8fe;

  --ease-mechanical: cubic-bezier(0.2, 0, 0, 1);
  --duration-fast: 120ms;
  --duration-base: 180ms;
  --duration-slow: 280ms;
}

/* Dark only. No light mode, no toggle. */
:root {
  color-scheme: dark;
  background-color: var(--color-bg);
  color: var(--color-text);
}

*,
*::before,
*::after {
  transition-timing-function: var(--ease-mechanical);
  transition-duration: var(--duration-base);
  transition-property: transform, opacity;
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
  }
}
```

- [ ] **Step 5: Write the API client and routes**

`web/src/lib/types.ts` — the single import site for everything `cargo xtask export-bindings` produces. CI already fails when the bindings are stale, but staleness is not the only failure: a type that is renamed or dropped on the Rust side regenerates cleanly and then silently has no consumer. This file is what breaks instead:

```ts
/**
 * Re-exports of the ts-rs output in ../bindings. Import domain types from here, never
 * from ../bindings directly — this is the one file that fails to compile when a Rust
 * type is renamed or removed.
 *
 * Phase 1 is the first real consumer, when the parts endpoint returns PartSummary.
 */
export type { Approximate } from '../bindings/Approximate'
export type { BlobHash } from '../bindings/BlobHash'
export type { LibraryId } from '../bindings/LibraryId'
export type { LibraryMode } from '../bindings/LibraryMode'
export type { PartId } from '../bindings/PartId'
export type { PartSummary } from '../bindings/PartSummary'
export type { RevisionId } from '../bindings/RevisionId'
```

`web/src/lib/api.ts` — `Health` is hand-written on purpose. It is `lapidary-api`'s own response shape, private to that crate and absent from `lapidary-core`, so no binding exists for it and inventing one would put an HTTP concern in the domain crate:

```ts
export interface Health {
  status: string
  database: { major: number; reachable: boolean }
}

export async function fetchHealth(): Promise<Health> {
  const response = await fetch('/api/healthz')
  if (!response.ok) {
    throw new Error(`healthz returned ${response.status}`)
  }
  return (await response.json()) as Health
}
```

`web/src/routes/__root.tsx`:

```tsx
import { Outlet, createRootRoute } from '@tanstack/react-router'
import { strings } from '../lib/strings'

export const Route = createRootRoute({
  component: () => (
    <div className="min-h-screen bg-[var(--color-bg)] text-[var(--color-text)]">
      <header className="border-b border-[var(--color-border)] px-6 py-4">
        <h1 className="text-sm font-medium tracking-widest uppercase">{strings.appName}</h1>
      </header>
      <main className="p-6">
        <Outlet />
      </main>
    </div>
  ),
})
```

`web/src/routes/index.tsx`:

```tsx
import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { fetchHealth } from '../lib/api'
import { strings } from '../lib/strings'

export const Route = createFileRoute('/')({ component: Index })

export function Index() {
  const { data, isPending, isError } = useQuery({ queryKey: ['health'], queryFn: fetchHealth })

  return (
    <section>
      <h2 className="text-lg">{strings.emptyLibrary.title}</h2>
      <p className="text-[var(--color-muted)]">{strings.emptyLibrary.body}</p>
      <p className="mt-6 text-sm text-[var(--color-muted)]">
        {isPending
          ? strings.health.checking
          : isError
            ? strings.health.failed
            : strings.health.ok(data.database.major)}
      </p>
    </section>
  )
}
```

`web/index.html`:

```html
<!doctype html>
<html lang="en" class="dark">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Lapidary</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`web/src/main.tsx`:

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider, createRouter } from '@tanstack/react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { routeTree } from './routeTree.gen'
import './styles.css'

const router = createRouter({ routeTree })
const queryClient = new QueryClient()

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('index.html is missing its #root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
)
```

`web/src/routeTree.gen.ts` is written by the router plugin during a Vite run and is **committed**, not gitignored — the same rule as `web/src/bindings/`, and this time it is load-bearing rather than a preference.

It has to be committed because both `npm run build` and `npm run typecheck` begin with `tsc --noEmit`, and `src/main.tsx` imports `./routeTree.gen`. On a clean clone — which is what CI checks out and what `deploy/web/Containerfile` copies — a gitignored route tree means `tsc` fails to resolve that import before Vite ever gets the chance to write it. This is TanStack's own default for file-based routing, for the same reason.

Generate it once now, with a Vite build that skips tsc:

```bash
cd web && npx vite build
```

Expected: `src/routeTree.gen.ts` appears, and the output carries **no** `does not export a Route` warning — if it does, `routeFileIgnorePattern` is missing from `vite.config.ts`. From here `npm run build` works normally, and Task 11's CI job re-runs the build and fails if the committed tree has drifted.

- [ ] **Step 6: Write the failing test**

`web/src/routes/index.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { Index } from './index'
import { strings } from '../lib/strings'

function renderIndex() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Index />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.restoreAllMocks()
})

test('renders the connected state from a healthy response', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ status: 'ok', database: { major: 18, reachable: true } }),
    }),
  )
  renderIndex()
  expect(await screen.findByText(strings.health.ok(18))).toBeDefined()
})

test('renders an actionable message when the server is unreachable', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 503 }))
  renderIndex()
  expect(await screen.findByText(strings.health.failed)).toBeDefined()
})
```

- [ ] **Step 7: Run the tests to verify they fail, then pass**

Run: `cd web && npm test`
Expected first: FAIL if any file above is missing. After all files exist: 2 tests PASS.

- [ ] **Step 8: Verify typecheck and build**

Run: `cd web && npm run typecheck && npm run build`
Expected: no type errors; `web/dist/index.html` and hashed assets produced.

`npm run typecheck` passing at all is the proof that Step 5's `npx vite build` wrote `src/routeTree.gen.ts`. If it reports `Cannot find module './routeTree.gen'`, that step was skipped.

- [ ] **Step 9: Commit**

```bash
git add web
git commit -m "feat(web): TanStack Router and Query SPA on Vite

Dark only, every string through lib/strings.ts, motion tokens at
120/180/280ms with prefers-reduced-motion honoured. The index route
renders the health endpoint's three states.

routeTree.gen.ts is committed rather than gitignored: build and
typecheck both run tsc first, so a clean clone must already have the
route tree on disk before Vite gets a chance to write it.

lib/types.ts is the single import site for the ts-rs bindings, so a
renamed Rust type fails the typecheck instead of going unnoticed."
```

---

### Task 10: Container images and the compose stack

**Files:**
- Create: `deploy/Containerfile`, `deploy/db/Containerfile`, `deploy/web/Containerfile`, `deploy/web/Caddyfile`, `deploy/compose.yaml`, `deploy/.env.example`
- Create: `.containerignore` **and** `.dockerignore` at the repo root, plus `deploy/.containerignore` and `deploy/.dockerignore` for the `db` build context

**Interfaces:**
- Consumes: `lapidary-server` from Task 7, `web/dist` from Task 9, migrations from Task 4.
- Produces: a four-service stack. Task 12 verifies it against the exit criteria.

**Image digests** — resolved 2026-09-01, use exactly these:

| Image | Digest |
|---|---|
| `postgres:18` | `sha256:7341002d2b8c7c5bdd7542a671a95b36196c0b5b888daf454ae4fc33ba5346d7` |
| `rust:1.95-trixie` | `sha256:443dd9a3260cf23c22fc05051dd5661dd7b4028d3d25dbaffab6563b63c3539c` |
| `debian:trixie-slim` | `sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f` |
| `node:24-trixie` | `sha256:499ac30d42645e5c2227c0f8ff2439499da08b8893326a991f192dfe8e6b1d98` |
| `caddy:2-alpine` | `sha256:98eb57d882ccd5213d1688764db10c1ca2c58a1ca3a6717a3411ad798f7a423a` |

- [ ] **Step 1: Write the Rust image**

`deploy/Containerfile`:

```dockerfile
# syntax=docker/dockerfile:1
# Rust workspace -> lapidary-server. OCCT is Phase 0b and is deliberately absent.

FROM docker.io/library/rust:1.95-trixie@sha256:443dd9a3260cf23c22fc05051dd5661dd7b4028d3d25dbaffab6563b63c3539c AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY bin ./bin
COPY xtask ./xtask
RUN cargo build --release --locked -p lapidary-server

FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --create-home lapidary
COPY --from=build /src/target/release/lapidary-server /usr/local/bin/lapidary-server
USER lapidary
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/lapidary-server"]
```

- [ ] **Step 2: Write the database image**

The official `postgres:18` image does not ship pgvector; `ROADMAP.md` requires verifying it installs here rather than discovering the problem in Phase 6.

`deploy/db/Containerfile`:

```dockerfile
# syntax=docker/dockerfile:1
FROM docker.io/library/postgres:18@sha256:7341002d2b8c7c5bdd7542a671a95b36196c0b5b888daf454ae4fc33ba5346d7
RUN apt-get update \
 && apt-get install -y --no-install-recommends "postgresql-${PG_MAJOR}-pgvector" \
 && rm -rf /var/lib/apt/lists/*
COPY db/init/10-extensions.sql /docker-entrypoint-initdb.d/10-extensions.sql
```

`deploy/db/init/10-extensions.sql`:

```sql
-- Verified at first boot rather than discovered in Phase 6.
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Turkish is the planned second locale; its snowball config ships with PostgreSQL.
-- Fail loudly at init if that ever stops being true.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_ts_config WHERE cfgname = 'turkish') THEN
    RAISE EXCEPTION 'The turkish text search configuration is missing from this PostgreSQL build.';
  END IF;
END
$$;
```

- [ ] **Step 3: Write the web image**

`deploy/web/Containerfile`:

```dockerfile
# syntax=docker/dockerfile:1
FROM docker.io/library/node:24-trixie@sha256:499ac30d42645e5c2227c0f8ff2439499da08b8893326a991f192dfe8e6b1d98 AS build
WORKDIR /src
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM docker.io/library/caddy:2-alpine@sha256:98eb57d882ccd5213d1688764db10c1ca2c58a1ca3a6717a3411ad798f7a423a
COPY deploy/web/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /src/dist /srv
EXPOSE 8080
```

`deploy/web/Caddyfile`:

```
:8080 {
	root * /srv
	encode gzip zstd

	# The SPA owns routing; proxy the API to the api service.
	handle /api/* {
		reverse_proxy api:8080
	}

	handle {
		try_files {path} /index.html
		file_server
	}
}
```

- [ ] **Step 4: Write the compose file**

`deploy/compose.yaml` — four services, no Redis, no broker:

```yaml
name: lapidary

services:
  db:
    build:
      context: .
      dockerfile: db/Containerfile
    environment:
      POSTGRES_USER: ${POSTGRES_USER:-lapidary}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?set POSTGRES_PASSWORD in .env}
      POSTGRES_DB: ${POSTGRES_DB:-lapidary}
    volumes:
      - lapidary-db:/var/lib/postgresql/data:Z
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ${POSTGRES_USER:-lapidary} -d ${POSTGRES_DB:-lapidary}"]
      interval: 5s
      timeout: 3s
      retries: 20

  api:
    build:
      context: ..
      dockerfile: deploy/Containerfile
    environment:
      DATABASE_URL: postgres://${POSTGRES_USER:-lapidary}:${POSTGRES_PASSWORD}@db:5432/${POSTGRES_DB:-lapidary}
      LAPIDARY_BIND: 0.0.0.0:8080
      LAPIDARY_LOG: ${LAPIDARY_LOG:-info}
    depends_on:
      db:
        condition: service_healthy
    ports:
      - "8080:8080"

  worker:
    build:
      context: ..
      dockerfile: deploy/Containerfile
    environment:
      DATABASE_URL: postgres://${POSTGRES_USER:-lapidary}:${POSTGRES_PASSWORD}@db:5432/${POSTGRES_DB:-lapidary}
      LAPIDARY_BIND: 0.0.0.0:8081
      LAPIDARY_LOG: ${LAPIDARY_LOG:-info}
    depends_on:
      db:
        condition: service_healthy

  web:
    build:
      context: ..
      dockerfile: deploy/web/Containerfile
    depends_on:
      - api
    ports:
      - "3000:8080"

volumes:
  lapidary-db:
```

In 0a the `worker` runs the same binary on a second port — it has no jobs to take because `lapidary-jobs` has no implementation. It exists to prove the topology. Phase 1 gives it a distinct command.

- [ ] **Step 5: Write the environment example**

`deploy/.env.example`:

```bash
# Copy to deploy/.env and edit. Never commit deploy/.env.

POSTGRES_USER=lapidary
POSTGRES_PASSWORD=change-me-before-first-run
POSTGRES_DB=lapidary

# Used by lapidary-server inside the containers. Compose builds it from the values
# above; set it directly only when running the binary outside a container.
# DATABASE_URL=postgres://lapidary:change-me-before-first-run@localhost:5432/lapidary

LAPIDARY_LOG=info
```

Two build contexts need ignore files, and each needs the file under two names. Podman
reads `.containerignore` and falls back to `.dockerignore`; Docker reads **only**
`.dockerignore`. Task 1 deleted the old `.dockerignore`, so without recreating it, the
`docker compose build` in Step 9 ships the entire repository as build context — `target/`
included, which is gigabytes — and `COPY web ./` in the web image copies a stale
`web/node_modules` straight over the one `npm ci` just installed.

`.containerignore` at the repo root, covering the `api`, `worker` and `web` builds whose
context is `..`:

```
target/
web/node_modules/
web/dist/
.git/
docs/
design/
fixtures/
deploy/.env
```

`deploy/.containerignore`, covering the `db` build whose context is `deploy/`:

```
.env
```

Then give each one its Docker name, copied so the content cannot differ:

```bash
cp .containerignore .dockerignore
cp deploy/.containerignore deploy/.dockerignore
```

Task 11 Step 5 asserts the pairs stay identical.

- [ ] **Step 6: Bring the stack up**

Run:

```bash
cp deploy/.env.example deploy/.env
sed -i 's/change-me-before-first-run/localdev/' deploy/.env
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
```

Expected: four containers running. First build takes several minutes on 8 cores.

`--env-file` is passed explicitly rather than relying on Compose auto-loading `deploy/.env`
from the compose file's directory. `docker compose` does that; `podman-compose` has not
always. Passing it removes the difference between the two runtimes, which is the whole
point of Step 9. `deploy/.env` is already ignored by the root `.gitignore` rule `.env`,
which has no leading slash and so matches at any depth.

- [ ] **Step 7: Verify the health path end to end**

Run:

```bash
curl -fsS http://localhost:8080/api/healthz
curl -fsS http://localhost:3000/api/healthz
```

Expected both: `{"status":"ok","database":{"major":18,"reachable":true}}`

The second proves Caddy's reverse proxy works, so the SPA can reach the API through one origin.

- [ ] **Step 8: Verify pgvector and the Turkish config**

Run:

```bash
podman compose --env-file deploy/.env -f deploy/compose.yaml exec db psql -U lapidary -d lapidary \
  -c "SELECT extname FROM pg_extension WHERE extname IN ('vector','pg_trgm');" \
  -c "SELECT cfgname FROM pg_ts_config WHERE cfgname = 'turkish';"
```

Expected: both `vector` and `pg_trgm` listed; `turkish` returned.

- [ ] **Step 9: Verify Docker as well as Podman**

Run:

```bash
podman compose --env-file deploy/.env -f deploy/compose.yaml down -v
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
curl -fsS http://localhost:8080/api/healthz
docker compose --env-file deploy/.env -f deploy/compose.yaml down -v
```

Expected: identical health JSON. `CLAUDE.md` supports both runtimes and this machine has both, so it is verified now rather than assumed.

- [ ] **Step 10: Commit**

```bash
git add deploy .containerignore .dockerignore
git commit -m "feat(deploy): four-service compose stack on pinned digests

web, api, worker, db. No Redis and no broker — the job queue is
FOR UPDATE SKIP LOCKED plus LISTEN/NOTIFY from Phase 1.

The db image adds pgvector, which postgres:18 does not ship, and fails
at init if the turkish text search config is ever absent. Verified under
both podman compose and docker compose.

Ignore files are written under both names: Docker reads only
.dockerignore, and without it the build context is the whole repo
including target/."
```

---

### Task 11: CI and supply chain

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/containers.yml`, `deny.toml`

**Interfaces:**
- Consumes: every check established in Tasks 2–10.
- Produces: the gate that keeps them true.

**Action SHAs** — resolved 2026-09-01 and **re-verified against the GitHub API before Task 11 was dispatched**. Two of the five were wrong on first writing and did not exist as commits at all; these are the checked values. Verify with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` if you touch them:

| Action | Tag | SHA |
|---|---|---|
| `actions/checkout` | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `actions/setup-node` | v7.0.0 | `820762786026740c76f36085b0efc47a31fe5020` |
| `Swatinem/rust-cache` | v2.9.2 | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` |
| `EmbarkStudios/cargo-deny-action` | v2.1.1 | `3c6349835b2b7b196a839186cb8b78e02f7b5f25` |
| `actions/upload-artifact` | v7.0.1 | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` |

- [ ] **Step 1: Write the cargo-deny policy**

`deny.toml`:

```toml
[graph]
targets = [{ triple = "x86_64-unknown-linux-gnu" }]

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
# AGPL-3.0-only, not the bare AGPL-3.0: the bare id is deprecated in SPDX and does not
# match what [workspace.package] declares, so cargo-deny would reject all fourteen of our
# own crates while passing every third-party one.
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib", "AGPL-3.0-only"]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "deny"

# No SQL outside lapidary-db. Enforced here rather than by review.
[[bans.deny]]
name = "sqlx"
# lapidary-api appears only because #[sqlx::test] is a dev-dependency; it must never
# gain a normal sqlx dependency. If this list grows any further, SQL has leaked.
wrappers = ["lapidary-db", "lapidary-api"]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [ ] **Step 2: Write the CI workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main, rust-rewrite]
  pull_request:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  rust:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:18@sha256:7341002d2b8c7c5bdd7542a671a95b36196c0b5b888daf454ae4fc33ba5346d7
        env:
          POSTGRES_PASSWORD: lapidary
        options: >-
          --health-cmd pg_isready --health-interval 5s --health-timeout 3s --health-retries 20
        ports: ["5432:5432"]
    env:
      DATABASE_URL: postgres://postgres:lapidary@localhost:5432/postgres
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings
      - run: cargo xtask check-layers
      - run: cargo test --workspace --all-features

  bindings:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
      - run: cargo xtask export-bindings
      - name: Bindings must be committed and current
        run: |
          if ! git diff --quiet --exit-code web/src/bindings; then
            echo "::error::web/src/bindings is stale. Run 'cargo xtask export-bindings' and commit the result."
            git diff --stat web/src/bindings
            exit 1
          fi

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
      - uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25
        with:
          command: check

  web:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
      - uses: actions/setup-node@820762786026740c76f36085b0efc47a31fe5020
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: web/package-lock.json
      - run: npm ci
      - run: npm run typecheck
      - run: npm test
      - run: npm run build
      - name: The committed route tree must match what the plugin generates
        run: |
          if ! git diff --quiet --exit-code src/routeTree.gen.ts; then
            echo "::error::web/src/routeTree.gen.ts is stale. Run 'npm run build' in web/ and commit the result."
            git diff --stat src/routeTree.gen.ts
            exit 1
          fi
```

- [ ] **Step 3: Write the container workflow**

The image build is too heavy for every push. `.github/workflows/containers.yml`:

```yaml
name: Containers

on:
  workflow_dispatch:
  push:
    tags: ["v*"]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
      - run: docker build -f deploy/Containerfile -t lapidary-server:${{ github.sha }} .
      - run: docker build -f deploy/web/Containerfile -t lapidary-web:${{ github.sha }} .
      - run: docker build -f deploy/db/Containerfile -t lapidary-db:${{ github.sha }} deploy
```

- [ ] **Step 4: Verify every check passes locally**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo test --workspace --all-features
cargo deny check
cargo xtask export-bindings && git diff --exit-code web/src/bindings
(cd web && npm ci && npm run typecheck && npm test && npm run build)
```

Expected: every command exits 0. `cargo deny check` may warn on duplicate versions — that is `multiple-versions = "warn"` and does not fail the build.

If `cargo fmt --all --check` fails here, run `cargo fmt --all` and amend. The code blocks
in this plan are written for readability and several run past 100 columns; rustfmt is the
authority, not the plan.

- [ ] **Step 5: Verify no unpinned Actions slipped in, and that the ignore files agree**

Run:

```bash
grep -rn "uses:" .github/workflows | grep -v "@[0-9a-f]\{40\}" && echo "FAIL: unpinned action" || echo "OK: all pinned"
cmp .containerignore .dockerignore && cmp deploy/.containerignore deploy/.dockerignore && echo "OK: ignore files in sync"
```

Expected: `OK: all pinned` and `OK: ignore files in sync`. The second check exists because
the two names are duplicated files rather than a symlink — symlinks do not survive a
Windows checkout — and a duplicate that drifts silently gives Docker and Podman different
build contexts.

- [ ] **Step 6: Commit and push**

```bash
git add .github deny.toml
git commit -m "ci: fmt, clippy, layering, tests, deny, bindings and web

Actions pinned to commit SHAs and the postgres service to a digest.
deny.toml confines sqlx to lapidary-db, so 'no SQL outside lapidary-db'
is a build failure rather than a review comment.

Image builds are a separate manual workflow — too heavy for every push."
git push -u origin rust-rewrite
```

- [ ] **Step 7: Confirm CI is green on GitHub**

Run: `gh run watch`
Expected: all four jobs pass. If `rust` fails on `sqlx::test`, confirm `DATABASE_URL` reaches the job and the service is healthy.

---

### Task 12: Exit-criteria verification

Every prior task verified its own deliverable. This task verifies the phase, in one pass, on a clean tree — the state a new machine would see.

**Files:**
- Create: `docs/superpowers/plans/2026-09-01-phase-0a-verification.md`

**Interfaces:**
- Consumes: everything.
- Produces: a recorded pass or a list of what failed.

- [ ] **Step 1: Verify from a clean checkout**

```bash
git clone --branch rust-rewrite . ../lapidary-verify && cd ../lapidary-verify
```

Working from a clone catches anything that only builds because of untracked local files.

- [ ] **Step 2: Walk the eight exit criteria**

Run each and record the actual output:

```bash
# 1. Layering check passes, and provably fails on an injected violation
cargo xtask check-layers
printf '\nlapidary-index.workspace = true\n' >> crates/lapidary-vcs/Cargo.toml
cargo xtask check-layers; echo "expected non-zero: $?"
git checkout crates/lapidary-vcs/Cargo.toml
cargo xtask check-layers

# 2. Clippy clean, tests green
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

# 3. Supply chain
cargo deny check
test -f Cargo.lock && test -f web/package-lock.json && echo "lockfiles committed"
grep -rn "uses:" .github/workflows | grep -v "@[0-9a-f]\{40\}" && echo "FAIL" || echo "actions pinned"

# 4. Stack serves a health-checked page
cp deploy/.env.example deploy/.env && sed -i 's/change-me-before-first-run/localdev/' deploy/.env
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
curl -fsS http://localhost:3000/api/healthz

# 5. pgvector and turkish
podman compose --env-file deploy/.env -f deploy/compose.yaml exec db psql -U lapidary -d lapidary \
  -c "SELECT extname FROM pg_extension WHERE extname IN ('vector','pg_trgm');" \
  -c "SELECT cfgname FROM pg_ts_config WHERE cfgname='turkish';"

# 6. Bindings track the Rust types, and the route tree tracks the routes
cargo xtask export-bindings && git diff --exit-code web/src/bindings && echo "bindings current"
(cd web && npm ci && npm run build) && git diff --exit-code web/src/routeTree.gen.ts && echo "route tree current"

# 7. Licence and docs — the LICENSE file and the prose around it must agree
head -3 LICENSE
grep -rn "MIGRATION" README.md docs/README.md && echo "FAIL: MIGRATION still referenced" || echo "OK: no MIGRATION references"
grep -rn -i "licence has not been decided\|not yet decided\|all rights are reserved\|Licensing — decision required\|Licensing conflict" \
  README.md CONTRIBUTING.md docs/ARCHITECTURE.md docs/ROADMAP.md \
  && echo "FAIL: docs still record the licence as undecided" || echo "OK: licence recorded as AGPL-3.0-only"

# 8. Docker as well as podman — and both ignore files present, or the context is the whole repo
cmp .containerignore .dockerignore && cmp deploy/.containerignore deploy/.dockerignore
podman compose --env-file deploy/.env -f deploy/compose.yaml down -v
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build && curl -fsS http://localhost:8080/api/healthz
docker compose --env-file deploy/.env -f deploy/compose.yaml down -v
```

- [ ] **Step 3: Record the results**

Write `docs/superpowers/plans/2026-09-01-phase-0a-verification.md` with one section per criterion: the command, the actual output, and PASS or FAIL. Paste real output — a criterion with no recorded output counts as FAIL.

- [ ] **Step 4: Handle any failures**

Any FAIL means Phase 0a is not complete. Fix it and re-run the whole list from Step 2 — not just the failing item, since fixes interact.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/2026-09-01-phase-0a-verification.md
git commit -m "docs: Phase 0a exit-criteria verification results"
```

- [ ] **Step 6: Clean up**

```bash
cd - && rm -rf ../lapidary-verify
```

---

## Follow-on, explicitly not in this plan

- **Phase 0b:** the `occt-bridge` C++ sidecar, OCCT built from source, `OcctKernel`, and the 200-part STEP exit test. That fixture does not exist and must be sourced or generated first.
- **Fixture licence audit.** `fixtures/` holds only `cube.stl`. Audit it, and find a licence-clean example part before Phase 1's first-run seeding.
- **Connecting the repo to the Lapidary project on claude.ai.** Do this after Task 1 lands, pointing at `rust-rewrite`, so the prototype and the withdrawn MIGRATION.md are never indexed.
