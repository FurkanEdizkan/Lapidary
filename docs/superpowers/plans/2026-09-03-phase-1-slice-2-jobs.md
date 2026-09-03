# Phase 1 slice 2 — job queue and leased worker: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the synchronous scan into durable per-file jobs drained by a leased worker,
so killing the worker mid-scan loses only the files actually in flight.

**Architecture:** One job row per file, grouped by a `batch_id` that is a column rather
than an entity. Workers lease with `FOR UPDATE SKIP LOCKED`; reclamation of expired leases
is folded into the dequeue itself, so there is no reaper process. `LISTEN`/`NOTIFY` wakes
the loop early over a 5-second polling floor that is the actual correctness mechanism.
`lapidary-jobs` (L2) owns delivery and policy behind a `JobHandler` trait and never learns
what a mesh is; `lapidary-ingest` (L3) implements that trait with slice 1's per-file body.

**Tech Stack:** Rust 1.95.0 edition 2024, axum 0.8.9 (library returning `Router`),
sqlx 0.9.0 against PostgreSQL 18, jiff for timestamps, ts-rs 12.0.1, React + TanStack
Query.

**Spec:** `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md` — read it
first. Every "why" below is argued there; this plan is the "how".

## Global Constraints

Copied from `CLAUDE.md` and the spec. Every task's requirements implicitly include this
section.

- **No SQL outside `lapidary-db`.** Everything goes through repository types. The queue's
  SQL is no exception: `lapidary-jobs` contains not one string of it.
- **Layering, CI-enforced by `cargo xtask check-layers`:** L2 crates may depend on L0 and
  L1, never on each other or on L3. `lapidary-jobs` is L2, so it may **not** depend on
  `lapidary-cad` (L2) or `lapidary-ingest` (L3). This is why `JobHandler` exists.
- **We never delete user data implicitly.** Nothing in this slice deletes a job row.
- **Errors say what broke and what to do.** "Could not read this STL — it declares 12
  facets but the file ends after 7. Re-export from your CAD tool and retry." Not
  "parse failed (3)".
- **Rust:** `thiserror` in libraries, `anyhow` at binary edges. **No `unwrap()` outside
  tests.**
- **Generated columns are explicitly `STORED`.** (No new ones here, but do not regress.)
- **Content addressing is not authorization.** Applies to job ids too: the status route is
  scoped under its library and verifies the batch belongs to it.
- **No bare user-facing strings in components.** Every web string goes through
  `web/src/lib/strings.ts`. Enforced by `cargo xtask check-strings` and
  `web/src/no-bare-strings.test.ts`.
- **Real content in fixtures.** `bracket-lp-1042-03.stl`, `spacer-lp-2001-00.stl`. Never
  "Part 1 / Part 2".
- **Frontend:** dark only. Motion 120/180/280 ms, `cubic-bezier(0.2, 0, 0, 1)`, transform
  and opacity only, respect `prefers-reduced-motion`.
- **Pin everything.** `Cargo.lock` is committed; add no dependency that is not already in
  `deny.toml`'s allow-list.
- **When unsure, prefer the boring option.**

## The verification bar

This is exactly what `.github/workflows/ci.yml` runs. A task is not done until it passes.
**Never pipe these through `tail` or `grep` when the exit code matters** — that mistake was
made twice during slice 1 and reported success both times. Use `; echo "exit=$?"`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo xtask check-strings
cargo xtask export-bindings      # must exit 0 AND leave web/src/bindings/ unchanged
cargo test --workspace --all-features
cargo deny check
cd web && npm test && npm run typecheck && npm run build
```

Tests need a live PostgreSQL 18:

```sh
podman run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/postgres:18
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```

`podman compose` needs the socket first: `systemctl --user start podman.socket`.

## File structure

| File | Responsibility |
|---|---|
| `crates/lapidary-db/migrations/0003_jobs.sql` | **Create.** The `job` table, its three indexes, and the two uniqueness constraints. |
| `crates/lapidary-core/src/job.rs` | **Create.** `JobState`, `Outcome`, `JobFailure`, `BatchStatus`, `ScanAccepted` — the ts-rs wire shapes. |
| `crates/lapidary-core/src/ids.rs` | **Modify.** Add `JobId` and `BatchId` via the existing `uuid_newtype!` macro. |
| `crates/lapidary-db/src/jobs.rs` | **Create.** `PgJobs` — every statement the queue issues, plus the `PgListener` factory. |
| `crates/lapidary-jobs/src/handler.rs` | **Create.** `JobHandler`, `HandlerError`. The seam. |
| `crates/lapidary-jobs/src/policy.rs` | **Create.** `next_state` — a pure function from (result, attempts) to the next row state. Where retry policy lives and where it is tested. |
| `crates/lapidary-jobs/src/worker.rs` | **Create.** The loop: permits, dequeue, listener-or-poll, shutdown. |
| `crates/lapidary-ingest/src/handler.rs` | **Create.** `IngestHandler` — slice 1's `ingest_one`, moved. |
| `crates/lapidary-ingest/src/scan.rs` | **Modify.** The walk stays; the per-file body leaves; the response becomes `202 ScanAccepted`. |
| `crates/lapidary-api/src/jobs.rs` | **Create.** `GET /api/libraries/{lib}/jobs/{batch_id}`. |
| `bin/lapidary-server/src/main.rs` | **Modify.** Spawn the worker loop under `Role::Worker`. |
| `web/src/lib/api.ts` | **Modify.** `fetchBatchStatus`. |
| `web/src/routes/index.tsx` | **Modify.** Poll while a batch runs; stop when it finishes. |
| `web/src/lib/strings.ts` | **Modify.** Scan-progress copy. |

`crates/lapidary-jobs/src/lib.rs` keeps its existing `JobsError` and gains the module
declarations plus re-exports. `scan.rs` is currently 360 lines carrying both the walk and
the per-file pipeline; moving the pipeline to `handler.rs` leaves each file with one
responsibility, which is the split this slice would want even if the queue did not force it.

---

### Task 1: Migration 0003 — the job table and two uniqueness constraints

**Files:**
- Create: `crates/lapidary-db/migrations/0003_jobs.sql`
- Test: `crates/lapidary-db/tests/migrations.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: the `job` table; the constraints `part_name_unique_per_library` and
  `derivative_kind_unique_per_revision`. Every later task depends on this schema.

**Read first:** spec §6. Note that this is the first migration added since `build.rs`
landed in `lapidary-db`; step 1 exists to prove that fix works before anything is built on
top of it.

- [ ] **Step 1: Prove the build.rs fix catches an added migration**

Before writing the real migration, add a throwaway file and confirm cargo rebuilds:

```sh
echo "-- probe" > crates/lapidary-db/migrations/0003_probe.sql
cargo build -p lapidary-db 2>&1 | grep -c "Compiling lapidary-db"; echo "exit-check"
rm crates/lapidary-db/migrations/0003_probe.sql
```

Expected: `Compiling lapidary-db` appears (count >= 1). If it prints 0, **stop** — the
`build.rs` fix has regressed and every test below would run against a stale embedded
migration set. That is the exact failure slice 1 recorded and fixed.

- [ ] **Step 2: Write the migration**

Create `crates/lapidary-db/migrations/0003_jobs.sql`:

```sql
-- Slice 2's queue. One row per file; batch_id groups the rows one scan created and is
-- deliberately NOT a table of its own -- see the design doc, section 3.1. Nothing here
-- is denormalized, so nothing here can go stale.
create table job (
    id               uuid primary key,
    batch_id         uuid        not null,
    library_id       uuid        not null references library(id),
    kind             text        not null,
    payload          jsonb       not null,
    state            text        not null default 'pending',
    outcome          text,
    attempts         int         not null default 0,
    max_attempts     int         not null default 3,
    run_after        timestamptz not null default now(),
    leased_by        text,
    lease_expires_at timestamptz,
    last_error       text,
    created_at       timestamptz not null default now(),
    updated_at       timestamptz not null default now(),

    constraint job_state_known
        check (state in ('pending', 'running', 'done', 'failed')),
    constraint job_outcome_known
        check (outcome is null or outcome in ('ingested', 'skipped')),
    -- A finished job says how it finished; a failed one says why. Neither is optional,
    -- because a row that says 'done' and nothing else is a row that lies about its work.
    constraint job_done_has_outcome
        check ((state = 'done') = (outcome is not null)),
    constraint job_failed_has_reason
        check ((state = 'failed') = (last_error is not null))
);

-- The dequeue index. Partial: only pending rows are ever ordered by run_after, and this
-- table is expected to accumulate 'done' rows without bound (design doc, section 11).
create index job_dequeue_idx on job (run_after) where state = 'pending';

-- Reclaiming an expired lease -- the other arm of the dequeue's WHERE.
create index job_expired_lease_idx on job (lease_expires_at) where state = 'running';

-- BatchStatus' GROUP BY.
create index job_batch_idx on job (batch_id);

-- At-least-once delivery means two workers can genuinely race the same file after a
-- lease expiry, and library_holds is not atomic with the insert -- so no amount of
-- checking harder prevents the duplicate. This constraint is what makes the race safe;
-- the handler maps its violation to Skipped.
--
-- The key must agree with PgBlobs::library_holds, which deliberately does NOT filter
-- deleted_at (slice 1, ledger item S11: a re-scan reports a soft-deleted part skipped
-- rather than resurrecting it). If one of the two starts filtering and the other does
-- not, library_holds returns false and this constraint throws on a path with no reason
-- to expect it.
alter table part add constraint part_name_unique_per_library unique (library_id, name);

-- Rider from slice 1's ledger, item S3: a revision has at most one derivative of each
-- kind. Scheduled there for "slice 2's first migration"; this is it.
alter table derivative add constraint derivative_kind_unique_per_revision
    unique (revision_id, kind);
```

- [ ] **Step 3: Write the failing tests**

Create `crates/lapidary-db/tests/migrations.rs`:

```rust
//! The schema constraints slice 2 adds are load-bearing, not decoration: one of them is
//! what makes at-least-once job delivery safe. Each is tested by trying to violate it.

use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

#[sqlx::test(migrations = "migrations")]
async fn two_parts_with_one_name_in_one_library_are_refused(pool: PgPool) {
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let insert = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(library)
                .bind("bracket-lp-1042-03")
                .execute(&pool)
                .await
        }
    };

    insert(Uuid::now_v7()).await.expect("the first part inserts");
    let second = insert(Uuid::now_v7()).await;

    let err = second.expect_err("a second part with the same name must be refused");
    assert!(
        err.to_string().contains("part_name_unique_per_library"),
        "expected the named constraint to be what refused it, got: {err}"
    );
}

#[sqlx::test(migrations = "migrations")]
async fn a_job_that_claims_done_without_an_outcome_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'done')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("done without an outcome must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "migrations")]
async fn a_job_that_claims_failed_without_a_reason_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'failed')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("failed without a reason must be refused");

    assert!(
        err.to_string().contains("job_failed_has_reason"),
        "expected job_failed_has_reason to refuse it, got: {err}"
    );
}
```

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-db --test migrations; echo "exit=$?"
```

Expected: all three PASS. If `two_parts_with_one_name_in_one_library_are_refused` fails
because the *first* insert was refused, an existing seeded row already uses that name —
pick a different plausible part number rather than weakening the test.

- [ ] **Step 5: Verify the constraints are not vacuous**

Mutation, one at a time — each must turn a passing test red:

1. Delete the `alter table part add constraint ...` line → the first test must fail.
2. Change `job_done_has_outcome` to `check (true)` → the second test must fail.
3. Change `job_failed_has_reason` to `check (true)` → the third test must fail.

Revert each mutation. Record in the ledger that all three were observed failing. A
constraint test that passes with the constraint removed is testing nothing, which is this
project's single most common historical defect.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-db/migrations/0003_jobs.sql crates/lapidary-db/tests/migrations.rs
git commit -m "feat(db): add the job table, and the constraints that make delivery safe"
```

