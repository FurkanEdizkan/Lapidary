# Phase 1 slice 4 — derive less, and track use: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ingest stops producing three quarters of what it produces today, and the system
starts recording when a blob was last touched.

**Architecture:** `KernelParams` gains `produce` — a list of what this call must make — and
`KernelOutput.tessellations` becomes a `Vec`. Ingest asks for `[TessellationL0]`, plus
`Thumbnail` when the library wants one. A new `derive` job kind asks for exactly one thing
against one revision, so L1 and L2 arrive when something wants them.

**Tech Stack:** Rust 1.95.0 edition 2024. **No new dependencies.**

**Spec:** `docs/superpowers/specs/2026-09-04-phase-1-slice-4-derivatives-design.md` — read
it first. Every "why" is argued there; this plan is the "how".

**Sequencing:** `docs/superpowers/plans/2026-09-05-phase-1-remaining-slices.md` explains
why `part_image` and every UI-facing route are slice 5's, not this slice's.

## Global Constraints

- **No new dependencies.** If a task feels like it needs one, it is the wrong task.
- **No SQL outside `lapidary-db`.**
- **`lapidary-api` may never depend on `lapidary-cad`**, and `check-deploy` fails if it so
  much as names `SourceStore`. The routes here only enqueue.
- **We never delete user data implicitly.** Migration `0005` removes no row.
- **Measurement must not lie.** A part with no thumbnail still appears in the grid.
- **Errors say what broke and what to do.**
- **No `unwrap()` outside tests**; the workspace lint denies it.
- **`cargo xtask check-strings`** scans new string literals for runs of 3+ spaces. Its
  `EXEMPT` list is pinned by line number and goes stale whenever a line shifts above one —
  expect to re-pin rather than debug it.
- **Commit messages** pass `cargo xtask check-commit-msg`: Conventional Commits, a known
  type, and no AI attribution trailer.
- **When unsure, prefer the boring option.**

## The verification bar

Exactly what CI runs. A task is not done until it passes. **Never pipe these through `tail`
or `grep` when the exit code matters** — a pipeline reports its LAST command's status.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo xtask check-strings
cargo xtask export-bindings      # exit 0 AND web/src/bindings/ unchanged
cargo xtask export-agents-md     # exit 0 AND AGENTS.md unchanged
cargo test --workspace --all-features
cargo deny check
cd web && npm test && npm run typecheck && npm run build
```

Postgres on 55432; the rest of the stack is stopped and not needed until task 10.

```sh
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```

Baseline: **372 Rust tests, 33 web tests.**

## File structure

| File | Responsibility |
|---|---|
| `crates/lapidary-core/src/derivative.rs` | **Create.** `DerivativeKind`. |
| `crates/lapidary-core/src/job.rs` | **Modify.** `JobPayload`, `Outcome::Rendered`, `BatchStatus.rendered`. |
| `crates/lapidary-db/migrations/0005_derive_on_demand.sql` | **Create.** `auto_thumbnail`, the outcome CHECK. |
| `crates/lapidary-db/src/jobs.rs` | **Modify.** Generic `enqueue`, `Rendered`, the NULL-path fix. |
| `crates/lapidary-cad/src/kernel.rs` | **Modify.** `produce`, optional thumbnail, `Vec<Tessellation>`. |
| `crates/lapidary-cad/src/mesh_kernel.rs` | **Modify.** Produce what was asked for. |
| `crates/lapidary-cad/src/cluster.rs` | **Modify.** Delete `ladder()`. |
| `crates/lapidary-db/src/repo.rs` | **Modify.** Optional thumbnail, `upsert_derivative`, `latest_revision`, `revisions_missing`, `touch_blob`. |
| `crates/lapidary-ingest/src/handler.rs` | **Modify.** Rename to `WorkerHandler`, dispatch on kind, the derive arm. |
| `crates/lapidary-api/src/derive.rs` | **Create.** Three enqueue routes. |
| `web/src/routes/index.tsx` | **Modify.** `filesSettled` must count `rendered`. |

---

## Task 1: Core types

**Files:** Create `crates/lapidary-core/src/derivative.rs`; modify
`crates/lapidary-core/src/job.rs`, `crates/lapidary-core/src/lib.rs`

**Read first:** spec §3.6, and **the note below — it corrects a pre-commitment.**

**Interfaces produced:** `DerivativeKind`, `JobPayload`, `JobPayload::{kind, to_json, from_row}`,
`Outcome::Rendered`, `BatchStatus.rendered`.

> **The slice-2 handoff said this becomes a `#[serde(tag = "kind")]` enum. That is wrong and
> would break production.** Every job row written so far carries a bare payload —
> `{"path": "bracket.stl"}` — with no `kind` key, because `kind` is a separate *column*. An
> internally-tagged enum fails on all of them with `missing field 'kind'`; verified against
> the live database. On upgrade the queue would stop draining.
>
> So the **column is the discriminator**, and the payload holds only the variant's fields.
> `to_json` for `IngestFile` emits exactly `{"path": …}` — byte-compatible with every row
> already written — and no migration is needed.

