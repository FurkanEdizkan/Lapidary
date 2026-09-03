# Phase 1 slice 2 — handoff

> ## ✅ SLICE COMPLETE — 2026-09-04
>
> All 14 tasks are landed on `main`, plus the two tests task 8 owed. The bar is
> **248 passed / 0 failed** with `fmt`, `clippy` (both feature configurations),
> `check-layers`, `check-deploy`, `check-strings` and `export-bindings` at exit 0,
> and web at **32 passed** with typecheck and build at exit 0.
>
> The spec's §10 exit criterion has been run live end to end against
> `docker compose`, including the `kill -9` clause — see **What the exit run
> actually showed** below.
>
> Everything from here down is the mid-slice state as it stood when execution
> stopped after task 9. It is kept because the rulings, the environment notes and
> the reachability war story are all still true and still useful. The task table
> immediately below is the part that is now historical.
>
> **Next:** slice 3 — the LOD ladder (L0/L1/L2), 3MF/OBJ, and settling
> `KernelOutput`'s shape. It has no spec yet.

**Written:** 2026-09-04, with tasks 1–9 of 14 landed and the tree clean.
**Branch:** `rust-rewrite`, HEAD `e806231` — tasks 1–9's 22 commits were pushed to
`origin/rust-rewrite` alongside this document, so CI has seen them.
**Plan:** `docs/superpowers/plans/2026-09-03-phase-1-slice-2-jobs.md` (14 tasks)
**Spec:** `docs/superpowers/specs/2026-09-03-phase-1-slice-2-jobs-design.md` — binding authority

The plan is being executed with `superpowers:subagent-driven-development`. **Its ledger
lives in `.superpowers/sdd/`, which is gitignored and will not survive a fresh clone.**
Everything from it that a successor needs is reproduced here.

---

## Where execution stopped

| Task | State |
|---|---|
| 1 — Migration 0003: `job` table + two uniqueness constraints | **complete**, reviewed |
| 2 — Job domain types in `lapidary-core` | **complete**, reviewed |
| 3 — `PgJobs::enqueue` — one statement, N rows, one notify | **complete**, reviewed |
| 4 — `PgJobs::dequeue` — leasing and reclamation | **complete**, reviewed |
| 5 — completion, failure, backoff, release | **complete**, reviewed + 2 fix rounds |
| 6 — `PgJobs::batch_status` | **complete**, reviewed |
| 7 — handler seam and retry policy | **complete**, reviewed |
| 8 — the worker loop | **production code complete; two tests still owed** |
| 9 — `ingest_one` becomes a `JobHandler` | **complete**, acceptance gate run by hand |
| 10–14 | **not started** ← resume here |

At HEAD the recorded bar is **242 passed / 0 failed**, with `fmt`, `clippy`,
`check-layers`, `check-deploy`, `check-strings` and `deny` all exiting 0.

---

## The exact next action

Two things are owed before task 10, and they are small:

**1. Task 8's two missing tests.** The await-in-flight fix shipped in `e70b9df` with its
production code verified but **its tests never written** — the commit message says so
rather than implying the fix is proven. Both still stand:

- *Mid-flight shutdown:* block a handler, cancel the token, assert the handler's real
  outcome is recorded rather than reverted.
- *The listener test's mechanism is narrower than its name claims.* Today both jobs are
  enqueued **before** the worker starts, so they are claimed on the first two iterations
  without ever entering `wait_for_work`; the mutation-induced hang comes from the third,
  idle iteration. That proves "an idle loop notices cancellation", not "the poll floor
  discovers work enqueued while the worker sleeps", which is what spec §3.4 is about.
  Fix: start the worker against an **empty** queue with `listen:false`, wait until it is
  genuinely idle, **then** enqueue — and correct the comment to describe what it proves.
  An overclaiming comment is how the next person inherits false confidence.

**2. Then task 10.** `scan` becomes an enqueue: the `read_dir` walk stays in the request
(a missing mount must be a response the user sees, not a job that fails behind a poll),
the body returns `202 ScanAccepted`, and
`the_worker_router_serves_both_health_and_scan` in `bin/lapidary-server/src/main.rs` moves
from `StatusCode::OK` to `StatusCode::ACCEPTED` in the same commit — leaving it is a red
workspace.

### Then, in order

