# Phase 1 slice 4 — resume here

Paused after task 7 at the owner's request. Everything below is what is left.

**Branch:** `feat/on-demand-derivatives`, forked from `ebaac83` on `main`.
**HEAD when paused:** `7e9ae50`.
**Tests when paused:** 404 passed / 0 failed, full bar exit 0, working tree clean.
**Plan:** `2026-09-05-phase-1-slice-4-derivatives.md` (tasks 8–10 still stand as written,
amended by the rulings below). **Spec:** `2026-09-04-phase-1-slice-4-derivatives-design.md`
— binding. **Ledger:** `.superpowers/sdd/2026-09-05-phase-1-slice-4-derivatives/progress.md`
holds every ruling with its cost-if-wrong.

Run the suite with `export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"`.
Read the current test count from cargo; do not trust a number written in any document.

## What already landed

| Task | Commit | What it does |
|---|---|---|
| 1 | `e48dc8e` | `DerivativeKind`, `JobPayload`, `Outcome::Rendered`, `BatchStatus.rendered` |
| 2 | `4acac9c` | Migration `0005`: `library.auto_thumbnail`, the outcome CHECK re-added with `'rendered'` |
| 3 | `b8d484f` | Generic `enqueue`, a real `rendered` aggregate, and a live 500 fixed |
| 4 | `b54f780` | The kernel produces only what the caller asked for; `ladder()` deleted |
| 5 | `7e5547c` | Optional thumbnail, `upsert_derivative`, `latest_revision`, `revision_source`, `revisions_missing` |
| 6 | `2fefe44` | `touch_blob`, wired into the blob route only |
| 7 | `7e9ae50` | `WorkerHandler` dispatches on job kind; ingest writes L0 (+ thumbnail); derive arm builds the rest |

Tasks 1–6 are reviewed and clean. **Task 7 has not been reviewed yet.**

## Step 1 — the guard fix round (do this first)

Six items, all found by task 5's review or raised by task 7's implementer. They belong in
one commit because they are one idea: *the guards that make `upsert_derivative` safe for the
caller task 7 just gave it.* Suggested subject: `fix(db): refuse the derivative shapes
nothing can display`.

1. **T5-C — refuse `Hashed` for `Thumbnail`.** `upsert_derivative(rev, Thumbnail, Hashed{..})`
   writes a valid row today. `PgParts::page` selects only `d.thumb_bytes` and hardcodes
   `PartSummary.thumbnail = None`, so the grid shows "no preview yet" for a part that has
   one — and `revisions_missing(library, Thumbnail)` **excludes** that revision because a row
   exists, so the sweep never heals it. `repo.rs` already says the hash-addressed thumbnail
   reader "arrives with the viewer" (Phase 3). Refuse the shape until the reader exists.
   `render_thumbnail`'s `MAX_THUMB_BYTES` ladder already guarantees inline-sized output.

2. **T5-D — refuse an empty `Inline`.** `DerivativeBytes` closed "both columns" and
   "neither"; it did not close *empty*. Verified end to end: `Inline(b"")` reaches the grid as
   `data:image/webp;base64,` — the broken `<img>` that `insert_part_chain` now refuses to
   write. Both guards go **inside `upsert_derivative`**, not at its call sites.

3. **T7-C — scope `revision_source` by library.** `derive_one` ignores `job.library_id`, so a
   derive job naming another library's revision renders onto it. `CLAUDE.md:45-46` requires
   checking tenant and part reachability, and a revision id is a uuid a caller might hold from
   anywhere. Take `(library, revision)` and scope through `part.library_id`, exactly as
   `jobs.rs:349-356` already does for failure reporting. This is ruling T3-A one layer down —
   the same finding, the same fix, the second time in this slice.
   **Test:** a derive job naming another library's revision must fail rather than render;
   removing the scope must make that test fail.