- [ ] **Step 1: `DerivativeKind`**

```rust
//! The four things a kernel call can produce, and the strings `derivative.kind` holds.

/// Also the discriminator a `derive` job carries, which is why it lives here rather than
/// in `lapidary-cad`: the job queue names it and the database stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DerivativeKind {
    Thumbnail,
    TessellationL0,
    TessellationL1,
    TessellationL2,
}

impl DerivativeKind {
    /// Exactly the strings already in `derivative.kind`. Changing one orphans every row
    /// written before the change.
    pub fn as_str(self) -> &'static str {
        match self {
            DerivativeKind::Thumbnail => "thumbnail",
            DerivativeKind::TessellationL0 => "tessellation_l0",
            DerivativeKind::TessellationL1 => "tessellation_l1",
            DerivativeKind::TessellationL2 => "tessellation_l2",
        }
    }
}
```

- [ ] **Step 2: `JobPayload`, dispatching on the column**

```rust
/// What a job carries, without its kind.
///
/// The `job.kind` COLUMN is the discriminator, not a key inside the payload. Every row
/// written before this slice holds a bare `{"path": …}`, so an internally-tagged enum
/// would fail to deserialise all of them and the queue would stop draining on upgrade.
#[derive(Debug, Clone, PartialEq)]
pub enum JobPayload {
    IngestFile { path: String },
    Derive { revision: RevisionId, produce: DerivativeKind },
}

impl JobPayload {
    pub const INGEST_FILE: &'static str = "ingest_file";
    pub const DERIVE: &'static str = "derive";

    pub fn kind(&self) -> &'static str {
        match self {
            JobPayload::IngestFile { .. } => Self::INGEST_FILE,
            JobPayload::Derive { .. } => Self::DERIVE,
        }
    }

    /// The `payload` column's value. `IngestFile` emits exactly what `enqueue_scan` has
    /// always written, so old and new rows are indistinguishable.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            JobPayload::IngestFile { path } => serde_json::json!({ "path": path }),
            JobPayload::Derive { revision, produce } => {
                serde_json::json!({ "revision": revision, "produce": produce })
            }
        }
    }

    /// Rebuild from a row. `kind` comes from the column.
    pub fn from_row(kind: &str, payload: &serde_json::Value) -> Result<Self, CoreError> {
        match kind {
            Self::INGEST_FILE => payload
                .get("path")
                .and_then(|p| p.as_str())
                .map(|path| JobPayload::IngestFile { path: path.to_owned() })
                .ok_or_else(|| CoreError::MalformedJobPayload {
                    kind: kind.to_owned(),
                    detail: "it has no file path".to_owned(),
                }),
            Self::DERIVE => serde_json::from_value(payload.clone()).map_err(|source| {
                CoreError::MalformedJobPayload { kind: kind.to_owned(), detail: source.to_string() }
            }),
            other => Err(CoreError::UnknownJobKind { kind: other.to_owned() }),
        }
    }
}
```

