# Phase 1 slice 3b — 3MF ingest: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a `.3mf` dropped in the ingest folder becomes a part with a thumbnail and three
LOD rungs, exactly like an STL, closing Phase 1's mesh-format line.

**Architecture:** `parse_3mf` reads the OPC package with `zip`, follows `_rels/.rels` to
the model part, and streams its XML with `quick-xml`, merging every `<build>` item's
transformed mesh into the one `Mesh` the rest of the pipeline already handles. Archive
reads are capped as bytes arrive. `SourceStore` learns a compression policy so 3MF is
stored as-is.

**Tech Stack:** Rust 1.95.0 edition 2024. Two new dependencies — `zip` 2 (deflate, pure
Rust) and `quick-xml` 0.41 — and nothing else changes.

**Spec:** `docs/superpowers/specs/2026-09-04-phase-1-slice-3b-3mf-design.md` — read it
first. Every "why" below is argued there; this plan is the "how".

## Global Constraints

Copied from `CLAUDE.md` and the spec. Every task's requirements implicitly include this
section.

- **Two new dependencies, both named in spec §3.2, and no others.** `zip` with
  `default-features = false, features = ["deflate"]` and `quick-xml`. If a task feels like
  it needs a third, it is the wrong task.
- **The open path never touches a source file and never invokes the CAD kernel.** Nothing
  in this slice goes near `lapidary-api`.
- **No SQL outside `lapidary-db`.** This slice writes no SQL at all — see spec §6.
- **We never delete user data implicitly.** A refused 3MF leaves no part and no orphaned
  blob; slice 3's reap already covers this and must keep working.
- **Errors say what broke and what to do.** "Could not read this 3MF — it declares the
  unit `furlong`, and only micron, millimeter, centimeter, inch, foot and meter are
  understood." Not "bad unit".
- **Measurement must not lie.** Unit conversion is not optional — spec §3.3.
- **Rust:** `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside
  tests**; the workspace lint denies it.
- **`cargo xtask check-strings`** scans every new string literal for runs of three or more
  spaces. Write continuation strings with a real `\` and no alignment padding inside
  literals. **Its `EXEMPT` list is pinned by line number** — any edit that shifts a line in
  `stl.rs` or `lapidary-db/tests/repo.rs` re-breaks it. It went stale five times in slice
  3. Expect to re-pin, and check before blaming your own change.
- **Real content in fixtures.** The 3MF fixture is a real part with a plausible number.
- **Commit messages** pass `cargo xtask check-commit-msg`: Conventional Commits, a closed
  type list, and no AI attribution trailer.
- **Branch, do not push.** Work on `feat/3mf-ingest`, merge to `main` with `--no-ff`
  locally. CI is the release gate, not the per-change gate.
- **When unsure, prefer the boring option.**

## The verification bar

Exactly what `.github/workflows/ci.yml` runs. A task is not done until it passes. **Never
pipe these through `tail` or `grep` when the exit code matters** — use `; echo "exit=$?"`.

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

Tests need a live PostgreSQL 18:

```sh
docker run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/postgres:18
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
bash -c 'cat < /dev/null > /dev/tcp/127.0.0.1/55432' && echo reachable
```

**`cargo-deny` is not installed on this machine.** Auditing two new dependencies is this
slice's whole premise, so task 1 installs it and runs it for real rather than deferring to
CI:

```sh
cargo install --locked cargo-deny
```

Baseline at the start of this slice: **336 passed / 0 failed**, web **33 passed**.

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | **Modify.** `zip` and `quick-xml` in `[workspace.dependencies]`. |
| `crates/lapidary-cad/Cargo.toml` | **Modify.** Consumes both. |
| `crates/lapidary-cad/src/tmf.rs` | **Create.** The whole 3MF reader: caps, archive, `.rels`, model XML, transforms. |
| `crates/lapidary-cad/src/lib.rs` | **Modify.** `mod tmf;` and `pub use tmf::parse_3mf;`. |
| `crates/lapidary-cad/src/kernel.rs` | **Modify.** `CadError::ArchiveRefused`. |
| `crates/lapidary-cad/src/mesh_kernel.rs` | **Modify.** A `"3mf"` arm in `parse`. |
| `crates/lapidary-storage/src/lib.rs` | **Modify.** `Compression`, and `SourceStore::put` takes it. |
| `crates/lapidary-ingest/src/scan.rs` | **Modify.** `MESH_EXTENSIONS` gains `"3mf"`. |
| `crates/lapidary-ingest/src/handler.rs` | **Modify.** Passes the compression policy. |
| `crates/lapidary-ingest/tests/handler.rs` | **Modify.** End-to-end 3MF coverage. |
| `fixtures/planetary-carrier-lp-3480-02.3mf` | **Create.** A real part, two build items. |

`tmf.rs` is one file rather than four. It is about 450 lines including tests, every part of
it is one format's reader, and splitting `.rels` resolution from the mesh parse would put
two halves of one decision in two files. `stl.rs` at 420 lines is the precedent.

---

## Task 1: The two dependencies, audited

**Files:**
- Modify: `Cargo.toml`, `crates/lapidary-cad/Cargo.toml`

**Read first:** spec §3.2, and `deny.toml`'s `[licenses]` block.

**Interfaces:**
- Produces: `zip` and `quick-xml` available to `lapidary-cad`.

This task adds no code. It exists on its own because the dependency addition is the reason
slice 3b was split out of slice 3, and a reviewer should be able to accept or reject it
without reading a parser.

- [ ] **Step 1: Add to the workspace**

In `Cargo.toml`'s `[workspace.dependencies]`, keeping alphabetical order:

```toml
quick-xml = "0.41"
zip = { version = "2.4.2", default-features = false, features = ["deflate"] }
```

`default-features = false` matters: the default set pulls bzip2, zstd and AES, none of
which a 3MF uses. `deflate` is the only working pure-Rust path — zip 2.4.2's
`deflate-flate2` feature does not compile, because it gates code needing `flate2` without
enabling the optional dependency. Spec §3.2 records this; do not try to be clever with it.

- [ ] **Step 2: Consume in `lapidary-cad`**

In `crates/lapidary-cad/Cargo.toml` under `[dependencies]`:

```toml
quick-xml.workspace = true
zip.workspace = true
```

- [ ] **Step 3: Confirm what actually entered the lock**

```sh
cargo fetch; echo "exit=$?"
git diff --stat Cargo.lock
```

Expected: **eight new packages that compile** — `zip`, `flate2`, `miniz_oxide`, `adler2`,
`simd-adler32`, `crc32fast`, `zopfli`, `quick-xml`.

`Cargo.lock` will gain two more lines than that: `arbitrary` and `derive_arbitrary`. They
are correct and expected. zip declares them under
`[target."cfg(fuzzing)".dependencies]`, so the lockfile records them while a normal build
never compiles them — `cargo tree -i arbitrary` reports "nothing to print", which is the
proof. Do not try to remove them.

If anything *else* appears, `default-features = false` did not take; stop and re-read
step 1.

- [ ] **Step 4: Audit the licences for real**

```sh
cargo install --locked cargo-deny
cargo deny check; echo "exit=$?"
```

Expected: exit 0. No `deny.toml` change should be needed — every new crate is MIT,
Apache-2.0, or offers one of those in an `OR` (`adler2` is `0BSD OR MIT OR Apache-2.0`,
`miniz_oxide` is `MIT OR Zlib OR Apache-2.0`, `zopfli` is Apache-2.0), and all of those are
already in the allow-list. **If cargo-deny asks for a new licence, stop and report it** —
that is a finding about the spec, not a line to add.

`multiple-versions = "warn"` may report two `miniz_oxide` majors. A warning is not a
failure; note it in the commit message and move on.

**`quick-xml` must be 0.41 or later.** 0.37 carries RUSTSEC-2026-0194 and
RUSTSEC-2026-0195 — a quadratic-time parse and an unbounded-allocation memory-exhaustion
DoS, both in the component that reads attacker-controlled XML. `cargo deny check` fails on
them. The API this plan uses is unchanged across the bump; it was compiled against 0.41
before this line was written.

- [ ] **Step 5: Verify**

```sh
cargo build --workspace --all-features; echo "exit=$?"
cargo test --workspace --all-features; echo "exit=$?"
```

Expected: 336 passed, unchanged. Nothing uses the new crates yet.

- [ ] **Step 6: Commit**

```sh
git add Cargo.toml Cargo.lock crates/lapidary-cad/Cargo.toml
git commit -m "build(cad): add zip and quick-xml for 3MF"
```

---

## Task 2: `SourceStore` learns a compression policy

**Files:**
- Modify: `crates/lapidary-storage/src/lib.rs`, `crates/lapidary-ingest/src/handler.rs`

**Read first:** spec §3.7, `DATA.md` §1.2's table, and `lapidary-storage/src/lib.rs:241`.

**Interfaces:**
- Produces: `Compression::{Zstd, AsIs}`, `Compression::for_source_format(&str) -> Compression`,
  and `SourceStore::put(&self, bytes: &[u8], compression: Compression) -> Result<StoredBlob, StorageError>`.

Independent of 3MF parsing, so it lands early and is provably correct before anything
depends on it. Every current caller passes `Zstd` behaviour, so the observable result for
STL and OBJ is unchanged.

- [ ] **Step 1: Write the failing tests**

In `crates/lapidary-storage/src/lib.rs`'s `mod tests`:

```rust
#[test]
fn the_compression_policy_follows_the_data_doc_table() {
    // DATA.md §1.2: STEP, STL and OBJ compress; 3MF is already a deflate ZIP.
    assert_eq!(Compression::for_source_format("stl"), Compression::Zstd);
    assert_eq!(Compression::for_source_format("obj"), Compression::Zstd);
    assert_eq!(Compression::for_source_format("step"), Compression::Zstd);
    assert_eq!(Compression::for_source_format("3mf"), Compression::AsIs);
    // Case is not the caller's problem: `source_format` lowercases, but a policy that
    // silently compressed an uppercase 3MF would be a very quiet bug.
    assert_eq!(Compression::for_source_format("3MF"), Compression::AsIs);
    // An unknown format compresses. Spending CPU is the safe wrong answer; storing an
    // already-packed format uncompressed costs only space.
    assert_eq!(Compression::for_source_format("wrl"), Compression::Zstd);
}

