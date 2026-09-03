# Phase 1, slice 2 — the job queue, and ingest that survives a crash

**Date:** 2026-09-03
**Status:** design approved, not yet planned
**Phase:** 1 (Ingest and grid), second of five slices
**Predecessor:** Slice 1, complete — `2026-09-02-phase-1-slice-1-ingest-design.md`, shipped
at `518dfb5` with CI green (run 33714239711)

---

## 1. Why this slice exists

Slice 1 made one sentence true: a mesh file on disk becomes a card with a thumbnail in the
grid. It did so **synchronously**. `POST /api/libraries/{id}/scan` walks the ingest
directory, and for each `.stl` hashes, parses, rasterizes and commits before the HTTP
response is written. A thousand files is a thousand sequential ingests inside one request.

Three things are wrong with that, and they are the whole of this slice:

1. **The request is the unit of durability.** If the worker restarts at file 700, the
   client gets a dropped connection and there is no record anywhere that files 1–699
   succeeded. Nothing resumes, because nothing was ever written down about the work.
2. **The failure list is in memory.** `ScanReport.failed` exists only in the response body.
   A user who closes the tab has no way to learn which file failed or why, short of
   scanning again and watching more carefully.
3. **One scan is served by exactly one process.** The commercial model meters a worker
   fleet (`ROADMAP.md`), and a fleet that cannot split a folder is not a fleet.

This slice makes a different sentence true: **a scan is a set of durable jobs, and killing
the worker mid-scan loses nothing but the files actually in flight.**

The roadmap's Phase 1 exit — "re-dropping the same folder completes in seconds via hash
short-circuit" — is already met by slice 1's short-circuit. What this slice adds to that
exit is the first half: dropping 1,000 STLs at all, without a request timeout deciding how
much of the work survives.

---

## 2. Scope

**In:**

- A `job` table and `JobRepository` in `lapidary-db` — enqueue, dequeue, complete, fail,
  reschedule, batch status
- Lease-based delivery: `FOR UPDATE SKIP LOCKED`, lease expiry, reclamation folded into
  the dequeue itself (§3.2)
- `LISTEN`/`NOTIFY` as a wake-up optimization over a polling floor (§3.4)
- The worker loop in `lapidary-jobs` — concurrency, backoff, graceful shutdown — behind a
  `JobHandler` trait, so the L2 crate never learns what a mesh is (§4.3)
- `lapidary-ingest` reimplemented as a `JobHandler`: slice 1's per-file body, lifted out
  of its loop unchanged
- `POST /scan` becomes an enqueue returning `202 { batch_id, queued }`
- `GET /api/libraries/{lib}/jobs/{batch_id}` returning `BatchStatus`, scoped under its
  library (§3.7)
- A partial-failure-safe uniqueness constraint on `part`, which is what makes at-least-once
  delivery correct rather than merely nominal (§3.5)
- The web grid polls batch status while a scan runs, via TanStack Query
- **Rider from slice 1's ledger (S3):** `unique (revision_id, kind)` on `derivative`

**Out, with the slice that covers each:**

| Deferred | Slice |
|---|---|
| SSE progress replacing the poll | 5 |
| Lease heartbeats (`renew_lease`) | Phase 2 — see §3.6 for the trigger |
| LOD ladder, 3MF/OBJ | 3 |
| Browser upload, `variant=original` download | 4 |
| Virtualized grid, seed part | 5 |
| Remote workers leasing over HTTP | 4 |
| A failed-file drawer in the UI | Phase 2 |
| Job kinds other than `ingest_file` | whichever slice needs the second kind |

**Explicitly not a goal:** priorities, fair scheduling between libraries, or job
cancellation. One `order by run_after` is the whole scheduling policy. When a second job
kind arrives and starves the first, that is the signal to revisit — not before.

---

## 3. Decisions

### 3.1 One job per file, grouped by a batch

The alternative was one job per scan, resumed by re-walking the directory and letting the
hash short-circuit skip what was done. That is genuinely cheap — slice 1 measured 177 ms
cold against 10 ms warm — and it was the simpler schema. It was rejected for two reasons
that are not about speed:

- A scan is served by one worker, so no fleet can ever split a folder. The queue's shape
  would have to change to make the metered product work.
