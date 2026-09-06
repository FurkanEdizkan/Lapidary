# Slice 7 — handoff

**Branch:** `feat/storage-lifecycle`, 9 commits, unmerged. Design:
`../specs/2026-09-06-phase-1-slice-7-storage-lifecycle-design.md`.

Lapidary can now remove things. Before this slice it could index 1,703 files and delete
none of them.

## What shipped

| Step | Route | What it does |
|---|---|---|
| Remove | `DELETE /api/parts/{id}` | Sets `deleted_at`. No bytes move. |
| Restore | `POST /api/parts/{id}/restore` | Clears it. Indefinitely available. |
| Purge | `POST /api/parts/{id}/purge` | Removes the part chain, recomputes references, quarantines what reaches zero. 409 on a live part. |
| Reap | none — an hourly timer in the worker | Removes blobs quarantined past 30 days, un-quarantines any that came back. |

Plus `?state=removed` on the grid query, `web/src/routes/removed.tsx`, and
`LibraryStorage.removed_bytes`.

## The three decisions worth knowing

**Purge recomputes; it does not decrement.** A counter maintained only by arithmetic
drifts, and the thing that acts on the drift deletes bytes. Recomputing from reachability
makes drift self-heal on the one path that matters. A test corrupts a shared blob's count
to 7 and watches purge land it on 1, then 0 — a decrementing purge lands on 6 and 5 and
quarantines nothing, ever.

**Quarantine is a timestamp, not a `quarantine/` tree.** `DATA.md` §1.6 said the bytes
move; they do not. Restore then needs no path rewrite, no reader needs a second lookup
location, and the folder-tree work is about to relocate the store anyway. §1.6 is amended.

**The schema is the safety property, not the reaper's `WHERE` clause.** I had this wrong in
the spec and the mutation proof caught it: dropping the reaper's `NOT EXISTS` pair for
`ref_count = 0` does not lose bytes, because `file.blake3` and `derivative.blake3` are
foreign keys to `blob` and the delete raises a constraint violation instead. What the pair
buys is *availability* — the sweep is one transaction, so one wrongly-quarantined blob
would otherwise roll back every legitimate removal beside it, every hour, forever, with a
log line as the only symptom. Both the spec and the doc comment now say this.

## What the audit proved, and what it did not

Recomputing every `ref_count` from reachability across the 150-part corpus agreed 300/300.
**That proves less than it looks like.** The same corpus has zero shared blobs, so it never
took the `link_existing` branch — which is exactly where a reference counter drifts. No
rung was ever re-rendered. One library.

The response was not a bigger corpus. It was to make `ref_count` non-load-bearing for every
destructive decision, so the audit's coverage stops gating the slice. It is now a display
figure and a hint.

## Verified live, not asserted

A worker and an api over six seeded parts, real blobs on disk, the production 30-day
constant and no injected cutoff:

- remove drops the totals and the same bytes reappear as `removedBytes` — 1,238 + 4,184 =
  5,422, exactly;
- restore returns the panel to its starting reading byte for byte;
- purging a live part is refused with the 409 and its wording;
- purge quarantines 2 blobs and both stay on disk at their exact sizes;
- a blob aged past its cutoff loses its row *and* its file; the one 29 days out keeps both;
- the five untouched parts keep every byte;
- re-scanning restores the part and clears both flags.

In Chrome: the grid, "Remove from library" with its hint, the navigate-back, the panel
reading "5.4 kB removed, still on disk", the removed list with name and path, and Restore
returning the grid to 6 parts and the panel to its original line.

**Not driven in the browser:** the purge confirmation. `window.confirm` blocks the
extension's event loop. `web/src/routes/removed.test.tsx` covers it — including that
declining sends no request, and that the dialog names the *path* rather than the name.

## Two things found by running it rather than by a test

1. **The storage panel reported a removal as a saving.** Both totals exclude soft-deleted
   parts, so removing a part dropped the number while the disk held the same bytes — the
   one claim `CLAUDE.md` says this area must never make, right beside a toast worded
   carefully never to make it. `repo.rs`'s own comment had deferred this to "whichever
   slice adds delete". `removed_bytes` is the fix.
2. **Re-ingest left the flag set for up to an hour.** Never unsafe — the reaper re-checks
   reachability — but "a referenced blob is never quarantined" was true only eventually.
   `quarantined_at = NULL` now rides on the `ref_count + 1` statement every ingest already
   runs against that row.

## Known gaps, all deliberate

- **Quarantined bytes appear in no library's storage figure, and cannot.** A purged blob has
  no part and therefore no library. Library-less by construction, not unimplemented. So a
  purge *does* drop the panel while the bytes wait. `ROADMAP.md`'s Phase 4 line now names
  the instance-wide view that can report it.
- **A part with zero revisions is in neither list.** `page`'s revision LATERAL is an inner
  join. Nothing in Phase 1 writes that row; closing it means null cases on the grid's
  hottest query. Phase 2 owns it.
- **No eviction.** Only its wording boundary, in `strings.removal`'s doc comment, so the
  second action does not borrow the first one's words.
- **The reaper is not reachable from the UI or a route.** It is a timer. `sweep()` takes the
  retention so tests can pass `Duration::ZERO`; there is no way to shorten it in production,
  deliberately — an operator who could shorten it could shorten it to zero.

## Mutations, all reverted byte-identically

| | Change | Caught by |
|---|---|---|
| M7-1 | `soft_delete` drops `AND deleted_at IS NULL` | repeat delete answers 204 |
| M7-2 | `library_holds` clears `deleted_at` for the path it recognises | the hash short-circuit revives the part |
| M7-3 | purge computes `ref_count - 1` | the drifted blob lands on 6, never reaches quarantine |
| M7-4 | reaper's `NOT EXISTS` pair → `ref_count = 0` | drifted-high blob immortal; one bad row takes the sweep down |
| M7-5 | confirmation names the part, not the path; and no confirmation at all | the purge test, both ways |

## Also closed here

Part F. Five stub crates said "Implementation lands in Phase 1" — now index 2, vcs and
targets 4, build 7, enterprise 8. The remaining-slices plan gets an amendment rather than a
rewrite, because its stale table is the evidence for why the re-cut happened.

The slice 6a carried item — `HEAD` warming `last_accessed_at` — needed no code.
`axum::routing::get` answers HEAD with the same handler and strips the body, so the touch
was always there. `crates/lapidary-api/tests/blob.rs` pins it, because that is a property of
a routing helper that swapping `get` for an explicit `MethodFilter` would silently remove.

And one correction: the spec's §0 originally attributed "`ref_count` is never written" to
the plan's Part D. No document says it — not the plan, not any spec, not `DATA.md`. It was a
working note of mine, and citing it as a document was the error, in a section whose whole
subject is documents disagreeing.

## Next

`cargo xtask verify slice` — 14 gates, green. Merge to `main`, then the folder-tree project
(13 tasks, 6 phases, which also owns the deferred `part_image` gallery), then Phase 0b
(`occt-bridge`).

The CI risk carried since slice 2 has grown, not shrunk: `main` has not seen the GitHub
matrix in a long time, and this branch adds a migration.