#[test]
fn an_as_is_blob_round_trips_and_records_equal_sizes() {
    let (dir, _) = store();
    let store = SourceStore::open(dir.path(), &WorkerRole::assume());
    // Deliberately compressible, so a stored size equal to the real size proves the
    // policy was honoured rather than proving the bytes were incompressible.
    let bytes = vec![0u8; 64 * 1024];
    let stored = store.put(&bytes, Compression::AsIs).expect("stores");
    assert_eq!(stored.stored_bytes, stored.size_bytes);
    assert_eq!(stored.zstd_level, 0);
    assert_eq!(store.get(&stored.hash, Compression::AsIs).expect("reads"), bytes);
}

#[test]
fn a_zstd_blob_still_shrinks() {
    let (dir, _) = store();
    let store = SourceStore::open(dir.path(), &WorkerRole::assume());
    let bytes = vec![0u8; 64 * 1024];
    let stored = store.put(&bytes, Compression::Zstd).expect("stores");
    assert!(stored.stored_bytes < stored.size_bytes);
    assert_eq!(store.get(&stored.hash, Compression::Zstd).expect("reads"), bytes);
}
```

- [ ] **Step 2: Run them and watch them fail**

```sh
cargo test -p lapidary-storage --all-features; echo "exit=$?"
```

Expected: FAIL — `Compression` does not exist.

- [ ] **Step 3: Implement**

In `crates/lapidary-storage/src/lib.rs`:

```rust
/// Whether a source blob is compressed on the way in.
///
/// `DATA.md` §1.2's table, in one place with its reasoning, rather than a boolean at each
/// call site where the next reader cannot tell what `true` meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Zstd,
    AsIs,
}

impl Compression {
    /// STEP, STL and OBJ compress 2–10×. 3MF is already a deflate ZIP, so re-compressing
    /// it spends CPU on every ingest to make the file very slightly larger.
    ///
    /// An unrecognised format compresses: that wastes a little CPU on something already
    /// packed, where the other default would waste disk on everything else.
    pub fn for_source_format(format: &str) -> Self {
        match format.to_ascii_lowercase().as_str() {
            "3mf" => Compression::AsIs,
            _ => Compression::Zstd,
        }
    }

    fn compresses(self) -> bool {
        matches!(self, Compression::Zstd)
    }
}
```

Then change `SourceStore`'s two methods:

```rust
    pub fn put(&self, bytes: &[u8], compression: Compression) -> Result<StoredBlob, StorageError> {
        write_blob(&self.root, bytes, compression.compresses())
    }

    pub fn get(&self, hash: &BlobHash, compression: Compression) -> Result<Vec<u8>, StorageError> {
        read_blob(&self.root, hash, compression.compresses())
    }
```

`get` takes it too, and must: reading a stored-as-is blob through the zstd path fails, and
nothing in the bytes says which was used. The caller knows the format; the store does not.

- [ ] **Step 4: Update the callers**

In `crates/lapidary-ingest/src/handler.rs`, the `source.put` call in step 5b becomes:

```rust
        let stored = source
            .put(&bytes, Compression::for_source_format(&params.format))
            .map_err(|e| HandlerError::Transient {
                message: e.to_string(),
            })?;
```

Add `Compression` to the `lapidary_storage` import. Fix any other `put`/`get` call the
compiler names — `DerivativeStore` is untouched, it has no policy.

- [ ] **Step 5: Run**

```sh
cargo test --workspace --all-features; echo "exit=$?"
```

Expected: 339 passed (336 + 3).

- [ ] **Step 6: Verify the mutation bites**

Make `for_source_format` return `Compression::Zstd` unconditionally:

```rust
    pub fn for_source_format(_format: &str) -> Self {
        Compression::Zstd
    }
```

Run `cargo test -p lapidary-storage --all-features; echo "exit=$?"`.
Expected: `the_compression_policy_follows_the_data_doc_table` FAILS on the `3mf` assertion.
**Revert byte-identically** and re-run to confirm green.

- [ ] **Step 7: Commit**

```sh
git add crates/lapidary-storage crates/lapidary-ingest
git commit -m "feat(storage): let a source blob choose whether it is compressed"
```

---

## Task 3: Capped reading, and the error a refusal deserves

**Files:**
- Create: `crates/lapidary-cad/src/tmf.rs`
- Modify: `crates/lapidary-cad/src/lib.rs`, `crates/lapidary-cad/src/kernel.rs`

**Read first:** spec §3.4 and §8, and `DATA.md` §5.4.

**Interfaces:**
- Produces: `CadError::ArchiveRefused { format: String, detail: String }`,
  `Caps { max_decompressed: u64, max_entries: usize, max_ratio: u64 }`,
  `Caps::DEFAULT`, and
  `read_capped<R: Read>(reader: R, cap: u64) -> Result<Vec<u8>, CadError>`.

The security core, built and tested before any 3MF exists. `read_capped` is generic over
`Read` rather than taking a zip entry, which is what makes the decisive test possible.

- [ ] **Step 1: Add the error variant**

In `crates/lapidary-cad/src/kernel.rs`, after `UnsupportedFormat`:

```rust
    #[error(
        "Refused this {format} — {detail}. The file may be corrupt or deliberately crafted; if it is genuinely this large, split it into separate parts."
    )]
    ArchiveRefused { format: String, detail: String },
```

Its own variant, not `MalformedMesh`: a bomb can be perfectly well-formed, and "re-export
it from your CAD tool" is wrong advice for a file refused on size.

- [ ] **Step 2: Write the failing tests**

Create `crates/lapidary-cad/src/tmf.rs` with only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A bounded source that records how many bytes were actually pulled from it.
    ///
    /// Bounded on purpose. An infinite reader proves the same point more elegantly, but
    /// the naive implementation this test exists to catch calls `read_to_end` on it and
    /// allocates until the machine dies — an OOM kill, not a test failure. A finite
    /// source plus a byte counter gives a deterministic red test for 64 KiB.
    ///
    /// Also deliberately not `std::io::repeat`: std specialises `Repeat::read_to_end` to
    /// fail with `OutOfMemory` immediately, so the naive version would return an error
    /// too and the test would pass against the exact bug it exists to catch.
    struct Counted<'a> {
        remaining: usize,
        pulled: &'a std::cell::Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.remaining);
            buf[..n].fill(0);
            self.remaining -= n;
            self.pulled.set(self.pulled.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn reading_stops_at_the_cap_rather_than_after_it() {
        // `DATA.md` §5.4 says abort "on breach, not after", and the byte count is what
        // tells those two apart. Both implementations return an error, so asserting on
        // the error alone would certify nothing.
        let pulled = std::cell::Cell::new(0);
        let source = Counted { remaining: 64 * 1024, pulled: &pulled };
        let err = read_capped(source, 1024).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
        assert!(
            pulled.get() <= 1024 + 4096,
            "pulled {} bytes for a 1024-byte cap: the cap must bound the read, not just \
             the result",
            pulled.get()
        );
    }

    #[test]
    fn a_stream_inside_the_cap_is_returned_whole() {
        let bytes = read_capped(&b"3MF"[..], 1024).expect("reads");
        assert_eq!(bytes, b"3MF");
    }

    #[test]
    fn a_stream_exactly_at_the_cap_is_allowed() {
        // Off-by-one guard: the cap is a maximum, not a strict bound. A 1024-byte entry
        // under a 1024-byte cap is legal, and a parser that refused it would reject
        // files for being exactly the documented size.
        let bytes = read_capped(&[7u8; 1024][..], 1024).expect("reads");
        assert_eq!(bytes.len(), 1024);
    }

    #[test]
    fn the_default_caps_are_the_documented_ones() {
        // Spec §3.4. These are a security control; a silent edit should fail a test.
        assert_eq!(Caps::DEFAULT.max_decompressed, 2 << 30);
        assert_eq!(Caps::DEFAULT.max_entries, 1024);
        assert_eq!(Caps::DEFAULT.max_ratio, 200);
    }
}
```