- One malformed file inside a scan job puts the *whole scan* into retry. Per-file grain is
  what lets a poison file fail alone.

A two-level parent/child design was also rejected: it buys nothing over a batch id and
imports the "when is the parent finished" race, which is where that pattern usually breaks.

`batch_id` is therefore a **grouping column, not a row**. There is no batch entity to keep
consistent; `BatchStatus` is a `GROUP BY` over jobs that share one id. Nothing can be
stale, because nothing is denormalized.

The directory walk stays synchronous inside the request. It is `read_dir` and nothing else
— no hashing, no parsing — so a thousand entries is one `read_dir` and one multi-row
insert. Keeping it in the request buys a real property: **a missing or unreadable ingest
directory is a `4xx`/`5xx` the user sees immediately**, rather than a job that fails
silently somewhere behind a poll.

### 3.2 Lease reclamation folds into the dequeue; there is no reaper

The dequeue's `where` clause admits two kinds of row: a pending job whose backoff has
elapsed, and a running job whose lease has expired.

```sql
update job set state = 'running',
               attempts = attempts + 1,
               leased_by = $1,
               lease_expires_at = now() + $2,
               updated_at = now()
where id = (
    select id from job
    where (state = 'pending' and run_after <= now())
       or (state = 'running' and lease_expires_at < now())
    order by run_after
    for update skip locked
    limit 1
)
returning id, batch_id, library_id, kind, payload, attempts, max_attempts;
```

A crashed worker's job is reclaimed by whichever worker next dequeues. No sweeper process
exists, so no sweeper process can be the thing that died.

Because `attempts` increments on reclamation exactly as it does on retry, the **poison-pill
case is capped by the same counter as ordinary failure**. A file that panics the worker
before it can record anything is tried three times; on the fourth claim the loop observes
`attempts > max_attempts` and marks it `failed` without running the handler, with an error
that says the worker holding it stopped responding rather than pretending the file is bad.

That check lives in the loop rather than in the SQL deliberately. Excluding exhausted rows
in the `where` clause would leave them `running` with an expired lease forever — invisible
to the dequeue and invisible to any cleanup, which is the classic way this table grows a
permanent population of zombies.

### 3.3 A parse failure is terminal on the first attempt

The bytes are content-addressed and immutable. Parsing the same blob again is guaranteed to
produce the same error, so retrying a malformed STL three times over 40 seconds only delays
an answer that was available immediately. The generic "retry everything N times" default is
actively wrong here, and it is wrong for a reason specific to this system rather than a
matter of taste.

The handler reports what happened; the loop decides what to do about it:

| Handler result | Loop's action |
|---|---|
| `Ok(Outcome::Ingested \| Skipped)` | `state = 'done'`, `outcome` set |
| `Err(Permanent { message })` | `state = 'failed'`, `last_error = message`, no retry |
| `Err(Transient { message })` | `state = 'pending'`, `run_after = now() + backoff` |
| either, with `attempts >= max_attempts` | `state = 'failed'` |

Backoff is `2 s, 8 s, 30 s` — fixed, not exponential-with-jitter, because with
`max_attempts = 3` the third value is the only one that matters and a table is easier to
reason about than a formula.

`Permanent` is the mesh errors: unparseable STL, a truncated file, a thumbnail that cannot
be encoded under 64 KB. `Transient` is everything about infrastructure: the database
unreachable, the blob store unwritable, a lost lease. **When in doubt the classification is
`Transient`**, because a retried permanent failure costs one wasted parse while a
non-retried transient failure costs the user a file.

### 3.4 `LISTEN`/`NOTIFY` is a latency optimization and must never be load-bearing

The worker polls every 5 seconds and wakes early on a notification. Both mechanisms are
present, and the polling floor is the correctness mechanism.

This is not belt-and-braces. `NOTIFY` fires into the void when nothing is listening, so a
worker that starts *after* an enqueue would never learn about that work if notification
were the only path. So would a worker whose listener connection dropped and reconnected. A
design where the queue drains only when a notification happens to be delivered is a design
that stalls silently under exactly the conditions — restarts, reconnects — this slice
exists to survive.