`Derive`'s arm needs a `#[derive(Deserialize)]` helper struct or `Deserialize` on a
`{revision, produce}` shape — either is fine; keep it boring.

- [ ] **Step 2b: The two `CoreError` variants this needs, which do not exist yet**

`CoreError` (`crates/lapidary-core/src/error.rs:5`) has only `BlobHashLength`,
`BlobHashHex` and `IdParse`. Add two, in the style of their neighbours — a full sentence
that tells an operator what to do:

```rust
    #[error(
        "A {kind} job's payload is not the shape that kind requires — {detail}. It was not written by Lapidary; check whether something else is inserting into the job table."
    )]
    MalformedJobPayload { kind: String, detail: String },

    #[error(
        "\"{kind}\" is not a job kind this build knows. A newer Lapidary may have written it; check that every worker and api container is running the same version."
    )]
    UnknownJobKind { kind: String },
```

- [ ] **Step 3: `Outcome::Rendered` and `BatchStatus.rendered`**

Add the variant and the field. `BatchStatus` is `#[serde(rename_all = "camelCase")]` and
ts-rs-exported, so the binding regenerates.

- [ ] **Step 4: Tests**

```rust
#[test]
fn an_existing_ingest_row_still_deserialises() {
    // The exact shape every row in the database holds today: no `kind` key, because the
    // kind is a column. This is the test that fails if someone reaches for
    // #[serde(tag = "kind")].
    let payload = serde_json::json!({ "path": "bracket-lp-1042-03.stl" });
    let got = JobPayload::from_row("ingest_file", &payload).expect("parses");
    assert_eq!(got, JobPayload::IngestFile { path: "bracket-lp-1042-03.stl".to_owned() });
}

#[test]
fn an_ingest_payload_round_trips_byte_identically() {
    let p = JobPayload::IngestFile { path: "x.stl".to_owned() };
    assert_eq!(p.to_json(), serde_json::json!({ "path": "x.stl" }));
    assert_eq!(p.kind(), "ingest_file");
}

#[test]
fn a_derive_payload_carries_a_revision_and_one_kind() {
    let rev = RevisionId::new();
    let p = JobPayload::Derive { revision: rev, produce: DerivativeKind::TessellationL2 };
    assert_eq!(p.kind(), "derive");
    assert_eq!(JobPayload::from_row("derive", &p.to_json()).expect("round trips"), p);
    assert_eq!(p.to_json()["produce"], "tessellation_l2");
}

#[test]
fn an_unknown_kind_names_itself() {
    let err = JobPayload::from_row("polish_the_brass", &serde_json::json!({}))
        .expect_err("must fail");
    assert!(err.to_string().contains("polish_the_brass"), "{err}");
}

#[test]
fn the_kind_strings_match_what_the_database_holds() {
    // These four strings are in `derivative.kind` on every row ever written.
    assert_eq!(DerivativeKind::Thumbnail.as_str(), "thumbnail");
    assert_eq!(DerivativeKind::TessellationL0.as_str(), "tessellation_l0");
    assert_eq!(DerivativeKind::TessellationL1.as_str(), "tessellation_l1");
    assert_eq!(DerivativeKind::TessellationL2.as_str(), "tessellation_l2");
}
```

- [ ] **Step 5: Verify the mutation bites**

Change `to_json`'s `IngestFile` arm to emit `{"kind": "ingest_file", "path": path}` — the
shape the tagged enum would have produced. `an_ingest_payload_round_trips_byte_identically`
fails on the extra key. That is the production break in miniature: new rows would carry a
key old readers do not expect and old rows would lack one new readers require. Revert
byte-identically.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-core
git commit -m "feat(core): give a job a typed payload without breaking old rows"
```

---

## Task 2: Migration `0005`

**Files:** Create `crates/lapidary-db/migrations/0005_derive_on_demand.sql`; modify
`crates/lapidary-db/tests/migrations.rs`

**Read first:** spec §6.

```sql
-- Slice 4. Two facts: a library may decline to render, and a job may report that it
-- rendered something. No table is created and no row is removed -- `part_image` lands in
-- slice 5, beside the routes that write it.

