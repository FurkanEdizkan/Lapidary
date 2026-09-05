# Phase 1 slice 4 — handoff

Derive less. Ingest stores **L0 only**; L1 and L2 are built the first time something asks
for one. Thumbnails become a per-library setting, and a sweep fills the ones ingest
skipped. A blob records when it was last read.

Ten tasks, twenty-five commits, on `feat/on-demand-derivatives`. **431 Rust tests, 0
failing; 40 web tests**, every gate at exit 0. Not pushed: branches merge to `main`
locally and CI is the release gate.

## What landed

| Task | Commit | What it does |
|---|---|---|
| — | `faca07d` `4daeba9` `cd3c96d` `373c188` | Spec, the re-cut of Phase 1's remaining slices, the plan, and the three gaps a pre-flight scan found in it |
| 1 | `e48dc8e` | `JobPayload` — a typed payload the `job.kind` COLUMN discriminates, so every pre-existing `{"path": …}` row still deserialises. `DerivativeKind`, `Outcome::Rendered` |
| 1 | `4e371ed` | The spec still showed the `#[serde(tag = "kind")]` design the implementation had already rejected. Corrected in §3.6 and §7 |
| 2 | `4acac9c` | Migration `0005`: `library.auto_thumbnail`, and `job_outcome_known` widened to accept `'rendered'` |
| 3 | `b8d484f` | `enqueue` takes any payload kind; a `derive` failure no longer 500s the batch-status route. Carries a fix round: the failure join is scoped to the job's library |
| 4 | `b54f780` | `KernelParams.produce` — the kernel builds what the caller asked for and nothing else. Carries a fix round: three tests behind three written promises that had none |
| — | `7cd7950` | The derive arm gets its own resolver, and the plan names the four tests task 7 breaks |
| 5 | `7e5547c` | `upsert_derivative` with a `DerivativeBytes` enum, `latest_revision`, `revision_source`, `revisions_missing`; the thumbnail becomes optional |
| — | `2baf6e4` `e698b67` | Plan stops asserting a test baseline that moved four times; a transient agent worktree stops being committable |
| 6 | `2fefe44` | `blob.last_accessed_at`, written by the blob route only — the one read that means "somebody wanted these bytes" |
| 7 | `7e9ae50` | **The commit the slice exists for.** Dispatch on `job.kind`; ingest writes one or two derivatives instead of four; the `derive` arm renders on demand |
| 7 | `b3f3373` `76c23c2` | Where the slice stopped, and the web fixture the binding outgrew nine tasks earlier |
| 7 | `f375f11` | Guard fix round: `upsert_derivative` refuses shapes nothing can display, and `revision_source` is scoped to a library |
| 8 | `c1c1ee5` | `GET`/`PATCH /api/libraries/{id}`, `POST /api/parts/{id}/thumbnail`, `POST /api/libraries/{id}/thumbnails` — all on `Role::Api` |
| 8 | `367695a` | Fix round: pin two removable guards, 404 a phantom-library sweep, correct spec §10 and §11 |
| 9 | `0ee244c` | `jobsSettled` counts `rendered`; the action bar triggers a sweep and a per-card render |
| 9 | `f0d31d4` | `GET /api/libraries/{id}` — the toggle reads its position from the server instead of asserting a default |
| 10 | `966f961` | `MalformedJobPayload` stops asserting who wrote the row — see below |
| 10 | `c3ae4d6` + this | The five product-doc reversals and the three from the ledger, with their measured numbers — then the exit run and this handoff |

Seven of the commits landed *during* execution are documentation or plumbing: five record
rulings made while the work was running, one closes the web-suite break below, one stops a
scratch agent worktree being committable.

## What the exit run showed

Run on 2026-09-05 from a cold start — `docker compose down -v` then `up -d` on empty
volumes — against **the same 150 real STLs slice 3 measured**
(`/home/jbo/lapidary-ingest-real`, 143 MB of dense tabletop scenery). Slice 3's run added
one OBJ fixture on top of them, which is where its 151 parts against this run's 150 comes
from. The compose ceilings from slice 3b are in force: worker 2.0 cores / 2 GiB and
`LAPIDARY_WORKER_CONCURRENCY=2`, db 1.0 core / 1 GiB, api 512 MB.