§9 pins this with a test that disables the listener entirely and asserts the queue still
drains, so the property cannot decay into "notify happens to be what works".

### 3.5 A uniqueness constraint is what makes at-least-once delivery safe

Lease expiry means two workers can genuinely run the same job concurrently: worker A stalls
past its 60-second lease but is still alive, worker B reclaims and starts. Slice 1's
short-circuit does not prevent the resulting duplicate, and this is worth stating plainly
because it looks like it should. Both workers call
`blobs.library_holds(library, name, hash)`, both get `false` — neither has committed yet —
both parse, and both insert a part. The check is not atomic with the insert, so no amount
of checking harder fixes it.

The fix is a database-level guarantee:

```sql
alter table part add constraint part_name_unique_per_library unique (library_id, name);
```

The loser's insert raises a unique violation, and the handler maps that specific error to
`Outcome::Skipped` — someone else already did this. At-least-once delivery becomes
effectively-once at the only place that can enforce it.

**The constraint's key must agree with `library_holds`'s filter.** Both ignore
`deleted_at`, which is S11's recorded decision: a re-scan does not resurrect a soft-deleted
part, it reports it skipped. If the two ever diverge — one filtering soft-deleted rows and
the other not — the check would pass and the insert would throw on a path that has no
reason to expect it. That is precisely the class of silent disagreement between two
statements of the same rule that this project has dug out repeatedly, so the migration and
`library_holds` carry a comment pointing at each other.

The walk is non-recursive `read_dir`, so filenames within one scan are unique by the
filesystem and the constraint cannot fire on a legitimate directory. The one case it can
fire on is `BRACKET.STL` beside `bracket.stl` on a case-sensitive filesystem, where the
stems collide. Failing that file loudly is the correct behaviour; S10's source-path column
is what will eventually let both exist.

### 3.6 No heartbeats in this slice

`lapidary-jobs`' module doc says workers "heartbeat" their leases. They will not, yet, and
the doc is corrected rather than left to over-promise.

A single-file mesh ingest measures roughly 200 ms against a 60-second lease. A heartbeat
task would renew a lease that has 59.8 seconds left, which is ceremony that still has to be
tested and can still be wrong. `renew_lease` lands when a job can realistically outlive its
lease — STEP ingest through the OCCT sidecar in Phase 2, where a single part can take
minutes. Recorded as a scheduled item with that trigger.

### 3.7 The status route is scoped under its library

`GET /api/libraries/{library_id}/jobs/{batch_id}`, not `GET /api/jobs/{batch_id}`.

`CLAUDE.md` says content addressing is not authorization: knowing a hash must never grant
access, and reachability is always checked. A batch id is a uuid the caller might hold from
any source, and the same reasoning applies to it. Scoping the route means the handler
verifies the batch belongs to that library before returning anything about it, and the
check is structural rather than a step someone can forget to write.

The route is served by the **api** role. It reads job rows; it touches no source file and
invokes no kernel, so it belongs on the open path.

---

## 4. Architecture

### 4.1 Where each piece lives

| Crate | Layer | Gains |
|---|---|---|
| `lapidary-core` | L0 | `JobId`, `BatchId`, `JobState`, `Outcome`, `BatchStatus`, `JobFailure` — the ts-rs-exported wire shapes |
| `lapidary-db` | L1 | migration `0003`, `JobRepository` impl, the `PgListener` — **all** the SQL |
| `lapidary-jobs` | L2 | the worker loop, backoff policy, concurrency, shutdown, the `JobHandler` trait |
| `lapidary-ingest` | L3 | `impl JobHandler` for `ingest_file`; `scan` becomes an enqueue |
| `lapidary-api` | L3 | the batch status route |
| `bin/lapidary-server` | bin | spawns the worker loop when `LAPIDARY_ROLE=worker` |
| `web` | — | polls batch status while a scan is running |

### 4.2 Routes by role

```
role=api      GET  /api/healthz
              GET  /api/libraries/{id}/parts?after=&limit=
              GET  /api/libraries/{lib}/jobs/{batch_id}          <- new

role=worker   GET  /api/healthz
              POST /api/libraries/{id}/scan                      <- now enqueues
```