- [ ] **Step 3: Run them and watch them fail**

Add `mod tmf;` to `crates/lapidary-cad/src/lib.rs` (after `mod stl;`, keeping order), then:

```sh
cargo test -p lapidary-cad --all-features tmf; echo "exit=$?"
```

Expected: FAIL to compile — `read_capped` and `Caps` do not exist.

- [ ] **Step 4: Implement**

At the top of `crates/lapidary-cad/src/tmf.rs`:

```rust
//! 3MF: a mesh in XML inside an OPC package, which is a ZIP.
//!
//! Unlike `stl.rs` and `obj.rs` this reader is not hand-rolled. The container is a
//! security boundary — zip64, data descriptors, local-versus-central header mismatch —
//! and a bug here is a vulnerability rather than a wrong mesh. See spec §3.2.

use crate::kernel::CadError;
use std::io::Read;

pub(crate) const FORMAT: &str = "3MF";

/// Limits on what an archive may expand to. `DATA.md` §5.4 requires all three.
///
/// A struct rather than three constants so tests can inject small values: proving the
/// 2 GiB cap fires would otherwise need a 2 GiB fixture in the repository. The mechanism
/// is the part that can break, and small caps exercise the same mechanism.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Caps {
    pub(crate) max_decompressed: u64,
    pub(crate) max_entries: usize,
    pub(crate) max_ratio: u64,
}

impl Caps {
    /// Sized in spec §3.4 against `DATA.md`'s 1 MB – 2 GB source range. Compiled in and
    /// deliberately not configurable: a security control with an environment override is
    /// one an operator can switch off by accident.
    pub(crate) const DEFAULT: Caps = Caps {
        max_decompressed: 2 << 30,
        max_entries: 1024,
        max_ratio: 200,
    };
}

/// Read at most `cap` bytes, refusing rather than allocating when the stream is longer.
///
/// `DATA.md` §5.4 says to abort "on breach, not after", and that is this function's whole
/// reason to exist: `Read::take(cap + 1)` means a hostile entry costs `cap + 1` bytes of
/// memory regardless of what it claims to expand to. Reading first and measuring second
/// is the bomb working exactly as designed.
pub(crate) fn read_capped<R: Read>(reader: R, cap: u64) -> Result<Vec<u8>, CadError> {
    let mut out = Vec::new();
    // cap + 1: reading exactly `cap` cannot distinguish "ended at the cap" from
    // "continues past it", and a file of exactly the documented maximum size is legal.
    reader
        .take(cap.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|source| CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("the archive could not be read: {source}"),
        })?;
    if out.len() as u64 > cap {
        return Err(CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("one entry expands past the {cap}-byte limit"),
        });
    }
    Ok(out)
}
```

- [ ] **Step 5: Run**

```sh
cargo test -p lapidary-cad --all-features; echo "exit=$?"
```

Expected: 4 new tests pass. Workspace total 343.

- [ ] **Step 6: Verify the mutation bites**

Replace the body of `read_capped` with the naive version (note `mut reader: R` — without
`.take()` the receiver must be mutable):

```rust
    let mut out = Vec::new();
    reader.read_to_end(&mut out).map_err(|source| CadError::ArchiveRefused {
        format: FORMAT.to_owned(),
        detail: format!("the archive could not be read: {source}"),
    })?;
    if out.len() as u64 > cap { /* … same error … */ }
```

```sh
cargo test -p lapidary-cad --all-features reading_stops_at_the_cap; echo "exit=$?"
```

Expected: a normal FAILED, on the byte-count assertion — "pulled 65536 bytes for a
1024-byte cap". The error assertion still passes, which is the point: both versions
refuse the stream, and only the byte count distinguishes aborting *during* from aborting
*after*.

**Do not "improve" this test by making the source infinite.** An earlier draft of this
plan did exactly that, and the naive implementation then allocated until the kernel's OOM
killer fired — 13 GB on a 15 GB machine, twice, taking the editor down with it. A hang
that eats all memory is not a test failure. **Revert byte-identically** afterwards.

- [ ] **Step 7: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): cap archive reads as the bytes arrive"
```

---

## Task 4: The archive — entry count, traversal, and finding the model

**Files:**
- Modify: `crates/lapidary-cad/src/tmf.rs`

**Read first:** spec §3.5 and §3.6.

**Use `attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)`, not `unescape_value()`.**
quick-xml 0.41 deprecates the latter, and this workspace builds clippy with `-D warnings`,
so the deprecation is an error. They are the same call: `unescape_value` is
`normalized_value_with(XmlVersion::Implicit1_0, 1, resolve_predefined_entity)` and
`normalized_value(v)` is `normalized_value_with(v, 1, resolve_predefined_entity)` —
verified against the 0.41 source, same version, same depth, same resolver.

**Interfaces:**
- Consumes: `Caps`, `read_capped`, `CadError::ArchiveRefused` (task 3).
- Produces: `open_archive(bytes: &[u8], caps: &Caps) -> Result<Archive<'_>, CadError>` where
  `type Archive<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>`,
  `entry(archive: &mut Archive<'_>, name: &str, caps: &Caps) -> Result<Vec<u8>, CadError>`,
  and `model_part_name(rels_xml: &[u8]) -> Result<String, CadError>`.

- [ ] **Step 1: Write the failing tests**

Add to `tmf.rs`'s `mod tests`. The hostile archives are built in memory with `ZipWriter` —
the `deflate` feature includes the write path, so no hostile fixture is committed:

```rust
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    /// Builds a ZIP in memory. `(name, contents)` pairs, deflated.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            w.start_file(*name, opts).expect("start");
            w.write_all(body).expect("write");
        }
        w.finish().expect("finish").into_inner()
    }

    fn tiny_caps() -> Caps {
        Caps { max_decompressed: 4096, max_entries: 4, max_ratio: 20 }
    }

    #[test]
    fn too_many_entries_is_refused_before_anything_is_read() {
        let many: Vec<(String, Vec<u8>)> =
            (0..9).map(|i| (format!("f{i}.txt"), b"x".to_vec())).collect();
        let refs: Vec<(&str, &[u8])> =
            many.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let err = open_archive(&zip_of(&refs), &tiny_caps()).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn an_entry_past_the_size_cap_is_refused() {
        let big = vec![b'A'; 8192];
        let bytes = zip_of(&[("3D/3dmodel.model", &big)]);
        let mut a = open_archive(&bytes, &tiny_caps()).expect("opens");
        let err = entry(&mut a, "3D/3dmodel.model", &tiny_caps()).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn an_entry_past_the_ratio_cap_is_refused() {
        // 4000 zero bytes deflate to far less than 4000/20, so this breaches the ratio
        // while staying inside max_decompressed -- the two caps are independent and this
        // proves the ratio one fires on its own.
        let squishy = vec![0u8; 4000];
        let bytes = zip_of(&[("3D/3dmodel.model", &squishy)]);
        let caps = Caps { max_decompressed: 1 << 20, max_entries: 4, max_ratio: 20 };
        let mut a = open_archive(&bytes, &caps).expect("opens");
        let err = entry(&mut a, "3D/3dmodel.model", &caps).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn a_traversing_entry_name_is_rejected() {
        // Defence in depth: nothing here extracts to disk, so this is not a live vector
        // in this design. See spec §3.5 -- the rule should not depend on that staying so.
        let bytes = zip_of(&[("../../etc/passwd", b"root:x:0:0")]);
        let err = open_archive(&bytes, &Caps::DEFAULT).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
    }

    #[test]
    fn the_model_part_is_found_through_the_relationships() {
        // Deliberately NOT the conventional 3D/3dmodel.model path: reading that directly
        // would pass a test that used it, and fail on a legal file that moved it.
        let rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/carrier.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
        assert_eq!(model_part_name(rels).expect("resolves"), "3D/carrier.model");
    }

    #[test]
    fn a_package_with_no_model_relationship_says_so() {
        let rels = br#"<Relationships><Relationship Id="r" Target="/docProps/thumbnail.png" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"/></Relationships>"#;
        let err = model_part_name(rels).expect_err("must fail");
        assert!(matches!(err, CadError::MalformedMesh { .. }), "{err}");
    }
```

- [ ] **Step 2: Run and watch them fail**

```sh
cargo test -p lapidary-cad --all-features tmf; echo "exit=$?"
```

Expected: FAIL to compile — `open_archive`, `entry`, `model_part_name` do not exist.

- [ ] **Step 3: Implement**