---

### Task 2: Job domain types in `lapidary-core`

**Files:**
- Create: `crates/lapidary-core/src/job.rs`
- Modify: `crates/lapidary-core/src/ids.rs`, `crates/lapidary-core/src/lib.rs`
- Test: in-module `#[cfg(test)]` in `job.rs`

**Interfaces:**
- Consumes: `uuid_newtype!` from `ids.rs`.
- Produces: `JobId`, `BatchId`, `JobState`, `Outcome`, `JobFailure`, `BatchStatus`,
  `ScanAccepted`. Tasks 5, 6, 10, 11 and 13 all use these exact names.

**Read first:** spec §7.

- [ ] **Step 1: Add the two id types**

In `crates/lapidary-core/src/ids.rs`, after the existing `uuid_newtype!` invocations:

```rust
uuid_newtype!(JobId, "Identifies one unit of queued work.");
uuid_newtype!(
    BatchId,
    "Groups the jobs one scan created. A grouping column, not an entity: nothing is \
     stored under this id, so nothing under it can go stale."
);
```

- [ ] **Step 2: Write the wire types**

Create `crates/lapidary-core/src/job.rs`:

```rust
//! The queue's wire shapes. `BatchStatus` is aggregated from job rows on every read and
//! never stored, so it cannot disagree with the rows it summarises.

use crate::{BatchId, LibraryId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum JobState {
    Pending,
    Running,
    Done,
    Failed,
}

/// How a job finished. Both are successes: `Skipped` means this library already held
/// this exact file, which is slice 1's hash short-circuit doing its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    Ingested,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobFailure {
    pub path: String,
    /// The handler's message, verbatim. A person reads this in the UI, so it says what
    /// broke and what to do about it.
    pub reason: String,
    pub attempts: u32,
}

/// What a scan turned into.
///
/// `ingested`, `skipped` and the per-file failures are slice 1's `ScanReport` counters,
/// relocated from a response body that vanished with the connection to rows that do not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BatchStatus {
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub total: u32,
    pub pending: u32,
    pub running: u32,
    pub ingested: u32,
    pub skipped: u32,
    pub failed_total: u32,
    /// The first 100 failures, ordered by creation, so the list is stable across polls
    /// rather than reshuffling under the reader. `failed_total` is the real count.
    pub failed: Vec<JobFailure>,
    /// Microseconds since the epoch, reconstructed with `jiff::Timestamp`. sqlx 0.9 has
    /// no `jiff` feature -- it ships `chrono` and `time` -- so every timestamp in this
    /// workspace crosses the database boundary as microseconds. Slice 1's grid query
    /// already does this; it cost a plan revision there and costs nothing here.
    pub started_at: i64,
    /// Set only once no job in the batch is pending or running.
    pub finished_at: Option<i64>,
}

/// The scan route's response. The work has been accepted, not done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanAccepted {
    pub batch_id: BatchId,
    /// How many `*.stl` candidates were enqueued. Zero is a success, not an error --
    /// and a batch with zero jobs has no status resource, so the client must not poll.
    pub queued: u32,
}
```

- [ ] **Step 3: Declare the module**

In `crates/lapidary-core/src/lib.rs`, beside the existing `mod part;`:

```rust
mod job;
pub use job::{BatchStatus, JobFailure, JobState, Outcome, ScanAccepted};
```

And extend the existing `ids` re-export to include `BatchId, JobId`.

- [ ] **Step 4: Write the failing test**

At the bottom of `crates/lapidary-core/src/job.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_serialises_camel_case_so_the_wire_matches_the_generated_type() {
        let json = serde_json::to_string(&JobState::Running).expect("serialises");
        assert_eq!(json, "\"running\"");
    }

    #[test]
    fn a_batch_status_round_trips() {
        let status = BatchStatus {
            batch_id: BatchId::new(),
            library_id: LibraryId::new(),
            total: 6,
            pending: 0,
            running: 0,
            ingested: 5,
            skipped: 0,
            failed_total: 1,
            failed: vec![JobFailure {
                path: "spacer-lp-2001-00.stl".to_owned(),
                reason: "Could not read this STL - it declares 24 facets but the file \
                         ends after 11. Re-export from your CAD tool and retry."
                    .to_owned(),
                attempts: 1,
            }],
            started_at: 1_756_857_600_000_000,
            finished_at: Some(1_756_857_604_000_000),
        };

        let json = serde_json::to_string(&status).expect("serialises");
        let back: BatchStatus = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(status, back);
    }
}
```

- [ ] **Step 5: Run the tests and regenerate bindings**

```sh
cargo test -p lapidary-core; echo "exit=$?"
cargo xtask export-bindings; echo "exit=$?"
git status --porcelain web/src/bindings/
```

Expected: tests PASS, `export-bindings` exits 0, and `web/src/bindings/` gains
`JobState.ts`, `Outcome.ts`, `JobFailure.ts`, `BatchStatus.ts`, `ScanAccepted.ts`,
`JobId.ts`, `BatchId.ts`. If `export-bindings` reports an expected/written mismatch, it is
working as designed — read its message; do not delete files to satisfy it.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-core web/src/bindings
git commit -m "feat(core): add the queue's id and wire types"
```

---

### Task 3: `PgJobs::enqueue` — one statement, N rows, one notify

**Files:**
- Create: `crates/lapidary-db/src/jobs.rs`
- Modify: `crates/lapidary-db/src/lib.rs`
- Test: `crates/lapidary-db/tests/jobs.rs` (create)

**Interfaces:**
- Consumes: Task 1's schema, Task 2's `BatchId`/`JobId`.
- Produces:
  ```rust
  pub struct PgJobs(pub PgPool);
  impl PgJobs {
      pub async fn enqueue_scan(
          &self, library: LibraryId, paths: &[String],
      ) -> Result<(BatchId, u32), DbError>;
  }
  pub const JOB_CHANNEL: &str = "lapidary_jobs";
  ```
  Task 10 calls `enqueue_scan`. Task 8 listens on `JOB_CHANNEL`.

**Read first:** spec §5 steps 1–5, §3.4.

- [ ] **Step 1: Write the failing test**

Create `crates/lapidary-db/tests/jobs.rs`:

```rust
use lapidary_core::LibraryId;
use lapidary_db::PgJobs;
use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

#[sqlx::test(migrations = "migrations")]
async fn enqueue_writes_one_pending_row_per_path_under_one_batch(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let paths = vec![
        "bracket-lp-1042-03.stl".to_owned(),
        "spacer-lp-2001-00.stl".to_owned(),
    ];

    let (batch, queued) = jobs.enqueue_scan(seeded(), &paths).await.expect("enqueues");
    assert_eq!(queued, 2);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT state, payload->>'path' FROM job WHERE batch_id = $1 ORDER BY payload->>'path'",
    )
    .bind(batch.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("reads back");

    assert_eq!(
        rows,
        vec![
            ("pending".to_owned(), "bracket-lp-1042-03.stl".to_owned()),
            ("pending".to_owned(), "spacer-lp-2001-00.stl".to_owned()),
        ]
    );
}

#[sqlx::test(migrations = "migrations")]
async fn enqueueing_nothing_issues_a_batch_id_and_writes_no_rows(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, queued) = jobs.enqueue_scan(seeded(), &[]).await.expect("enqueues");

    assert_eq!(queued, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE batch_id = $1")
        .bind(batch.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(count, 0, "an empty scan must not invent a job");
}
```

- [ ] **Step 2: Run it to confirm it fails**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: FAIL to compile — `PgJobs` does not exist.

- [ ] **Step 3: Implement**

Create `crates/lapidary-db/src/jobs.rs`:

```rust
//! Every statement the job queue issues. `lapidary-jobs` holds the policy and contains
//! no SQL at all -- CLAUDE.md: no SQL outside this crate.

use crate::DbError;
use lapidary_core::{BatchId, JobId, LibraryId};
use sqlx::PgPool;
use uuid::Uuid;

/// The `LISTEN`/`NOTIFY` channel. A wake-up only: the payload is empty and nothing reads
/// it. Notification is a latency optimization over the worker's polling floor and must
/// never become the mechanism the queue depends on -- a NOTIFY fires into the void when
/// nothing is listening, so a worker that starts after an enqueue would never learn about
/// that work. See the design doc, section 3.4, and the test that disables the listener.
pub const JOB_CHANNEL: &str = "lapidary_jobs";

pub struct PgJobs(pub PgPool);

impl PgJobs {
    /// Enqueue one job per path under a fresh batch. One statement regardless of N: a
    /// thousand files is one insert, because this runs inside the HTTP request.
    pub async fn enqueue_scan(
        &self,
        library: LibraryId,
        paths: &[String],
    ) -> Result<(BatchId, u32), DbError> {
        let batch = BatchId::new();

        if paths.is_empty() {
            // No rows, and deliberately no NOTIFY: waking every worker to find nothing
            // is the one case where the optimization is pure cost.
            return Ok((batch, 0));
        }

        let ids: Vec<Uuid> = (0..paths.len()).map(|_| JobId::new().as_uuid()).collect();

        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload) \
             SELECT id, $2, $3, 'ingest_file', jsonb_build_object('path', path) \
             FROM unnest($1::uuid[], $4::text[]) AS t(id, path)",
        )
        .bind(&ids)
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(paths)
        .execute(&self.0)
        .await?;

        sqlx::query("SELECT pg_notify($1, '')")
            .bind(JOB_CHANNEL)
            .execute(&self.0)
            .await?;

        Ok((batch, paths.len() as u32))
    }
}
```

In `crates/lapidary-db/src/lib.rs`:

```rust
mod jobs;
pub use jobs::{JOB_CHANNEL, PgJobs};
```

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): enqueue a scan as one job per file under one batch"
```

---

### Task 4: `PgJobs::dequeue` — leasing, and reclamation without a reaper

**Files:**
- Modify: `crates/lapidary-db/src/jobs.rs`, `crates/lapidary-db/tests/jobs.rs`

**Interfaces:**
- Consumes: Task 3's `PgJobs`.
- Produces:
  ```rust
  pub struct JobRow {
      pub id: JobId,
      pub batch_id: BatchId,
      pub library_id: LibraryId,
      pub kind: String,
      pub payload: serde_json::Value,
      pub attempts: i32,
      pub max_attempts: i32,
  }
  impl PgJobs {
      pub async fn dequeue(
          &self, worker_id: &str, lease: Duration,
      ) -> Result<Option<JobRow>, DbError>;
  }
  ```
  Task 8's loop calls this. Task 9's handler receives `&JobRow`.