No route moves, as slice 1 §4.2 promised. The scan route stays on the worker because the
ingest directory is mounted only there, and the walk still needs to read it.

### 4.3 The `JobHandler` seam

`lapidary-jobs` is L2, so it may not depend on `lapidary-cad` (also L2) or on
`lapidary-ingest` (L3). The layering rule forbids exactly the edge a naive worker loop
would need, which is fortunate, because the resulting design is the better one anyway:

```rust
#[async_trait]
pub trait JobHandler: Send + Sync + 'static {
    async fn handle(&self, job: &Job) -> Result<Outcome, HandlerError>;
}

pub enum HandlerError {
    /// Re-running this job cannot succeed. The input is immutable.
    Permanent { message: String },
    /// Something outside the job failed. Worth another attempt.
    Transient { message: String },
}
```

The trait has no `kinds()` method and the dequeue does not filter by `kind`, because
there is one handler and one kind. Adding routing now would export a mechanism nothing
consumes — the same shape as `Approximate<T>` and `volume_approximate()`, both of which
slice 1 shipped unconsumed and had to schedule. `kind` is still stored, so the second job
kind is a migration-free change; routing arrives with it.

`lapidary-jobs` owns delivery and policy and knows nothing about meshes.
`lapidary-ingest` owns one file's worth of work and knows nothing about leases.
`bin/lapidary-server` is the only place that has seen both.

The handler's body is slice 1's per-file logic **moved, not rewritten** — hash, then
`library_holds`, then kernel, then `link_existing` or `put`+`record`+reap. That ordering is
load-bearing and documented at length in `scan.rs`; this slice changes what calls it, not
what it does. The one addition is mapping a unique violation to `Outcome::Skipped` (§3.5).

### 4.4 The worker loop

```
loop {
    if shutting_down { break }

    // The permit is acquired BEFORE the dequeue, never after. Leasing a job we have no
    // capacity to start would burn lease time sitting in a queue, and a lease that
    // expires while the job waits its turn is indistinguishable from a crashed worker.
    let permit = concurrency.acquire().await;

    match repo.dequeue(worker_id, LEASE).await {
        Ok(Some(job)) if job.attempts > job.max_attempts => {
            repo.fail(job.id, ABANDONED_MESSAGE).await
        }
        Ok(Some(job)) => spawn_bounded(handle_and_record(job)),
        Ok(None)      => select! {
                             _ = listener.recv() => {}       // woken early
                             _ = sleep(POLL_INTERVAL) => {}  // the floor
                         },
        Err(e)        => { warn!(...); sleep(POLL_INTERVAL).await }
    }
}
// graceful shutdown: stop dequeuing, await in-flight, release their leases
```

| Setting | Default | Env |
|---|---|---|
| Lease duration | 60 s | `LAPIDARY_JOB_LEASE_SECS` |
| Poll interval | 5 s | `LAPIDARY_JOB_POLL_SECS` |
| Concurrency | 4 | `LAPIDARY_WORKER_CONCURRENCY` |
| Worker identity | `{hostname}-{pid}` | `LAPIDARY_WORKER_ID` |

**Graceful shutdown releases leases** — in-flight jobs go back to `pending` with
`run_after = now()`. A planned restart therefore resumes instantly instead of waiting out a
60-second lease. A crash does not get this, which is what lease expiry is for; the two
paths are separate because only one of them can run cleanup code.

---

## 5. Data flow

```
POST /api/libraries/{id}/scan            [worker role]
  1. read_dir(ingest_dir), filter *.stl (case-insensitive)   <- no hashing, no parsing
  2. batch_id = uuidv7
  3. INSERT ... SELECT * FROM unnest($paths)   one statement, N rows, state='pending'
  4. NOTIFY lapidary_jobs
  5. 202 { batch_id, queued: N }

worker loop                              [worker role, background task]
  6. dequeue -> lease  (or reclaim an expired one, §3.2)
  7. attempts > max_attempts? -> fail as abandoned, goto 6
  8. handler: read bytes -> BLAKE3 -> library_holds? -> kernel -> record
  9. Ok        -> done, outcome = ingested | skipped
     Permanent -> failed, last_error
     Transient -> pending, run_after = now() + backoff  (or failed if exhausted)

GET /api/libraries/{lib}/jobs/{batch_id} [api role]
 10. verify the batch belongs to {lib}                        <- §3.7
 11. GROUP BY state -> BatchStatus, failures capped at 100
```