| Claim (spec §10) | Result |
|---|---|
| A library with `auto_thumbnail = false` ingests with **zero** thumbnail rows and exactly one tessellation row each | **150 parts → 150 derivative rows, all `tessellation_l0`**. Zero `thumbnail` rows, zero inline bytes |
| …and every part still appears in the grid | **150 of 150**, every card reading "No preview yet", every `thumbnail` field `null` over both API pages |
| Derivative bytes drop from slice 3's ~100 MB per 151 parts to under 10 MB | **7,501,328 bytes (7.50 MB)** of rungs on disk, against slice 3's **L0 7.3 + L1 41 + L2 52 = 100.3 MB** on these same files — a **92.5% drop**. Accounting is slice 3's: rung bytes on disk, thumbnails counted separately because they are inline. That the L0 half matches its 7.3 MB to within its own rounding is the check that the two runs are measuring the same thing. After the sweep fills thumbnails, add **5,718,866 bytes** inline in Postgres; **13.22 MB** all-in |
| The sweep drains to `finishedAt` with `rendered` equal to the part count | **`total 150, rendered 150, ingested 0, skipped 0, failedTotal 0`**, `finishedAt` set, drained in **3.14 s** |
| The grid refetches without a manual reload | **Yes, observed in Chrome.** Clicked "Generate missing previews" in the action bar; the 50 visible cards went from 0 images / 50 "No preview yet" to **50 images / 0 "No preview yet"** and the line read *"Preview rendering complete — 150 previews rendered."* A probe object planted in `window` before the click was **still alive 94 s later**, `performance.getEntriesByType('navigation').length === 1` with type `navigate`, URL unchanged. See the caveat below |
| An on-demand rung opens in an independent validator | **Khronos `gltf-validator`, 0 errors / 0 warnings / 0 infos / 0 hints** on the on-demand L2 (102,852 bytes). The ingest-built L0 beside it: the same, 0 / 0 / 0 / 0 |
| Ingest throughput, **measured and recorded**, against slice 3b's 42.8 files/s | **150 files in 1.783 s = 84.1 files/s**, 80.2 MB/s of source bytes. A second, independent run of the same thing measured **1.551 s = 96.7 files/s**. The briefed comparison cannot be made cleanly — see below |
| Peak worker RSS against the 2 GiB ceiling | **19.7 MiB sampled working set — 1.0% of the ceiling** (slice 3b measured 40 MiB). cgroup `memory.peak`, which counts page cache, was 110.4 MiB; the worker wrote 80 MB of blobs, and its `anon` never exceeded 20 MiB |
| Zero WARN on a cold start, in `api` and `worker` | **0 WARN and 0 ERROR** — in all four services, across the whole run. Nine log lines total between `api` and `worker`, all INFO |

Four more, not asked for but measured while the stack was up:

| | Result |
|---|---|
| `GET /api/blob/{hash}` serves an on-demand rung | **200**, `model/gltf-binary`, `immutable`, quoted ETag |
| `last_accessed_at` discriminates rather than smearing | **Exactly 2 of 301 blob rows** carry a timestamp — the two rungs actually fetched. The other 299, including every source blob ingest wrote, are NULL. That is the signal ruling F2 chose the blob route to protect |
| Grid page under `DATA.md` §2.5's 80 ms warm | **1.2 – 1.3 ms** |
| The toggle reads the server rather than the default | Switched off by `PATCH`, page loaded fresh, checkbox rendered **off**. That is `T9-B`'s route working in the product, not in a test |

**Determinism, unasked and worth keeping.** The two independent runs sat on separate
volumes and separate databases. Ingest wrote **7,501,328 bytes of L0 both times**, and the
on-demand L2 came out as BLAKE3 `0b15ff92…` both times. Content addressing means a
re-derivation is free of drift by construction, and here it is observed rather than argued.

### The throughput comparison the brief asked for cannot be made cleanly

Spec §10 says to compare against slice 3b's **42.8 files/s** rather than slice 3's 89.4,
on the grounds that 3b ran under these ceilings. That is true, but incomplete: **3b ran a
different corpus.**

