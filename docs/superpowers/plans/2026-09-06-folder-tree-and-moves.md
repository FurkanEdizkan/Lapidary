# Storage Layout, Folder Tree and Moves — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the blob store into a browsable storage folder holding one directory per
model — its source file, its `metadata.json` and its images — with PostgreSQL indexing the
paths, a category tree the user can reorganise, and an audit log of where each model has
been.

**Architecture:** Source files move from content-addressed (`blobs/ab/cd/<hash>`) to
path-addressed (`libraries/<lib>/<category…>/<model>/<file>`). Derivatives stay
content-addressed under `cache/`, which keeps `/api/blob/{hash}`'s immutable caching and
makes "eviction is not data loss" self-evident. `part.source_path` remains the immutable
ingest identity key; a new `part.folder_id` carries mutable location and a new
`file.storage_path` carries the bytes' real path. Existing stores convert through a
resumable `migrate_storage` job, not through the migration.

**Tech Stack:** Rust (axum, sqlx, tokio, thiserror), PostgreSQL 18.6, ts-rs, React + Vite +
TanStack Router/Query, Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md`

## Global Constraints

- **No SQL outside `lapidary-db`.** Everything through repository types.
- **`lapidary-api` may never name `SourceStore`**, and may name `SourceReader` only in
  `crates/lapidary-api/src/download.rs`. `SourceRelocator` gets one allowed module too
  (Task 2). `cargo xtask check-deploy` enforces all three.
- **L2 crates never depend on each other or on L3.** Shared types go to `lapidary-core`.
- **Generated columns are explicitly `STORED`.** PG 18 defaults to virtual.
- **`thiserror` in libraries, `anyhow` at binary edges. No `unwrap()` outside tests.**
- **Errors say what broke and what to do.** Not "parse failed (3)".
- **No bare user-facing strings in components.** Everything through `web/src/lib/strings.ts`;
  `web/src/no-bare-strings.test.ts` gates it.
- **Frontend is dark only.** Motion 120/180/280 ms on `cubic-bezier(0.2, 0, 0, 1)`, transform
  and opacity only, `prefers-reduced-motion` respected.
- **Real content in fixtures.** Plausible part numbers and dimensions — `bracket-lp-1042-03`,
  `Terrain/Rocks/Cliffs` — never "Part 1 / Folder 1".
- **Commit messages:** Conventional Commits; the repo's `commit-msg` hook rejects AI
  attribution trailers. Do not add them.
- **Every task ends green:** `cargo xtask check` (fmt, layers, deploy, strings) plus the
  tests named in that task.

## File Structure

**Created**

| Path | Responsibility |
|---|---|
| `crates/lapidary-core/src/slug.rs` | Filesystem-safe names: `slugify`, `disambiguate`, `reject_escaping_path` |
| `crates/lapidary-core/src/manifest.rs` | `ModelManifest` — the `metadata.json` shape and its schema version |
| `crates/lapidary-db/migrations/0008_folders.sql` | `folder`, `part.folder_id`, `file.storage_path`, `part_move`, tree backfill |
| `crates/lapidary-db/src/folders.rs` | `PgFolders` — get-or-create, tree, cycle check, subtree delete, moves |
| `crates/lapidary-api/src/folders.rs` | Folder CRUD routes |
| `crates/lapidary-api/src/moves.rs` | Part move route and move history |
| `web/src/components/FolderTree.tsx` | The category sidebar |

**Modified**

| Path | Change |
|---|---|
| `crates/lapidary-core/src/ids.rs` | `FolderId` |
| `crates/lapidary-core/src/lib.rs` | `mod slug; mod manifest;` and re-exports |
| `crates/lapidary-core/src/job.rs` | `JobPayload::MigrateStorage` |
| `crates/lapidary-storage/src/lib.rs` | `SourceRelocator` |
| `crates/lapidary-ingest/src/handler.rs` | Write the model directory; use core's path guard |
| `crates/lapidary-db/src/repo.rs` | `storage_path` on `IngestRequest` and `DownloadSource` |
| `crates/lapidary-api/src/download.rs` | Read from `storage_path`, falling back to the CAS path |
| `crates/lapidary-api/src/lib.rs` | Mount the new routers |
| `xtask/src/deploy.rs` | `SourceRelocator` boundary rule |
| `web/src/lib/strings.ts`, `web/src/routes/index.tsx` | Tree, dialogs, `folderId` search param |

---

# Phase A — Primitives

Nothing user-visible. Three self-contained units the rest of the plan depends on.

### Task 1: Filesystem-safe names

**Files:**
- Create: `crates/lapidary-core/src/slug.rs`
- Modify: `crates/lapidary-core/src/lib.rs`
- Modify: `crates/lapidary-ingest/src/handler.rs:370-395` (drop the local guard, use core's)

**Interfaces:**
- Produces: `lapidary_core::slug::slugify(name: &str) -> String`,
  `lapidary_core::slug::disambiguate(slug: &str, hash: &BlobHash) -> String`,
  `lapidary_core::slug::reject_escaping_path(path: &str) -> Result<(), CoreError>`

- [ ] **Step 1: Write the failing tests**

```rust
// crates/lapidary-core/src/slug.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_pass_through() {
        assert_eq!(slugify("bracket-lp-1042-03"), "bracket-lp-1042-03");
        assert_eq!(slugify("Terrain"), "Terrain");
    }

    #[test]
    fn turkish_characters_survive() {
        // DATA.md §5.1 already carries these through Content-Disposition. Stripping them
        // would make the directory unreadable to whoever named the part.
        assert_eq!(slugify("köşebent-ğ-ışık"), "köşebent-ğ-ışık");
    }

    #[test]
    fn windows_hostile_characters_become_dashes() {
        assert_eq!(slugify("Rocks?"), "Rocks-");
        assert_eq!(slugify("Rocks*"), "Rocks-");
        assert_eq!(slugify("A1234:56"), "A1234-56");
        assert_eq!(slugify("a/b\\c"), "a-b-c");
    }

    #[test]
    fn trailing_dots_and_spaces_are_trimmed() {
        // Windows silently drops them, so "bracket." and "bracket" would collide after a
        // round trip through a Windows client.
        assert_eq!(slugify("bracket."), "bracket");
        assert_eq!(slugify("bracket   "), "bracket");
        assert_eq!(slugify("  bracket  ."), "bracket");
    }

    #[test]
    fn reserved_device_names_get_a_suffix() {
        // A model legitimately called AUX is not hypothetical in a parts library.
        assert_eq!(slugify("AUX"), "AUX_");
        assert_eq!(slugify("con"), "con_");
        assert_eq!(slugify("COM4"), "COM4_");
        assert_eq!(slugify("LPT9"), "LPT9_");
        // Not reserved: only the exact names are.
        assert_eq!(slugify("COMET"), "COMET");
        assert_eq!(slugify("COM10"), "COM10");
    }

    #[test]
    fn an_empty_result_still_names_something() {
        assert_eq!(slugify(""), "unnamed");
        assert_eq!(slugify("???"), "---");
        assert_eq!(slugify("   ..."), "unnamed");
    }

    #[test]
    fn long_names_are_capped_on_a_char_boundary() {
        // Model-pack filenames run long. 120 chars leaves room inside a 255-byte
        // component for the disambiguation suffix.
        let long = "ş".repeat(200);
        let out = slugify(&long);
        assert_eq!(out.chars().count(), 120);
        assert!(out.is_char_boundary(out.len()), "must not split a UTF-8 sequence");
    }

    #[test]
    fn disambiguate_appends_six_hex_of_the_hash() {
        let hash = crate::BlobHash::from_bytes([0xa1, 0xb2, 0xc3, 0x44, 0x55, 0x66, 0x77, 0x88,
                                                0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
                                                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                                                0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00]);
        assert_eq!(disambiguate("cliff", &hash), "cliff_a1b2c3");
    }

    #[test]
    fn a_relative_path_that_escapes_is_refused() {
        assert!(reject_escaping_path("Terrain/rock.stl").is_ok());
        assert!(reject_escaping_path("../../etc/passwd").is_err());
        assert!(reject_escaping_path("/etc/passwd").is_err());
        assert!(reject_escaping_path("Terrain/../../etc/passwd").is_err());
        assert!(reject_escaping_path("").is_err());
    }
}
```

- [ ] **Step 2: Run and verify they fail**

Run: `cargo test -p lapidary-core slug`
Expected: FAIL — `could not find slug in the crate root`.

- [ ] **Step 3: Implement**

```rust
//! Turning a human name into something every filesystem this ships on can hold.
//!
//! The store is browsable by design, so these names are read by people in a file manager
//! — but they also have to survive Windows, which the agent binary and the Tauri shell
//! both target. Nothing parses them: `metadata.json` inside each directory carries
//! identity, so these optimise for legibility rather than round-tripping.