```rust
pub(crate) type Archive<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

/// The 3MF core specification's relationship type for the model part.
const MODEL_REL_TYPE: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";

fn refused(detail: String) -> CadError {
    CadError::ArchiveRefused { format: FORMAT.to_owned(), detail }
}

fn malformed(detail: String) -> CadError {
    CadError::MalformedMesh { format: FORMAT.to_owned(), detail }
}

pub(crate) fn open_archive<'a>(bytes: &'a [u8], caps: &Caps) -> Result<Archive<'a>, CadError> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|source| malformed(format!("it is not a readable ZIP package: {source}")))?;
    if archive.len() > caps.max_entries {
        return Err(refused(format!(
            "it holds {} entries, past the {} allowed",
            archive.len(),
            caps.max_entries
        )));
    }
    // Names are checked once, up front, so no later lookup can reach a rejected one.
    for name in archive.file_names() {
        if is_unsafe_name(name) {
            return Err(refused(format!("it holds an unsafe entry path: {name}")));
        }
    }
    Ok(archive)
}

/// Absolute paths and `..` segments. Defence in depth here — spec §3.5 — because nothing
/// in this module writes an extracted file anywhere.
fn is_unsafe_name(name: &str) -> bool {
    name.starts_with('/')
        || name.starts_with('\\')
        || name.contains(':')
        || std::path::Path::new(name)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

pub(crate) fn entry(
    archive: &mut Archive<'_>,
    name: &str,
    caps: &Caps,
) -> Result<Vec<u8>, CadError> {
    let file = archive
        .by_name(name)
        .map_err(|_| malformed(format!("the package has no {name} part")))?;
    let compressed = file.compressed_size().max(1);
    // The ratio bound and the absolute bound, whichever is tighter. Ratio catches the
    // classic bomb: a few kilobytes claiming to be gigabytes.
    let cap = caps
        .max_decompressed
        .min(compressed.saturating_mul(caps.max_ratio));
    read_capped(file, cap)
}

/// The StartPart target from `_rels/.rels`, normalised to an archive entry name.
///
/// Spec §3.6: read the relationships rather than assuming `3D/3dmodel.model`. The cost is
/// one small parse; the benefit is that a legal package that moved its model still opens.
pub(crate) fn model_part_name(rels_xml: &[u8]) -> Result<String, CadError> {
    let mut reader = quick_xml::Reader::from_reader(rels_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut target = None;
                    let mut is_model = false;
                    for attr in e.attributes().flatten() {
                        let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap_or_default()
                            .into_owned();
                        match attr.key.local_name().as_ref() {
                            b"Target" => target = Some(value),
                            b"Type" => is_model = value == MODEL_REL_TYPE,
                            _ => {}
                        }
                    }
                    if is_model {
                        let t = target.ok_or_else(|| {
                            malformed("its model relationship names no target".to_owned())
                        })?;
                        // Relationship targets are package-absolute (`/3D/x.model`); ZIP
                        // entry names are not.
                        return Ok(t.trim_start_matches('/').to_owned());
                    }
                }
            }
            Err(source) => return Err(malformed(format!("its relationships are not valid XML: {source}"))),
            _ => {}
        }
        buf.clear();
    }
    Err(malformed(
        "it declares no 3D model relationship, so there is nothing to read".to_owned(),
    ))
}
```

- [ ] **Step 4: Run**

```sh
cargo test -p lapidary-cad --all-features; echo "exit=$?"
```

Expected: 6 new tests pass. Workspace total 349.

- [ ] **Step 5: Verify the mutation bites**

Replace `model_part_name`'s body with the convention:

```rust
    Ok("3D/3dmodel.model".to_owned())
```

Run `cargo test -p lapidary-cad --all-features model_part; echo "exit=$?"`.
Expected: `the_model_part_is_found_through_the_relationships` FAILS — it gets
`3D/3dmodel.model` where `3D/carrier.model` was declared — and
`a_package_with_no_model_relationship_says_so` FAILS too, because a package with no model
now silently claims to have one. **Revert byte-identically.**

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): open a 3MF package and find its model part"
```

---

## Task 5: The model XML — units, vertices, triangles

**Files:**
- Modify: `crates/lapidary-cad/src/tmf.rs`

**Read first:** spec §3.3.

**Interfaces:**
- Consumes: everything from task 4.
- Produces: `unit_scale(unit: Option<&str>) -> Result<f64, CadError>`, and an internal
  `Object { vertices: Vec<[f64; 3]>, triangles: Vec<[usize; 3]>, components: Vec<(String, [f64; 12])> }`
  keyed by id in a `BTreeMap<String, Object>`.

Vertices are `f64` here and only become `f32` at the end: a micron-unit file scaled by
0.001, or an inch file by 25.4, loses precision if the multiply happens in `f32`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn every_unit_scales_to_millimetres() {
        assert_eq!(unit_scale(Some("millimeter")).expect("mm"), 1.0);
        assert_eq!(unit_scale(Some("micron")).expect("um"), 0.001);
        assert_eq!(unit_scale(Some("centimeter")).expect("cm"), 10.0);
        assert_eq!(unit_scale(Some("inch")).expect("in"), 25.4);
        assert_eq!(unit_scale(Some("foot")).expect("ft"), 304.8);
        assert_eq!(unit_scale(Some("meter")).expect("m"), 1000.0);
    }

    #[test]
    fn an_absent_unit_is_millimetres() {
        // The 3MF core specification's default. Not a guess.
        assert_eq!(unit_scale(None).expect("default"), 1.0);
    }

    #[test]
    fn an_unrecognised_unit_is_refused_and_named() {
        // Defaulting here would scale an `inch` file by 25.4 and produce measurements
        // that are wrong, plausible and silent. CLAUDE.md: measurement must not lie.
        let err = unit_scale(Some("furlong")).expect_err("must fail");
        assert!(err.to_string().contains("furlong"), "{err}");
    }

    #[test]
    fn a_single_object_parses_with_its_unit_applied() {
        let mesh = parse_3mf(&package(r#"<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="1" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="2" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles>
</mesh></object></resources>
<build><item objectid="1"/></build>
</model>"#)).expect("parses");
        // 1 cm -> 10 mm, 2 cm -> 20 mm. Asserting coordinates, not just that it parsed:
        // a parser that ignored the unit would still return one triangle.
        assert_eq!(mesh.triangles, vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 20.0, 0.0]]]);
    }

    #[test]
    fn a_model_with_no_triangles_fails_through_the_shared_gate() {
        let err = parse_3mf(&package(r#"<model unit="millimeter"><resources><object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/></vertices><triangles/></mesh></object></resources>
<build><item objectid="1"/></build></model>"#)).expect_err("must fail");
        let CadError::MalformedMesh { format, detail } = err else { panic!("wrong variant") };
        assert_eq!(format, "3MF");
        assert!(detail.contains("no triangles"), "{detail}");
    }

    #[test]
    fn a_triangle_naming_a_missing_vertex_is_rejected() {
        let err = parse_3mf(&package(r#"<model><resources><object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/></vertices>
<triangles><triangle v1="0" v2="7" v3="9"/></triangles></mesh></object></resources>
<build><item objectid="1"/></build></model>"#)).expect_err("must fail");
        assert!(err.to_string().contains("vertex"), "{err}");
    }
```

And the helper that wraps a model XML into a minimal package:

```rust
    /// A minimal but real OPC package around one model part, at a deliberately
    /// unconventional path so every test also exercises §3.6's relationship lookup.
    fn package(model_xml: &str) -> Vec<u8> {
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/carrier.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
        zip_of(&[("_rels/.rels", rels), ("3D/carrier.model", model_xml.as_bytes())])
    }
```

- [ ] **Step 2: Run and watch them fail**

```sh
cargo test -p lapidary-cad --all-features tmf; echo "exit=$?"
```

Expected: FAIL to compile — `unit_scale` and `parse_3mf` do not exist.

- [ ] **Step 3: Implement**

```rust
/// Millimetres per unit of the file's declared unit.
///
/// An absent attribute is millimetres, which the 3MF core specification states. An
/// unrecognised one is refused rather than defaulted — spec §3.3.
pub(crate) fn unit_scale(unit: Option<&str>) -> Result<f64, CadError> {
    match unit.unwrap_or("millimeter") {
        "micron" => Ok(0.001),
        "millimeter" => Ok(1.0),
        "centimeter" => Ok(10.0),
        "inch" => Ok(25.4),
        "foot" => Ok(304.8),
        "meter" => Ok(1000.0),
        other => Err(malformed(format!(
            "it declares the unit {other}, and only micron, millimeter, centimeter, inch, \
             foot and meter are understood"
        ))),
    }
}

#[derive(Default)]
struct Object {
    vertices: Vec<[f64; 3]>,
    triangles: Vec<[usize; 3]>,
    components: Vec<(String, [f64; 12])>,
}

pub fn parse_3mf(bytes: &[u8]) -> Result<Mesh, CadError> {
    let caps = Caps::DEFAULT;
    let mut archive = open_archive(bytes, &caps)?;
    let rels = entry(&mut archive, "_rels/.rels", &caps)?;
    let model_name = model_part_name(&rels)?;
    let model = entry(&mut archive, &model_name, &caps)?;
    let (objects, build, scale) = read_model(&model)?;
    let mut triangles = Vec::new();
    for (id, transform) in &build {
        emit(&objects, id, *transform, scale, 0, &mut triangles)?;
    }
    finish(FORMAT, triangles)
}
```