| Task | Shape | Notes |
|---|---|---|
| 11 | `GET /api/libraries/{lib}/jobs/{batch}` → `200 BatchStatus` \| `404` | New `crates/lapidary-api/src/jobs.rs`, mounted under `Role::Api` only. `batch_status` returning `None` **is** the 404 |
| 12 | Spawn `lapidary_jobs::run` under `Role::Worker` | Touches `bin/lapidary-server`, `deploy/compose.yaml`, `deploy/.env.example`. Worker knobs default in `WorkerConfig::default()` so one place decides them |
| 13 | The grid polls while a scan runs | `web/src/lib/{api,strings}.ts`, `web/src/routes/index.tsx`. Spec §11's last risk: **the poll must stop** |
| 14 | Crash resumption, end to end | The slice's whole claim — killing the worker mid-scan loses nothing but the files actually in flight — tested as stated, not inferred from unit tests of its parts |

Task 12 step 5 is the live exit check; task 14 is its automated twin. Both are required.

To regenerate the SDD workspace and the remaining briefs:

```sh
S=~/.claude/plugins/cache/claude-plugins-official/superpowers/6.3.0/skills/subagent-driven-development
P=docs/superpowers/plans/2026-09-03-phase-1-slice-2-jobs.md
"$S/scripts/sdd-workspace" "$P"
for n in $(seq 10 14); do "$S/scripts/task-brief" "$P" $n; done
```

---

## Environment needed to resume

Tests require a live PostgreSQL 18. Either runtime works — `CLAUDE.md` says Podman **and**
Docker, and nothing in the workspace can tell the difference: `cargo xtask check-deploy`
parses `deploy/compose.yaml` structurally rather than invoking a runtime.

```sh
# Podman
podman run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/postgres:18

# Docker — same image, same ports
docker run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/postgres:18

export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```

**Plain `postgres:18`, not the `lapidary-db` image.** An earlier draft of this document
said `docker.io/library/lapidary-db:latest`, which does not exist on any registry —
`lapidary-db` is built locally from `deploy/db/Containerfile`. It is not needed here
either: `ci.yml`'s rust job runs the whole suite against stock `postgres:18`, and nothing
under `#[sqlx::test]` touches `pgvector` or the Turkish text-search config. The custom
image is only for the compose stack, where task 12 step 5's live check needs it.

`podman compose` needs the socket first: `systemctl --user start podman.socket`. `docker
compose` needs nothing equivalent.

The verification bar, which is exactly what `ci.yml` runs:

```sh
cargo fmt --all --check          # CI's first gate; keep it first
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo test --workspace --all-features
cargo deny check
```

### Check the database is *reachable*, not merely *up*

This slice cost a session to a new failure mode. Seven `lapidary-api` tests failed with
`PoolTimedOut` at 30 s while `podman ps` said "Up 2 hours" and `pg_isready` **inside** the
container reported accepting connections. `ss -ltn` showed nothing listening on host port
55432: podman's rootless port forwarder had died while the container stayed healthy
internally. Zero leftover `_sqlx_test` databases and 9 connections against
`max_connections=100` ruled out pool exhaustion, which is what pointed at the network path.

The story below is Podman's, but the check is not: `docker ps` and an in-container
`pg_isready` answer the same wrong question. **Use a host-side TCP connect:**

```sh
bash -c 'cat < /dev/null > /dev/tcp/127.0.0.1/55432' && echo reachable
```

"Container up" and "container reachable" are different claims, and only the second is the
one the tests depend on. Restarting the container recreates the forwarder.

---

## Rulings made on the owner's behalf

1. **Execute on `rust-rewrite`, no worktree** — consistent with slice 1 and every prior
   round. The branch is isolated from `main` and CI-verified on push. Cost if wrong: none
   structural.
2. **Never two implementers in one worktree at once.** This bit for the first time in this
   slice: task 8's fix round and task 9 both died mid-flight with their work intermingled
   and uncommitted in one tree. They were separated by inspection, not by guessing —
   `crates/lapidary-jobs/src/worker.rs` was task 8's await-in-flight fix,
   `crates/lapidary-db/src/jobs.rs` was task 8's stale-write logging, `crates/lapidary-ingest/*`
   was task 9 — and committed separately as `e70b9df` and `e806231` so history stays honest
   about which task owned which change. Note also that `git commit` commits the whole index,
   not just what one agent added.

---

## What this slice proved, and what it did not