alter table library add column auto_thumbnail boolean not null default true;

-- PostgreSQL cannot modify a CHECK in place, so this is a drop and an add rather than an
-- alteration. The name is reused deliberately: one constraint, one meaning.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in ('ingested', 'skipped', 'rendered'));
```

- [ ] **Tests** — four `#[sqlx::test]` cases:
  - a job with `state='done'` and `outcome='rendered'` is **accepted** (the positive case;
    without it an inverted CHECK passes every negative)
  - `outcome='rendered'` with `state='pending'` is rejected by `job_done_has_outcome`
  - `outcome='polished'` is still rejected
  - **a database carrying pre-existing `tessellation_l1` and `tessellation_l2` rows still
    has them after `0005`** — one `SELECT count(*)`, and it is the "we did not implicitly
    delete anything" claim made testable

- [ ] **Mutation:** drop `'rendered'` from the re-added CHECK. The accept case fails,
  proving the drop-and-add replaced the constraint rather than adding a second beside it.

- [ ] **Commit:** `feat(db): let a library decline to render, and a job report that it did`

---

## Task 3: `PgJobs` — generic enqueue, and a live 500

**Files:** `crates/lapidary-db/src/jobs.rs`, `crates/lapidary-db/tests/jobs.rs`

**Read first:** `jobs.rs:306` — the query below is a bug waiting for this slice's first
failed `derive` job.

- [ ] **Step 1: `enqueue`**

```rust
pub async fn enqueue(&self, library: LibraryId, jobs: &[JobPayload]) -> Result<(BatchId, u32), DbError>
```

Takes the `kind` column value from `JobPayload::kind()` and the payload from `to_json()`,
so the column and the payload cannot disagree by construction — which is why this signature
is better than `enqueue(library, kind, payloads)`. `enqueue_scan` becomes a two-line wrapper
over it; keep it, `scan.rs` and its tests are fine as they are.

- [ ] **Step 2: The NULL-path fix**

`jobs.rs:306` selects `payload->>'path'` into a **non-`Option<String>`**. A `derive`
payload has no `path`, so the first failed derive job makes
`GET /api/libraries/{lib}/jobs/{batch}` return 500 — for the whole batch, including the
scan batches it serves today. Nothing in the type system catches it.

```sql
SELECT COALESCE(j.payload->>'path', p.name, ''), j.last_error, j.attempts
FROM job j
LEFT JOIN revision rv
       ON rv.id = CASE WHEN j.kind = 'derive' THEN (j.payload->>'revision')::uuid END
LEFT JOIN part p ON p.id = rv.part_id
WHERE j.batch_id = $1 AND j.library_id = $2 AND j.state = 'failed'
ORDER BY j.created_at, j.id LIMIT $3
```

Keep `ORDER BY created_at, id` — `jobs.rs:303-305` explains at length why `, id` is
load-bearing. Decode into `Option<String>` regardless, so a row that matches nothing still
returns a value rather than a decode error.

- [ ] **Step 3: `complete`'s match and `batch_status`'s aggregate** gain `Rendered`.

- [ ] **Tests:** enqueue one `Derive`, fail it, read `batch_status` — today this 500s, and
  the test must show a real **part name** in `failed[0].path`. Plus: a mixed batch's `total`
  equals the sum of its per-state counts.

- [ ] **Mutation:** revert the failures query to `payload->>'path'` with a non-`Option`
  decode. The test must fail with a **decode error**, not a wrong string — that is the
  production 500 exactly.

- [ ] **Commit:** `feat(db): enqueue any job kind, and stop a derive failure 500ing a batch`

---

## Task 4: The kernel produces what it was asked for

**Files:** `crates/lapidary-cad/src/{kernel.rs,mesh_kernel.rs,cluster.rs,mock.rs,lib.rs}`,
and a mechanical fix-up in `crates/lapidary-ingest/src/handler.rs`

**Read first:** spec §7.

- [ ] **Step 1: The types**