`read_model` streams the XML. Attribute reading is factored so the same three lines are
not repeated for vertices and triangles:

```rust
fn attr(e: &quick_xml::events::BytesStart<'_>, want: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == want)
        .map(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .unwrap_or_default()
                .into_owned()
        })
}

fn number(e: &quick_xml::events::BytesStart<'_>, want: &[u8]) -> Result<f64, CadError> {
    let raw = attr(e, want)
        .ok_or_else(|| malformed(format!("a {} attribute is missing", String::from_utf8_lossy(want))))?;
    let value: f64 = raw
        .parse()
        .map_err(|_| malformed(format!("{raw:?} is not a number")))?;
    if !value.is_finite() {
        return Err(malformed(format!("{raw:?} is not a finite number")));
    }
    Ok(value)
}

fn index(e: &quick_xml::events::BytesStart<'_>, want: &[u8], count: usize) -> Result<usize, CadError> {
    let raw = attr(e, want)
        .ok_or_else(|| malformed("a triangle is missing a vertex reference".to_owned()))?;
    let i: usize = raw
        .parse()
        .map_err(|_| malformed(format!("{raw:?} is not a vertex reference")))?;
    if i >= count {
        return Err(malformed(format!(
            "a triangle names vertex {i}, but its object has defined {count}"
        )));
    }
    Ok(i)
}

type Model = (std::collections::BTreeMap<String, Object>, Vec<(String, [f64; 12])>, f64);

fn read_model(xml: &[u8]) -> Result<Model, CadError> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut objects: std::collections::BTreeMap<String, Object> = Default::default();
    let mut build = Vec::new();
    let mut scale = 1.0;
    let mut current: Option<String> = None;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|source| malformed(format!("its model XML is not valid: {source}")))?;
        match &event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                match e.local_name().as_ref() {
                    b"model" => scale = unit_scale(attr(e, b"unit").as_deref())?,
                    b"object" => {
                        let id = attr(e, b"id")
                            .ok_or_else(|| malformed("an object has no id".to_owned()))?;
                        objects.entry(id.clone()).or_default();
                        current = Some(id);
                    }
                    b"vertex" => {
                        if let Some(o) = current.as_ref().and_then(|id| objects.get_mut(id)) {
                            o.vertices.push([
                                number(e, b"x")?,
                                number(e, b"y")?,
                                number(e, b"z")?,
                            ]);
                        }
                    }
                    b"triangle" => {
                        if let Some(o) = current.as_ref().and_then(|id| objects.get_mut(id)) {
                            let n = o.vertices.len();
                            let t = [index(e, b"v1", n)?, index(e, b"v2", n)?, index(e, b"v3", n)?];
                            o.triangles.push(t);
                        }
                    }
                    b"component" => {
                        if let Some(id) = current.clone() {
                            let target = attr(e, b"objectid").ok_or_else(|| {
                                malformed("a component names no object".to_owned())
                            })?;
                            let m = matrix(attr(e, b"transform").as_deref())?;
                            if let Some(o) = objects.get_mut(&id) {
                                o.components.push((target, m));
                            }
                        }
                    }
                    b"item" => {
                        let target = attr(e, b"objectid")
                            .ok_or_else(|| malformed("a build item names no object".to_owned()))?;
                        build.push((target, matrix(attr(e, b"transform").as_deref())?));
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::End(e) if e.local_name().as_ref() == b"object" => {
                current = None;
            }
            _ => {}
        }
        buf.clear();
    }
    Ok((objects, build, scale))
}
```

`matrix` and `emit` arrive in task 6; for this task, stub them so the file compiles and the
single-object tests pass:

```rust
/// Task 6 replaces this with real transform parsing.
fn matrix(_raw: Option<&str>) -> Result<[f64; 12], CadError> {
    Ok(IDENTITY)
}

const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// Task 6 adds component recursion and the depth cap.
fn emit(
    objects: &std::collections::BTreeMap<String, Object>,
    id: &str,
    _transform: [f64; 12],
    scale: f64,
    _depth: u32,
    out: &mut Vec<[[f32; 3]; 3]>,
) -> Result<(), CadError> {
    let object = objects
        .get(id)
        .ok_or_else(|| malformed(format!("a build item names object {id}, which does not exist")))?;
    for t in &object.triangles {
        let corner = |i: usize| {
            let v = object.vertices[i];
            [(v[0] * scale) as f32, (v[1] * scale) as f32, (v[2] * scale) as f32]
        };
        out.push([corner(t[0]), corner(t[1]), corner(t[2])]);
    }
    Ok(())
}
```

Add `use crate::stl::{finish, Mesh};` to the imports.

- [ ] **Step 4: Run**

```sh
cargo test -p lapidary-cad --all-features; echo "exit=$?"
```

Expected: 6 new tests pass. Workspace total 355.

- [ ] **Step 5: Verify the mutation bites**

Make `unit_scale` ignore the file and always return millimetres:

```rust
pub(crate) fn unit_scale(_unit: Option<&str>) -> Result<f64, CadError> {
    Ok(1.0)
}
```

Run `cargo test -p lapidary-cad --all-features tmf; echo "exit=$?"`.
Expected: `every_unit_scales_to_millimetres`, `an_unrecognised_unit_is_refused_and_named`
and — the one that matters — `a_single_object_parses_with_its_unit_applied` all FAIL, the
last on coordinates (`1.0` where `10.0` was expected) rather than on an error. A parser
that ignores units still returns a plausible mesh, which is exactly why that test asserts
numbers. **Revert byte-identically.**

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): read a 3MF model's units, vertices and triangles"
```

---

## Task 6: Build transforms and component recursion

**Files:**
- Modify: `crates/lapidary-cad/src/tmf.rs`

**Read first:** spec §3.1 and §3.8.

**Interfaces:**
- Consumes: `Object`, `emit`, `matrix` (task 5).
- Produces: real `matrix` and recursive `emit`, plus `MAX_DEPTH: u32`.

3MF's `transform` is twelve numbers: a 3×3 rotation-and-scale in the first nine, read
row-major, then a translation in the last three.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_build_items_transform_moves_the_geometry() {
        // Translation only: (10, 20, 30). The triangle is at the origin, so every
        // coordinate must shift by exactly that.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 20 30"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[10.0, 20.0, 30.0], [11.0, 20.0, 30.0], [10.0, 21.0, 30.0]]]
        );
    }

    #[test]
    fn two_build_items_of_one_object_become_one_merged_mesh() {
        // Spec §3.1: one file is one part. Two placements of the same object produce two
        // triangles in one mesh, at different positions.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build>
<item objectid="1"/>
<item objectid="1" transform="1 0 0 0 1 0 0 0 1 100 0 0"/>
</build></model>"#)).expect("parses");
        assert_eq!(mesh.triangles.len(), 2);
        assert_eq!(mesh.triangles[1][0], [100.0, 0.0, 0.0]);
    }

    #[test]
    fn a_component_composes_its_transform_with_the_items() {
        // Object 2 holds object 1 shifted by x+5; the build item shifts object 2 by
        // x+100. The composed result is x+105 -- a parser that applied only one of the
        // two transforms would land on 5 or 100 and this asserts the composition.
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
<object id="2"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object>
</resources>
<build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 100 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(mesh.triangles[0][0], [105.0, 0.0, 0.0]);
    }

    #[test]
    fn a_translation_is_scaled_by_the_unit_too() {
        // The ONE test that distinguishes transform-then-scale from scale-then-transform.
        // A pure scale matrix commutes with the unit scalar and a translation in a
        // millimetre file has scale 1, so neither of the other transform tests can tell
        // the two orderings apart -- both give the same answer. A translation in a
        // centimetre file cannot: correct is (v + t) * 10, wrong is v * 10 + t.
        let mesh = parse_3mf(&package(r#"<model unit="centimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles,
            vec![[[100.0, 0.0, 0.0], [110.0, 0.0, 0.0], [100.0, 10.0, 0.0]]],
            "scaling before transforming would give 10/20/10 -- the translation must be \
             scaled with the geometry, because it is expressed in the same units"
        );
    }

    #[test]
    fn a_components_rotation_composes_in_the_right_order() {
        // The 3x3 half of composition, which
        // `a_component_composes_its_transform_with_the_items` cannot pin: both of its
        // transforms have identity 3x3 blocks, so a transposed product gives the same
        // answer. Here the component rotates 90 degrees about z and the build item scales
        // x by two. Rotating first sends (1,0,0) to (0,1,0), which the scale leaves alone;
        // the other order gives (0,2,0).
        let mesh = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="1" y="0" z="0"/><vertex x="0" y="0" z="0"/><vertex x="0" y="0" z="1"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
<object id="2"><components><component objectid="1" transform="0 1 0 -1 0 0 0 0 1 0 0 0"/></components></object>
</resources>
<build><item objectid="2" transform="2 0 0 0 1 0 0 0 1 0 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(
            mesh.triangles[0][0],
            [0.0, 1.0, 0.0],
            "the component's rotation must apply before the build item's scale"
        );
    }

    #[test]
    fn a_transform_with_too_many_numbers_is_rejected() {
        // `zip` stops at the shorter side, so counting inside the loop accepts thirteen
        // numbers by truncating to twelve while correctly rejecting eleven.
        let err = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 0 0 0 99"/></build></model>"#))
            .expect_err("must fail");
        assert!(err.to_string().contains("13 numbers"), "{err}");
    }

    #[test]
    fn a_component_cycle_terminates_instead_of_hanging() {
        // Object 1 contains object 2 contains object 1. Without a depth cap this
        // recurses until the stack dies.
        let err = parse_3mf(&package(r#"<model unit="millimeter"><resources>
<object id="1"><components><component objectid="2"/></components></object>
<object id="2"><components><component objectid="1"/></components></object>
</resources>
<build><item objectid="1"/></build></model>"#)).expect_err("must fail");
        assert!(err.to_string().contains("nested"), "{err}");
    }

    #[test]
    fn a_scale_in_the_transform_is_applied_with_the_unit() {
        // 2x scale in a centimetre file: 1 -> 2 cm -> 20 mm. Order matters, and getting
        // it backwards still produces a plausible number, so this pins it.
        let mesh = parse_3mf(&package(r#"<model unit="centimeter"><resources>
<object id="1"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
<build><item objectid="1" transform="2 0 0 0 2 0 0 0 2 0 0 0"/></build></model>"#))
            .expect("parses");
        assert_eq!(mesh.triangles[0][1], [20.0, 0.0, 0.0]);
    }
```