Steps 1–5 are the request. Everything after it survives the client disconnecting, the
worker restarting, and the worker being killed.

---

## 6. Schema — migration `0003_jobs.sql`

```sql
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
    -- An implication, not an equivalence, and the asymmetry with the constraint above
    -- is deliberate. A failed job must say why. But a job that is RETRYING after a
    -- transient failure keeps its last error too, so an operator can see why a batch is
    -- slow without waiting for it to exhaust its attempts -- and §3.3's retry path sets
    -- state='pending' while holding last_error, which an equivalence would refuse.
    -- `job_done_has_outcome` stays an equivalence because an outcome on a non-terminal
    -- row genuinely is incoherent; a last error on one is not.
    constraint job_failed_has_reason
        check (state <> 'failed' or last_error is not null)
);

-- The dequeue index. Partial: only pending rows are ever selected by run_after, and the
-- table is expected to accumulate 'done' rows in the millions.
create index job_dequeue_idx on job (run_after) where state = 'pending';

-- Reclamation of expired leases (§3.2).
create index job_expired_lease_idx on job (lease_expires_at) where state = 'running';

-- BatchStatus' GROUP BY (§3.7).
create index job_batch_idx on job (batch_id);

-- §3.5. The key must agree with PgBlobs::library_holds, which deliberately does NOT
-- filter deleted_at (slice 1, S11). If one of the two starts filtering and the other
-- does not, library_holds returns false and this constraint throws on a path with no
-- reason to expect it.
alter table part add constraint part_name_unique_per_library unique (library_id, name);

-- Rider from slice 1's ledger, S3: a revision has at most one derivative of each kind.
alter table derivative add constraint derivative_kind_unique_per_revision
    unique (revision_id, kind);
```

`0003` is the **first migration added since the `build.rs` fix** in `lapidary-db`. Slice 1's
ledger recorded that adding a new `.sql` file previously compiled nothing, leaving
`sqlx::migrate!` embedding a stale set — the worse of the two staleness cases, and exactly
this slice's opening move. The fix is in place; this migration is its first real exercise,
and the plan should verify that a clean `cargo test` picks up `0003` before anything is
built on top of it.

---

## 7. Domain types

In `lapidary-core`, ts-rs-exported, alongside `LibraryId` and `PartId`:

```rust
pub struct JobId(Uuid);
pub struct BatchId(Uuid);

pub enum JobState { Pending, Running, Done, Failed }
pub enum Outcome  { Ingested, Skipped }

/// What a scan turned into. Aggregated from job rows; never stored.
pub struct BatchStatus {
    pub batch_id: BatchId,
    pub library_id: LibraryId,
    pub total: u32,
    pub pending: u32,
    pub running: u32,
    pub ingested: u32,
    pub skipped: u32,
    pub failed_total: u32,
    /// Capped at 100, ordered by `(created_at, id)`. The `id` tiebreaker is
    /// load-bearing, not decoration: `enqueue_scan` writes a whole batch in one
    /// statement and Postgres's `now()` is constant per transaction, so every job in a
    /// batch shares an identical `created_at` and ordering by it alone discriminates
    /// nothing. `JobId` is uuidv7 generated in insertion order, so the tiebreaker
    /// reproduces enqueue order — which, since the scan sorts paths first, is the
    /// alphabetical order a reader expects. Without it the list would reshuffle between
    /// polls under whatever scan order the planner happened to pick.
    pub failed: Vec<JobFailure>,
    pub started_at: Timestamp,
    /// Set once no job in the batch is pending or running.
    pub finished_at: Option<Timestamp>,
}

pub struct JobFailure {
    pub path: String,
    pub reason: String,
    pub attempts: u32,
}
```

`started_at` is `min(created_at)` over the batch and `finished_at` is `max(updated_at)`,
set only once no job in the batch is `pending` or `running`. `failed` is the first 100
failures **ordered by `(created_at, id)`** — see the type's own comment for why the
tiebreaker is required rather than defensive. An earlier draft of this spec claimed
`created_at` alone made the list stable across polls; it does not, because a batch is
inserted in one transaction and therefore shares one timestamp.