`KernelParams` gains `produce: Vec<DerivativeKind>` and **loses `#[derive(Default)]`**. A
default `produce` is either "everything", which is silently expensive, or "nothing", which
silently makes no derivatives; removing `Default` turns every construction site into a
compile error that forces the decision. `KernelOutput.thumbnail_webp` becomes
`Option<Vec<u8>>` and `tessellations` becomes `Vec<Tessellation>`.

- [ ] **Step 2: Delete `ladder()`**

And its `lib.rs` re-export. `cluster(mesh, lod)` is already public and is what both callers
want. There is **no `compile_fail` test** guarding `[Tessellation; 3]` — the type was the
guarantee, and the plan that introduced it said so only in prose.

- [ ] **Step 3: `process` loops `params.produce`**

```rust
let mesh = parse(bytes, &params.format)?;
let mut tessellations = Vec::new();
let mut thumbnail_webp = None;
for want in &params.produce {
    match want {
        DerivativeKind::Thumbnail => thumbnail_webp = Some(render_thumbnail(&mesh)?),
        DerivativeKind::TessellationL0 => tessellations.push(cluster(&mesh, Lod::L0)?),
        DerivativeKind::TessellationL1 => tessellations.push(cluster(&mesh, Lod::L1)?),
        DerivativeKind::TessellationL2 => tessellations.push(cluster(&mesh, Lod::L2)?),
    }
}
```

**`version()` keeps `RASTER_VERSION` even on a tessellation-only call.** It names the build,
not the run; making it conditional breaks
`the_reported_version_pins_the_parser_the_writer_and_the_rasterizer`. Write that as a
comment so nobody "fixes" it.

- [ ] **Step 4: Behaviour-preserving fix-up in `handler.rs`**

This is the one collision: deleting `Default` and reshaping the output breaks the handler
immediately. Pass `produce: DerivativeKind::ALL.to_vec()` and
`output.thumbnail_webp.as_deref().unwrap_or(&[])` — one line each, no behaviour change,
workspace green. Task 7 rewrites both.

- [ ] **Tests:** `process` with `produce: vec![TessellationL1]` returns exactly one rung,
  that rung is L1, and `thumbnail_webp` is `None`. Plus the existing tests reshaped for
  `Vec`.

- [ ] **Mutation:** make `process` ignore `produce` and build everything — which is today's
  code. The one-rung test fails. That mutation is the bug this whole slice exists to
  prevent.

- [ ] **Commit:** `feat(cad): produce only what the caller asked for`

---

## Task 5: The write side

**Files:** `crates/lapidary-db/src/repo.rs`, `crates/lapidary-db/tests/repo.rs`

- [ ] `IngestRequest.thumbnail_webp` becomes `Option<&[u8]>`; `insert_part_chain` skips the
  derivative INSERT when `None`. `DerivativeKind::as_str()` replaces the `'thumbnail'`
  literal at `repo.rs:254`.

- [ ] `upsert_derivative`, with the clause that matters:

```sql
ON CONFLICT (revision_id, kind) DO UPDATE
   SET thumb_bytes = excluded.thumb_bytes,
       blake3 = excluded.blake3,
       kernel_version = excluded.kernel_version,
       params_json = excluded.params_json
```

Both columns are set from `excluded` so exactly one stays non-null — without that, an
upsert over a row stored the other way leaves both set and trips
`derivative_storage_is_exclusive`.

- [ ] **Two resolvers, not one** (ruling T5-A). This bullet originally specified a single
  `latest_revision(part) -> Option<(RevisionId, BlobHash, String)>`, and task 7 then read
  "`latest_revision` → `SourceStore::get`". That reintroduces the bug §3.7 forbids: the
  derive payload already *carries* a `RevisionId`, so an arm that re-resolves "latest"
  ignores the revision it was given — enqueue against revision A, a second revision lands,
  the job renders and upserts onto B, silently. Spec §5's data flow is the authority and
  says "latest source **for that revision**".

  - `latest_revision(part) -> Option<RevisionId>` — for task 8's enqueue routes, which need
    to know which revision to name in the payload. **Ordered identically to
    `PgParts::page`'s LATERAL** (`repo.rs:352-353`, `ORDER BY created_at DESC, id DESC`).
  - `revision_source(revision) -> Option<(BlobHash, String)>` — for task 7's derive arm,
    which needs the source hash and format for a revision it already holds. Filter
    `role = 'source'` and pick deterministically (`ORDER BY created_at DESC, id DESC LIMIT 1`):
    `file` has no unique constraint on `(revision_id, role)` — `0002_parts.sql:83-91` has
    only two plain indexes — so "there is exactly one" is today's data, not the schema's
    promise. Say that in a comment.

  Neither returns a field its caller does not use.