- [ ] **Step 2: Run and watch them fail**

```sh
cargo test -p lapidary-cad --all-features tmf; echo "exit=$?"
```

Expected: the transform tests FAIL on coordinates (the stub returns identity), and the
cycle test FAILS or hangs.

- [ ] **Step 3: Implement**

Replace the two stubs:

```rust
/// Deep enough for any real assembly, shallow enough that a cycle ends quickly. A 3MF
/// nested eight levels is already pathological; a 3MF that references itself is hostile.
const MAX_DEPTH: u32 = 8;

/// 3MF's `transform`: nine numbers of a row-major 3×3, then a translation.
fn matrix(raw: Option<&str>) -> Result<[f64; 12], CadError> {
    let Some(raw) = raw else { return Ok(IDENTITY) };
    let mut m = IDENTITY;
    // Counted up front rather than inferred from the loop. `zip` stops at the shorter
    // side, so a loop that counts as it goes rejects a transform with too FEW numbers and
    // silently truncates one with too many -- an asymmetric hole in exactly the input
    // validation CLAUDE.md says never to simplify away, on an untrusted file.
    let count = raw.split_whitespace().count();
    if count != 12 {
        return Err(malformed(format!(
            "a transform has {count} numbers, and a 3MF transform has exactly twelve"
        )));
    }
    for (slot, token) in m.iter_mut().zip(raw.split_whitespace()) {
        *slot = token
            .parse()
            .map_err(|_| malformed(format!("a transform holds {token:?}, which is not a number")))?;
        if !slot.is_finite() {
            return Err(malformed(format!("a transform holds {token:?}, which is not finite")));
        }
    }
    Ok(m)
}

/// `outer` applied after `inner` — the order a component nested in an item needs.
fn compose(outer: [f64; 12], inner: [f64; 12]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = (0..3)
                .map(|k| inner[row * 3 + k] * outer[k * 3 + col])
                .sum();
        }
    }
    for col in 0..3 {
        out[9 + col] = (0..3)
            .map(|k| inner[9 + k] * outer[k * 3 + col])
            .sum::<f64>()
            + outer[9 + col];
    }
    out
}

fn apply(m: [f64; 12], v: [f64; 3]) -> [f64; 3] {
    [
        v[0] * m[0] + v[1] * m[3] + v[2] * m[6] + m[9],
        v[0] * m[1] + v[1] * m[4] + v[2] * m[7] + m[10],
        v[0] * m[2] + v[1] * m[5] + v[2] * m[8] + m[11],
    ]
}

fn emit(
    objects: &std::collections::BTreeMap<String, Object>,
    id: &str,
    transform: [f64; 12],
    scale: f64,
    depth: u32,
    out: &mut Vec<[[f32; 3]; 3]>,
) -> Result<(), CadError> {
    if depth > MAX_DEPTH {
        return Err(malformed(format!(
            "its objects are nested more than {MAX_DEPTH} deep, or reference each other in a cycle"
        )));
    }
    let object = objects
        .get(id)
        .ok_or_else(|| malformed(format!("a build item names object {id}, which does not exist")))?;

    for t in &object.triangles {
        // Transform first in the file's own units, then scale to millimetres: the
        // transform's numbers are expressed in those units too, so scaling first would
        // apply the unit twice to the translation.
        let corner = |i: usize| {
            let v = apply(transform, object.vertices[i]);
            [(v[0] * scale) as f32, (v[1] * scale) as f32, (v[2] * scale) as f32]
        };
        out.push([corner(t[0]), corner(t[1]), corner(t[2])]);
    }

    for (child, child_transform) in &object.components {
        emit(objects, child, compose(transform, *child_transform), scale, depth + 1, out)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run**

```sh
cargo test -p lapidary-cad --all-features; echo "exit=$?"
```

Expected: 5 new tests pass. Workspace total 360.

- [ ] **Step 5: Verify the mutations bite**

Three, run separately.

**Mutation A — ignore the transform.** In `emit`, replace `apply(transform, …)` with
`object.vertices[i]`. Expected: `a_build_items_transform_moves_the_geometry`,
`two_build_items_of_one_object_become_one_merged_mesh`,
`a_component_composes_its_transform_with_the_items` and
`a_translation_is_scaled_by_the_unit_too` FAIL on coordinates.

**Mutation C — scale before transforming.** In `emit`, scale each vertex first and then
apply the matrix. Expected: **only** `a_translation_is_scaled_by_the_unit_too` fails, at
10/20/10 against the expected 100/110/100. Every other transform test still passes, which
is exactly why that test had to be added: a pure scale matrix commutes with the unit
scalar, and a translation in a millimetre file has scale 1, so nothing else can tell the
two orderings apart.

**Mutation B — remove the depth cap.** Delete the `if depth > MAX_DEPTH` block. The
expected result is a fast stack overflow, not a red test.

This is safe **only because the cycle fixture's objects carry `<components>` and no
`<mesh>`**: nothing is pushed to `out`, so the recursion consumes stack (bounded, aborts
in milliseconds) rather than heap. Do not add geometry to those two objects. If you do,
every recursion level appends triangles and the mutation becomes an unbounded allocation
that the kernel's OOM killer ends — which happened twice during task 3 of this slice,
taking the editor down with it. Run it under a timeout anyway:

```sh
timeout 20 cargo test -p lapidary-cad --all-features a_component_cycle; echo "exit=$?"
```

Expected: non-zero — a stack overflow (SIGABRT/SIGSEGV) or exit 124. That is the
confirmation; a cycle has no natural end.

**Revert both byte-identically** and re-run.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-cad
git commit -m "feat(cad): place a 3MF's build items and resolve its components"
```

---

## Task 7: The fixture

**Files:**
- Create: `fixtures/planetary-carrier-lp-3480-02.3mf`

**Read first:** `CLAUDE.md`'s fixture rule, and slice 3's OBJ fixture generator approach.

**Interfaces:**
- Produces: a committed 3MF with two build items.

- [ ] **Step 1: Generate it**

Write the generator to a scratch path — **not** into the repository. `fixtures/` holds
committed artifacts and no generator, which is the existing practice; the 3MF documents
itself through its own XML.

A planetary carrier plate: a disc with three planet-pin bosses, emitted as one object and
placed twice so the merge in spec §3.1 is exercised end to end.