One inherited trap: **sqlx 0.9 has no `jiff` feature** — it ships `chrono` and `time`. The
slice-1 grid query already selects microseconds and reconstructs with
`jiff::Timestamp::from_microsecond`, and these two timestamps must do the same. This cost a
plan revision in slice 1; it is written down here so it costs nothing in this one.

`ScanReport`'s counters are not deleted; they move here. `ingested`, `skipped` and the
per-file failure with its reason are the same four things slice 1 returned, relocated from
a response body that vanished with the connection to a row that does not.

`POST /scan`'s new response is `ScanAccepted { batch_id, queued }`.

Both are new ts-rs bindings, so `cargo xtask export-bindings` must produce them — a gate
that was red in CI from `9a79045` until `518dfb5` and is now genuinely green, which is what
makes it worth relying on here.

---

## 8. Error handling

Per `CLAUDE.md`, errors say what broke and what to do.

`JobsError` already exists in the `lapidary-jobs` stub with two variants written for this
slice; both are used as written:

- `QueueUnavailable` — the database is down. Its message already makes the point that
  matters, which is that Lapidary queues in PostgreSQL, so this is not a broker outage to
  go hunting for.
- `LeaseExpired { job_id }` — the handler finished but the lease was gone. The result is
  still recorded: the work is idempotent by §3.5, so the second writer takes `Skipped`
  rather than throwing away a completed ingest.

New:

- The walk failing is a synchronous response, reusing slice 1's existing ingest-directory
  errors unchanged.
- An empty directory is `202 { batch_id, queued: 0 }` — not an error. Scanning an empty
  folder is a reasonable thing to do and it succeeded.

  **A batch with zero jobs has no status resource**, and `GET .../jobs/{batch_id}` returns
  `404` for it. `batch_id` is a grouping column rather than a row (§3.1), so a batch that
  enqueued nothing has nothing to group: it is not distinguishable from an id that was
  never issued, and §3.7's ownership check has no row to verify against. The client does
  not need the status route here, because `queued: 0` already told it the whole story —
  and the web must therefore not begin polling when `queued == 0`. This is the one place
  the batch-as-grouping-column decision costs something, and paying it is still cheaper
  than keeping a batch entity consistent for the sake of the empty case.
- `last_error` stores the handler's message verbatim. A user reading the failed list in the
  UI must see "Could not read this STL — it declares 12 facets but the file ends after 7",
  not a code.

---

## 9. Testing

The dominant defect pattern across this project has been *a test whose name claims a
mechanism while its body asserts an outcome that would have held anyway*. Every test below
is therefore specified with the mutation that must break it, and the plan must record that
each mutation was applied and observed failing.

**Delivery, against a live database:**

| Test | Mutation that must make it fail |
|---|---|
| two concurrent dequeues, one job, exactly one winner | drop `for update` -> both win |
| an expired lease is reclaimed and `attempts` incremented | drop the `lease_expires_at <` arm -> job stranded |
| `attempts > max_attempts` is failed as abandoned, never handled | remove the loop's cap -> runs forever |
| a `pending` job with `run_after` in the future is not dequeued | drop `run_after <= now()` -> backoff ignored |

**Policy:**

| Test | Mutation |
|---|---|
| a malformed STL is `failed` at `attempts = 1` | classify `Permanent` as `Transient` -> reaches 3 |
| a transient failure sets `run_after` in the future | set `run_after = now()` -> no backoff |
| a unique violation becomes `Skipped`, not a failure | remove the mapping -> a spurious failed file |

**Status:**

| Test | Mutation |
|---|---|
| `BatchStatus` counts only its own batch | drop `where batch_id =` -> counts bleed across scans |
| a batch id from another library is not readable via `{lib}` | drop the ownership check -> §3.7 defeated |
| `failed` caps at 100 while `failed_total` reports all | drop the cap -> unbounded payload |
| a scan of an empty directory returns `queued: 0`, and its batch id 404s | return an empty `BatchStatus` instead -> an unissued id reads as a real, finished batch |
| the web stops polling once `finished_at` is set | leave `refetchInterval` on -> a completed batch is requested forever |