- [ ] `revisions_missing(library, kind) -> Vec<RevisionId>` for the sweep.

- [ ] **Tests:** ingest with `thumbnail_webp: None` → the part **still appears** in
  `PgParts::page` with `thumbnail_webp = None`; upsert twice with different bytes → one row,
  second bytes win; `revisions_missing` returns exactly the thumbnail-less revisions.

- [ ] **Mutation:** change `latest_revision`'s ordering to `ASC`. A **two-revision fixture**
  must show the grid and the resolver disagreeing. If nothing fails, the fixture is too
  thin — three mutations in slice 3b needed a sharper fixture before they bit.

- [ ] **Commit:** `feat(db): make the thumbnail optional and derivatives upsertable`

---

## Task 6: `last_accessed_at`

**Files:** `crates/lapidary-db/src/repo.rs`, `crates/lapidary-db/tests/repo.rs`

**Read first:** spec §3.10.

The column has existed since `0002_parts.sql:25` and nothing has ever written it.

- [ ] `touch_blob(hash)` — `UPDATE blob SET last_accessed_at = now() WHERE blake3 = $1`.
  **Fire-and-forget**: a failed touch must never fail the read it accompanies, and it is not
  worth a transaction. Log at debug, not warn — a missed timestamp is not an incident.

- [ ] **The call site is `crates/lapidary-api/src/blob.rs:39`**, after a successful
  `DerivativeStore::get`, and **only** there.

  Not the ingest handler's reads (`handler.rs:112`, `:116`): those are the system writing
  and regenerating, not somebody looking at data, and counting them would mark every blob
  "recently used" the moment a sweep ran — destroying the signal the column exists to carry.

  **Be aware the signal is thin until slice 5.** The grid serves thumbnails inline from
  `bytea` and reads no blob at all, and nothing in the UI calls the blob route yet, so today
  this fires only for a deliberate fetch. It becomes meaningful when slice 5 adds
  `variant=original` download and the detail view. The plumbing lands now because the read
  path is already open here and because a column nobody has ever written is worth nothing at
  the moment you first want it.

- [ ] **Tests:** reading a blob sets `last_accessed_at`; reading it again moves the
  timestamp forward; a blob never read has it `NULL`.

- [ ] **Mutation:** make `touch_blob` a no-op. The first test fails on a `NULL` timestamp.

- [ ] **Commit:** `feat(db): record when a blob was last read`

---

## Task 7: `WorkerHandler`

**Files:** `crates/lapidary-ingest/src/handler.rs` (rename `IngestHandler` →
`WorkerHandler`), new `crates/lapidary-ingest/src/derive.rs`,
`crates/lapidary-ingest/src/lib.rs`, `bin/lapidary-server/src/main.rs`,
`crates/lapidary-ingest/tests/handler.rs`

- [ ] `handle` builds a `JobPayload` with `JobPayload::from_row(&job.kind, &job.payload)`
  and matches. **Unknown kind fails `Permanent` naming the kind** — today it answers any
  unrecognised job with "This job has no file path in its payload", which is simply false.

- [ ] **Ingest arm:** read `library.auto_thumbnail`; `produce` is `[TessellationL0]`, plus
  `Thumbnail` when the flag is set.