```python
#!/usr/bin/env python3
"""fixtures/planetary-carrier-lp-3480-02.3mf — a carrier plate, two placements."""
import math, zipfile, sys

R, T, N, PIN_R, PIN_H, PCD, SEG = 24.0, 6.0, 3, 3.0, 9.0, 15.0, 48

verts, tris = [], []

def prism(cx, cy, r, z0, z1, seg):
    """A closed cylinder as triangles; returns nothing, appends to verts/tris."""
    base = len(verts)
    for i in range(seg):
        a = 2 * math.pi * i / seg
        verts.append((cx + r * math.cos(a), cy + r * math.sin(a), z0))
        verts.append((cx + r * math.cos(a), cy + r * math.sin(a), z1))
    cb, ct = len(verts), len(verts) + 1
    verts.append((cx, cy, z0)); verts.append((cx, cy, z1))
    for i in range(seg):
        b0, b1 = base + 2 * i, base + 2 * ((i + 1) % seg)
        # .extend, not `tris += [...]`: augmented assignment rebinds the name, so
        # Python treats `tris` as local to this function and the read raises
        # UnboundLocalError. `verts.append` and `tris.append` below are method calls
        # and are fine.
        tris.extend([(b0, b1, b1 + 1), (b0, b1 + 1, b0 + 1)])  # wall
        tris.append((cb, b1, b0))                              # bottom cap
        tris.append((ct, b0 + 1, b1 + 1))                      # top cap

prism(0.0, 0.0, R, 0.0, T, SEG)
for k in range(N):
    a = 2 * math.pi * k / N
    prism(PCD * math.cos(a), PCD * math.sin(a), PIN_R, T, T + PIN_H, 16)

# Watertight check: every directed edge exactly once, every undirected edge twice.
seen = set()
for a, b, c in tris:
    for e in ((a, b), (b, c), (c, a)):
        assert e not in seen, f"duplicate directed edge {e}"
        seen.add(e)
for a, b in seen:
    assert (b, a) in seen, f"edge {(a, b)} has no partner"

model = ['<?xml version="1.0" encoding="UTF-8"?>',
 '<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">',
 ' <resources>', '  <object id="1" type="model">', '   <mesh>', '    <vertices>']
model += ['     <vertex x="%.4f" y="%.4f" z="%.4f"/>' % v for v in verts]
model += ['    </vertices>', '    <triangles>']
model += ['     <triangle v1="%d" v2="%d" v3="%d"/>' % t for t in tris]
model += ['    </triangles>', '   </mesh>', '  </object>', ' </resources>', ' <build>',
 '  <item objectid="1" transform="1 0 0 0 1 0 0 0 1 0 0 0"/>',
 '  <item objectid="1" transform="1 0 0 0 1 0 0 0 1 60 0 0"/>',
 ' </build>', '</model>']
model = "\n".join(model) + "\n"

rels = """<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rel0" Target="/3D/3dmodel.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"""
ctypes = """<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"""
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", ctypes)
    z.writestr("_rels/.rels", rels)
    z.writestr("3D/3dmodel.model", model)

sys.stderr.write("vertices=%d triangles=%d merged=%d\n" % (len(verts), len(tris), 2 * len(tris)))
```

Run it, then confirm the numbers and that it really is two items:

```sh
python3 /tmp/gen3mf.py fixtures/planetary-carrier-lp-3480-02.3mf
python3 -c "import zipfile;z=zipfile.ZipFile('fixtures/planetary-carrier-lp-3480-02.3mf');print(z.namelist());print(z.read('3D/3dmodel.model').decode().count('<item '))"
ls -l fixtures/planetary-carrier-lp-3480-02.3mf
```

Expected: three entries, `2` items, and a file comfortably under 100 KB.

- [ ] **Step 2: Write the test**

```rust
    #[test]
    fn the_real_fixture_parses_with_both_of_its_placements() {
        let bytes = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");
        let mesh = parse_3mf(bytes).expect("the fixture parses");
        // Two build items of the same object, so the merged count is exactly twice the
        // object's own -- the number that goes wrong if the second item is dropped.
        assert_eq!(mesh.triangles.len() % 2, 0);
        let half = mesh.triangles.len() / 2;
        assert_eq!(&mesh.triangles[..half.min(1)], &mesh.triangles[..half.min(1)]);
        // The second placement is offset by x+60, so the overall bounding box is wider
        // than one carrier: 48 mm for one, 108 mm for two.
        let xs: Vec<f32> = mesh.triangles.iter().flatten().map(|v| v[0]).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((width - 108.0).abs() < 0.01, "width {width}");
    }
```

- [ ] **Step 3: Run**

```sh
cargo test -p lapidary-cad --all-features the_real_fixture_parses_with_both; echo "exit=$?"
```

Expected: PASS. Workspace total 361.

- [ ] **Step 4: Verify the mutation bites**

In `parse_3mf`, take only the first build item:

```rust
    for (id, transform) in build.iter().take(1) {
```

Expected: `the_real_fixture_parses_with_both_of_its_placements` FAILS on width — 48 where
108 was expected — rather than on a count, which is what makes it a test of the merge
rather than of the parse. **Revert byte-identically.**

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-cad fixtures
git commit -m "test(cad): add a real 3MF fixture with two build items"
```

---

## Task 8: Wiring — dispatch, the walk, and the version string

**Files:**
- Modify: `crates/lapidary-cad/src/lib.rs`, `crates/lapidary-cad/src/mesh_kernel.rs`,
  `crates/lapidary-ingest/src/scan.rs`

**Read first:** slice 3's spec §3.7 and §3.8, and `mesh_kernel.rs:17`.

**Interfaces:**
- Consumes: `parse_3mf` (tasks 5–7).
- Produces: `.3mf` files reaching `parse_3mf` through the ordinary pipeline.

- [ ] **Step 1: Export and dispatch, and drop the dead-code allow**

`tmf.rs` carries `#![allow(dead_code)]` from task 3. **Remove it in this task** — this is
the task that makes the module reachable, so this is the first point at which the
attribute is no longer load-bearing. Removing it earlier fails `clippy -D warnings`,
because `mod tmf;` is private and nothing outside the module's own tests reaches any of it
until the `pub use` below exists. After removing it, clippy must still exit 0; if anything
is still reported dead, that is a finding, not a reason to put the attribute back.

In `crates/lapidary-cad/src/lib.rs`, beside the other re-exports:

```rust
pub use tmf::parse_3mf;
```

In `crates/lapidary-cad/src/mesh_kernel.rs`, add the arm and the import:

```rust
        "3mf" => parse_3mf(bytes),
```

- [ ] **Step 2: Let the walk admit it**

In `crates/lapidary-ingest/src/scan.rs`:

```rust
pub(crate) const MESH_EXTENSIONS: [&str; 3] = ["stl", "obj", "3mf"];
```

- [ ] **Step 3: Write the tests**

In `mesh_kernel.rs`'s `mod tests`:

```rust
    #[tokio::test]
    async fn a_3mf_file_is_parsed_by_the_3mf_parser() {
        let bytes = include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");
        let out = MeshKernel
            .process(bytes, &params("3mf"))
            .await
            .expect("ingests");
        assert!(out.measurements.triangle_count > 0);
        assert!(!out.thumbnail_webp.is_empty());
    }

    #[tokio::test]
    async fn the_three_formats_report_three_versions() {
        // slice 3 §3.7's correctness rule, now with a third parser: a derivative's
        // kernel_version must say which parser produced it.
        let versions = [
            MeshKernel.version(&params("stl")).version,
            MeshKernel.version(&params("obj")).version,
            MeshKernel.version(&params("3mf")).version,
        ];
        assert_eq!(versions[2], format!("3mf-1+{GLB_VERSION}+{}", crate::RASTER_VERSION));
        let unique: std::collections::BTreeSet<&String> = versions.iter().collect();
        assert_eq!(unique.len(), 3, "each parser needs its own version: {versions:?}");
    }
```

In `scan.rs`'s `mod tests`, extend whichever test enumerates candidates, plus:

```rust
    #[test]
    fn a_3mf_is_a_mesh_candidate_and_a_readme_is_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in ["carrier.3mf", "carrier.3MF", "bracket.stl", "README.md"] {
            std::fs::write(dir.path().join(name), b"x").expect("write");
        }
        let mut found: Vec<String> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter(|e| is_mesh_candidate(&e.path()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, vec!["bracket.stl", "carrier.3MF", "carrier.3mf"]);
    }
```

- [ ] **Step 4: Run**

```sh
cargo test --workspace --all-features; echo "exit=$?"
```

Expected: 364 passed.

- [ ] **Step 5: Verify the mutation bites**