| Run | Corpus | Worker CPU | Concurrency | Result |
|---|---|---|---|---|
| Slice 3 | These files plus one OBJ — 151 | unlimited, 12-core host | 4 | 89.4 files/s |
| Slice 3b | Repo fixtures + example parts + one OBJ + one 3MF — 160 small files | 2.0 cores | 2 | 42.8 files/s |
| Slice 4 | This one — 150 real STLs, 143 MB | 2.0 cores | 2 | **84.1 files/s** |

So there is no baseline that differs in one variable. Against the ceiling-matched
baseline it is roughly **2× faster**, and files/s is the wrong unit for that comparison
because the files are an order of magnitude apart in size. Against the **corpus**-matched
baseline it is **within 6% of slice 3 on a sixth of the CPU and half the concurrency** —
which is the number that actually says the ladder work is gone, and it is the honest way
to read the "it should improve" prediction. Run-to-run variance on this machine is about
15% (1.551 s vs 1.783 s for the identical work), so neither figure should be read to more
than two significant digits.

### The grid-refetch caveat, stated plainly

This automated Chrome window reports `document.visibilityState === 'hidden'`, and
react-query does not fire `refetchInterval` on a hidden document unless
`refetchIntervalInBackground` is set. On the first attempt the poll therefore stopped
after six ticks: the grid had refetched — seven cards filled with no reload, so the
mechanism was already demonstrated — but the progress line then froze at *"Rendering
previews — 6 of 150."* while all 150 jobs completed behind it.

The observation above was taken after overriding `document.visibilityState` and
`document.hidden` **in the browser only**, so the app's own poll → invalidate → refetch
path was what got watched. Nothing in Lapidary was stubbed, and the DOM/navigation
evidence is from the real page.