- [ ] **Derive arm:** `revision_source(payload.revision)` — **not** `latest_revision`; the
  payload names the revision precisely so that nothing re-resolves it (§3.7, ruling T5-A) —
  → `SourceStore::get(hash, Compression::for_source_format(format))`
  → `process` with a single-element `produce` → `upsert_derivative` → `Outcome::Rendered`.
  It does **not** need `ingest_dir`.

- [ ] **Four existing tests assert the four-kind set you are shrinking, and you must decide
  what each becomes** — they are not a surprise to solve mid-task. In
  `crates/lapidary-ingest/tests/handler.rs`: `a_real_stl_writes_three_tessellation_blobs_and_rows`
  (:521), `a_real_obj_yields_the_same_with_its_format_recorded` (:579),
  `each_rung_is_valid_gltf_and_l0_is_smaller_than_l2` (:678), and
  `a_real_3mf_yields_a_thumbnail_and_three_rungs` (:709). The first, second and fourth
  should assert the *new* truth — one rung, and it is `tessellation_l0`. The third **loses
  its subject entirely**: with only L0 written at ingest there is no L2 to compare against,
  so it moves behind a `Derive` job rather than being trimmed to a tautology. Deleting it
  is not an option; comparing L0 to itself is worse than deleting it.

- [ ] **Tests:** a library with `auto_thumbnail = false` ingests and writes **zero**
  thumbnail rows, and the part still appears in the grid; ingest writes exactly **one**
  tessellation row and it is `tessellation_l0`; a `Derive` job then writes the missing
  thumbnail and returns `Rendered`; an unknown kind fails Permanent with the kind in the
  message.

- [ ] **Mutations, two:** (a) ignore `auto_thumbnail` and always request the thumbnail — the
  `false` test fails; (b) delete the `match` and always take the ingest arm — the `Derive`
  job fails with the *old* message, which is today's bug made visible.

- [ ] **Commit:** `feat(ingest): dispatch on job kind and derive on demand`

---

## Task 8: The three enqueue routes

**Files:** Create `crates/lapidary-api/src/derive.rs` and
`crates/lapidary-api/tests/derive.rs`; modify `crates/lapidary-api/src/lib.rs`

**Read first:** spec §3.5 — these are on `Role::Api` out of necessity, because the browser
cannot reach the worker at all.

- [ ] **`lapidary-db` needs a library write method, and has none.** `repo.rs` has
  `library_holds` and nothing else that touches `library`. Add
  `PgLibraries::set_auto_thumbnail(library, bool) -> Result<bool, DbError>` returning
  whether a row matched, so the route can answer 404 for a library that does not exist
  rather than 200 for a write that hit nothing.

- [ ] `PATCH /api/libraries/{id}` `{ autoThumbnail }` → 200, or 404 when no row matched
- [ ] `POST /api/parts/{id}/thumbnail` → 202 `ScanAccepted`, a batch of one
- [ ] `POST /api/libraries/{id}/thumbnails` → 202, `queued: 0` is a success

All enqueue-only, all reusing `ScanAccepted` rather than inventing a second accepted shape,
so the existing `GET /api/libraries/{lib}/jobs/{batch}` polling works unchanged.

- [ ] **Tests:** each route's shape; the returned `batchId` is immediately readable by the
  existing batch-status route; a sweep over a library where nothing is missing returns
  `queued: 0`; **the routes are absent under `Role::Worker`**.

- [ ] **Mutation:** mount them in the `Role::Worker` arm. The absence test fails — and that
  mutation encodes the whole finding: on the worker they are unreachable from the browser.

- [ ] **Commit:** `feat(api): trigger derivation without a terminal`

---

## Task 9: Bindings and the frontend

**Files:** `web/src/bindings/*` (regenerated), `web/src/lib/strings.ts`,
`web/src/routes/index.tsx`, `web/src/routes/index.test.tsx`

**The bug this task exists to prevent** is at `index.tsx:41-43`:

```ts
function filesSettled(status: BatchStatus): number {
  return status.ingested + status.skipped + status.failedTotal
}
```

Add `Outcome::Rendered` and forget this, and a sweep leaves `settled === 0` **forever**, so
the `useEffect` at `index.tsx:66-72` never fires and **the grid never refetches** — every
job succeeds and the cards stay blank until a manual reload. No backend test can see it.