Remove `"3mf"` from `MESH_EXTENSIONS` (back to `[&str; 2]`).
Expected: `a_3mf_is_a_mesh_candidate_and_a_readme_is_not` FAILS — the two 3MF names are
missing from `found`. The kernel tests still pass, which is the point: dispatch and the
walk are separate decisions and each needs its own test. **Revert byte-identically.**

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-cad crates/lapidary-ingest
git commit -m "feat(ingest): scan and dispatch 3MF"
```

---

## Task 9: End to end

**Files:**
- Modify: `crates/lapidary-ingest/tests/handler.rs`

**Read first:** the existing handler tests, especially
`a_real_obj_yields_the_same_with_its_format_recorded`; these follow their shape.

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the tests**

```rust
const CARRIER: &str = "planetary-carrier-lp-3480-02.3mf";
const CARRIER_FIXTURE: &[u8] =
    include_bytes!("../../../fixtures/planetary-carrier-lp-3480-02.3mf");

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_3mf_yields_a_thumbnail_and_three_rungs(pool: PgPool) {
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    assert_eq!(
        handler.handle(&job_for(CARRIER)).await.expect("ingests"),
        Outcome::Ingested
    );

    let kinds: Vec<String> = derivatives(&pool).await.into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds,
        vec!["tessellation_l0", "tessellation_l1", "tessellation_l2", "thumbnail"],
        "a 3MF produces the same four derivatives an STL does"
    );

    let (format, version): (String, String) = sqlx::query_as(
        "SELECT f.format, d.kernel_version FROM file f \
         JOIN derivative d ON d.revision_id = f.revision_id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(format, "3mf");
    assert_eq!(version, "mesh 3mf-1+glb-1+cpu-1");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_3mf_source_blob_is_stored_uncompressed(pool: PgPool) {
    // DATA.md §1.2: 3MF is already a deflate ZIP. Re-compressing it spends CPU on every
    // ingest to make the file very slightly larger.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join(CARRIER), CARRIER_FIXTURE).expect("write fixture");
    std::fs::write(ingest_dir.path().join(BRACKET), BRACKET_FIXTURE).expect("write stl");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());
    handler.handle(&job_for(CARRIER)).await.expect("3mf");
    handler.handle(&job_for(BRACKET)).await.expect("stl");

    let rows: Vec<(String, i64, i64, Option<i16>)> = sqlx::query_as(
        "SELECT f.format, b.size_bytes, b.stored_bytes, b.zstd_level \
         FROM blob b JOIN file f ON f.blake3 = b.blake3 ORDER BY f.format",
    )
    .fetch_all(&pool)
    .await
    .expect("rows");
    let three_mf = rows.iter().find(|r| r.0 == "3mf").expect("the 3mf row");
    assert_eq!(three_mf.1, three_mf.2, "a 3MF is stored at its own size");
    // And the STL beside it still compresses, so this proves a policy rather than a
    // pipeline that stopped compressing everything.
    let stl = rows.iter().find(|r| r.0 == "stl").expect("the stl row");
    assert!(stl.2 < stl.1, "an STL still compresses: {} vs {}", stl.2, stl.1);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_refused_3mf_leaves_no_part_and_no_blob(pool: PgPool) {
    // A ZIP whose model entry expands far past the ratio cap. Built here rather than
    // committed: a fixture that is genuinely hostile is not something to keep in a repo.
    //
    // The relationships part is NOT optional padding. `parse_3mf` reads `_rels/.rels`
    // before it reads the model, so a bomb without one fails on the missing rels part and
    // never touches the cap — the test would still pass, still prove the reap, and
    // silently stop testing the thing it is named for.
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("_rels/.rels", opts).expect("start");
    std::io::Write::write_all(
        &mut w,
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/3dmodel.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#,
    )
    .expect("write");
    w.start_file("3D/3dmodel.model", opts).expect("start");
    std::io::Write::write_all(&mut w, &vec![0u8; 64 * 1024 * 1024]).expect("write");
    let bomb = w.finish().expect("finish").into_inner();

    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join("bomb.3mf"), &bomb).expect("write");
    let handler = handler_over(&pool, ingest_dir.path(), blob_root.path());

    let err = handler
        .handle(&job_for("bomb.3mf"))
        .await
        .expect_err("a refused archive is a permanent failure");
    // Assert on WHICH refusal. Without this the test passes on any error at all, which is
    // how it came to prove the reap while never reaching the cap.
    // `{err:?}`, not `to_string()`: HandlerError derives Debug but not thiserror::Error,
    // so it has no Display impl.
    assert!(
        format!("{err:?}").contains("Refused this 3MF"),
        "expected the archive cap to refuse it, got: {err:?}"
    );
    assert_eq!(part_count(&pool).await, 0);
    assert!(
        all_files(&blob_root.path().join("blobs")).is_empty(),
        "a refused file must leave nothing behind"
    );
}
```

Add `zip.workspace = true` to `crates/lapidary-ingest`'s `[dev-dependencies]` for the last
test.

- [ ] **Step 2: Run**

```sh
cargo test --workspace --all-features; echo "exit=$?"
```

Expected: 367 passed.

- [ ] **Step 3: Verify the mutation bites**

Make `Compression::for_source_format` return `Zstd` for everything (task 2's mutation,
from the other end).
Expected: `a_3mf_source_blob_is_stored_uncompressed` FAILS — `stored_bytes` is smaller than
`size_bytes` where they were required to be equal. The unit test in task 2 catches the
policy; this catches the wiring, and they can break independently. **Revert
byte-identically.**

- [ ] **Step 4: Commit**

```sh
git add crates/lapidary-ingest
git commit -m "test(ingest): pin 3MF end to end, stored as it arrived"
```

---

## Task 10: The exit criterion, measured

**Files:** none until step 6 — this task produces the handoff's numbers.

**Read first:** spec §10.

- [ ] **Step 1: Bring the stack up**

```sh
cd deploy
LAPIDARY_INGEST_DIR=<a directory of real files> docker compose up -d --build
```

Confirm before measuring: migration `0004` applied, `role=api` and `role=worker` in the
logs, and **zero** WARN or ERROR lines on a cold start in both.

- [ ] **Step 2: Scan a mixed directory**

The repository's STL and OBJ fixtures plus the new 3MF. Every part must end with one
`thumbnail` inline and three `tessellation_l*` by hash, whatever its source format:

```sh
docker exec lapidary-db-1 psql -U lapidary -d lapidary -c "
SELECT f.format, d.kind, count(*) FROM derivative d
  JOIN revision r ON r.id = d.revision_id JOIN file f ON f.revision_id = r.id
 GROUP BY f.format, d.kind ORDER BY f.format, d.kind;"
```

- [ ] **Step 3: Check the 3MF's own row**

```sh
docker exec lapidary-db-1 psql -U lapidary -d lapidary -c "
SELECT f.format, b.size_bytes, b.stored_bytes, b.zstd_level, d.kernel_version
  FROM file f JOIN blob b ON b.blake3 = f.blake3
  JOIN derivative d ON d.revision_id = f.revision_id
 WHERE f.format = '3mf' LIMIT 1;"
```

Expected: `stored_bytes = size_bytes`, `zstd_level` null, and
`kernel_version = mesh 3mf-1+glb-1+cpu-1`.

- [ ] **Step 4: Validate a 3MF-derived rung independently**

Slice 3 established this: our own reader shares our reading of the specification, so it
cannot be the only check. Fetch a rung through the route and run it through Khronos'
validator:

```sh
curl -s "http://localhost:8080/api/blob/<hash>" -o /tmp/rung.glb
npx --yes gltf-validator /tmp/rung.glb    # or the node harness slice 3 used
```

Expected: zero errors. A rung from a 3MF is a rung.

- [ ] **Step 5: Measure**

150 real files as slices 2 and 3 used. Record throughput against slice 3's measured
**89.4 files/s** (spec §10 allows 3×) and the warm grid page against `DATA.md` §2.5's
80 ms. Also record a multi-item plate's triangle count and bounding box, to show the merge
held outside the test suite.

- [ ] **Step 6: Write the handoff**

`docs/superpowers/plans/2026-09-04-phase-1-slice-3b-HANDOFF.md`, following slice 3's: what
landed, the measured numbers, what the plan got wrong, which mutations did not bite as
written, and the ledger below.

- [ ] **Step 7: Commit**

```sh
git add docs/superpowers/plans
git commit -m "docs(plan): record what the slice 3b exit run showed"
```

- [ ] **Step 8: Finish the branch**

Announce and use `superpowers:finishing-a-development-branch`. Merge `feat/3mf-ingest` into
`main` with `--no-ff`; do not push.

---

## Ledger items this slice closes or opens

**Closes:** Phase 1's mesh-format line — `ROADMAP.md`'s "Mesh ingest (STL/3MF/OBJ)".

**Opens, with triggers:**

| Item | Trigger |
|---|---|
| Archive ingest — one archive, many parts | The next slice. Needs `Outcome`, `part_name_unique_per_library`, `insert_part_chain` and the batch counts reworked for one job producing N parts |
| The RAR licence decision | That same slice, and it must be made before any code — spec §11 |
| `zopfli` in the tree, unused | A `zip` major that fixes the `deflate-flate2` feature |
| 3MF **writing** for the round-trip | Phase 4 |
| Streaming the parse rather than buffering | Phase 2, for every format at once — a 2 GB STL already buffers today |

## Self-review

**Spec coverage.** §3.1 → tasks 5–7. §3.2 → task 1. §3.3 → task 5. §3.4 → task 3. §3.5 →
task 4. §3.6 → task 4. §3.7 → task 2. §3.8 → task 6. §4 → tasks 5 and 8. §5 → tasks 3–6.
§6 asserts no migration, and no task adds one. §7 → tasks 3, 5, 6. §8 → task 3 for
`ArchiveRefused`, tasks 4–6 for `MalformedMesh`. §9 → the tests in tasks 2–9. §10 → task
10. §11's RAR risk is carried into the ledger above.

**Ordering.** `read_capped` (3) precedes its only caller (4); `Object` and the stubbed
`matrix`/`emit` (5) precede their real versions (6); `parse_3mf` is complete (7) before
anything dispatches to it (8); `Compression` (2) precedes the end-to-end assertion that
depends on it (9). Slice 3's plan shipped two ordering defects that cost a resequence
mid-execution — this section exists because of them.

**Type consistency.** `Caps` fields are `max_decompressed`/`max_entries`/`max_ratio`
throughout. `FORMAT` is `"3MF"` in errors and `"3mf"` in `MESH_EXTENSIONS`,
`for_source_format` and `file.format` — the first is prose shown to a person, the second is
a lowercase key, and `for_source_format` lowercases before matching so both spellings land
on `AsIs`. `emit` keeps the same six-parameter signature between tasks 5 and 6.