**Read first:** spec §3.2 — this is the task the whole design turns on.

- [ ] **Step 1: Write the failing tests**

Append to `crates/lapidary-db/tests/jobs.rs`:

```rust
use lapidary_db::JobRow;
use std::time::Duration;

const LEASE: Duration = Duration::from_secs(60);

#[sqlx::test(migrations = "migrations")]
async fn two_workers_racing_one_job_produce_exactly_one_winner(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    // Both dequeues run concurrently against a queue holding exactly one job. This is
    // the property FOR UPDATE SKIP LOCKED exists for; without the row lock both
    // transactions read the same row and both claim it.
    let a = PgJobs(pool.clone());
    let b = PgJobs(pool.clone());
    let (first, second) = tokio::join!(a.dequeue("worker-a", LEASE), b.dequeue("worker-b", LEASE));

    let claimed = [first.expect("a dequeues"), second.expect("b dequeues")]
        .into_iter()
        .flatten()
        .count();
    assert_eq!(claimed, 1, "exactly one worker may hold a lease on one job");
}

#[sqlx::test(migrations = "migrations")]
async fn a_job_whose_lease_expired_is_reclaimed_and_its_attempts_counted(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let first = jobs
        .dequeue("worker-that-will-die", LEASE)
        .await
        .expect("dequeues")
        .expect("a job is available");
    assert_eq!(first.attempts, 1);

    // Simulate the worker dying: the row stays 'running', and its lease lapses.
    sqlx::query("UPDATE job SET lease_expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(first.id.as_uuid())
        .execute(&pool)
        .await
        .expect("expires the lease");

    let reclaimed = jobs
        .dequeue("worker-that-survives", LEASE)
        .await
        .expect("dequeues")
        .expect("an expired lease must be reclaimable");

    assert_eq!(reclaimed.id, first.id, "the same job comes back");
    assert_eq!(
        reclaimed.attempts, 2,
        "reclaiming counts as an attempt, which is what caps the poison-pill case"
    );
}

#[sqlx::test(migrations = "migrations")]
async fn a_job_still_in_backoff_is_not_dequeued(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    sqlx::query("UPDATE job SET run_after = now() + interval '1 hour'")
        .execute(&pool)
        .await
        .expect("pushes it into the future");

    let claimed: Option<JobRow> = jobs.dequeue("worker-a", LEASE).await.expect("dequeues");
    assert!(claimed.is_none(), "backoff must actually withhold the job");
}

#[sqlx::test(migrations = "migrations")]
async fn an_empty_queue_yields_nothing_rather_than_blocking(pool: PgPool) {
    let jobs = PgJobs(pool);
    assert!(jobs.dequeue("worker-a", LEASE).await.expect("dequeues").is_none());
}
```

- [ ] **Step 2: Run them to confirm they fail**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: FAIL to compile — `dequeue` and `JobRow` do not exist.

- [ ] **Step 3: Implement**

Add to `crates/lapidary-db/src/jobs.rs`:

```rust
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: JobId,
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub kind: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

impl PgJobs {
    /// Claim one job, or reclaim one whose lease expired.
    ///
    /// Reclamation is folded in here rather than given to a sweeper, so there is no
    /// sweeper process to be the thing that died. `attempts` increments on reclamation
    /// exactly as it does on retry, which is what caps the poison-pill case: a file that
    /// panics the worker before it can record anything is tried `max_attempts` times and
    /// then abandoned by the caller, rather than re-leased forever.
    ///
    /// Exhausted rows are deliberately NOT excluded here. Filtering them out in SQL would
    /// leave them 'running' with a dead lease, invisible to this query and to any cleanup
    /// -- which is how this table would grow a permanent population of zombies. The
    /// caller claims them and fails them (see `lapidary_jobs::worker`).
    pub async fn dequeue(
        &self,
        worker_id: &str,
        lease: Duration,
    ) -> Result<Option<JobRow>, DbError> {
        let row: Option<(Uuid, Uuid, Uuid, String, serde_json::Value, i32, i32)> =
            sqlx::query_as(
                "UPDATE job SET state = 'running', \
                                attempts = attempts + 1, \
                                leased_by = $1, \
                                lease_expires_at = now() + make_interval(secs => $2), \
                                updated_at = now() \
                 WHERE id = ( \
                     SELECT id FROM job \
                     WHERE (state = 'pending' AND run_after <= now()) \
                        OR (state = 'running' AND lease_expires_at < now()) \
                     ORDER BY run_after \
                     FOR UPDATE SKIP LOCKED \
                     LIMIT 1 \
                 ) \
                 RETURNING id, batch_id, library_id, kind, payload, attempts, max_attempts",
            )
            .bind(worker_id)
            .bind(lease.as_secs_f64())
            .fetch_optional(&self.0)
            .await?;

        Ok(row.map(
            |(id, batch_id, library_id, kind, payload, attempts, max_attempts)| JobRow {
                id: JobId::from_uuid(id),
                batch_id: BatchId::from_uuid(batch_id),
                library_id: LibraryId::from_uuid(library_id),
                kind,
                payload,
                attempts,
                max_attempts,
            },
        ))
    }
}
```

Export `JobRow` from `lib.rs`.

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: all six PASS.

- [ ] **Step 5: Verify each test tests its own mechanism**

Apply each mutation, confirm the named test fails, then revert:

| Mutation | Test that must fail |
|---|---|
| delete `FOR UPDATE SKIP LOCKED` | `two_workers_racing_one_job_produce_exactly_one_winner` |
| delete the `OR (state = 'running' AND lease_expires_at < now())` arm | `a_job_whose_lease_expired_is_reclaimed_and_its_attempts_counted` |
| change `attempts = attempts + 1` to `attempts = attempts` | same test, on the `attempts == 2` assertion |
| delete `AND run_after <= now()` | `a_job_still_in_backoff_is_not_dequeued` |

The first mutation is the important one. If removing `FOR UPDATE SKIP LOCKED` leaves the
race test green, the test is not actually racing — check that both dequeues really run
concurrently via `tokio::join!` on two separate `PgJobs` handles, not sequentially.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): lease jobs, and reclaim expired leases in the same query"
```

---

### Task 5: `PgJobs` — completion, failure, backoff and release

**Files:**
- Modify: `crates/lapidary-db/src/jobs.rs`, `crates/lapidary-db/tests/jobs.rs`

**Interfaces:**
- Produces:
  ```rust
  impl PgJobs {
      pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError>;
      pub async fn fail(&self, id: JobId, reason: &str) -> Result<(), DbError>;
      pub async fn reschedule(&self, id: JobId, reason: &str, backoff: Duration)
          -> Result<(), DbError>;
      pub async fn release_leases(&self, worker_id: &str) -> Result<u64, DbError>;
  }
  ```
  Task 8's loop calls all four.

**Read first:** spec §3.3, §4.4.

- [ ] **Step 1: Write the failing tests**

Append to `crates/lapidary-db/tests/jobs.rs`:

```rust
use lapidary_core::Outcome;