- [ ] Regenerate bindings and commit them **in this task**, so `export-bindings` leaves the
  tree clean.
- [ ] `filesSettled` counts `rendered`.
- [ ] **Test:** a sweep batch whose jobs all return `rendered` triggers exactly one
  `['parts']` invalidation.
- [ ] **Mutation:** drop `rendered` from `filesSettled`. The refetch assertion fails.
- [ ] `web/src/no-bare-strings.test.ts` scans every `.tsx` for literals not routed through
  `strings.ts` — any new UI text goes there.

- [ ] **Commit:** `fix(web): count rendered jobs as settled`

---

## Task 10: Docs, and the exit run

**Files:** `docs/DATA.md`, `docs/ROADMAP.md`, `docs/FEATURES.md`, and the handoff

- [ ] Amend the rules this slice reverses, each with the measured numbers:
  - `DATA.md:112` "LOD ladder — all generated at ingest" and `:121-122`'s ruling — rewrite
    in place, do not silently amend the heading
  - `DATA.md:118-119` L1/L2 listed as ingest products
  - `ROADMAP.md:32` "→ thumbnail + L0/L1/L2"
  - `ROADMAP.md:38-39` "**Exit:** … all thumbnails land" — false for a library with
    rendering off, and it is a *phase exit criterion*, so reword rather than footnote
  - `FEATURES.md:30` "Thumbnails inline from Postgres `bytea`" as unconditional

- [ ] **The exit run.** Bring the stack up, ingest the corpus into a library with
  `auto_thumbnail = false`, and record: derivative bytes before and after (target: ~100 MB
  → under 10 MB per 151 parts), ingest throughput against slice 3b's 42.8 files/s (it should
  **improve** — two clustering passes, two glTF writes and a render per file are gone), and
  peak worker RSS against the 2 GiB ceiling.

- [ ] Then the sweep: `POST /api/libraries/{id}/thumbnails`, poll to `finishedAt`, confirm
  `rendered` equals the part count and **the grid refetches without a manual reload**.

- [ ] An on-demand L2 must be **byte-identical** to one built at ingest. Build one each way
  and compare hashes — a lazily-built rung that differs from an eager one is the drift spec
  §11 names.

- [ ] Write the handoff, then use `superpowers:finishing-a-development-branch`.

---

## Self-review

**Spec coverage.** §3.1 → tasks 4, 7. §3.2 → tasks 2, 7. §3.5 → task 8. §3.6 → tasks 1, 3.
§3.7 → task 5's `latest_revision`. §3.8 → nothing, deliberately. §3.9 → task 2's
survival test. §3.10 → task 6. §6 → task 2. §7 → tasks 1, 4. §9 → the tests throughout.
§10 → task 10. §3.3/§3.4 are slice 5's.

**Ordering.** `DerivativeKind` (1) precedes everything that names it; `JobPayload` (1)
precedes `enqueue` (3) and the dispatch (7); the kernel reshape (4) carries its own
behaviour-preserving handler fix-up so the tree stays green until task 7 rewrites it;
`upsert_derivative` and `latest_revision` (5) precede the derive arm (6, 7) that calls them;
`Outcome::Rendered` (1) precedes the frontend that counts it (9).

**Type consistency.** `DerivativeKind` is the discriminator in `JobPayload::Derive`, the
key of `upsert_derivative`'s conflict target, and the value of `derivative.kind` — one type,
three uses, no mapping between them. `produce` is `Vec<DerivativeKind>` in `KernelParams`
and a single value in a `Derive` payload; the handler wraps it in a one-element vector, and
that asymmetry is deliberate: ingest asks for two things at once, a job asks for one.

**What this plan does differently.** Nine of slice 3b's findings were defects in its plan
rather than its execution, and five were tests that could not fail for the reason their
names gave. Every assertion above names a specific number or string. The `JobPayload`
design was corrected by testing a tagged enum against a real row from the database before
writing the task, not after.