use crate::{BlobHash, CoreError};

/// Windows reserves these exactly, case-insensitively, with or without an extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters no path component may carry: separators, the set Windows reserves, and
/// control characters.
const HOSTILE: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];

/// 120 chars, not 255 bytes: leaves room for `_a1b2c3` inside the smallest component limit
/// we ship against, and counts characters so a multi-byte name is not silently halved.
const MAX_CHARS: usize = 120;

pub fn slugify(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if HOSTILE.contains(&c) || c.is_control() { '-' } else { c })
        .collect();

    let trimmed = replaced.trim().trim_end_matches(['.', ' ']).trim();

    let capped: String = trimmed.chars().take(MAX_CHARS).collect();

    if capped.is_empty() {
        return "unnamed".to_owned();
    }

    // Windows reserves the *stem*, so `AUX.stl` is refused too. Compare before any
    // extension.
    let stem = capped.split('.').next().unwrap_or(&capped);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return format!("{capped}_");
    }

    capped
}

/// The suffix that resolves a directory collision. Deterministic, so the same model
/// re-ingested lands on the same name.
pub fn disambiguate(slug: &str, hash: &BlobHash) -> String {
    format!("{slug}_{}", &hash.to_hex()[..6])
}

/// Refuse a relative path that would leave the directory it is joined to.
///
/// `Path::join` resolves nothing and refuses nothing: `root.join("/etc/passwd")` *is*
/// `/etc/passwd`. `DATA.md` §5.4 states this rule for archive entries; it belongs on every
/// path that reaches a filesystem from data.
pub fn reject_escaping_path(path: &str) -> Result<(), CoreError> {
    use std::path::{Component, Path};
    let p = Path::new(path);
    let escapes = path.is_empty()
        || p.components().any(|c| {
            matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        });
    if escapes {
        return Err(CoreError::PathEscapes { got: path.to_owned() });
    }
    Ok(())
}
```

Add to `crates/lapidary-core/src/lib.rs`:

```rust
pub mod manifest;
pub mod slug;
```

Add to `crates/lapidary-core/src/error.rs`:

```rust
    #[error(
        "Refused the path {got:?}: it points outside the directory it belongs to. Paths \
         stored by Lapidary are relative and may not contain `..` or start at the root."
    )]
    PathEscapes { got: String },
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-core slug`
Expected: PASS (9 tests).

- [ ] **Step 5: Switch ingest to the shared guard**

Delete `reject_escaping_path` from `crates/lapidary-ingest/src/handler.rs` and its local
tests, then at the call site (`handler.rs:136`):

```rust
        lapidary_core::slug::reject_escaping_path(source_path).map_err(|e| {
            HandlerError::Permanent { message: e.to_string() }
        })?;
```

Run: `cargo test -p lapidary-ingest`
Expected: PASS — one implementation, two callers.

- [ ] **Step 6: Commit**

```bash
git add crates/lapidary-core crates/lapidary-ingest
git commit -m "feat(core): make a human name safe for every filesystem we ship on"
```

---

### Task 2: `SourceRelocator`, and the boundary rule that keeps it narrow

**Files:**
- Modify: `crates/lapidary-storage/src/lib.rs`
- Modify: `xtask/src/deploy.rs:68` (add `RELOCATE_MODULE`), `:115` (Violation), `:685`
  (`check_open_path_boundary`)

**Interfaces:**
- Consumes: `lapidary_core::slug::reject_escaping_path` (Task 1)
- Produces: `SourceRelocator::open(root: &Path) -> Self`,
  `SourceRelocator::rename(&self, from: &str, to: &str) -> Result<(), StorageError>`,
  `SourceRelocator::create_dir(&self, rel: &str) -> Result<(), StorageError>`

- [ ] **Step 1: Write the failing tests**

```rust
// in crates/lapidary-storage/src/lib.rs tests module
    #[test]
    fn a_relocator_moves_a_directory_and_its_contents() {
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        r.create_dir("libraries/default/Terrain/cliff").expect("mkdir");
        std::fs::write(dir.path().join("libraries/default/Terrain/cliff/cliff.stl"), b"solid\n")
            .expect("write");
        r.create_dir("libraries/default/Bases").expect("mkdir");

        r.rename("libraries/default/Terrain/cliff", "libraries/default/Bases/cliff")
            .expect("rename");

        assert!(dir.path().join("libraries/default/Bases/cliff/cliff.stl").exists());
        assert!(!dir.path().join("libraries/default/Terrain/cliff").exists());
    }

    #[test]
    fn a_relocator_refuses_a_path_that_escapes_the_root() {
        // The whole point of the narrow handle: it cannot be talked into touching
        // anything outside the store.
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        assert!(r.create_dir("../escaped").is_err());
        assert!(r.rename("../a", "b").is_err());
        assert!(r.rename("a", "/etc/lapidary").is_err());
    }

    #[test]
    fn renaming_a_missing_directory_says_which_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let r = SourceRelocator::open(dir.path());
        let err = r.rename("libraries/default/Terrain/gone", "libraries/default/Bases/gone")
            .expect_err("must fail");
        assert!(err.to_string().contains("Terrain/gone"), "names the path: {err}");
    }
```

- [ ] **Step 2: Run and verify they fail**

Run: `cargo test -p lapidary-storage relocator`
Expected: FAIL — `cannot find type SourceRelocator`.

- [ ] **Step 3: Implement**

```rust
/// Move a model or category directory, and nothing else.
///
/// The third handle onto source paths, and deliberately the narrowest: no read, no write
/// of contents, no delete. It exists because a move is a rename plus a row update, and
/// neither `SourceStore` (which demands a `WorkerRole`) nor `SourceReader` (read-only) can
/// rename.
///
/// Giving `lapidary-api` this rather than routing an O(1) syscall through the job queue is
/// a decision the spec argues (§10): the open-path boundary exists so the interactive path
/// never *parses* a source file to draw something, and a rename parses nothing, reads no
/// bytes and invokes no kernel. `xtask/src/deploy.rs` keeps it to one module, the same way
/// it keeps `SourceReader` to `download.rs`.
///
/// If this ever grows a method that reads or writes contents, it has become `SourceStore`
/// and the route belongs in `lapidary-ingest`.
pub struct SourceRelocator {
    root: PathBuf,
}