#[sqlx::test(migrations = "migrations")]
async fn completing_a_job_records_how_it_finished(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs.dequeue("worker-a", LEASE).await.expect("dequeues").expect("a job");

    jobs.complete(job.id, Outcome::Skipped).await.expect("completes");

    let (state, outcome): (String, Option<String>) =
        sqlx::query_as("SELECT state, outcome FROM job WHERE id = $1")
            .bind(job.id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads back");
    assert_eq!(state, "done");
    assert_eq!(outcome.as_deref(), Some("skipped"));
}

#[sqlx::test(migrations = "migrations")]
async fn rescheduling_pushes_the_job_into_the_future_and_keeps_the_reason(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let job = jobs.dequeue("worker-a", LEASE).await.expect("dequeues").expect("a job");

    jobs.reschedule(job.id, "the database was unreachable", Duration::from_secs(8))
        .await
        .expect("reschedules");

    let (state, in_future, reason): (String, bool, Option<String>) = sqlx::query_as(
        "SELECT state, run_after > now(), last_error FROM job WHERE id = $1",
    )
    .bind(job.id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("reads back");

    assert_eq!(state, "pending", "a rescheduled job is queued again, not failed");
    assert!(in_future, "backoff must actually delay the next attempt");
    assert_eq!(reason.as_deref(), Some("the database was unreachable"));
}

#[sqlx::test(migrations = "migrations")]
async fn releasing_a_workers_leases_makes_its_jobs_immediately_available(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    jobs.dequeue("worker-shutting-down", LEASE).await.expect("dequeues").expect("a job");

    let released = jobs
        .release_leases("worker-shutting-down")
        .await
        .expect("releases");
    assert_eq!(released, 1);

    // A planned restart resumes instantly instead of waiting out a 60-second lease.
    let picked_up = jobs.dequeue("worker-restarted", LEASE).await.expect("dequeues");
    assert!(picked_up.is_some(), "a released job must be available at once");
}
```

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add to `crates/lapidary-db/src/jobs.rs`:

```rust
use lapidary_core::Outcome;

impl PgJobs {
    pub async fn complete(&self, id: JobId, outcome: Outcome) -> Result<(), DbError> {
        let outcome = match outcome {
            Outcome::Ingested => "ingested",
            Outcome::Skipped => "skipped",
        };
        sqlx::query(
            "UPDATE job SET state = 'done', outcome = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(outcome)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Terminal. `reason` is the handler's own message and is shown to a person.
    pub async fn fail(&self, id: JobId, reason: &str) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE job SET state = 'failed', last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Back to the queue behind a backoff. `last_error` is kept so a job that is still
    /// retrying can say what went wrong last time.
    pub async fn reschedule(
        &self,
        id: JobId,
        reason: &str,
        backoff: Duration,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE job SET state = 'pending', \
                            run_after = now() + make_interval(secs => $3), \
                            last_error = $2, leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(reason)
        .bind(backoff.as_secs_f64())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Graceful shutdown: hand back whatever this worker still holds so a restart
    /// resumes at once rather than waiting out every lease. A crash does not get this,
    /// which is what lease expiry is for -- the two paths are separate because only one
    /// of them can run cleanup code.
    pub async fn release_leases(&self, worker_id: &str) -> Result<u64, DbError> {
        let result = sqlx::query(
            "UPDATE job SET state = 'pending', run_after = now(), leased_by = NULL, \
                            lease_expires_at = NULL, updated_at = now() \
             WHERE leased_by = $1 AND state = 'running'",
        )
        .bind(worker_id)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected())
    }
}
```

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: all nine PASS.

- [ ] **Step 5: Verify**

Mutate `reschedule`'s `now() + make_interval(...)` to plain `now()`;
`rescheduling_pushes_the_job_into_the_future_and_keeps_the_reason` must fail on
`in_future`. Mutate `release_leases`' `WHERE` to drop `AND state = 'running'` and confirm
the test still passes — this one is a **control mutation** that must be MISSED, proving the
test is not simply sensitive to any edit. Revert both.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): complete, fail, reschedule and release jobs"
```

---

### Task 6: `PgJobs::batch_status` — the aggregate

**Files:**
- Modify: `crates/lapidary-db/src/jobs.rs`, `crates/lapidary-db/tests/jobs.rs`

**Interfaces:**
- Produces:
  ```rust
  impl PgJobs {
      /// `None` when the batch has no jobs -- an id never issued and a scan that
      /// enqueued nothing are indistinguishable, and both mean "no status resource".
      pub async fn batch_status(
          &self, library: LibraryId, batch: BatchId,
      ) -> Result<Option<BatchStatus>, DbError>;
  }
  ```
  Task 11's route calls this.

**Read first:** spec §3.7, §7, §8.

The `library` parameter is not decoration. It is how "content addressing is not
authorization" is enforced for job ids: a batch id from another library must not resolve.

- [ ] **Step 1: Write the failing tests**

Append to `crates/lapidary-db/tests/jobs.rs`:

```rust
#[sqlx::test(migrations = "migrations")]
async fn batch_status_counts_only_its_own_batch(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (first, _) = jobs
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let (second, _) = jobs
        .enqueue_scan(
            seeded(),
            &["spacer-lp-2001-00.stl".to_owned(), "vee-block-lp-3072-02.stl".to_owned()],
        )
        .await
        .expect("enqueues");

    let a = jobs.batch_status(seeded(), first).await.expect("reads").expect("exists");
    let b = jobs.batch_status(seeded(), second).await.expect("reads").expect("exists");

    assert_eq!(a.total, 1, "the first batch must not see the second's jobs");
    assert_eq!(b.total, 2);
}

#[sqlx::test(migrations = "migrations")]
async fn a_batch_is_unfinished_while_any_job_is_pending(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue_scan(
            seeded(),
            &["bracket-lp-1042-03.stl".to_owned(), "spacer-lp-2001-00.stl".to_owned()],
        )
        .await
        .expect("enqueues");

    let job = jobs.dequeue("worker-a", LEASE).await.expect("dequeues").expect("a job");
    jobs.complete(job.id, Outcome::Ingested).await.expect("completes");

    let mid = jobs.batch_status(seeded(), batch).await.expect("reads").expect("exists");
    assert_eq!(mid.ingested, 1);
    assert_eq!(mid.pending, 1);
    assert!(mid.finished_at.is_none(), "one job still pending is not finished");

    let last = jobs.dequeue("worker-a", LEASE).await.expect("dequeues").expect("a job");
    jobs.fail(last.id, "Could not read this STL - the file ends mid-facet.")
        .await
        .expect("fails");

    let done = jobs.batch_status(seeded(), batch).await.expect("reads").expect("exists");
    assert!(done.finished_at.is_some(), "nothing left to run means finished");
    assert_eq!(done.failed_total, 1);
    assert_eq!(done.failed.len(), 1);
    assert_eq!(done.failed[0].path, "spacer-lp-2001-00.stl");
}

#[sqlx::test(migrations = "migrations")]
async fn a_batch_id_from_another_library_does_not_resolve(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, _) = jobs
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let elsewhere = LibraryId::new();
    let found = jobs.batch_status(elsewhere, batch).await.expect("reads");
    assert!(
        found.is_none(),
        "a batch id must not be a capability -- content addressing is not authorization"
    );
}

#[sqlx::test(migrations = "migrations")]
async fn a_batch_with_no_jobs_has_no_status(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    let (batch, queued) = jobs.enqueue_scan(seeded(), &[]).await.expect("enqueues");
    assert_eq!(queued, 0);
    assert!(
        jobs.batch_status(seeded(), batch).await.expect("reads").is_none(),
        "an empty batch is indistinguishable from an id never issued, and both 404"
    );
}
```

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

- [ ] **Step 3: Implement**

Add to `crates/lapidary-db/src/jobs.rs`:

```rust
use lapidary_core::{BatchStatus, JobFailure};

/// The most failures one status response carries. A thousand-file disaster must return a
/// readable payload, and `failed_total` still reports the real number.
const FAILED_SAMPLE: i64 = 100;

impl PgJobs {
    pub async fn batch_status(
        &self,
        library: LibraryId,
        batch: BatchId,
    ) -> Result<Option<BatchStatus>, DbError> {
        // `library_id = $2` is the ownership check, not a performance filter: it is what
        // stops a batch id alone from being a capability.
        let counts: Option<(i64, i64, i64, i64, i64, i64, i64, Option<i64>)> = sqlx::query_as(
            "SELECT count(*), \
                    count(*) FILTER (WHERE state = 'pending'), \
                    count(*) FILTER (WHERE state = 'running'), \
                    count(*) FILTER (WHERE outcome = 'ingested'), \
                    count(*) FILTER (WHERE outcome = 'skipped'), \
                    count(*) FILTER (WHERE state = 'failed'), \
                    (extract(epoch FROM min(created_at)) * 1000000)::bigint, \
                    CASE WHEN count(*) FILTER (WHERE state IN ('pending','running')) = 0 \
                         THEN (extract(epoch FROM max(updated_at)) * 1000000)::bigint \
                    END \
             FROM job WHERE batch_id = $1 AND library_id = $2",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;

        // An aggregate over zero rows still returns one row, with count 0 -- so "no
        // jobs" is detected on the count, not on fetch_optional returning None.
        let Some((total, pending, running, ingested, skipped, failed_total, started, finished)) =
            counts
        else {
            return Ok(None);
        };
        if total == 0 {
            return Ok(None);
        }

        let failures: Vec<(String, String, i32)> = sqlx::query_as(
            "SELECT payload->>'path', last_error, attempts \
             FROM job WHERE batch_id = $1 AND library_id = $2 AND state = 'failed' \
             ORDER BY created_at LIMIT $3",
        )
        .bind(batch.as_uuid())
        .bind(library.as_uuid())
        .bind(FAILED_SAMPLE)
        .fetch_all(&self.0)
        .await?;

        Ok(Some(BatchStatus {
            batch_id: batch,
            library_id: library,
            total: total as u32,
            pending: pending as u32,
            running: running as u32,
            ingested: ingested as u32,
            skipped: skipped as u32,
            failed_total: failed_total as u32,
            failed: failures
                .into_iter()
                .map(|(path, reason, attempts)| JobFailure {
                    path,
                    reason,
                    attempts: attempts.max(0) as u32,
                })
                .collect(),
            started_at: started.unwrap_or_default(),
            finished_at: finished,
        }))
    }
}
```

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-db --test jobs; echo "exit=$?"
```

Expected: all thirteen PASS.

- [ ] **Step 5: Verify**

| Mutation | Test that must fail |
|---|---|
| drop `AND batch_id = $1` from the counts query | `batch_status_counts_only_its_own_batch` |
| drop `AND library_id = $2` from both queries | `a_batch_id_from_another_library_does_not_resolve` |
| replace the `CASE WHEN ... = 0` guard with an unconditional max | `a_batch_is_unfinished_while_any_job_is_pending` |
| delete the `if total == 0` early return | `a_batch_with_no_jobs_has_no_status` |

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-db
git commit -m "feat(db): aggregate a batch's status, scoped to its library"
```

---

### Task 7: `lapidary-jobs` — the handler seam and the retry policy

**Files:**
- Create: `crates/lapidary-jobs/src/handler.rs`, `crates/lapidary-jobs/src/policy.rs`
- Modify: `crates/lapidary-jobs/src/lib.rs`, `crates/lapidary-jobs/Cargo.toml`

**Interfaces:**
- Consumes: `lapidary_db::JobRow`, `lapidary_core::Outcome`.
- Produces:
  ```rust
  pub trait JobHandler: Send + Sync + 'static {
      fn handle(&self, job: &JobRow)
          -> impl Future<Output = Result<Outcome, HandlerError>> + Send;
  }
  pub enum HandlerError { Permanent { message: String }, Transient { message: String } }
  pub enum Next { Complete(Outcome), Fail { reason: String }, Retry { reason: String, backoff: Duration } }
  pub fn next_state(result: Result<Outcome, HandlerError>, attempts: i32, max_attempts: i32) -> Next;
  pub const BACKOFF: [Duration; 3];
  ```
  Task 8's loop applies `next_state`; Task 9 implements `JobHandler`.

**Read first:** spec §3.3, §4.3.

`async_trait` is **not** used — this workspace is on Rust 1.95 with edition 2024, where
`impl Future` in traits is native. Adding a dependency that `deny.toml` would have to
allow, for a feature the compiler already has, is the opposite of the boring option.

- [ ] **Step 1: Write the seam**

Create `crates/lapidary-jobs/src/handler.rs`:

```rust
//! The seam between delivery and work.
//!
//! `lapidary-jobs` is L2, so it may not depend on `lapidary-cad` (also L2) or on
//! `lapidary-ingest` (L3) -- `cargo xtask check-layers` forbids exactly the edge a loop
//! that called ingest directly would need. The resulting design is the better one anyway:
//! the handler reports what happened, and the loop decides what to do about it, so the
//! rule about immutable bytes is stated in one place instead of three.

use lapidary_core::Outcome;
use lapidary_db::JobRow;
use std::future::Future;

pub trait JobHandler: Send + Sync + 'static {
    fn handle(
        &self,
        job: &JobRow,
    ) -> impl Future<Output = Result<Outcome, HandlerError>> + Send;
}

/// Why a job did not finish. The distinction is the whole retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// Re-running this job cannot succeed. Blobs are content-addressed and immutable, so
    /// parsing the same bytes again is guaranteed to produce the same error; retrying
    /// only delays an answer that was already available.
    Permanent { message: String },
    /// Something outside the job failed -- the database, the blob store, a lost lease.
    /// Worth another attempt.
    ///
    /// When in doubt, choose this: a retried permanent failure costs one wasted parse,
    /// while a non-retried transient failure costs the user a file.
    Transient { message: String },
}
```

- [ ] **Step 2: Write the failing policy tests**

Create `crates/lapidary-jobs/src/policy.rs` with tests first:

```rust
//! What the loop does with a handler's answer. A pure function, so the policy that
//! decides whether a user's file is retried or abandoned is testable without a database,
//! a worker, or a clock.

use crate::HandlerError;
use lapidary_core::Outcome;
use std::time::Duration;

/// Fixed, not exponential-with-jitter: with `max_attempts = 3` only the third value is
/// ever reached, and a table is easier to reason about than a formula.
pub const BACKOFF: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(30),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Complete(Outcome),
    Fail { reason: String },
    Retry { reason: String, backoff: Duration },
}

pub fn next_state(
    result: Result<Outcome, HandlerError>,
    attempts: i32,
    max_attempts: i32,
) -> Next {
    match result {
        Ok(outcome) => Next::Complete(outcome),
        Err(HandlerError::Permanent { message }) => Next::Fail { reason: message },
        Err(HandlerError::Transient { message }) => {
            if attempts >= max_attempts {
                Next::Fail {
                    reason: format!(
                        "{message} Gave up after {attempts} attempts."
                    ),
                }
            } else {
                let index = (attempts.max(1) as usize - 1).min(BACKOFF.len() - 1);
                Next::Retry {
                    reason: message,
                    backoff: BACKOFF[index],
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permanent_failure_is_terminal_on_the_first_attempt() {
        // The bytes are content-addressed and immutable: attempt two would parse the
        // same bytes and produce the same error, so there is no attempt two.
        let next = next_state(
            Err(HandlerError::Permanent {
                message: "Could not read this STL - it declares 24 facets but the file \
                          ends after 11. Re-export from your CAD tool and retry."
                    .to_owned(),
            }),
            1,
            3,
        );
        assert!(
            matches!(next, Next::Fail { .. }),
            "a permanent failure must not be retried, got {next:?}"
        );
    }

    #[test]
    fn a_transient_failure_retries_with_a_growing_backoff() {
        let reason = HandlerError::Transient {
            message: "The database was unreachable.".to_owned(),
        };
        assert_eq!(
            next_state(Err(reason.clone()), 1, 3),
            Next::Retry {
                reason: "The database was unreachable.".to_owned(),
                backoff: Duration::from_secs(2),
            }
        );
        assert_eq!(
            next_state(Err(reason), 2, 3),
            Next::Retry {
                reason: "The database was unreachable.".to_owned(),
                backoff: Duration::from_secs(8),
            }
        );
    }

    #[test]
    fn a_transient_failure_on_the_last_attempt_is_terminal() {
        let next = next_state(
            Err(HandlerError::Transient {
                message: "The database was unreachable.".to_owned(),
            }),
            3,
            3,
        );
        match next {
            Next::Fail { reason } => assert!(
                reason.contains("Gave up after 3 attempts"),
                "the message must say why it stopped trying, got: {reason}"
            ),
            other => panic!("expected a terminal failure, got {other:?}"),
        }
    }

    #[test]
    fn a_success_carries_its_outcome_through() {
        assert_eq!(
            next_state(Ok(Outcome::Skipped), 1, 3),
            Next::Complete(Outcome::Skipped)
        );
    }
}
```

- [ ] **Step 3: Wire the modules**

In `crates/lapidary-jobs/src/lib.rs`, keep the existing `JobsError` and add:

```rust
mod handler;
mod policy;

pub use handler::{HandlerError, JobHandler};
pub use policy::{BACKOFF, Next, next_state};
```

Correct the module doc's heartbeat claim, which this slice deliberately does not
implement:

```rust
//! The job queue: PostgreSQL `FOR UPDATE SKIP LOCKED` plus `LISTEN`/`NOTIFY`, deliberately
//! with no Redis and no message broker. Workers take leases on jobs.
//!
//! Leases are NOT heartbeated yet. A single-file mesh ingest measures roughly 200 ms
//! against a 60-second lease, so a renewal task would be ceremony that still has to be
//! tested and can still be wrong. `renew_lease` arrives when a job can realistically
//! outlive its lease -- STEP ingest through the OCCT sidecar in Phase 2.
```

- [ ] **Step 4: Run the tests**

```sh
cargo test -p lapidary-jobs; echo "exit=$?"
cargo xtask check-layers; echo "exit=$?"
```

Expected: four tests PASS, `check-layers` exits 0.

- [ ] **Step 5: Verify the policy tests test the policy**

Change `Err(HandlerError::Permanent { message }) => Next::Fail { reason: message }` to
route to the `Transient` arm. `a_permanent_failure_is_terminal_on_the_first_attempt` must
fail. Revert. This is the mutation the whole of §3.3 exists to prevent.

- [ ] **Step 6: Commit**

```sh
git add crates/lapidary-jobs
git commit -m "feat(jobs): add the handler seam and the retry policy"
```

---

### Task 8: `lapidary-jobs` — the worker loop

**Files:**
- Create: `crates/lapidary-jobs/src/worker.rs`
- Modify: `crates/lapidary-jobs/src/lib.rs`, `crates/lapidary-db/src/jobs.rs`
- Test: `crates/lapidary-jobs/tests/worker.rs` (create)

**Interfaces:**
- Consumes: Tasks 4–7.
- Produces:
  ```rust
  pub struct WorkerConfig {
      pub worker_id: String,
      pub lease: Duration,
      pub poll_interval: Duration,
      pub concurrency: usize,
      /// When false the loop never opens a LISTEN connection and runs on the polling
      /// floor alone. Set false by the test that proves NOTIFY is not load-bearing.
      pub listen: bool,
  }
  impl Default for WorkerConfig { ... }
  pub async fn run<H: JobHandler>(
      jobs: PgJobs, handler: Arc<H>, config: WorkerConfig, shutdown: CancellationToken,
  ) -> Result<(), JobsError>;
  ```
  Task 12 calls `run`.

**Read first:** spec §3.4, §4.4.

- [ ] **Step 1: Add the listener factory to `lapidary-db`**

The `LISTEN` connection is SQL, so it lives here, not in `lapidary-jobs`. Add to
`crates/lapidary-db/src/jobs.rs`:

```rust
use sqlx::postgres::PgListener;

impl PgJobs {
    /// A dedicated connection listening for enqueue notifications. Outside the pool by
    /// necessity: a LISTEN occupies its connection for as long as it is listening.
    pub async fn listener(&self) -> Result<PgListener, DbError> {
        let mut listener = PgListener::connect_with(&self.0).await?;
        listener.listen(JOB_CHANNEL).await?;
        Ok(listener)
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/lapidary-jobs/tests/worker.rs`:

```rust
//! The loop, against a live database and a handler that records what it saw.

use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{JobRow, PgJobs};
use lapidary_jobs::{HandlerError, JobHandler, WorkerConfig, run};
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

/// Counts what it handled. Real enough to prove the loop delivers work; it does not
/// touch a mesh, which is the point of the seam.
struct CountingHandler {
    seen: AtomicUsize,
}

impl JobHandler for CountingHandler {
    async fn handle(&self, _job: &JobRow) -> Result<Outcome, HandlerError> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome::Ingested)
    }
}

async fn drain(pool: PgPool, config: WorkerConfig, expect: usize) -> usize {
    let handler = Arc::new(CountingHandler { seen: AtomicUsize::new(0) });
    let shutdown = CancellationToken::new();
    let worker = tokio::spawn(run(
        PgJobs(pool.clone()),
        handler.clone(),
        config,
        shutdown.clone(),
    ));

    // Poll for completion rather than sleeping a fixed amount: a fixed sleep makes this
    // test's pass/fail depend on how loaded the machine is.
    for _ in 0..200 {
        if handler.seen.load(Ordering::SeqCst) >= expect {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    shutdown.cancel();
    worker.await.expect("the worker task joins").expect("the worker exits cleanly");
    handler.seen.load(Ordering::SeqCst)
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_queue_drains_with_the_listener_disabled(pool: PgPool) {
    // NOTIFY is a latency optimization, never the correctness mechanism. A NOTIFY fires
    // into the void when nothing is listening, so a worker that starts after an enqueue
    // -- or whose listener connection dropped -- must still find the work by polling.
    // If this test ever hangs, the polling floor has stopped being the mechanism.
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(
        seeded(),
        &["bracket-lp-1042-03.stl".to_owned(), "spacer-lp-2001-00.stl".to_owned()],
    )
    .await
    .expect("enqueues");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 2,
        listen: false,
    };

    assert_eq!(drain(pool, config, 2).await, 2, "polling alone must drain the queue");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_job_past_its_attempt_cap_is_abandoned_without_running_the_handler(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    // The poison-pill shape: a worker that dies before it can record anything. attempts
    // is already past the cap, so claiming it must fail it rather than hand it out.
    sqlx::query("UPDATE job SET attempts = 5, max_attempts = 3")
        .execute(&pool)
        .await
        .expect("exhausts the job");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 1,
        listen: false,
    };
    let handled = drain(pool.clone(), config, 0).await;
    assert_eq!(handled, 0, "an exhausted job must never reach the handler");

    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state, last_error FROM job")
            .fetch_one(&pool)
            .await
            .expect("reads back");
    assert_eq!(state, "failed");
    assert!(
        reason.unwrap_or_default().contains("stopped responding"),
        "the message must blame the worker, not the file"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn shutting_down_hands_back_what_the_worker_still_holds(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    jobs.enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    jobs.dequeue("test-worker", Duration::from_secs(60))
        .await
        .expect("dequeues")
        .expect("a job");

    let config = WorkerConfig {
        worker_id: "test-worker".to_owned(),
        lease: Duration::from_secs(60),
        poll_interval: Duration::from_millis(100),
        concurrency: 1,
        listen: false,
    };
    drain(pool.clone(), config, 1).await;

    let state: String = sqlx::query_scalar("SELECT state FROM job")
        .fetch_one(&pool)
        .await
        .expect("reads back");
    assert_ne!(state, "running", "shutdown must not leave a job leased to a dead worker");
}
```

Add `tokio-util` (for `CancellationToken`) and `tokio` test features to
`crates/lapidary-jobs/Cargo.toml`. Check `deny.toml` allows `tokio-util`; it is a
tokio-org crate and the workspace already depends on `tokio`. If `cargo deny check` objects,
use an `Arc<AtomicBool>` plus `tokio::sync::Notify` instead rather than adding an
allow-list entry for convenience.

- [ ] **Step 3: Run to confirm failure**

```sh
cargo test -p lapidary-jobs --test worker; echo "exit=$?"
```

Expected: FAIL to compile.

- [ ] **Step 4: Implement the loop**

Create `crates/lapidary-jobs/src/worker.rs`:

```rust
//! The worker loop. Owns delivery and policy; knows nothing about meshes.

use crate::{JobHandler, JobsError, Next, next_state};
use lapidary_db::PgJobs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// What a claimed-but-exhausted job is failed with. It blames the worker that vanished,
/// not the file, because the file was never the problem.
const ABANDONED: &str =
    "This file was claimed three times and never finished. The worker holding it stopped \
     responding each time. Check the worker's logs for a crash, then scan again.";

pub struct WorkerConfig {
    pub worker_id: String,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub concurrency: usize,
    /// When false, no LISTEN connection is opened and the loop runs on its polling floor
    /// alone. The floor is the correctness mechanism; this flag exists so a test can
    /// prove that, and so a database that refuses an extra connection degrades to slower
    /// rather than to broken.
    pub listen: bool,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: default_worker_id(),
            lease: Duration::from_secs(60),
            poll_interval: Duration::from_secs(5),
            concurrency: 4,
            listen: true,
        }
    }
}

fn default_worker_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
    format!("{host}-{}", std::process::id())
}

pub async fn run<H: JobHandler>(
    jobs: PgJobs,
    handler: Arc<H>,
    config: WorkerConfig,
    shutdown: CancellationToken,
) -> Result<(), JobsError> {
    let permits = Arc::new(Semaphore::new(config.concurrency));
    let mut listener = if config.listen {
        match jobs.listener().await {
            Ok(listener) => Some(listener),
            // Degrade to the polling floor rather than refusing to start. A worker that
            // polls is slower; a worker that will not start ingests nothing.
            Err(error) => {
                tracing::warn!(%error, "could not open the job listener; polling only");
                None
            }
        }
    } else {
        None
    };

    let jobs = Arc::new(jobs);

    loop {
        if shutdown.is_cancelled() {
            break;
        }

        // The permit is acquired BEFORE the dequeue, never after. Leasing a job we have
        // no capacity to start would burn lease time while it waits its turn, and a
        // lease that expires in a queue is indistinguishable from a crashed worker --
        // manufacturing the exact failure the lease exists to detect.
        let permit = match Arc::clone(&permits).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };

        match jobs.dequeue(&config.worker_id, config.lease).await {
            Ok(Some(job)) if job.attempts > job.max_attempts => {
                if let Err(error) = jobs.fail(job.id, ABANDONED).await {
                    tracing::warn!(%error, job = %job.id, "could not record an abandoned job");
                }
                drop(permit);
            }
            Ok(Some(job)) => {
                let jobs = Arc::clone(&jobs);
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let result = handler.handle(&job).await;
                    let outcome = next_state(result, job.attempts, job.max_attempts);
                    let recorded = match outcome {
                        Next::Complete(outcome) => jobs.complete(job.id, outcome).await,
                        Next::Fail { reason } => jobs.fail(job.id, &reason).await,
                        Next::Retry { reason, backoff } => {
                            jobs.reschedule(job.id, &reason, backoff).await
                        }
                    };
                    if let Err(error) = recorded {
                        // The lease will lapse and another worker will reclaim it. That
                        // is safe: the ingest is idempotent by the part uniqueness
                        // constraint, so a redo becomes Skipped rather than a duplicate.
                        tracing::warn!(%error, job = %job.id, "could not record a job result");
                    }
                    drop(permit);
                });
            }
            Ok(None) => {
                drop(permit);
                wait_for_work(&mut listener, config.poll_interval, &shutdown).await;
            }
            Err(error) => {
                drop(permit);
                tracing::warn!(%error, "could not reach the job queue; retrying");
                wait_for_work(&mut listener, config.poll_interval, &shutdown).await;
            }
        }
    }

    // Hand back whatever is still leased so a restart resumes at once.
    if let Err(error) = jobs.release_leases(&config.worker_id).await {
        tracing::warn!(%error, "could not release this worker's leases on shutdown");
    }
    Ok(())
}

/// Sleep until there is plausibly work, the poll interval elapses, or we are shutting
/// down. The poll interval is the floor and the only thing correctness rests on.
async fn wait_for_work(
    listener: &mut Option<sqlx::postgres::PgListener>,
    poll_interval: Duration,
    shutdown: &CancellationToken,
) {
    match listener {
        Some(listener) => {
            tokio::select! {
                _ = listener.recv() => {}
                _ = tokio::time::sleep(poll_interval) => {}
                _ = shutdown.cancelled() => {}
            }
        }
        None => {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {}
                _ = shutdown.cancelled() => {}
            }
        }
    }
}
```

Export from `lib.rs`: `pub use worker::{WorkerConfig, run};`

- [ ] **Step 5: Run the tests**

```sh
cargo test -p lapidary-jobs; echo "exit=$?"
cargo xtask check-layers; echo "exit=$?"
```

Expected: PASS, and `check-layers` exits 0 — `lapidary-jobs` depends only on
`lapidary-core` (L0) and `lapidary-db` (L1).

- [ ] **Step 6: Verify NOTIFY is genuinely not load-bearing**

Replace `wait_for_work`'s `None` arm with `std::future::pending::<()>().await` (wait
forever unless notified). `the_queue_drains_with_the_listener_disabled` must **fail by
timing out**, not pass slowly. Revert.

Also confirm the permit ordering matters: move `acquire_owned` to after the `dequeue` and
check that the tests still pass — they will, because this is a **latency and lease-safety
property, not a correctness one at concurrency 2**. Record that in the ledger honestly
rather than claiming a test covers it; the reason it is written this way is argued in the
code comment, and a test that would catch it needs a job slower than a lease, which
arrives with Phase 2.

- [ ] **Step 7: Commit**

```sh
git add crates/lapidary-jobs crates/lapidary-db
git commit -m "feat(jobs): drain the queue with leases, backoff and graceful shutdown"
```

---

### Task 9: `lapidary-ingest` — `ingest_one` becomes a `JobHandler`

**Files:**
- Create: `crates/lapidary-ingest/src/handler.rs`
- Modify: `crates/lapidary-ingest/src/scan.rs`, `crates/lapidary-ingest/src/lib.rs`,
  `crates/lapidary-ingest/Cargo.toml`
- Test: `crates/lapidary-ingest/tests/handler.rs` (create)

**Interfaces:**
- Consumes: `lapidary_jobs::{JobHandler, HandlerError}`, `lapidary_db::JobRow`.
- Produces:
  ```rust
  pub struct IngestHandler {
      pub db: PgPool,
      pub ingest_dir: PathBuf,
      pub blob_root: PathBuf,
  }
  impl JobHandler for IngestHandler { ... }
  ```
  Task 12 constructs it.

**Read first:** `scan.rs`'s module doc — the per-file ordering is load-bearing and is
**moved, not rewritten**. Spec §3.5 for the one addition.

- [ ] **Step 1: Move the pipeline**

Create `crates/lapidary-ingest/src/handler.rs`. Move `ingest_one`, `part_name` and
`FileOutcome`'s two variants out of `scan.rs` unchanged, and wrap them:

```rust
//! One file's worth of ingest, as a job. The pipeline below is slice 1's, moved rather
//! than rewritten: read, BLAKE3, library_holds, kernel, link-or-put. That ordering is the
//! design and its reasoning lives in `scan.rs`'s module doc.

use lapidary_core::{BlobHash, LibraryId, Outcome};
use lapidary_db::{DbError, IngestRequest, JobRow, PgBlobs, PgIngest, PgPool, StoredBlobRow};
use lapidary_jobs::{HandlerError, JobHandler};
use lapidary_storage::{SourceStore, WorkerRole};
use std::path::PathBuf;

pub struct IngestHandler {
    pub db: PgPool,
    pub ingest_dir: PathBuf,
    pub blob_root: PathBuf,
}

impl JobHandler for IngestHandler {
    async fn handle(&self, job: &JobRow) -> Result<Outcome, HandlerError> {
        let Some(file_name) = job.payload.get("path").and_then(|p| p.as_str()) else {
            return Err(HandlerError::Permanent {
                message: "This job has no file path in its payload. It was not written by \
                          Lapidary's scan endpoint."
                    .to_owned(),
            });
        };
        self.ingest_one(job.library_id, file_name).await
    }
}
```

`ingest_one` keeps its body, with two changes: it returns
`Result<Outcome, HandlerError>` instead of `Result<FileOutcome, String>`, and every
`map_err(|e| e.to_string())` becomes a classification. Read the error, then choose:

```rust
// The kernel: the bytes are immutable, so this error is the final answer about them.
let output = kernel.ingest(&bytes).map_err(|e| HandlerError::Permanent {
    message: e.to_string(),
})?;

// The database and the blob store: the file is fine, something else was not.
if blobs
    .library_holds(library, name, &hash)
    .await
    .map_err(transient_db)?
{
    return Ok(Outcome::Skipped);
}
```

with the two helpers:

```rust
fn transient_db(error: DbError) -> HandlerError {
    HandlerError::Transient { message: error.to_string() }
}

/// A unique violation on `part_name_unique_per_library` is not a failure: another worker
/// won the race for this file after a lease expiry, and the part exists. Mapping it to
/// `Skipped` is what makes at-least-once delivery effectively-once -- see the design
/// doc, section 3.5.
fn classify_write(error: DbError) -> Result<Outcome, HandlerError> {
    if let DbError::Query(sqlx::Error::Database(db)) = &error
        && db.constraint() == Some("part_name_unique_per_library")
    {
        return Ok(Outcome::Skipped);
    }
    Err(HandlerError::Transient { message: error.to_string() })
}
```

Reading the file itself is `Transient`: a missing file may mean the mount is not ready yet.

- [ ] **Step 2: Write the failing tests**

Create `crates/lapidary-ingest/tests/handler.rs`:

```rust
//! The handler, exercised the way the worker exercises it.

use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{JobRow, PgJobs};
use lapidary_ingest::IngestHandler;
use lapidary_jobs::{HandlerError, JobHandler};
use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &str = "bracket-lp-1042-03.stl";

fn seeded() -> LibraryId {
    LibraryId::from_uuid(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
}

fn job_for(file: &str) -> JobRow { /* build a JobRow with payload {"path": file} */ }

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_real_stl_ingests_with_its_real_measurements_and_a_decodable_thumbnail(
    pool: PgPool,
) {
    // The direct descendant of slice 1's `scanning_one_real_stl_ingests_it_once`. Moving
    // ingest behind a queue puts a brand-new seam exactly where the untested one was, so
    // it gets its guard on day one instead of in a fix wave.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    let blob_root = tempfile::tempdir().expect("temp dir");
    std::fs::copy(
        format!("{}/../../fixtures/{BRACKET}", env!("CARGO_MANIFEST_DIR")),
        ingest_dir.path().join(BRACKET),
    )
    .expect("stages the fixture");

    let handler = IngestHandler {
        db: pool.clone(),
        ingest_dir: ingest_dir.path().to_path_buf(),
        blob_root: blob_root.path().to_path_buf(),
    };

    let outcome = handler.handle(&job_for(BRACKET)).await.expect("ingests");
    assert_eq!(outcome, Outcome::Ingested);

    let (name, tri, x, y, z, watertight, thumb): (
        String, i32, f64, f64, f64, bool, Option<Vec<u8>>,
    ) = sqlx::query_as(
        "SELECT p.name, m.triangle_count, m.bbox_x, m.bbox_y, m.bbox_z, m.is_watertight, \
                d.thumb_bytes \
         FROM part p \
         JOIN revision r ON r.part_id = p.id \
         JOIN measurement m ON m.revision_id = r.id \
         JOIN derivative d ON d.revision_id = r.id \
         WHERE p.library_id = $1",
    )
    .bind(seeded().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("the part, its measurements and its thumbnail all landed");

    assert_eq!(name, "bracket-lp-1042-03");
    assert_eq!(tri, 20, "the fixture's real triangle count");
    assert_eq!((x, y, z), (88.0, 40.0, 25.0), "the fixture's real bounding box");
    assert!(watertight);

    let thumb = thumb.expect("a thumbnail was written");
    let decoded = image::load_from_memory(&thumb).expect("the thumbnail decodes as an image");
    assert_eq!(decoded.width(), 512, "the thumbnail is a real 512px render");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_same_file_twice_is_skipped_the_second_time(pool: PgPool) {
    // ... stage the fixture as above ...
    let first = handler.handle(&job_for(BRACKET)).await.expect("ingests");
    let second = handler.handle(&job_for(BRACKET)).await.expect("runs again");
    assert_eq!(first, Outcome::Ingested);
    assert_eq!(second, Outcome::Skipped, "slice 1's short-circuit, through the queue");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_truncated_stl_fails_permanently_so_it_is_never_retried(pool: PgPool) {
    // Write a binary STL header claiming 24 facets, then stop after 11.
    // ... stage it in ingest_dir ...
    let error = handler
        .handle(&job_for("spacer-lp-2001-00.stl"))
        .await
        .expect_err("a truncated file must fail");

    assert!(
        matches!(error, HandlerError::Permanent { .. }),
        "the bytes are immutable, so retrying cannot help: {error:?}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn losing_the_race_for_a_file_is_a_skip_rather_than_a_failure(pool: PgPool) {
    // Two handlers over one staged file, run concurrently -- the lease-expiry race from
    // the design doc, section 3.5. One inserts; the other hits
    // part_name_unique_per_library and must report Skipped, not a failure the user sees.
    // ... build two IngestHandlers over the same dirs, tokio::join! their handle() ...
    let outcomes = [first.expect("one succeeds"), second.expect("the other does too")];
    assert!(outcomes.contains(&Outcome::Ingested));
    assert!(outcomes.contains(&Outcome::Skipped));

    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE library_id = $1")
        .bind(seeded().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(parts, 1, "the race must not produce two parts");
}
```

- [ ] **Step 3: Run the tests**

```sh
cargo test -p lapidary-ingest --test handler; echo "exit=$?"
```

Expected: all four PASS.

- [ ] **Step 4: Verify the seam is pinned**

This is the most important verification in the plan. Apply each mutation, confirm the named
test fails, revert:

| Mutation | Must fail |
|---|---|
| return an empty `Vec` from the thumbnail encoder | `a_real_stl_ingests_with_its_real_measurements_and_a_decodable_thumbnail` |
| zero every field of `MeshMeasurements` | same test |
| delete the `classify_write` unique-violation branch | `losing_the_race_for_a_file_is_a_skip_rather_than_a_failure` |
| classify the kernel error `Transient` | `a_truncated_stl_fails_permanently_so_it_is_never_retried` |

If the first two do **not** fail, the new seam is as untested as slice 1's was, and the
task is not done regardless of what else passes.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-ingest
git commit -m "feat(ingest): run one file's ingest as a job handler"
```

---

### Task 10: `scan` becomes an enqueue

**Files:**
- Modify: `crates/lapidary-ingest/src/scan.rs`, `crates/lapidary-ingest/src/lib.rs`
- Test: `crates/lapidary-ingest/tests/scan.rs` (modify)

**Interfaces:**
- Consumes: `PgJobs::enqueue_scan`.
- Produces: `POST /api/libraries/{id}/scan` → `202 ScanAccepted`.

**Read first:** spec §3.1's last paragraph and §5 steps 1–5.

The walk stays in the request. It is `read_dir` and nothing else, so a thousand entries is
one `read_dir` and one insert — and keeping it here means a missing mount is a response the
user sees rather than a job that fails behind a poll.

- [ ] **Step 1: Rewrite the handler**

`scan.rs` keeps `is_stl_candidate`, `ingest_dir_unreadable`, `entry_read_failure` and their
tests. The body becomes:

```rust
pub async fn scan(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    let entries = match std::fs::read_dir(&state.ingest_dir) {
        Ok(entries) => entries,
        Err(source) => return ingest_dir_unreadable(&state.ingest_dir, &source),
    };

    let mut paths = Vec::new();
    let mut unreadable = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) if is_stl_candidate(&entry.path()) => {
                paths.push(
                    entry
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| entry.path().display().to_string()),
                );
            }
            Ok(_) => {}
            Err(source) => unreadable.push(entry_read_failure(&state.ingest_dir, &source)),
        }
    }
    // Deterministic order, so the job ids a scan issues are ordered the way a person
    // reading the directory would expect. `unnest` preserves array order.
    paths.sort();

    match PgJobs(state.db.clone()).enqueue_scan(library, &paths).await {
        Ok((batch_id, queued)) => (
            StatusCode::ACCEPTED,
            Json(ScanAccepted { batch_id, queued }),
        )
            .into_response(),
        Err(error) => enqueue_failed(&error),
    }
}
```

Delete `ScanReport` and `FileOutcome` from this crate; `ScanFailure`'s role is taken by
`lapidary_core::JobFailure`. Keep `entry_read_failure` returning a small local struct used
only for logging — a directory entry the OS could not name cannot be enqueued, so it is
logged with `tracing::warn!` and does not appear in `ScanAccepted`.

- [ ] **Step 2: Update the tests**

In `crates/lapidary-ingest/tests/scan.rs`, replace the report assertions:

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_enqueues_one_job_per_stl_and_parses_nothing(pool: PgPool) {
    // The whole point of the slice: the request must not touch the CAD kernel. Staging a
    // deliberately unparseable file proves it -- under slice 1 this returned a failure,
    // and now it must be accepted, because nothing has looked at the bytes yet.
    let ingest_dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(ingest_dir.path().join("bracket-lp-1042-03.stl"), b"not an stl")
        .expect("stages a file");
    std::fs::write(ingest_dir.path().join("README.md"), b"not a candidate")
        .expect("stages a non-candidate");

    let response = /* POST the scan route */;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let accepted: ScanAccepted = /* deserialize the body */;
    assert_eq!(accepted.queued, 1, "the README is not a candidate and is counted nowhere");

    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(parts, 0, "the request must enqueue, not ingest");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn scanning_an_empty_directory_is_accepted_with_nothing_queued(pool: PgPool) {
    let response = /* POST against an empty temp dir */;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted: ScanAccepted = /* deserialize */;
    assert_eq!(accepted.queued, 0, "an empty folder scanned successfully");
}
```

