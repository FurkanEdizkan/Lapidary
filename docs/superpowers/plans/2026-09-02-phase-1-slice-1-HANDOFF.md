> **SUPERSEDED — this document describes a state that no longer exists.**
>
> It was written mid-plan, when execution had stopped after task 4 of 12. All twelve
> tasks are complete, and the slice went on through several review rounds and a final
> fix wave; nothing below about "where execution stopped", "what to do next" or the
> unpushed-commit count is true any more.
>
> It is kept, rather than deleted, for the one thing it is still the only committed
> record of: the reasoning and review findings for tasks 1-4, reproduced here because
> the live ledger lives in the gitignored `.superpowers/sdd/`. Read it as history.
> For what the slice actually does, read
> `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`.

# Phase 1 slice 1 — handoff

**Written:** 2026-09-02, at the end of a session that ran out of time mid-plan.
**Branch:** `rust-rewrite`, HEAD `1d3146d`, clean tree, **7 commits unpushed**.
**Plan:** `docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md` (12 tasks, 51 steps)
**Spec:** `docs/superpowers/specs/2026-09-02-phase-1-slice-1-ingest-design.md`

The plan was being executed with `superpowers:subagent-driven-development`. **Its ledger lives
in `.superpowers/sdd/`, which is gitignored and will not survive.** Everything from it that
matters is reproduced here.

---

## Where execution stopped

| Task | State |
|---|---|
| 1 — Schema and seeded library | **complete**, reviewed clean |
| 2 — Measurement vocabulary | **complete**, reviewed clean (zero issues) |
| 3 — STL parse + measure | **complete**, reviewed clean after 2 fix rounds |
| 4 — CPU rasterizer | **implemented and fixed, NOT re-reviewed** ← resume here |
| 5–12 | not started |

### The exact next action

Task 4's fix round landed in `1d3146d` and has **not had its scoped re-review**. Resume by
either running that re-review over `d41b9d7..1d3146d`, or accepting it — I verified it myself:
19/19 `lapidary-cad` tests pass, the golden image is unchanged, `fmt` and `clippy` are clean,
and the new retry test decodes the result and asserts it is smaller than `THUMB_PX`, which is
what proves the fallback fired.

Then continue with Task 5 (`MeshKernel`), which is small: it wires `parse_stl` + `measure` +
`render_thumbnail` behind the existing `Kernel` trait and finally consumes `RASTER_VERSION`.

To regenerate the SDD workspace and briefs:

```sh
S=~/.claude/plugins/cache/claude-plugins-official/superpowers/6.3.0/skills/subagent-driven-development
P=docs/superpowers/plans/2026-09-02-phase-1-slice-1-ingest.md
"$S/scripts/sdd-workspace" "$P"
for n in $(seq 5 12); do "$S/scripts/task-brief" "$P" $n; done
```

---

## Environment needed to resume

Tests require a live PostgreSQL 18:

```sh
podman run -d --rm --name lapidary-test-db \
  -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
  -p 55432:5432 docker.io/library/lapidary-db:latest
export DATABASE_URL="postgres://lapidary:localdev@localhost:55432/lapidary"
```

`podman compose` needs the socket first — this is **not** in `README.md` and cost time to
discover: `systemctl --user start podman.socket`.

The verification bar, which is exactly what `ci.yml` runs:

```sh
cargo fmt --all --check          # CI's first gate; keep it first
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask check-layers
cargo xtask check-deploy
cargo test --workspace --all-features
cargo deny check
```

---

## Two rulings made on the owner's behalf

1. **Execute on `rust-rewrite`, no worktree** — consistent with all prior rounds. Cost if
   wrong: none structural; the branch is isolated and CI-verified.
2. **Defer two `facet_open` gaps in the STL parser rather than open a third fix round.** An
   `outer loop` with no enclosing `facet`, and a stray `endfacet`, both parse cleanly. They
   yield structurally valid, **non-mixed** triangles, so they do not reproduce the
   vertex-mixing failure the round was about; the protection that matters lives in the
   `loop_open` gate on `vertex`. This parser had two rewrite rounds and each touch surfaced
   the next finding. Cost if wrong: a corrupt STL missing a `facet` keyword parses as valid
   geometry instead of erroring.

---

## Deferred minors — carry these to the final whole-branch review