**Proved, and it is the most important thing here:** task 9's acceptance gate pinned the
ingest seam that slice 1 shipped untested, *by name*. Two mutations were run by hand —
zeroing the thumbnail at the seam, then zeroing the measurements — and in both cases
`a_real_stl_ingests_with_its_real_measurements_and_a_decodable_thumbnail` failed while the
other three tests passed. Both were reverted; `handler.rs` was byte-identical to its backup
afterwards.

**Not proved:** permit-before-dequeue. It needs a job slower than a lease, which arrives
with Phase 2's STEP ingest.

---

## Ledger items this slice opens

| Item | Trigger |
|---|---|
| Lease heartbeats (`renew_lease`) | Phase 2's STEP ingest, where one job can outlive its lease |
| `job` table retention | The first library large enough for `done` rows to matter. Deleting them is adjacent to the no-implicit-deletion rule: the failed list is the only record of why a file never appeared |
| `payload` is untyped `jsonb` | The second job kind. Then it becomes a `#[serde(tag = "kind")]` enum in `lapidary-core` |
| Permit-before-dequeue is untested | A job slower than a lease — Phase 2 |

Closed by this slice: **S3** (unique `derivative(revision_id, kind)`), in task 1.

---

## What the exit run actually showed

Run on 2026-09-04 with `docker compose`, against 150 real STLs staged from a live
parts library rather than the six repository fixtures — the fixtures drain in about
70 ms, which is not long enough to kill a worker mid-scan with any reliability.

| Claim (spec §10) | Result |
|---|---|
| 202 well under a second, with a batch id | **13.9 ms** for 150 files |
| The grid fills as the worker commits | 150 ingested in **2.0 s**, ~74 files/s at concurrency 4 |
| `kill -9` mid-scan, then restart, and the batch completes | SIGKILL at 52 done / **4 running** / 94 pending → after restart **150 ingested, 0 failed** |
| No part duplicated | **0** duplicate `(library_id, name)` pairs |
| Every part has a thumbnail | **150 / 150** |
| Re-scanning the same directory | new batch drains to **skipped 6** (six-fixture run) |
| A truncated STL | **failedTotal 1 at attempts 1**, not 3, with a message naming the byte counts |
| Zero WARN lines on a cold start | **0** in both `api` and `worker` |
| Grid page, warm | **8.9 ms**, 50 cards, all with real thumbnails |

The four jobs in flight at SIGKILL came back as `ingested`, not `skipped`, which
says they died before committing their parts. `crates/lapidary-jobs/tests/resumption.rs`
covers the harder case deterministically: a worker killed *after* its part row
commits and before its outcome is recorded.

## Two things this slice's execution corrected in the plan

**Task 10's mutation check does not bite with the fixture the plan gives it.** An
unparseable file produces no part whether the walk enqueues or ingests, so
`part_count == 0` holds either way. The test stages a genuinely ingestable fixture
alongside it, which is what makes restoring the synchronous call fail.

**Task 14's second mutation does not bite at all, and that is correct.** Dropping
`part_name_unique_per_library` leaves the resumption test green, because the
reclaiming worker's redo hits `library_holds(library, name, hash)` and returns
`Skipped` before reaching the insert. The constraint guards the *concurrent* race —
two live workers on one file — which is
`losing_the_race_for_a_file_is_a_skip_rather_than_a_failure` in
`crates/lapidary-ingest/tests/handler.rs`. The plan reads a non-biting mutation here
as evidence the worker was not really aborted mid-job; it was, and the test's
intermediate assertions pin that.

## After the slice

Slice 2's exit is not Phase 1's exit. Reading `docs/ROADMAP.md`'s Phase 1 against what
exists, these remain before the phase can be called done:

- Upload: client-side WASM BLAKE3 → probe → chunked resumable transfer. Today ingest is
  a server-side walk of a mounted directory; there is no upload path at all.
- SSE progress. Slice 2 deliberately ships **polling** (task 13); the roadmap's Phase 1
  line says SSE, and Phase 6 later reuses that stream for live dashboard patches.
- `variant=original` download with the hash displayed — byte-identical ingested bytes.
- Keyset pagination and the sub-80 ms warm grid load, measured rather than assumed.

The phase exit criterion to measure against, verbatim: drop a folder of 1,000 STLs, grid
interactive immediately, all thumbnails land, re-dropping the same folder completes in
seconds via the hash short-circuit.