impl SourceRelocator {
    pub fn open(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    /// Resolve a store-relative path, refusing anything that would leave the root.
    fn resolve(&self, rel: &str) -> Result<PathBuf, StorageError> {
        lapidary_core::slug::reject_escaping_path(rel)
            .map_err(|e| StorageError::PathRefused { detail: e.to_string() })?;
        Ok(self.root.join(rel))
    }

    pub fn create_dir(&self, rel: &str) -> Result<(), StorageError> {
        let path = self.resolve(rel)?;
        std::fs::create_dir_all(&path).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    /// A same-filesystem rename: atomic, and O(1) however large the subtree. The parent of
    /// `to` must exist — the caller creates the category before moving into it.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let from_path = self.resolve(from)?;
        let to_path = self.resolve(to)?;
        std::fs::rename(&from_path, &to_path).map_err(|source| StorageError::Io {
            path: from_path.display().to_string(),
            source,
        })
    }
}
```

Add to `StorageError`:

```rust
    #[error("{detail}")]
    PathRefused { detail: String },
```

Add `lapidary-core` to `crates/lapidary-storage/Cargo.toml` dependencies if it is not
already there (it is — `BlobHash` comes from it).

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-storage`
Expected: PASS.

- [ ] **Step 5: Teach `check-deploy` the third handle**

In `xtask/src/deploy.rs`, beside `DOWNLOAD_MODULE`:

```rust
/// The single file `SourceRelocator` is allowed in — the part-move route.
const RELOCATE_MODULE: &str = "crates/lapidary-api/src/moves.rs";
```

Add the variant beside `OpenPathNamesSourceReaderOutsideDownload`:

```rust
    OpenPathNamesSourceRelocatorOutsideMoves { path: String },
```

Its `Display` arm:

```rust
            Violation::OpenPathNamesSourceRelocatorOutsideMoves { path } => write!(
                f,
                "{path} names SourceRelocator. It is allowed only in {RELOCATE_MODULE}, the \
                 part-move route, which renames a model's directory and reads nothing. A \
                 second module reaching for it is how a narrow capability becomes a wide \
                 one by copy-paste — move the rename behind the move route, or if the new \
                 caller needs a file's contents it needs SourceStore and belongs in \
                 lapidary-ingest."
            ),
```

Extend the check:

```rust
    let relocator_outside_moves = api_sources
        .iter()
        .filter(|(path, body)| {
            body.contains("SourceRelocator") && !std::path::Path::new(path).ends_with(RELOCATE_MODULE)
        })
        .map(|(path, _)| Violation::OpenPathNamesSourceRelocatorOutsideMoves { path: path.clone() });
    names_source_store
        .chain(reader_outside_download)
        .chain(relocator_outside_moves)
        .collect()
```

And a test mirroring `source_reader_is_allowed_in_one_file_and_nowhere_else`:

```rust
    #[test]
    fn source_relocator_is_allowed_in_the_move_route_and_nowhere_else() {
        let sources = vec![
            (
                "crates/lapidary-api/src/moves.rs".to_owned(),
                "let relocator = SourceRelocator::open(&root);".to_owned(),
            ),
            (
                "crates/lapidary-api/src/parts.rs".to_owned(),
                "let relocator = SourceRelocator::open(&root);".to_owned(),
            ),
        ];
        let violations = check_open_path_boundary(&sources);
        assert_eq!(
            violations,
            vec![Violation::OpenPathNamesSourceRelocatorOutsideMoves {
                path: "crates/lapidary-api/src/parts.rs".to_owned()
            }]
        );
    }
```

- [ ] **Step 6: Run the gate and commit**

```bash
cargo test -p xtask && cargo xtask check
git add crates/lapidary-storage xtask
git commit -m "feat(storage): add a handle that can move a model and nothing else"
```

---

### Task 3: `ModelManifest` — what makes the store self-describing

**Files:**
- Create: `crates/lapidary-core/src/manifest.rs`

**Interfaces:**
- Produces: `ModelManifest { schema, part, revisions }`, `ManifestPart`, `ManifestRevision`,
  `ManifestFile`, `ModelManifest::SCHEMA: u32`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn a_manifest() -> ModelManifest {
        ModelManifest {
            schema: ModelManifest::SCHEMA,
            part: ManifestPart {
                id: PartId::from_uuid("01931b6e-0000-7000-8000-0000000000aa".parse().unwrap()),
                library: LibraryId::from_uuid("01931b6e-0000-7000-8000-000000000001".parse().unwrap()),
                name: "bracket-lp-1042-03".to_owned(),
                part_number: Some("LP-1042-03".to_owned()),
                classification: None,
                source_path: "Terrain/Rocks/bracket-lp-1042-03.stl".to_owned(),
                metadata: serde_json::json!({}),
            },
            revisions: vec![ManifestRevision {
                id: RevisionId::from_uuid("01931b6e-0000-7000-8000-0000000000bb".parse().unwrap()),
                rev_label: "1".to_owned(),
                origin: "ingest".to_owned(),
                volume_mm3: Some(21_478.5),
                volume_source: Some("tessellated".to_owned()),
                bbox_mm: Some([61.0, 42.0, 18.5]),
                triangle_count: Some(48_112),
                is_watertight: Some(true),
                units: Some("mm".to_owned()),
                files: vec![ManifestFile {
                    role: "source".to_owned(),
                    format: "stl".to_owned(),
                    blake3: "ab".repeat(32),
                    size_bytes: 204_800,
                    file_name: "bracket-lp-1042-03.stl".to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let json = serde_json::to_string_pretty(&a_manifest()).expect("serializes");
        let back: ModelManifest = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.part.name, "bracket-lp-1042-03");
        assert_eq!(back.revisions[0].files[0].format, "stl");
        assert_eq!(back.schema, ModelManifest::SCHEMA);
    }

    #[test]
    fn a_manifest_from_a_newer_schema_is_refused_by_version_not_by_shape() {
        // A store written by a newer build must fail with something a person can act on,
        // not with a serde field error listing what changed.
        let mut v = serde_json::to_value(a_manifest()).expect("to value");
        v["schema"] = serde_json::json!(999);
        let parsed: ModelManifest = serde_json::from_value(v).expect("still parses");
        assert!(parsed.is_future_schema());
    }

    #[test]
    fn unknown_fields_are_kept_not_rejected() {
        // Forward compatibility: a field a newer build added must not make this directory
        // an orphan on an older one.
        let mut v = serde_json::to_value(a_manifest()).expect("to value");
        v["part"]["invented_later"] = serde_json::json!("hello");
        assert!(serde_json::from_value::<ModelManifest>(v).is_ok());
    }
}
```

- [ ] **Step 2: Run and verify it fails**

Run: `cargo test -p lapidary-core manifest`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```rust
//! `metadata.json` — what sits beside a model's file and makes the store self-describing.
//!
//! This is the whole of re-adoption: delete the database and each model directory still
//! says what it is. It is machine-owned but lives in a folder the user has been promised
//! they may edit, so readers treat a missing or malformed one as an orphan to report, never
//! as a reason to fail a walk.

use crate::{LibraryId, PartId, RevisionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Bumped when a field's *meaning* changes. Additive fields do not bump it — readers
    /// keep unknown ones rather than refusing them.
    pub schema: u32,
    pub part: ManifestPart,
    pub revisions: Vec<ManifestRevision>,
}

impl ModelManifest {
    pub const SCHEMA: u32 = 1;

    /// Written by a build newer than this one. The caller reports the directory and moves
    /// on rather than guessing at fields it does not know.
    pub fn is_future_schema(&self) -> bool {
        self.schema > Self::SCHEMA
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPart {
    pub id: PartId,
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    pub classification: Option<String>,
    /// The ingest identity key. Never the storage path — see the spec §3.
    pub source_path: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRevision {
    pub id: RevisionId,
    pub rev_label: String,
    pub origin: String,
    pub volume_mm3: Option<f64>,
    /// `tessellated` or `analytic`. Provenance travels with the value, per `0002`'s
    /// per-column `_source` design.
    pub volume_source: Option<String>,
    pub bbox_mm: Option<[f64; 3]>,
    pub triangle_count: Option<i32>,
    pub is_watertight: Option<bool>,
    pub units: Option<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub role: String,
    pub format: String,
    /// Hex. Re-adoption verifies bytes against this before trusting the directory.
    pub blake3: String,
    pub size_bytes: i64,
    /// The file's name inside this directory. Not a path — a model's files are flat.
    pub file_name: String,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-core manifest`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-core
git commit -m "feat(core): give a model directory a manifest that describes itself"
```

---

# Phase B — Schema and the folder repository

At the end of this phase the tree exists in the database and every existing library has one.

### Task 4: Migration `0008`

**Files:**
- Create: `crates/lapidary-db/migrations/0008_folders.sql`
- Create tests in: `crates/lapidary-db/tests/migrations.rs` (append)

**Interfaces:**
- Produces: tables `folder`, `part_move`; columns `part.folder_id`, `file.storage_path`

- [ ] **Step 1: Write the failing tests**

```rust
// append to crates/lapidary-db/tests/migrations.rs
#[sqlx::test(migrations = "./migrations")]
async fn two_root_folders_with_one_name_are_refused(pool: PgPool) {
    // NULLs are distinct in a unique constraint by default, so a plain
    // unique(library_id, parent_id, name) would silently allow this — and a corpus scan
    // produces it on the first two top-level directories.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let insert = |id: Uuid, name: &'static str, slug: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO folder (id, library_id, parent_id, name, slug) \
                 VALUES ($1, $2, NULL, $3, $4)",
            )
            .bind(id).bind(library).bind(name).bind(slug)
            .execute(&pool).await
        }
    };
    insert(Uuid::now_v7(), "Terrain", "Terrain").await.expect("the first inserts");
    let err = insert(Uuid::now_v7(), "Terrain", "Terrain").await.expect_err("the second must not");
    assert_eq!(
        err.as_database_error().and_then(|e| e.constraint()),
        Some("folder_name_unique_per_parent")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn two_distinct_names_that_slug_alike_are_refused(pool: PgPool) {
    // "Rocks?" and "Rocks*" are different names and the same directory.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let insert = |id: Uuid, name: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO folder (id, library_id, parent_id, name, slug) \
                 VALUES ($1, $2, NULL, $3, 'Rocks-')",
            )
            .bind(id).bind(library).bind(name)
            .execute(&pool).await
        }
    };
    insert(Uuid::now_v7(), "Rocks?").await.expect("the first inserts");
    let err = insert(Uuid::now_v7(), "Rocks*").await.expect_err("the second must not");
    assert_eq!(
        err.as_database_error().and_then(|e| e.constraint()),
        Some("folder_slug_unique_per_parent")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_backfill_rebuilds_the_tree_from_nested_source_paths(pool: PgPool) {
    // Slice 6a made the scan recursive, so parts ingested since carry nested source_paths.
    // Without this backfill they are stranded flat forever: folders are only created for
    // files that actually ingest, and a re-scan settles every one of them as Skipped.
    //
    // This test seeds the table the way 6a leaves it, runs 0008's backfill by hand against
    // the already-migrated pool, and asserts the tree. Because sqlx has already run the
    // migration on an empty database, the rows are inserted first and the backfill's
    // statement is re-executed here.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    for (name, path) in [
        ("bracket-lp-1042-03", "bracket-lp-1042-03.stl"),
        ("rock", "Terrain/rock.stl"),
        ("cliff", "Terrain/Rocks/cliff.stl"),
        ("spire", "Terrain/Rocks/Cliffs/spire.stl"),
        ("round-32mm", "Bases/round-32mm.stl"),
        ("base-rock", "Bases/Rocks/base-rock.stl"),
    ] {
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1,$2,$3,$4)")
            .bind(Uuid::now_v7()).bind(library).bind(name).bind(path)
            .execute(&pool).await.expect("seeds a part");
    }

    sqlx::query(include_str!("../migrations/0008_backfill.sql"))
        .execute(&pool).await.expect("the backfill runs");

    let paths: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE t AS (
           SELECT id, name::text AS path FROM folder WHERE parent_id IS NULL
           UNION ALL SELECT f.id, t.path||'/'||f.name FROM folder f JOIN t ON f.parent_id = t.id)
         SELECT path FROM t ORDER BY path",
    ).fetch_all(&pool).await.expect("reads the tree");

    assert_eq!(
        paths,
        vec!["Bases", "Bases/Rocks", "Terrain", "Terrain/Rocks", "Terrain/Rocks/Cliffs"],
        "Terrain/Rocks and Bases/Rocks are two folders, not one"
    );

    let root_parts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM part WHERE folder_id IS NULL AND source_path NOT LIKE '%/%'",
    ).fetch_one(&pool).await.expect("counts");
    assert_eq!(root_parts, 1, "the flat part stays at the library root");
}
```

- [ ] **Step 2: Run and verify they fail**

Run: `cargo test -p lapidary-db --test migrations`
Expected: FAIL — `relation "folder" does not exist`.

- [ ] **Step 3: Write the migration**

Create `crates/lapidary-db/migrations/0008_folders.sql`:

```sql
-- Location becomes a thing the user can change, and the store becomes a folder they can
-- open. See docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md.
--
-- Three columns, three jobs, and the names must stay distinct:
--   part.source_path   -- immutable. Where the file sat in the INGEST directory (0007).
--   part.folder_id     -- mutable. Which category the user has put it in.
--   file.storage_path  -- mutable. Where the bytes actually sit in the STORE.

create table folder (
  id          uuid primary key,
  library_id  uuid not null references library(id),
  parent_id   uuid references folder(id),          -- null = library root
  name        text not null,                       -- what the user typed
  slug        text not null,                       -- what the filesystem got
  created_at  timestamptz not null default now(),
  deleted_at  timestamptz,

  -- `nulls not distinct` is load-bearing, not decoration. PostgreSQL treats NULLs as
  -- distinct in a unique constraint by default, so the plain form silently permits two
  -- root categories both named Terrain -- the first thing a corpus scan produces.
  constraint folder_name_unique_per_parent
    unique nulls not distinct (library_id, parent_id, name),

  -- Both are needed. "Rocks?" and "Rocks*" are distinct names that slug to the same
  -- directory, so without this the tree is legal and the disk is not.
  constraint folder_slug_unique_per_parent
    unique nulls not distinct (library_id, parent_id, slug)
);

create index folder_library_parent on folder (library_id, parent_id);

alter table part add column folder_id uuid references folder(id);   -- null = library root
create index part_folder_id on part (folder_id);

-- Where the bytes actually are, relative to the storage root. Stays nullable: null means
-- "still at the old content-addressed path", which is a live state for as long as the
-- migrate_storage job takes to drain -- hours on a real corpus. A later migration makes it
-- NOT NULL, once it has drained everywhere.
alter table file add column storage_path text;

create table part_move (
  id           uuid primary key,
  part_id      uuid not null references part(id),
  from_folder  uuid references folder(id),
  to_folder    uuid references folder(id),
  moved_at     timestamptz not null default now(),
  moved_by     uuid                                -- null until Phase 8 has a principal
);

create index part_move_part_id on part_move (part_id, moved_at desc);
```

Then the backfill, in its own file so the test above can `include_str!` it — and appended
to `0008_folders.sql` by the same content so there is one definition:

Create `crates/lapidary-db/migrations/0008_backfill.sql`:

```sql
-- Rebuild the category tree from the nested source_paths slice 6a's recursive scan wrote.
--
-- Level by level rather than one recursive CTE: a CTE cannot insert rows and then use the
-- ids it just generated as the next level's parents.
do $$
declare lvl int := 1; maxlvl int;
begin
  create temporary table _dirs on commit drop as
    select p.id as part_id, p.library_id,
           string_to_array(regexp_replace(p.source_path, '/[^/]*$', ''), '/') as segs
      from part p where position('/' in p.source_path) > 0;

  create temporary table _map (library_id uuid, path text, folder_id uuid,
                               primary key (library_id, path)) on commit drop;

  select max(array_length(segs, 1)) into maxlvl from _dirs;

  -- 16 matches scan.rs's MAX_DEPTH. A tree deeper than that was already truncated on the
  -- way in, so following it here would build folders holding nothing.
  while lvl <= coalesce(maxlvl, 0) and lvl <= 16 loop
    with want as (
      select distinct d.library_id,
             array_to_string(d.segs[1:lvl], '/') as path,
             d.segs[lvl] as name,
             case when lvl = 1 then null
                  else array_to_string(d.segs[1:lvl-1], '/') end as parent_path
        from _dirs d where array_length(d.segs, 1) >= lvl
    ), ins as (
      insert into folder (id, library_id, parent_id, name, slug)
      select gen_random_uuid(), w.library_id, m.folder_id, w.name, w.name
        from want w
        left join _map m on m.library_id = w.library_id and m.path = w.parent_path
      returning id, library_id, parent_id, name
    )
    insert into _map (library_id, path, folder_id)
    select i.library_id,
           case when lvl = 1 then i.name
                else (select m2.path from _map m2 where m2.folder_id = i.parent_id)
                     || '/' || i.name end,
           i.id
      from ins i;
    lvl := lvl + 1;
  end loop;

  update part p set folder_id = m.folder_id
    from _dirs d
    join _map m on m.library_id = d.library_id
               and m.path = array_to_string(d.segs, '/')
   where p.id = d.part_id;
end $$;
```

Append its contents to the end of `0008_folders.sql` so `sqlx` runs it as one migration.
(`0008_backfill.sql` is not itself a migration — prefix it so `sqlx` ignores it: name it
`crates/lapidary-db/backfill/0008_backfill.sql` and adjust the `include_str!` path in the
test to `../backfill/0008_backfill.sql`.)

**Note the slug on backfilled rows:** the backfill writes `slug = name`, unslugged, because
the names came from real directories that already exist on disk and are therefore already
valid. Running `slugify` over them would rename a directory the store is about to be told
to find.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-db --test migrations`
Expected: PASS (3 new tests plus the existing ones).

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-db
git commit -m "feat(db): add the category tree, and back-fill it from nested source paths"
```

---

### Task 5: `PgFolders`

**Files:**
- Create: `crates/lapidary-db/src/folders.rs`
- Modify: `crates/lapidary-db/src/lib.rs` (`mod folders; pub use folders::*;`)
- Modify: `crates/lapidary-core/src/ids.rs` (`FolderId`)
- Test: `crates/lapidary-db/tests/folders.rs`

**Interfaces:**
- Produces:
  - `FolderId` (uuid newtype, same macro as `PartId`)
  - `FolderRow { id, parent_id, name, slug }`
  - `PgFolders::get_or_create(library, parent: Option<FolderId>, name, slug) -> Result<FolderId, DbError>`
  - `PgFolders::tree(library) -> Result<Vec<FolderRow>, DbError>`
  - `PgFolders::would_cycle(folder, new_parent: FolderId) -> Result<bool, DbError>`
  - `PgFolders::slug_path(folder) -> Result<String, DbError>`
  - `PgFolders::reparent(folder, parent: Option<FolderId>) -> Result<bool, DbError>`
  - `PgFolders::rename(folder, name, slug) -> Result<bool, DbError>`
  - `PgFolders::soft_delete_subtree(folder) -> Result<(u64, u64), DbError>` — (folders, parts)
  - `PgFolders::library_of(folder) -> Result<Option<LibraryId>, DbError>`

- [ ] **Step 1: Add `FolderId`**

In `crates/lapidary-core/src/ids.rs`, beside the others:

```rust
uuid_newtype!(
    FolderId,
    "Identifies a category folder. Location, never identity — a part's identity is its \
     `source_path`."
);
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/lapidary-db/tests/folders.rs
use lapidary_core::{FolderId, LibraryId};
use lapidary_db::PgFolders;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

#[sqlx::test(migrations = "./migrations")]
async fn get_or_create_is_idempotent(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let a = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("creates");
    let b = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("finds");
    assert_eq!(a, b, "two workers racing one directory get one row");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_same_name_under_two_parents_is_two_folders(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("Terrain");
    let bases = f.get_or_create(library(), None, "Bases", "Bases").await.expect("Bases");
    let a = f.get_or_create(library(), Some(terrain), "Rocks", "Rocks").await.expect("a");
    let b = f.get_or_create(library(), Some(bases), "Rocks", "Rocks").await.expect("b");
    assert_ne!(a, b);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_folder_cannot_move_into_its_own_descendant(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("Terrain");
    let rocks = f.get_or_create(library(), Some(terrain), "Rocks", "Rocks").await.expect("Rocks");
    let cliffs = f.get_or_create(library(), Some(rocks), "Cliffs", "Cliffs").await.expect("Cliffs");

    assert!(f.would_cycle(terrain, cliffs).await.expect("checks"), "into a descendant");
    assert!(!f.would_cycle(cliffs, terrain).await.expect("checks"), "the legal direction");
}

#[sqlx::test(migrations = "./migrations")]
async fn slug_path_joins_the_ancestors(pool: sqlx::PgPool) {
    let f = PgFolders(pool.clone());
    let terrain = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("Terrain");
    let rocks = f.get_or_create(library(), Some(terrain), "Rocks", "Rocks").await.expect("Rocks");
    assert_eq!(f.slug_path(rocks).await.expect("path"), "Terrain/Rocks");
}

#[sqlx::test(migrations = "./migrations")]
async fn deleting_a_folder_cascades_through_subfolders(pool: sqlx::PgPool) {
    // The one-level bug passes every other test in this file.
    let f = PgFolders(pool.clone());
    let terrain = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("Terrain");
    let rocks = f.get_or_create(library(), Some(terrain), "Rocks", "Rocks").await.expect("Rocks");
    let _cliffs = f.get_or_create(library(), Some(rocks), "Cliffs", "Cliffs").await.expect("Cliffs");

    let (folders, _parts) = f.soft_delete_subtree(terrain).await.expect("deletes");
    assert_eq!(folders, 3, "Terrain, Rocks and Cliffs — not just Terrain");
    assert!(f.tree(library()).await.expect("tree").is_empty(), "all hidden");
}
```

- [ ] **Step 3: Run and verify they fail**

Run: `cargo test -p lapidary-db --test folders`
Expected: FAIL — `PgFolders` not found.

- [ ] **Step 4: Implement**

```rust
//! The category tree. Location, never identity.

use crate::{DbError, PgPool};
use lapidary_core::{FolderId, LibraryId};

/// One node. The tree is returned flat and assembled by the caller — hundreds of rows at
/// corpus scale, and a nested JSON build in SQL is a second shape to keep in step with the
/// TypeScript one.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct FolderRow {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub slug: String,
}

/// Matches `scan.rs`'s `MAX_DEPTH`. Real trees do not cycle, but a bound keeps a corrupt
/// `parent_id` from looping the walk forever.
const MAX_DEPTH: i32 = 16;

pub struct PgFolders(pub PgPool);

impl PgFolders {
    /// Insert-or-find, in that order. Two workers scanning concurrently genuinely race the
    /// same directory, so the constraint is what makes it safe rather than a prior SELECT
    /// that another worker can invalidate between statements.
    pub async fn get_or_create(
        &self,
        library: LibraryId,
        parent: Option<FolderId>,
        name: &str,
        slug: &str,
    ) -> Result<FolderId, DbError> {
        let id = FolderId::new();
        let inserted: Option<uuid::Uuid> = sqlx::query_scalar(
            "INSERT INTO folder (id, library_id, parent_id, name, slug) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(id.as_uuid())
        .bind(library.as_uuid())
        .bind(parent.map(|p| p.as_uuid()))
        .bind(name)
        .bind(slug)
        .fetch_optional(&self.0)
        .await?;

        if let Some(uuid) = inserted {
            return Ok(FolderId::from_uuid(uuid));
        }

        // `IS NOT DISTINCT FROM`, not `=`: parent_id is NULL at the library root, and `=`
        // is never true against NULL, so the plain form would find nothing and the caller
        // would loop forever trying to create a row that already exists.
        let found: uuid::Uuid = sqlx::query_scalar(
            "SELECT id FROM folder WHERE library_id = $1 \
             AND parent_id IS NOT DISTINCT FROM $2 AND name = $3",
        )
        .bind(library.as_uuid())
        .bind(parent.map(|p| p.as_uuid()))
        .bind(name)
        .fetch_one(&self.0)
        .await?;
        Ok(FolderId::from_uuid(found))
    }

    pub async fn tree(&self, library: LibraryId) -> Result<Vec<FolderRow>, DbError> {
        Ok(sqlx::query_as::<_, FolderRow>(
            "SELECT id, parent_id, name, slug FROM folder \
             WHERE library_id = $1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?)
    }

    /// Would moving `folder` under `new_parent` put it inside itself? Walks up from the
    /// proposed parent looking for the folder being moved.
    pub async fn would_cycle(
        &self,
        folder: FolderId,
        new_parent: FolderId,
    ) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(
            "WITH RECURSIVE up AS (
               SELECT id, parent_id, 1 AS depth FROM folder WHERE id = $1
               UNION ALL
               SELECT f.id, f.parent_id, up.depth + 1 FROM folder f
                 JOIN up ON f.id = up.parent_id WHERE up.depth < $3)
             SELECT coalesce(bool_or(id = $2), false) FROM up",
        )
        .bind(new_parent.as_uuid())
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .fetch_one(&self.0)
        .await?)
    }

    /// The `/`-joined slugs from the library root down to this folder — the directory it
    /// lives at inside `libraries/<lib>/`.
    pub async fn slug_path(&self, folder: FolderId) -> Result<String, DbError> {
        Ok(sqlx::query_scalar(
            "WITH RECURSIVE up AS (
               SELECT id, parent_id, slug, 1 AS depth FROM folder WHERE id = $1
               UNION ALL
               SELECT f.id, f.parent_id, f.slug, up.depth + 1 FROM folder f
                 JOIN up ON f.id = up.parent_id WHERE up.depth < $2)
             SELECT string_agg(slug, '/' ORDER BY depth DESC) FROM up",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .fetch_one(&self.0)
        .await?)
    }

    pub async fn library_of(&self, folder: FolderId) -> Result<Option<LibraryId>, DbError> {
        let found: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT library_id FROM folder WHERE id = $1 AND deleted_at IS NULL")
                .bind(folder.as_uuid())
                .fetch_optional(&self.0)
                .await?;
        Ok(found.map(LibraryId::from_uuid))
    }

    pub async fn rename(&self, folder: FolderId, name: &str, slug: &str) -> Result<bool, DbError> {
        let done = sqlx::query("UPDATE folder SET name = $2, slug = $3 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(name)
            .bind(slug)
            .execute(&self.0)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn reparent(
        &self,
        folder: FolderId,
        parent: Option<FolderId>,
    ) -> Result<bool, DbError> {
        let done = sqlx::query("UPDATE folder SET parent_id = $2 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(parent.map(|p| p.as_uuid()))
            .execute(&self.0)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Soft-delete a folder, every descendant, and every part in any of them. Returns
    /// (folders hidden, parts hidden) so the confirmation can name the count it warned
    /// about and the caller can check it matched.
    pub async fn soft_delete_subtree(&self, folder: FolderId) -> Result<(u64, u64), DbError> {
        let mut tx = self.0.begin().await?;

        let folders = sqlx::query(
            "WITH RECURSIVE down AS (
               SELECT id, 1 AS depth FROM folder WHERE id = $1
               UNION ALL
               SELECT f.id, down.depth + 1 FROM folder f
                 JOIN down ON f.parent_id = down.id WHERE down.depth < $2)
             UPDATE folder SET deleted_at = now()
              WHERE id IN (SELECT id FROM down) AND deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let parts = sqlx::query(
            "WITH RECURSIVE down AS (
               SELECT id, 1 AS depth FROM folder WHERE id = $1
               UNION ALL
               SELECT f.id, down.depth + 1 FROM folder f
                 JOIN down ON f.parent_id = down.id WHERE down.depth < $2)
             UPDATE part SET deleted_at = now()
              WHERE folder_id IN (SELECT id FROM down) AND deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        tx.commit().await?;
        Ok((folders, parts))
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p lapidary-db --test folders`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/lapidary-core crates/lapidary-db
git commit -m "feat(db): read and write the category tree, cycles refused at the write"
```

---

# Phase C — Ingest writes the new layout

At the end of this phase a fresh scan produces a browsable store.

### Task 6: Ingest writes a model directory

**Files:**
- Modify: `crates/lapidary-ingest/src/handler.rs:118-330`
- Modify: `crates/lapidary-db/src/repo.rs:136` (`IngestRequest` gains `folder`, `storage_path`)
- Test: `crates/lapidary-ingest/tests/handler.rs`

**Interfaces:**
- Consumes: `slugify`, `disambiguate` (Task 1), `ModelManifest` (Task 3),
  `PgFolders::get_or_create`, `PgFolders::slug_path` (Task 5)
- Produces: `IngestRequest.folder: Option<FolderId>`, `IngestRequest.storage_path: &'a str`

- [ ] **Step 1: Write the failing test**

```rust
// crates/lapidary-ingest/tests/handler.rs
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn ingesting_a_nested_file_writes_a_model_directory(pool: sqlx::PgPool) {
    // The shape the owner asked for: one directory per model, holding its file and its
    // metadata, reachable by opening the storage folder in a file manager.
    let ingest = tempfile::tempdir().expect("ingest dir");
    let store = tempfile::tempdir().expect("store");
    std::fs::create_dir_all(ingest.path().join("Terrain/Rocks")).expect("mkdir");
    std::fs::write(ingest.path().join("Terrain/Rocks/cliff.stl"), sample_stl()).expect("write");

    let handler = WorkerHandler::new(pool.clone(), ingest.path().into(), store.path().into());
    let outcome = handler
        .ingest_one(library(), "Terrain/Rocks/cliff.stl")
        .await
        .expect("ingests");
    assert_eq!(outcome, Outcome::Ingested);

    let dir = store.path().join("libraries/default/Terrain/Rocks/cliff");
    assert!(dir.join("cliff.stl").exists(), "the source sits under its own name");
    let manifest: lapidary_core::manifest::ModelManifest =
        serde_json::from_slice(&std::fs::read(dir.join("metadata.json")).expect("reads"))
            .expect("parses");
    assert_eq!(manifest.part.source_path, "Terrain/Rocks/cliff.stl");
    assert_eq!(manifest.revisions[0].files[0].file_name, "cliff.stl");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn two_models_with_one_name_get_two_directories(pool: sqlx::PgPool) {
    // 6a decided two parts called `cliff` are the truth. Two directories called `cliff/`
    // are impossible, so the second gets a deterministic suffix.
    let ingest = tempfile::tempdir().expect("ingest dir");
    let store = tempfile::tempdir().expect("store");
    for sub in ["Terrain", "Bases"] {
        std::fs::create_dir_all(ingest.path().join(sub)).expect("mkdir");
    }
    std::fs::write(ingest.path().join("Terrain/cliff.stl"), sample_stl()).expect("a");
    std::fs::write(ingest.path().join("Bases/cliff.stl"), other_stl()).expect("b");

    let handler = WorkerHandler::new(pool.clone(), ingest.path().into(), store.path().into());
    handler.ingest_one(library(), "Terrain/cliff.stl").await.expect("a");
    handler.ingest_one(library(), "Bases/cliff.stl").await.expect("b");

    // Different categories, so no collision at all — both are plain `cliff`.
    assert!(store.path().join("libraries/default/Terrain/cliff/cliff.stl").exists());
    assert!(store.path().join("libraries/default/Bases/cliff/cliff.stl").exists());

    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM part ORDER BY source_path")
        .fetch_all(&pool).await.expect("reads");
    assert_eq!(names, vec!["cliff", "cliff"], "one name, two parts — 6a's decision stands");
}
```

- [ ] **Step 2: Run and verify it fails**

Run: `cargo test -p lapidary-ingest writes_a_model_directory`
Expected: FAIL — the store holds `blobs/…`, not `libraries/…`.

- [ ] **Step 3: Implement**

In `crates/lapidary-db/src/repo.rs`, add to `IngestRequest`:

```rust
    /// The category this model lands in. `None` is the library root.
    pub folder: Option<FolderId>,
    /// Where the bytes were written, relative to the storage root. Distinct from
    /// `source_path`: that names a directory we only read, this names one we own.
    pub storage_path: &'a str,
```

Thread both into `PgIngest::record` and `PgIngest::link_existing`'s inserts
(`part.folder_id`, `file.storage_path`).

In `handler.rs`, after the `library_holds` short-circuit and before writing bytes:

```rust
        // The category tree mirrors the ingest directory. Created here, after the
        // short-circuit -- not during the walk -- so a re-scan of a directory whose models
        // have all been moved away does not silently re-create the empty originals.
        let folders = PgFolders(self.db.clone());
        let mut parent: Option<FolderId> = None;
        let dirs: Vec<&str> = FsPath::new(source_path)
            .parent()
            .and_then(|p| p.to_str())
            .filter(|p| !p.is_empty())
            .map(|p| p.split('/').collect())
            .unwrap_or_default();
        for segment in dirs {
            let slug = slugify(segment);
            parent = Some(
                folders
                    .get_or_create(library, parent, segment, &slug)
                    .await
                    .map_err(classify_db)?,
            );
        }

        // libraries/<lib>/<category…>/<model>/
        let category = match parent {
            Some(f) => folders.slug_path(f).await.map_err(classify_db)?,
            None => String::new(),
        };
        let mut model_dir = slugify(name);
        let base = format!("libraries/{library_slug}/{category}").trim_end_matches('/').to_owned();
        let relocator = SourceRelocator::open(&self.blob_root);
        if std::path::Path::new(&self.blob_root).join(&base).join(&model_dir).exists() {
            model_dir = disambiguate(&model_dir, &hash);
        }
        let model_rel = format!("{base}/{model_dir}");
        relocator.create_dir(&model_rel).map_err(|e| HandlerError::Transient {
            message: e.to_string(),
        })?;
        let file_name = FsPath::new(source_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name);
        let storage_path = format!("{model_rel}/{file_name}");
```

Write the source at `storage_path` instead of `blob_path`, and write `metadata.json` beside
it, both through `SourceStore` (ingest holds the `WorkerRole`; extend it with
`put_at(rel: &str, bytes: &[u8], compression: Compression)`).

`library_slug` comes from a new `PgParts::library_slug(library) -> Result<Option<String>, DbError>`
returning `slugify(library.name)`; the seeded library is `Default`, which slugs to `Default`.
Use lowercase for the directory: `slugify(&name).to_lowercase()` gives `default`, matching
the layout in the spec.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-ingest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-ingest crates/lapidary-db
git commit -m "feat(ingest): write each model into its own folder, with its metadata beside it"
```

---

### Task 7: Reads tolerate both layouts

**Files:**
- Modify: `crates/lapidary-db/src/repo.rs:745` (`source_for_download` selects `storage_path`)
- Modify: `crates/lapidary-api/src/download.rs`
- Test: `crates/lapidary-api/tests/download.rs`

**Interfaces:**
- Produces: `DownloadSource.storage_path: Option<String>`

- [ ] **Step 1: Write the failing test**

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_part_with_no_storage_path_still_downloads(pool: sqlx::PgPool) {
    // The half-migrated state is live for hours on a real corpus, so it is a supported
    // state, not an edge case: null storage_path means the bytes are still at the old
    // content-addressed path.
    let store = tempfile::tempdir().expect("store");
    let (revision, bytes) = seed_revision_at_cas_path(&pool, store.path()).await;

    let response = download(&pool, store.path(), revision, "original").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.into_body_bytes().await, bytes);
}
```

- [ ] **Step 2: Run and verify it fails**

Run: `cargo test -p lapidary-api no_storage_path`
Expected: FAIL — the route reads only `storage_path` and finds `None`.

- [ ] **Step 3: Implement**

```rust
    // `storage_path` is null while migrate_storage is still draining, and null means the
    // bytes are where the content-addressed store put them. Both are correct answers for
    // as long as that job takes, so both are read rather than one being treated as an
    // error the operator has to wait out.
    let bytes = match source.storage_path.as_deref() {
        Some(rel) => reader.get_at(rel, source.zstd_level)?,
        None => reader.get(&source.hash, source.zstd_level)?,
    };
```

Add `SourceReader::get_at(&self, rel: &str, zstd_level: Option<i16>) -> Result<Vec<u8>, StorageError>`
mirroring `get`, resolving through `reject_escaping_path` first.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-api && cargo xtask check-deploy`
Expected: PASS — `SourceReader` is still named only in `download.rs`.

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-api crates/lapidary-db crates/lapidary-storage
git commit -m "fix(download): serve a part whether or not its bytes have migrated yet"
```

---

# Phase D — Migrating what is already there

### Task 8: The `migrate_storage` job

**Files:**
- Modify: `crates/lapidary-core/src/job.rs`
- Modify: `crates/lapidary-ingest/src/handler.rs` (new arm)
- Test: `crates/lapidary-ingest/tests/migrate.rs`

**Interfaces:**
- Produces: `JobPayload::MigrateStorage`, `JobPayload::MIGRATE_STORAGE: &str`,
  `Outcome::Migrated`

- [ ] **Step 1: Write the failing tests**

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn migrate_storage_moves_a_cas_blob_into_a_model_directory(pool: sqlx::PgPool) {
    let store = tempfile::tempdir().expect("store");
    seed_cas_part(&pool, store.path(), "Terrain/Rocks/cliff.stl").await;

    let handler = WorkerHandler::new(pool.clone(), store.path().into(), store.path().into());
    let outcome = handler.migrate_storage(library()).await.expect("migrates");
    assert_eq!(outcome, Outcome::Migrated);

    let dir = store.path().join("libraries/default/Terrain/Rocks/cliff");
    assert!(dir.join("cliff.stl").exists());
    assert!(dir.join("metadata.json").exists());
    let path: Option<String> = sqlx::query_scalar("SELECT storage_path FROM file")
        .fetch_one(&pool).await.expect("reads");
    assert_eq!(path.as_deref(), Some("libraries/default/Terrain/Rocks/cliff/cliff.stl"));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn an_interrupted_migration_loses_no_file(pool: sqlx::PgPool) {
    // Copy before delete: at every instant the bytes are readable at the old path, the new
    // path, or both — never at neither.
    let store = tempfile::tempdir().expect("store");
    let (hash, bytes) = seed_cas_part(&pool, store.path(), "Terrain/cliff.stl").await;

    let handler = WorkerHandler::new(pool.clone(), store.path().into(), store.path().into());
    handler.migrate_one_copying_only(&hash).await.expect("copies but does not unlink");

    // Both paths hold the file, and both hold the same bytes.
    let old = store.path().join(cas_rel(&hash));
    let new = store.path().join("libraries/default/Terrain/cliff/cliff.stl");
    assert_eq!(std::fs::read(&old).expect("old"), bytes);
    assert_eq!(std::fs::read(&new).expect("new"), bytes);

    // Resuming completes and the old path goes.
    handler.migrate_storage(library()).await.expect("resumes");
    assert!(!old.exists(), "the CAS copy is removed only after the new one is durable");
}
```

- [ ] **Step 2: Run and verify they fail**

Run: `cargo test -p lapidary-ingest migrate`
Expected: FAIL — no such method.

- [ ] **Step 3: Implement**

In `job.rs`:

```rust
    /// Move every source blob in this library out of the content-addressed store and into
    /// its model's own directory, writing a `metadata.json` beside it.
    ///
    /// A job rather than part of migration `0008`, because sqlx runs a migration in one
    /// transaction at startup and copying a corpus is neither transactional nor fast.
    /// Resumable because the queue is: it selects the next batch of `file` rows whose
    /// `storage_path` is still null, so a killed worker resumes where it stopped.
    MigrateStorage,
```

`pub const MIGRATE_STORAGE: &'static str = "migrate_storage";` and the `kind()` arm.
Add `Outcome::Migrated` and extend `job_outcome_known` in a new migration `0009`:

```sql
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in
           ('ingested', 'skipped', 'rendered', 'scanned', 'migrated'));
```

Handler, per file: read the CAS blob, resolve the model directory exactly as Task 6 does,
**write the new copy and `fsync` it**, write `metadata.json`, update `file.storage_path` in
one transaction, and only then unlink the CAS blob. Batch 200 rows per job run and
re-enqueue while any `storage_path` remains null, so progress is visible through the
existing `BatchStatus`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-ingest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-core crates/lapidary-ingest crates/lapidary-db
git commit -m "feat(ingest): move an existing store into model folders, resumably"
```

---

# Phase E — Moving things

### Task 9: Move a part

**Files:**
- Create: `crates/lapidary-api/src/moves.rs`
- Modify: `crates/lapidary-api/src/lib.rs`
- Test: `crates/lapidary-api/tests/moves.rs`

**Interfaces:**
- Consumes: `SourceRelocator` (Task 2), `PgFolders::slug_path` (Task 5)
- Produces: `PATCH /api/parts/{id}`, `GET /api/parts/{id}/moves`

- [ ] **Step 1: Write the failing tests**

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_move_survives_a_rescan(pool: sqlx::PgPool) {
    // THE test. `source_path` is immutable identity, `folder_id` is mutable location, and
    // this is the case that proves splitting them was right: after a move, a re-scan of
    // the original directory still recognises the file and leaves it where the user put it.
    let (store, ingest) = seed_scanned_corpus(&pool).await;   // Terrain/rock.stl
    let part = only_part(&pool).await;
    let bases = PgFolders(pool.clone())
        .get_or_create(library(), None, "Bases", "Bases").await.expect("Bases");

    move_part(&pool, store.path(), part, Some(bases)).await.expect("moves");

    let handler = WorkerHandler::new(pool.clone(), ingest.path().into(), store.path().into());
    let outcome = handler.ingest_one(library(), "Terrain/rock.stl").await.expect("re-scans");

    assert_eq!(outcome, Outcome::Skipped, "the same bytes at the same source path");
    let (folder, source_path): (Option<uuid::Uuid>, String) =
        sqlx::query_as("SELECT folder_id, source_path FROM part WHERE id = $1")
            .bind(part.as_uuid()).fetch_one(&pool).await.expect("reads");
    assert_eq!(folder, Some(bases.as_uuid()), "still in Bases");
    assert_eq!(source_path, "Terrain/rock.stl", "identity never moved");
    assert!(store.path().join("libraries/default/Bases/rock/rock.stl").exists());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_failed_rename_leaves_the_database_untouched(pool: sqlx::PgPool) {
    // Rename first, inside the transaction: a failure must move nothing and change nothing.
    let (store, _ingest) = seed_scanned_corpus(&pool).await;
    let part = only_part(&pool).await;
    let before: Option<String> = sqlx::query_scalar("SELECT storage_path FROM file")
        .fetch_one(&pool).await.expect("reads");

    let readonly = deny_writes(store.path().join("libraries/default"));
    let result = move_part(&pool, store.path(), part, None).await;
    restore(readonly);

    assert!(result.is_err(), "the rename failed");
    let after: Option<String> = sqlx::query_scalar("SELECT storage_path FROM file")
        .fetch_one(&pool).await.expect("reads");
    assert_eq!(before, after, "no row moved");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn moving_into_a_name_collision_needs_an_acknowledgement(pool: sqlx::PgPool) {
    let (store, _ingest) = seed_two_parts_named_cliff(&pool).await;
    let (a, b) = two_parts(&pool).await;
    let bases = folder_of(&pool, b).await;

    let refused = move_request(&pool, store.path(), a, bases, false).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);

    let accepted = move_request(&pool, store.path(), a, bases, true).await;
    assert_eq!(accepted.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run and verify they fail**

Run: `cargo test -p lapidary-api --test moves`
Expected: FAIL — no route.

- [ ] **Step 3: Implement**

```rust
//! Moving a model between categories.
//!
//! The one module allowed to name `SourceRelocator` — `xtask/src/deploy.rs`'s
//! `RELOCATE_MODULE` names this file exactly, so splitting the route across two files
//! fails the gate rather than quietly widening the capability.
//!
//! **Ordering: rename first, inside the transaction, commit only if it succeeded.** A
//! failed rename rolls back and nothing moved. The window that remains is a rename that
//! succeeds and a commit that then fails, leaving the disk ahead of the database — which
//! `metadata.json` makes repairable, because every model directory identifies itself. The
//! reverse ordering was rejected: its failure leaves the database pointing at a path that
//! does not exist, which every read then hits. A disk ahead of the database is a repair
//! job; a database ahead of the disk is a broken grid.

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MovePart {
    pub folder_id: Option<FolderId>,
    /// The client has seen the collision warning and wants it anyway. Slice 6a decided two
    /// parts called `bracket` are the truth; this is how the UI says it knows.
    #[serde(default)]
    pub acknowledge_duplicate: bool,
}
```

Handler outline, each branch returning an error that says what to do:

1. Load the part, its library, its current `folder_id` and its `file.storage_path`.
2. If `storage_path` is null → `409`, *"This model has not finished moving into the new
   storage layout yet. Wait for the storage migration to finish, then try again."*
3. Target folder must be in the same library → `409`.
4. Unless `acknowledge_duplicate`, a part with the same `name` already in the target →
   `409` naming it.
5. Compute the destination: `libraries/{lib}/{slug_path(target)}/{model_dir}`, applying
   `disambiguate` if the directory exists.
6. `BEGIN`; `UPDATE part SET folder_id`; `UPDATE file SET storage_path`;
   `INSERT INTO part_move`; `relocator.rename(old_dir, new_dir)?`; `COMMIT`.
7. Rewrite `metadata.json` — it carries no path, so nothing in it changes on a move. Skip.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lapidary-api && cargo xtask check-deploy`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lapidary-api
git commit -m "feat(api): move a model between categories, rename first and row second"
```

---

### Task 10: Folder CRUD

**Files:**
- Create: `crates/lapidary-api/src/folders.rs`
- Test: `crates/lapidary-api/tests/folders.rs`

**Interfaces:**
- Produces: `GET/POST /api/libraries/{id}/folders`, `PATCH/DELETE /api/folders/{id}`,
  `FolderNode { id, parentId, name }` (ts-rs)

- [ ] **Step 1: Write the failing tests**

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn renaming_into_a_collision_is_refused_with_no_override(pool: sqlx::PgPool) {
    // The asymmetry with parts, and it is deliberate: two parts named `bracket` are told
    // apart by source_path; two categories named `Terrain` are told apart by nothing.
    let f = PgFolders(pool.clone());
    f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("Terrain");
    let bases = f.get_or_create(library(), None, "Bases", "Bases").await.expect("Bases");

    let response = rename_folder(&pool, bases, "Terrain").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_folder_cannot_be_parented_into_another_library(pool: sqlx::PgPool) {
    let other = seed_second_library(&pool).await;
    let f = PgFolders(pool.clone());
    let mine = f.get_or_create(library(), None, "Terrain", "Terrain").await.expect("mine");
    let theirs = f.get_or_create(other, None, "Terrain", "Terrain").await.expect("theirs");

    let response = reparent_folder(&pool, mine, Some(theirs)).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_folder_hides_its_models_and_touches_no_file(pool: sqlx::PgPool) {
    let (store, _ingest) = seed_scanned_corpus(&pool).await;   // Terrain/rock.stl
    let terrain = folder_named(&pool, "Terrain").await;
    let file_path = store.path().join("libraries/default/Terrain/rock/rock.stl");

    let response = delete_folder(&pool, store.path(), terrain).await;

    assert_eq!(response.status(), StatusCode::OK);
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE deleted_at IS NULL")
        .fetch_one(&pool).await.expect("counts");
    assert_eq!(visible, 0);
    assert!(file_path.exists(), "soft delete never removes a file");
}
```

- [ ] **Step 2–5:** implement the four routes over `PgFolders`, run
  `cargo test -p lapidary-api --test folders`, then commit:

```bash
git commit -m "feat(api): create, rename, move and soft-delete a category"
```

---

# Phase F — The UI

### Task 11: The category sidebar

**Files:**
- Create: `web/src/components/FolderTree.tsx`
- Modify: `web/src/routes/index.tsx`, `web/src/lib/api.ts`, `web/src/lib/strings.ts`
- Test: `web/src/components/FolderTree.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
it('nests a child under its parent and filters the grid on select', async () => {
  renderWithTree([
    { id: 'f1', parentId: null, name: 'Terrain' },
    { id: 'f2', parentId: 'f1', name: 'Rocks' },
  ])
  await userEvent.click(screen.getByRole('button', { name: 'Terrain' }))
  expect(screen.getByRole('button', { name: 'Rocks' })).toBeVisible()
  await userEvent.click(screen.getByRole('button', { name: 'Rocks' }))
  expect(router.state.location.search).toContain('folderId=f2')
})
```

- [ ] **Step 2:** `npm test -- FolderTree` → FAIL (no component).
- [ ] **Step 3:** implement. `folderId` is a typed TanStack Router search param so the filter
  lives in the URL and survives a reload, matching how every other filter behaves. Strings
  through `strings.ts`:

```ts
export const folders = {
  root: 'All models',
  empty: 'No categories yet — they appear when you scan a folder.',
  moveHere: 'Move here',
  duplicateTitle: (name: string) => `“${name}” is already in this folder`,
  duplicateBody: 'Two models can share a name — they are told apart by where they came from.',
  duplicateConfirm: 'Move anyway',
  deleteTitle: (name: string) => `Delete ${name}?`,
  deleteBody: (parts: number) =>
    `The ${parts} models inside will be moved to deleted. Nothing is removed from your ` +
    `storage folder, and you can undo this.`,
  showInFolder: 'Show in folder',
}
```

- [ ] **Step 4:** `npm test` → PASS.
- [ ] **Step 5:** commit `feat(web): show the category tree, and filter the grid by it`.

---

### Task 12: Drag to move, and the two dialogs

**Files:** `web/src/routes/index.tsx`, `web/src/components/FolderTree.tsx`

- [ ] **Step 1:** test that a `409` from the move renders the duplicate dialog and that
  confirming re-sends with `acknowledgeDuplicate: true`; test that the delete dialog names
  the count.
- [ ] **Step 2:** `npm test` → FAIL.
- [ ] **Step 3:** implement drag-to-move plus a context-menu *"Move to…"* — drag into a
  scrolled tree is a poor trackpad target and unusable from a keyboard, so the menu is the
  accessible path, not a fallback. Motion: 120 ms on transform/opacity only.
- [ ] **Step 4:** `npm test && npm run build` → PASS.
- [ ] **Step 5:** commit `feat(web): drag a model to a category, and confirm what it costs`.

---

### Task 13: Show in folder, and the docs that now disagree

**Files:** `web/src/routes/index.tsx`, `docs/DATA.md`, `deploy/compose.yaml`, `docs/FEATURES.md`

- [ ] **Step 1:** add the *"Show in folder"* action revealing a model's directory. The point
  of this layout is that the user can go and look; an app that hides the path did not need
  the layout.
- [ ] **Step 2:** rewrite `DATA.md` §1.1 for the new layout, keeping the CAS description for
  `cache/` only, and note that source dedup is gone and why.
- [ ] **Step 3:** rewrite `deploy/compose.yaml`'s volume comment — it currently argues for a
  named volume on grounds this slice reverses. Point `LAPIDARY_BLOB_ROOT` at a bind-mounted
  host directory with `:z`, not `:Z`, for the reason the ingest mount already documents.
- [ ] **Step 4:** add the folder rows to `FEATURES.md` §1 at Phase 1.
- [ ] **Step 5:** `cargo xtask check && npm run build` → PASS. Commit
  `docs: describe the store we now have, not the one we replaced`.

---

## Self-Review

**Spec coverage.** §1 layout → Tasks 6, 8. §1 `metadata.json` → Task 3. §2 naming → Task 1.
§3 identity split → Tasks 6, 9. §4 ordering → Task 9. §5 schema → Task 4. §5.1 backfill →
Task 4. §5.2 `migrate_storage` → Task 8. §6 scan → Task 6. §7 moves/rename/delete → Tasks 9,
10. §8 tests → distributed; every numbered case has a home. §9 `part_move` → Tasks 4, 9. §10
API → Tasks 9, 10. §11 frontend → Tasks 11–13. §12 non-goals → not implemented, by design.

**Gap found and closed:** §7's *"Show in folder"* had no task; it is now Task 13, together
with the doc rewrites that the two reversals oblige — without them `DATA.md` §1.1 and
`compose.yaml` describe a store that no longer exists.

**Second gap found and closed:** `Outcome::Migrated` needs a `job_outcome_known` change, and
`0006` shows that constraint is drop-and-add, not alter. Migration `0009` is named in Task 8.

**Type consistency.** `FolderId` (Task 5) is used identically in Tasks 6, 9, 10.
`storage_path` is `Option<String>` at every reader (Tasks 7, 8, 9) because it stays nullable
until `migrate_storage` drains. `slugify`/`disambiguate` signatures match between Tasks 1, 6
and 9. `PgFolders::slug_path` returns the `/`-joined slugs used to build the directory in
Tasks 6, 8 and 9.