**That stall is worth carrying as a finding, not just as a testing artifact.** A user who
starts a 10,000-part sweep and switches to another tab comes back to a progress line
frozen at whatever the last poll saw, over a batch that finished minutes ago. Nothing is
lost — the jobs complete, and a reload shows the truth — but the page tells a stale story
until the tab is focused. `index.tsx`'s comment already reasons about the *other* half of
this ("a batch that finishes while the tab is backgrounded must not leave a closed laptop
asking about a completed scan forever"); the missing half is that it stops asking *before*
it learns the batch finished. In the Opens table below.

## What the slice found that was not about deriving less

**The web suite had been failing to typecheck since task 1, and nobody ran it for nine
tasks.** `tsc --noEmit` failed on `web/src/routes/index.test.tsx` because task 1 added
`BatchStatus.rendered` and regenerated the bindings, while the fixture that hand-builds a
`BatchStatus` never grew the field. The reason it survived nine tasks is the part worth
keeping: the per-task bar ran eight Rust gates plus `export-bindings`, and
`export-bindings` regenerates and diffs the generated *files* — it proved `BatchStatus.ts`
was current, which it was. **Nothing in the bar compiled the TypeScript that consumes
those bindings**, and `npm test` passes regardless because vitest transpiles without
typechecking. A generated-file freshness check is not a typecheck. Fixed in `76c23c2`;
`cargo deny check` and the web suite are **permanently in the bar** from here.

**Two cross-tenant findings, one of them a write.** Both were raised by implementers who
judged them out of scope, and both were overruled on placement for the same reason —
`CLAUDE.md` requires checking tenant and part reachability, and a guard that lives away
from the query it guards is the one that gets dropped.

- **T3-A** (`b8d484f`): the batch-status failure join was unscoped, so a hand-inserted
  cross-library derive job leaked `vee-block-lp-3072-02`, another library's part name, into
  the first library's failure list. A **leak**.
- **T7-C** (`f375f11`): `revision_source` took only a revision id, so a derive job naming
  another library's revision **rendered onto it** — the reviewer re-probed with
  `TessellationL2` (the suite's original tenant test used `Thumbnail`, which cannot produce
  a blob row even unguarded) and confirmed no derivative row, no blob row, no `ref_count`
  change and no bytes on disk after the fix. A cross-tenant **write**, not a leak.

**The old unknown-kind bug was worse than either binding document said.** Spec §8 and the
plan both described it as *the wrong message*. Mutation proved it is also **the wrong
classification**: `payload.get("path")` succeeds on any ingest-shaped payload, so an
unknown kind never reached the "no file path" message at all — it fell through to a file
read and returned `Transient`, and the queue **retried a job it could never run, three
times, before failing**. Spec §8 now says so.

**`CoreError::MalformedJobPayload` was telling operators something false.** Its message
ended *"It was not written by Lapidary; check whether something else is inserting into the
job table."* `JobPayload::from_row` exists precisely because an older Lapidary wrote rows a
newer one must read, so **version skew is the expected way to reach this error**, and the
message pointed the operator at the one cause that is not it. Fixed here rather than
filed, in its own commit, with a test that fails if the claim comes back — the same shape
`DbError::CorruptBlobHash` got in `f375f11`, and for the same reason: nothing outside
`src/` pinned the wording, which is how it survived. It also closes the deferred minor
about the unquoted `{kind}` ("A ingest_file job's") in the same edit.

## Four times a briefed assertion could not fail as written

The recurring defect this slice, as in 3b, was not wrong code — it was assertions that
would have passed against the bug they named. All four were in briefs, not implementations:

1. **The migration survival test** used `#[sqlx::test]`'s auto-migration, so the
   "pre-existing" rows were inserted into a database that had already run `0005`. The
   implementer caught this in its own draft and rewrote it to drive the migrator by hand
   (`run_to(4)`, insert, `migrator.run()`), adding an `information_schema` guard proving
   `0005` actually ran — otherwise the row count passes vacuously.
2. **The byte-identity mutation I briefed could not kill the test I asked for.** I wrote
   that a derived L0 matches an ingest-built one "because `kernel.version()` depends only
   on `format`, not `produce`". True sentence, wrong thing: `kernel_version` is a recorded
   *column* and never reaches the bytes. Running my mutation changed the version string
   (`"mesh stl-1+glb-1+none"` vs `"…+cpu-1"`) and left the hash assertion untouched.
3. **The obvious fixture would have given a vacuous pass.** The byte-identity test uses the
   gear, not the bracket: at 20 triangles the bracket is coarser than L0's own grid and
   clusters to itself, so on the bracket the L2-drift mutation produces *identical* bytes.
   In this repo, "pick the standard fixture" is not a safe default for any test about
   cluster output.
4. **"The toggle shows off for a library the server says is off"** passes in both worlds,
   because an *unknown* toggle also renders unchecked. Only the mixed-state assertions on
   either side of the read discriminate; deleting them moved the failure from
   `toggle.indeterminate` to `toggle.checked`, two runs rather than one.

## Three times an implementer-chosen mutation found more than the briefed one

1. **Task 8**: unscoping the library write (`WHERE id = $1` → `WHERE $1 IS NOT NULL`)
   failed **two** tests, not one — the same missing scope both un-404s a phantom library
   and turns a per-library setting into a global write (`left: Some(false) / right:
   Some(true)` on another library's row). The 404 test was pinning scoping as well as row
   count, which neither of us had noticed.
2. **The task-7 fix round**: the fixture finding above.
3. **Task 9 — a live UI defect no briefed test could see.** react-query clears a mutation's
   `data` on `mutate`, so `settings.data?.autoThumbnail ?? true` made the checkbox spring
   back to its previous position **and disable itself** for the whole round trip: the click
   read as ignored. Invisible to any stub that resolves immediately, which is what all four
   briefed tests used. The test now holds the response open and asserts the in-flight frame.

And, three times, an implementer found my reasoning wrong on its premise and checked rather
than complied: the two `handler.rs` call sites where `Some(...)` would have preserved the
empty-`bytea` shim past the commit that existed to remove it (T5-B); `CorruptBlobHash`'s
"well-behaved siblings", two of which carried the same defect I was citing them against
(FIX-A); and `FIX-C`'s premise that task 8's routes would be the first to reach two guards,
which they are not — the implementer fixed it anyway, at `classify_db` rather than at the
call sites, making the classification a property of the variant instead of a mapper a
future caller could pick wrongly (T8-A).

## Rulings made during execution

Thirty-one, plus one correction to one of them, recorded in full with what each costs if wrong, in
`.superpowers/sdd/2026-09-05-phase-1-slice-4-derivatives/progress.md`. In brief: create the
two `CoreError` variants the plan used without saying to; touch a blob on the **blob route
only**, never in the handler, or a sweep marks everything warm and destroys the signal; put
`set_auto_thumbnail` on `PgParts` rather than invent a newtype to hold one method; give the
derive arm `revision_source(library, revision)` rather than `latest_revision(part)`, which
would silently render onto the wrong revision; sequence tasks 5 and 7 so the empty-`bytea`
hole never opens; refuse a hash-addressed thumbnail and an empty derivative *inside*
`upsert_derivative`, before the transaction, so a refusal orphans no blob row; scope both
cross-tenant queries; 404 a sweep on a library that does not exist, because
`PATCH` on the same id already discloses that fact and a silent `202 queued: 0` reads as
"nothing was missing" to someone who mistyped an id; make L0, not L2, the subject of the
byte-identity claim, since ingest builds no L2 to compare against; amend §11 as well as
§10, or the spec contradicts itself one section later; render the toggle **indeterminate
and disabled** before its `GET` resolves rather than showing a confident "on" that flips.

## Ledger

**Closes:**

- **Spec §10's exit criterion**, every clause, measured above.
- `DATA.md` §2.1's "all generated at ingest" rule and the reasoning under it — rewritten
  in place with the measured numbers and the constraint that changed it, not footnoted.
- `ROADMAP.md`'s Phase 1 mesh-ingest line and its **phase exit criterion**, which asserted
  "all thumbnails land" and is false for a library that renders on demand.
- `FEATURES.md`'s unconditional "Thumbnails inline from Postgres `bytea`".
- Spec §8's understatement of the unknown-kind bug, and spec §2's `unique (part_id)`,
  reversed by the owner on 2026-09-05: a part carries an **ordered gallery** — user images
  first, generated views appended after, never replacing — both freely added and deleted.
  Free to reverse because `part_image` was never created; `0005` added only
  `library.auto_thumbnail` and the widened outcome CHECK. A spec reversed, not a schema.
- `CoreError::MalformedJobPayload`'s false claim about who wrote the row.

**Opens:**

| Item | Trigger |
|---|---|
| A backgrounded tab's progress line goes stale mid-sweep | Slice 5, with the scan trigger — same poll, and a 10,000-part sweep makes it visible. Either `refetchIntervalInBackground`, or say "checked N seconds ago" instead of a number that has stopped moving |
| `HandlerError` has no `Display` | Carried unchanged from slice 3b. It is a library error type and `CLAUDE.md` says `thiserror` in libraries. Worth fixing when something else touches `lapidary-jobs` |
| `strings.render.unknown` and the sweep-with-work composition are untested | Named rather than hidden. The poll-error string has no stub worth writing, and the composition's two halves are pinned separately. Slice 5 renders both reachable when the scan trigger moves into the UI |
| The `want`-based rung filing rests on **one** test | `produce: vec![want]` → `ALL.to_vec()` fails only `each_rung_is_valid_gltf_and_l0_is_smaller_than_l2`. Concentration risk, not a defect — but a second guard belongs with whatever next changes `derive_one` |
| `ThumbnailNotInline` and `EmptyDerivative` are unreachable | By construction today: the Thumbnail arm hardcodes `Inline`, and `render_thumbnail` either errors or encodes a real WebP. Reachable when the viewer takes a hash-addressed thumbnail (Phase 3), which is also when the guard is lifted |
| No source blob's `last_accessed_at` is ever written | Slice 5's `variant=original`. `lapidary-api` cannot open the source store — `check-deploy` fails if it names the type — so the column serves render-cache eviction now and the cold tier only later |
| A route enqueuing L1/L2 on demand | Phase 3's viewer. The exit run reached the derive arm by inserting a job row directly; the three trigger routes only render thumbnails, which is all any consumer needs today |
| `part_image` as an ordered gallery | Slice 5, per the reversal above |

## The exact next action

Slice 5 — *a library you can drive from a browser* — is planned in
`docs/superpowers/plans/2026-09-05-phase-1-remaining-slices.md`. Scan from the UI (the
`Scan` job kind is the shape `derive` established here, and the reason the payload became
a typed enum), `variant=original` download, a part detail view, storage visibility, and
`part_image` upload. It is the slice that removes the last reason to reach for `curl`.

Before that, this branch merges to `main`:

```sh
git checkout main && git merge --no-ff feat/on-demand-derivatives
```