**The two that matter most:**

- **The queue drains with the listener disabled.** Start a worker with `LISTEN` switched
  off, enqueue, assert the batch completes on the polling floor alone. This exists so
  `NOTIFY` can never quietly become the correctness mechanism (§3.4). Mutation: make the
  loop wait only on notification — the test must hang and fail, not pass slowly.
- **A real STL end to end through the queue**, asserting the part lands with the fixture's
  actual bounding box (88 x 40 x 25), its 20 triangles, watertightness, and a
  `thumb_bytes` that decodes as a 512 px WebP. This is the direct descendant of
  `scanning_one_real_stl_ingests_it_once`, whose absence let empty thumbnails and zeroed
  measurements pass all 183 tests in slice 1. Moving ingest behind a queue puts a brand-new
  seam at precisely that spot, so it gets its guard on day one instead of in a fix wave.
  Mutations: zero the thumbnail; zero the measurements. Both must fail this test **by
  name**.

**Crash resumption, as an integration test rather than a claim:** enqueue six real STLs,
run a worker until two are `done`, drop the worker without cleanup, start a second worker,
assert all six land and that the two already done were not re-ingested. This is the
sentence in §1 that the slice exists to make true, so it is tested as stated rather than
inferred from unit tests of its parts.

---

## 10. Exit criterion

Drop the six fixture STLs into the ingest directory and `POST /scan`. The response is
`202` in well under a second with a `batch_id`. The grid fills in as the worker commits
parts. `kill -9` on the worker mid-scan, then restart it: the batch completes, every part
has a thumbnail, and no part is duplicated.

`GET /api/libraries/{lib}/jobs/{batch_id}` reports `total: 6` and, once drained,
`ingested: 6, failed_total: 0` with `finished_at` set. Scanning the same directory again
returns a new `batch_id` that drains to `skipped: 6` — slice 1's short-circuit still doing
its job, now through the queue.

Add one deliberately truncated STL and the batch reports `failed_total: 1` with a reason a
person can act on, at `attempts: 1` rather than 3.

---

## 11. Risks

**The `job` table grows without bound.** Every ingested file leaves a `done` row forever.
At the roadmap's 1,000-STL exit that is 1,000 rows, which is nothing; at a million-part
library it is a million. The partial dequeue index means query performance does not
degrade, so this is a storage question rather than a correctness one, and it is left
deliberately unsolved. Retention is a policy decision, and deleting job history is
adjacent to `CLAUDE.md`'s rule against implicit deletion of user data — the failed list is
the only record of why a file never appeared. Recorded as scheduled with the trigger:
**the first library that makes the table large enough to notice.**

**`payload` as `jsonb` is weakly typed.** A second job kind could put a different shape in
the same column with nothing to catch it. Accepted for one kind; the moment there are two,
the shapes belong in `lapidary-core` as an enum with a `#[serde(tag = "kind")]`
representation so the column is at least parsed against a closed set.

**Polling costs a query every 5 seconds per worker even when idle.** With the partial index
this is an index scan returning nothing. It is the price of not depending on `NOTIFY` for
correctness, and it is the right trade.

**The web poll can outlive the scan.** A batch that finishes while the tab is backgrounded
must stop the poll on `finished_at`, or a closed laptop wakes up requesting a completed
batch forever. TanStack Query's `refetchInterval` returning `false` on a finished batch is
the whole fix, and it is easy to forget — so it gets a test.

---

## 12. What this unblocks

Slice 3 extends `MeshKernel` to the LOD ladder and settles `KernelOutput`'s shape; with the
queue in place, generating three LODs per file is more work per job rather than a longer
request. Slice 4 adds the browser upload path, which enqueues into the same table from a
different producer. Slice 5 replaces the status poll with SSE over the same `BatchStatus`
shape, so the API does not change — only its transport.

Phase 2's STEP ingest is what finally makes lease heartbeats necessary (§3.6), and Phase 4's
remote workers lease over HTTP against this same table, which is why leasing is modelled as
a row the coordinator owns rather than as anything held in a worker's memory.