| From | Finding |
|---|---|
| Task 1 | `derivative.thumb_bytes` and `derivative.blake3` are both nullable with no `CHECK` enforcing "inline or filesystem, never neither". **Task 7 is the first writer of derivative rows** — decide there. |
| Task 1 | **cargo does not rebuild the test binary when only a `.sql` migration changes**, so `sqlx::migrate!`'s embedded migrations go stale and you get a green run against the old schema. Hit twice, independently. A 5-line `build.rs` emitting `cargo:rerun-if-changed=migrations` fixes it. Harmless in CI; **will bite slice 2's first migration**. |
| Task 3 | Binary/ASCII detection can theoretically misclassify if `84 + 50n` holds by coincidence; misleading "truncated" diagnosis for a BOM-prefixed ASCII file; `measure()` on a directly-constructed empty `Mesh` yields `[-inf,-inf,-inf]`; the `1e-4` mm quantum assumes mm scale; far-from-origin f64 cancellation in signed volume. |
| Task 3 | The two `facet_open` gaps above. |
| Task 4 | `RASTER_VERSION` exported but unconsumed until Task 5 wires it. |

---

## What this plan got wrong about itself — read before writing more tasks

Three defects were found **in the plan's own code listings**, not in anyone's implementation.
Two are the same mistake:

**A test whose name claims a mechanism and whose body asserts an outcome that holds anyway.**

- Task 3's `a_closed_cube_measures_its_bbox_and_volume` passed with vertex quantisation
  *entirely defeated* — proven by replacing the quantising key with one over raw f32 bits and
  watching all 8 tests stay green. The plan's cube fixture shares literal float values across
  facets, which real STL never does.
- Task 4's `an_oversized_render_is_downscaled_rather_than_written_oversized` never rendered
  anything oversized. It re-asserted that the ordinary bracket fits the budget — the same
  assertion as the test directly above it.

The third: Task 4's size-guard error reported the 512 px encode's byte count while naming
256 px as the size tried.

**Practical consequence for tasks 5–12:** for any test whose name describes a *mechanism*, ask
what mutation would break the mechanism, and confirm the test fails under it. Several tasks
ahead have this shape — Task 6's `WorkerRole` boundary, Task 7's ref-count and soft-delete
assertions, Task 9's hash short-circuit and orphan reaping.

Two more defects were caught before execution, by verifying rather than assuming:

- **sqlx 0.9 has no `jiff` feature** (it ships `chrono` and `time`), so Task 7's grid query
  would not have compiled against `PartSummary`'s `jiff::Timestamp`. The plan now selects
  microseconds and uses `jiff::Timestamp::from_microsecond`.
- `image` 0.25.10 encodes WebP with no C dependency and its whole tree is inside
  `deny.toml`'s allow-list — checked before writing it into the plan.

---

## Cross-task couplings that still matter

- **Task 5 consumes `RASTER_VERSION`** and must embed it in `KernelVersion`, which becomes
  `derivative.kernel_version`. That is the only thing distinguishing a regenerated thumbnail
  from a stale one.
- **Tasks 6 and 7 duplicate a struct on purpose.** `StoredBlob` in `lapidary-storage`,
  `StoredBlobRow` in `lapidary-db` — both are L1 and `check-layers` forbids L1→L1. The api
  layer converts. Do not "fix" this.
- **Task 8 breaks two existing call sites.** `router(state)` becomes `router(state, role)`,
  changing `bin/lapidary-server` and both tests in `crates/lapidary-api/tests/health.rs`. That
  is the point: a route can no longer mount without declaring which role serves it.
- **Task 9 must not hardcode container paths.** Its tests use `TempDir`; Task 12 supplies the
  real `/ingest` mount.
- **Task 11 rewrites copy a previous round reasoned about.** The empty state currently reads
  "Parts will appear here as your library grows." Once scanning exists, "empty" means something
  different — but it must still not imply an upload control, because there isn't one.
- **`mock.rs`'s fixture keys must stay fictional.** Its mesh entry was renamed to
  `flange-lp-4400-02.stl` precisely because it collided with the real
  `bracket-lp-1042-03.stl` and gave two contradictory answers for one filename.

---

## Repository state notes

- **7 commits unpushed.** Last push was `3c6a0ea`; CI run 33625184925 was green.
- A Phase 0a compose stack may still be running (`lapidary-{db,api,worker,web}-1`) holding host
  ports 3000 and 8080. Unrelated to slice 1's work; Task 12 rebuilds it.
- ~~`example/` holds an untracked 163 MB `.rar`.~~ **This was wrong when written.** The archive
  was already *tracked* — swept into `3fcd43f` ("docs(spec): define the scan response counters")
  by a broad `git add`, two commits before this handoff. At 156 MB it exceeded GitHub's 100 MB
  per-file limit and made the branch unpushable. It has since been purged from history, moved
  outside the repository, and `*.rar` is now in `.gitignore`. The warning about `git add -A`
  was correct; it simply arrived after the fact.
- `fixtures/` now holds `bracket-lp-1042-03.stl` (binary, watertight, 20 triangles),
  `spacer-lp-2001-00.stl` (ASCII), `bracket-lp-1042-03.thumb.webp` (the golden image, 2,844
  bytes — an L-section bracket in three-quarter isometric), and the pre-existing `cube.stl`.