4. **T5-E — `DbError::CorruptBlobHash`'s message and its missing test.** It ends in
   speculation about causation ("probably written by something other than lapidary-db") where
   every sibling variant ends in an imperative. `CLAUDE.md` requires what broke **and what to
   do**. It is reachable (task 5's reviewer reached it by inserting a `blob` row with a
   non-hex `blake3` and a `file` row naming it) but has zero references outside `src/`.

5. **Minor — `PgParts::page`'s derivative LATERAL still carries the `'thumbnail'` literal**
   that task 5 retired at the write site. Same literal, same file.

6. **T7-B — the missing mutation.** Neither mutation task 7 ran touches the `produce` list
   itself. Restore `DerivativeKind::ALL.to_vec()` in the ingest arm and confirm it fails
   `a_real_stl_writes_one_tessellation_blob_and_row`. If it does not, that test is not
   pinning the thing the whole slice is about.

Then **review task 7** (it is the only unreviewed commit). If the reviewer runs in a
worktree, tell it to `git checkout` the commit under review as its first action —
`isolation: "worktree"` branched task 5's reviewer from an unrelated commit and it had to
move itself.

## Step 2 — task 8: the three enqueue routes

As written in the plan. `PATCH /api/libraries/{id}`, `POST /api/parts/{id}/thumbnail`,
`POST /api/libraries/{id}/thumbnails`, all `Role::Api`, all enqueue-only, all returning the
existing `ScanAccepted`. Needs `PgLibraries::set_auto_thumbnail` returning whether a row
matched, so the route answers 404 rather than 200 for a write that hit nothing.

The mutation — mount them under `Role::Worker` — encodes the finding that makes the whole
placement necessary: `deploy/web/Caddyfile` proxies only to `api:8080`, so a route on the
worker is unreachable from a browser.

Task 8 no longer owns the tenant check; step 1 item 3 moved it into the query.

## Step 3 — task 9: bindings and the frontend

As written. The bug it exists to prevent is `web/src/routes/index.tsx`'s `filesSettled`
summing `ingested + skipped + failedTotal`: add `Rendered` without updating it and a sweep
leaves `settled === 0` forever, the refetch `useEffect` never fires, and every job succeeds
while the cards stay blank until a manual reload. **No backend test can see this.**
Regenerate and commit the bindings in this task so `export-bindings` leaves the tree clean.

## Step 4 — task 10: docs and the exit run

The doc amendments are listed in the plan. **Add one the plan does not have:** spec §8 and
the plan both describe the old unknown-kind bug as *the wrong message*. Task 7's mutation (b)
proved it was also *the wrong classification* — it fell through to a file read and returned
`Transient`, so the queue retried a job it could never run, three times, before failing.
Say that when amending, because it is the part that mattered.

Exit run, on the live stack against `/home/jbo/lapidary-ingest-real`:

- derivative bytes before and after (target ~100 MB → under 10 MB for 151 parts);
- ingest throughput against slice 3b's **42.8 files/s** — it should improve, since two
  clustering passes, two glTF writes and one render per file are gone;
- peak worker RSS against the 2 GiB compose ceiling;
- a library with `auto_thumbnail = false` ingests, writes zero thumbnails, and every part
  still appears in the grid;
- the sweep then fills them, `rendered` equals the part count, and **the grid refetches
  without a manual reload**;
- an on-demand L2 is byte-identical to an ingest-built one.

Then the handoff, then `superpowers:finishing-a-development-branch`, then merge to `main`.

## Carried risks

| Risk | Where it bites |
|---|---|
| `main` has ~40 unpushed commits and CI has not run since slice 2 | Everything. The local bar is green; the GitHub matrix is unproven |
| Task 7 is unreviewed | Step 1 closes it |
| `MockKernel` is unreachable from the ingest path | Not a bug; it means ingest tests run against the real kernel and real Postgres, which is why they can fail. Do not introduce a mock to speed them up |
| The `last_accessed_at` signal is thin until slice 5 | Only the blob route touches. Source blobs get no timestamp until `variant=original` exists |
| `HandlerError` has no `Display` | Pre-existing, carried from slice 3b |