Keep the existing test that a nonexistent ingest directory is a 500 with an actionable
message — that path is unchanged and still matters.

- [ ] **Step 3: Run the tests**

```sh
cargo test -p lapidary-ingest; echo "exit=$?"
```

- [ ] **Step 4: Verify**

Restore the synchronous call inside the walk (call `IngestHandler::ingest_one` directly);
`scanning_enqueues_one_job_per_stl_and_parses_nothing` must fail on `parts == 0`. Revert.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-ingest
git commit -m "feat(ingest): scan enqueues a batch instead of ingesting inline"
```

---

### Task 11: The batch status route

**Files:**
- Create: `crates/lapidary-api/src/jobs.rs`
- Modify: `crates/lapidary-api/src/lib.rs`
- Test: `crates/lapidary-api/tests/jobs.rs` (create)

**Interfaces:**
- Consumes: `PgJobs::batch_status`.
- Produces: `GET /api/libraries/{lib}/jobs/{batch_id}` → `200 BatchStatus` | `404`.
  Mounted under `Role::Api` only.

- [ ] **Step 1: Write the failing tests**

Create `crates/lapidary-api/tests/jobs.rs`:

```rust
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_running_batch_reports_its_counts(pool: PgPool) {
    let (batch, _) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");

    let app = router(AppState { db: pool }, Role::Api);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/libraries/{SEEDED_LIBRARY}/jobs/{batch}"))
                .body(Body::empty())
                .expect("builds"),
        )
        .await
        .expect("responds");

    assert_eq!(response.status(), StatusCode::OK);
    let status: BatchStatus = /* deserialize */;
    assert_eq!(status.total, 1);
    assert_eq!(status.pending, 1);
    assert!(status.finished_at.is_none());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_id_that_was_never_issued_is_not_found(pool: PgPool) {
    let app = router(AppState { db: pool }, Role::Api);
    let response = /* GET with a fresh BatchId::new() */;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_batch_belonging_to_another_library_is_not_found(pool: PgPool) {
    // Content addressing is not authorization, and a job id is no different.
    let (batch, _) = PgJobs(pool.clone())
        .enqueue_scan(seeded(), &["bracket-lp-1042-03.stl".to_owned()])
        .await
        .expect("enqueues");
    let other = LibraryId::new();
    let response = /* GET /api/libraries/{other}/jobs/{batch} */;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_does_not_serve_batch_status(pool: PgPool) {
    let app = router(AppState { db: pool }, Role::Worker);
    let response = /* GET a batch status URL */;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Implement**

Create `crates/lapidary-api/src/jobs.rs`:

```rust
//! Batch status: what a scan turned into. `api` role only -- it reads job rows, touches
//! no source file and invokes no kernel, so it belongs on the open path.
//!
//! The route is scoped under its library rather than being a bare `/api/jobs/{id}`.
//! CLAUDE.md: content addressing is not authorization. A batch id is a uuid a caller
//! might hold from anywhere, so reachability is checked, and scoping the route makes that
//! check structural instead of a step someone can forget.

pub async fn batch_status(
    State(state): State<AppState>,
    Path((library, batch)): Path<(LibraryId, BatchId)>,
) -> Response {
    match PgJobs(state.db.clone()).batch_status(library, batch).await {
        Ok(Some(status)) => Json(status).into_response(),
        // Also the answer for a scan that enqueued nothing: with no rows there is no
        // resource, and an empty batch is indistinguishable from an id never issued.
        // The client already knows from `ScanAccepted.queued == 0` and must not poll.
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "message": "No scan with that id has run in this library. Check the id, or \
                            start a new scan."
            })),
        )
            .into_response(),
        Err(error) => ApiError::from(error).into_response(),
    }
}
```

Mount in `lib.rs`'s `Role::Api` arm only:

```rust
.route("/api/libraries/{library}/jobs/{batch}", get(jobs::batch_status))
```

- [ ] **Step 3: Run the tests**

```sh
cargo test -p lapidary-api; echo "exit=$?"
cargo xtask check-layers; echo "exit=$?"
```

- [ ] **Step 4: Verify**

Move the route out of the `Role::Api` arm to an unconditional `.route(...)`;
`the_worker_role_does_not_serve_batch_status` must fail. Revert.

- [ ] **Step 5: Regenerate bindings and commit**

```sh
cargo xtask export-bindings; echo "exit=$?"
git add crates/lapidary-api web/src/bindings
git commit -m "feat(api): report a scan batch's status, scoped to its library"
```

---

### Task 12: Spawn the worker in `lapidary-server`

**Files:**
- Modify: `bin/lapidary-server/src/main.rs`, `bin/lapidary-server/Cargo.toml`,
  `deploy/compose.yaml`, `deploy/.env.example`

**Interfaces:**
- Consumes: `lapidary_jobs::run`, `lapidary_ingest::IngestHandler`.

- [ ] **Step 1: Extend the config**

Add to `Config`, following the existing `Option` + hand-check pattern and its reasoning:

```rust
    /// Only the worker reads these. Defaults live in `WorkerConfig::default()` so there
    /// is one place that decides them.
    worker_concurrency: Option<usize>,
    job_lease_secs: Option<u64>,
    job_poll_secs: Option<u64>,
    worker_id: Option<String>,
```

- [ ] **Step 2: Spawn the loop under `Role::Worker`**

In `main`, after the router is built and before `axum::serve`:

```rust
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = if role == Role::Worker {
        Some(spawn_worker(db.clone(), &config, shutdown.clone())?)
    } else {
        None
    };
```

with, under the same `#[cfg(feature = "mock-kernel")]` gate `worker_router` uses:

```rust
#[cfg(feature = "mock-kernel")]
fn spawn_worker(
    db: lapidary_db::PgPool,
    config: &Config,
    shutdown: tokio_util::sync::CancellationToken,
) -> Result<tokio::task::JoinHandle<()>> {
    let ingest_dir = config
        .ingest_dir
        .clone()
        .context("Could not start the worker loop: LAPIDARY_INGEST_DIR is not set.")?;
    let blob_root = config
        .blob_root
        .clone()
        .context("Could not start the worker loop: LAPIDARY_BLOB_ROOT is not set.")?;

    let defaults = lapidary_jobs::WorkerConfig::default();
    let worker_config = lapidary_jobs::WorkerConfig {
        worker_id: config.worker_id.clone().unwrap_or(defaults.worker_id),
        lease: config.job_lease_secs.map(Duration::from_secs).unwrap_or(defaults.lease),
        poll_interval: config
            .job_poll_secs
            .map(Duration::from_secs)
            .unwrap_or(defaults.poll_interval),
        concurrency: config.worker_concurrency.unwrap_or(defaults.concurrency),
        listen: true,
    };

    tracing::info!(
        worker = %worker_config.worker_id,
        concurrency = worker_config.concurrency,
        "job worker starting"
    );

    let handler = std::sync::Arc::new(lapidary_ingest::IngestHandler {
        db: db.clone(),
        ingest_dir,
        blob_root,
    });
    Ok(tokio::spawn(async move {
        if let Err(error) =
            lapidary_jobs::run(lapidary_db::PgJobs(db), handler, worker_config, shutdown).await
        {
            tracing::error!(%error, "the job worker stopped");
        }
    }))
}
```

- [ ] **Step 3: Shut it down with the server**

Replace the bare `axum::serve(...)` with a graceful-shutdown form that cancels the token on
`SIGTERM`/Ctrl-C, awaits the worker handle, and only then returns. A container restart must
release leases rather than orphan them for 60 seconds.

- [ ] **Step 4: Add the env vars to deploy**

In `deploy/compose.yaml`, on the `worker` service only, and in `deploy/.env.example` with a
comment each. Then:

```sh
cargo xtask check-deploy; echo "exit=$?"
```

Expected: exit 0. If `check-deploy` gained a rule about worker-only variables, satisfy the
rule rather than relaxing it.

- [ ] **Step 5: Prove the wiring end to end, live**

```sh
systemctl --user start podman.socket
podman compose -f deploy/compose.yaml up -d --build
podman logs lapidary-worker-1 2>&1 | grep "job worker starting"
curl -s -X POST "http://localhost:8081/api/libraries/01931b6e-0000-7000-8000-000000000001/scan"
# -> {"batchId":"...","queued":6}
curl -s "http://localhost:8080/api/libraries/01931b6e-0000-7000-8000-000000000001/jobs/<batchId>"
# poll until finishedAt is non-null; expect ingested 6, failedTotal 0
```

Confirm **zero WARN lines** in both containers on a cold start — slice 1 fixed that and it
must stay fixed.

- [ ] **Step 6: Commit**

```sh
git add bin/lapidary-server deploy
git commit -m "feat(server): run the job worker in the worker process"
```

---

### Task 13: The grid polls while a scan runs

**Files:**
- Modify: `web/src/lib/api.ts`, `web/src/routes/index.tsx`, `web/src/lib/strings.ts`
- Test: `web/src/routes/index.test.tsx` (modify)

**Read first:** spec §11's last risk. The poll must stop.

- [ ] **Step 1: Add the strings**

In `web/src/lib/strings.ts`, inside a new `scan` key:

```ts
  scan: {
    running: (done: number, total: number) =>
      `Scanning — ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} files.`,
    finished: (ingested: number, skipped: number) =>
      skipped === 0
        ? `Scan complete — ${ingested.toLocaleString('en-US')} added.`
        : `Scan complete — ${ingested.toLocaleString('en-US')} added, ${skipped.toLocaleString('en-US')} already here.`,
    /**
     * A file that will never appear. The count is what belongs on screen; the reason
     * per file is the failed-file drawer, which arrives in Phase 2.
     */
    failed: (count: number) =>
      count === 1
        ? '1 file could not be read. It will not appear in the grid.'
        : `${count.toLocaleString('en-US')} files could not be read. They will not appear in the grid.`,
  },
```

- [ ] **Step 2: Add the fetcher**

In `web/src/lib/api.ts`, following `fetchParts`' shape exactly:

```ts
export async function fetchBatchStatus(
  library: string,
  batch: string,
): Promise<BatchStatus> { /* ... */ }
```

- [ ] **Step 3: Poll, and stop**

In `index.tsx`:

```tsx
  const batch = useQuery({
    queryKey: ['batch', libraryId, batchId],
    queryFn: () => fetchBatchStatus(libraryId, batchId!),
    enabled: batchId !== null,
    // The poll stops itself. A batch that finishes while the tab is backgrounded must
    // not leave a closed laptop requesting a completed batch forever.
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })
```

- [ ] **Step 4: Write the failing test**

In `web/src/routes/index.test.tsx`:

```tsx
it('stops polling once the batch reports it finished', async () => {
  // Mutation guard: with `refetchInterval` left as a constant, the fetch count keeps
  // climbing after finishedAt is set and this test fails.
  const fetches = vi.fn()
  // ... mock fetchBatchStatus to count calls and return finishedAt on the second ...
  await waitFor(() => expect(fetches).toHaveBeenCalledTimes(2))
  await new Promise((r) => setTimeout(r, 100))
  expect(fetches).toHaveBeenCalledTimes(2)
})
```

- [ ] **Step 5: Run the web bar**

```sh
cd web && npm test; echo "exit=$?"
npm run typecheck; echo "exit=$?"
npm run build; echo "exit=$?"
cd .. && cargo xtask check-strings; echo "exit=$?"
```

All four must exit 0. `check-strings` and `no-bare-strings.test.ts` both fail if any copy
above was inlined in a component.

- [ ] **Step 6: Commit**

```sh
git add web
git commit -m "feat(web): show a scan's progress, and stop polling when it ends"
```

---

### Task 14: Crash resumption, end to end

**Files:**
- Create: `crates/lapidary-jobs/tests/resumption.rs`

This task exists because §1's sentence — *killing the worker mid-scan loses nothing but the
files actually in flight* — is the slice's whole claim, and a claim is tested as stated
rather than inferred from unit tests of its parts.

- [ ] **Step 1: Write the test**

```rust
//! The slice's central claim, tested as stated.

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_worker_dying_mid_scan_loses_only_what_it_held(pool: PgPool) {
    let jobs = PgJobs(pool.clone());
    // Six real fixtures, staged under plausible part numbers.
    // The six real generated fixtures from `example/parts/` -- the same set the live
    // stack ingests. Do NOT invent filenames here: `flange-lp-4400-02.stl` in particular
    // is `lapidary-cad`'s mock fixture KEY and is deliberately fictional (slice 1 renamed
    // it precisely because it once collided with a real fixture and gave two
    // contradictory answers for one filename).
    let files = [
        "flange-dn40-lp-3310-02.stl",
        "hex-spacer-m4x20-lp-2145-01.stl",
        "idler-pulley-lp-4820-00.stl",
        "mounting-plate-lp-1180-01.stl",
        "spur-gear-m2-20t-lp-5140-00.stl",
        "vee-block-lp-3072-02.stl",
    ];
    // ... copy all six from example/parts/ into a TempDir, enqueue them ...

    // First worker: let exactly two finish, then drop it WITHOUT cancelling -- no
    // graceful shutdown, no lease release. This is `kill -9`, not a restart.
    // ... run a worker with concurrency 1 until two jobs are 'done', then abort() it ...

    // The job it held is still 'running' with a live lease. Expire it the way time would.
    sqlx::query(
        "UPDATE job SET lease_expires_at = now() - interval '1 second' WHERE state = 'running'",
    )
    .execute(&pool)
    .await
    .expect("expires the dead worker's lease");

    // Second worker drains the rest.
    // ... run a fresh worker to completion ...

    let (done, parts): (i64, i64) = /* count done jobs and parts */;
    assert_eq!(done, 6, "every file lands");
    assert_eq!(parts, 6, "and none is ingested twice");
}
```

- [ ] **Step 2: Run it**

```sh
cargo test -p lapidary-jobs --test resumption; echo "exit=$?"
```

- [ ] **Step 3: Verify it tests resumption**

Remove the dequeue's expired-lease arm; this test must fail with `done == 5`, not hang.
Revert. Then remove `part_name_unique_per_library`; the test must fail on `parts == 6`
becoming 7 if the reclaimed job re-ingests. If it does not, the race is not being
reproduced — check that the first worker really was aborted mid-job.

- [ ] **Step 4: Run the full bar**

Every command in "The verification bar" above, each with `; echo "exit=$?"`. Do not report
this task complete on a partial run.

- [ ] **Step 5: Commit**

```sh
git add crates/lapidary-jobs
git commit -m "test(jobs): pin that a dying worker loses only the files it held"
```

---

## Ledger items this slice closes or opens

**Closes:** S3 (unique `derivative(revision_id, kind)`) — Task 1.

**Opens, with triggers:**

| Item | Trigger |
|---|---|
| Lease heartbeats (`renew_lease`) | Phase 2's STEP ingest, where one job can outlive its lease |
| `job` table retention | The first library large enough for `done` rows to matter. Deleting them is adjacent to the no-implicit-deletion rule: the failed list is the only record of why a file never appeared |
| `payload` is untyped `jsonb` | The second job kind. Then it becomes a `#[serde(tag = "kind")]` enum in `lapidary-core` |
| Permit-before-dequeue is untested | A job slower than a lease — Phase 2 |

## Self-review

Checked against the spec, section by section:

- §3.1 job grain → Tasks 1, 3, 10. §3.2 dequeue and reclamation → Task 4.
  §3.3 retry classification → Tasks 7, 9. §3.4 notify-not-load-bearing → Task 8 step 6.
  §3.5 uniqueness → Tasks 1, 9. §3.6 no heartbeats → Task 7 step 3 (doc corrected).
  §3.7 scoped status route → Tasks 6, 11.
- §4.1 crate placement → every task's Files block. §4.2 routes → Tasks 10, 11.
  §4.3 the seam → Task 7. §4.4 the loop → Task 8.
- §5 data flow → Tasks 10 (1–5), 8 (6–9), 11 (10–11).
- §6 schema → Task 1, verbatim. §7 types → Task 2, verbatim.
- §8 error handling → Tasks 9 (classification), 10 (walk failures), 11 (404 copy).
- §9 testing → every listed test appears, each with its mutation.
- §10 exit criterion → Task 12 step 5 (live) and Task 14 (automated).
- §11 risks → the ledger table above; the poll-stop risk is Task 13 step 4.

Type consistency: `PgJobs`, `JobRow`, `BatchStatus`, `ScanAccepted`, `HandlerError`,
`Next`, `next_state`, `WorkerConfig`, `run`, `IngestHandler` are each defined in exactly
one task and referenced by the same name everywhere after.

Known gap, stated rather than hidden: Tasks 9, 10, 11 and 13 give test bodies with elided
setup (marked `/* ... */`) where that setup is a verbatim repeat of an existing test's
fixture staging in the same file. The assertions — the part that decides whether the test
is worth anything — are complete in every case.
