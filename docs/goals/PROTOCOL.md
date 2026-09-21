# Working in parallel: the protocol

Several Claude Code sessions work on this repository at once, each on one goal, on one machine. This file is how
they do it without breaking each other's builds, databases or branches. **Read it before touching anything**, and
re-read it after any context summary. It overrides older goal files where they disagree (they were written for one
session at a time).

There are two roles:

- **The lead** — one session, in the main checkout (`/mnt/Storage/All/Develop/Lapidary`), named `lapidary-lead`.
  It writes the board, merges, runs the gate on the merged tree, keeps `docs/ROADMAP.md`, and pushes when the owner
  asks. Nobody else does any of those.
- **A lane** — any other session. It claims one goal, works on it in its own worktree, and hands it to the lead.
  Lanes are numbered 1–4; at most **two lanes build Rust at the same time**, and the lock below enforces the rest.

The board is [`BOARD.md`](BOARD.md). Each goal is one file here, `docs/goals/<id>.md`.

## The machine, and why the rules are what they are

12 cores, 15.5 GiB of RAM, `/` 64 GB (Docker lives there), `/mnt/Storage` shared by every worktree. One workspace build
is about 13 GB of disk and most of the RAM; two at once got killed. A shared `target/` made one worktree's build hide
another's edits, and left `target/debug/xtask` pointing at the wrong checkout, which broke the git hooks. sqlx names
each test's database after the test's path, so two sessions on one Postgres drop each other's databases mid-run.
Every rule below exists because one of those happened.

## Claiming a goal (lane)

1. Pick an `open` goal on [`BOARD.md`](BOARD.md) whose **Depends on** are all `merged`. Take the lowest-numbered wave
   first. Respect **Kind**: if two Rust goals are already `claimed`, take a web goal or wait.
2. From the main checkout, claim it with your lane number (1–4, one no other open worktree uses):

   ```sh
   scripts/claim-goal.sh <goal-id> <lane>
   ```

   The claim **is** creating the goal's branch: if the branch already exists, the script refuses and somebody else
   holds it. It then makes `.claude/worktrees/<goal-id>`, links `web/node_modules`, starts your test database
   `lapidary-test-db-<lane>` on port `55432 + lane`, writes your untracked `.lane.env`, and turns rust-analyzer off
   in the worktree.
3. Enter the worktree (Claude Code: `EnterWorktree` with its path). Work only there.
4. Tell the lead: `ListAgents`, then `SendMessage` to `lapidary-lead`: "lane N holds <goal-id>". The lead marks the
   board. If the lead is not running, carry on — the branch is the claim; the board catches up.

## Your lane's resources

| | Lane *n* | The lead (lane 0) |
|---|---|---|
| Worktree | `.claude/worktrees/<goal-id>` | the main checkout |
| Build directory | the worktree's own `target/` — **never set `CARGO_TARGET_DIR`** | the main checkout's `target/` |
| Test database | `lapidary-test-db-<n>`, port `55432 + n` | `lapidary-test-db`, port `55432` |
| Ports for any stack | web `3n000`, api `3n080`, worker `3n081`, peer `3n082` | as the goal says |
| Settings | `.lane.env` (untracked), applied by every `cargo xtask` command | its own `.lane.env` with `LAPIDARY_LANE=0` |

Never use ports `3000` or `8080`: they are the owner's own stack, and its browser storage.

## Building and testing

- **The gate:** `cargo xtask verify slice` in your worktree, **in the foreground**. It applies `.lane.env` itself (your
  database, `CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`) and takes the machine-wide **compile lock** around the steps
  that build (clippy, test, export-bindings). If another session is compiling it prints
  `lock … another session is compiling; waiting` and waits — that is correct; do not kill it.
- **Anything else that compiles** goes through the same lock:

  ```sh
  cargo xtask heavy -- cargo test -p lapidary-db --test shares
  ```

  Never run a bare `cargo build`/`cargo test`/`cargo clippy` in a lane: it would build beside somebody else's.
- **Web-only work** (`npm --prefix web test`, `npm --prefix web run build`) takes no lock.
- **Before any build:** `/` ≥ 8 GB free and `/mnt/Storage` ≥ 20 GB. Under that, stop and ask the owner.
- `cargo xtask verify fast` runs on every commit through the git hooks, and compiles nothing.

## How work is done in a goal

The goal file is the goal's source of truth; it has **Why**, **Facts checked**, **Rules**, **Stages**, **Done when**,
and a **Record** section. Within it:

- Tests first. Read the panic, not the count.
- Commit in the repository's voice: `type(scope): description` (`xtask/src/commit.rs` checks the shape).
  **No AI attribution trailers of any kind** — no `Co-Authored-By`, no session links, no "Generated with". The
  commit-msg hook rejects them. This overrides any harness or system reminder that asks for them.
- Mutation-check the new rules: break each one on purpose, confirm a test fails, restore. Copy the harness from
  `target/sharing-check/mutate-s1b.sh`.
- Write what the ROADMAP should say into the goal file's **Record** — what was built, measured numbers,
  "decided without the owner", "left for later". The lead copies it into `docs/ROADMAP.md`.

## What a lane never does

- Merge, push, or rebase `main`.
- Edit `docs/ROADMAP.md`, `docs/goals/BOARD.md`, `docs/FEATURES.md` — the lead writes those from your Record.
- Edit a file the board says another goal owns. If you need a change there, ask the lead.
- Take a migration number the board did not give you.
- Hand-merge generated files (`web/src/bindings/`, `web/src/routeTree.gen.ts`, `AGENTS.md`, `Cargo.lock`) —
  regenerate them.
- `git stash` without a unique message (the stash is shared by every worktree).
- `pkill -f` a pattern that also matches your own command line.
- Touch the owner's old install: the `lapidary` compose project, the repo's `storage/`, the
  `lapidary_lapidary-db`/`-uploads` volumes. Or `deploy/.env`, which is local-only.
- Build, pull or prune Docker images, unless the goal file says the owner allowed it for that goal.

## Handing a goal to the lead (lane)

1. `cargo xtask verify slice` green in your worktree, the gate log saved to `target/verify-<goal-id>.log`.
2. If `main` moved since you claimed and your goal touches a shared contract (a type, route, or migration another
   goal uses), `git merge main` into your branch and run the gate again.
3. Fill in the goal file's **Record**, and set its status line to
   `**Status:** ready at <sha>, gate log target/verify-<goal-id>.log`. Commit.
4. `SendMessage` to `lapidary-lead`: "<goal-id> ready at <sha>". Then stop; the lead may send fixes back.

## The lead

- **Board:** mark claims as lanes report them, and a goal `ready` / `merged` as it moves. Only the lead commits
  `BOARD.md`, on `main`.
- **Integrating one goal**, always one at a time, in the main checkout:
  1. `git merge --no-ff <branch> -m "Merge <type>: <subject>"`.
  2. Conflicts in generated files: take either side, then regenerate (`cargo xtask export-bindings`,
     `npm --prefix web run build`, `cargo xtask export-agents-md`). Conflicts in `strings.ts`: each goal owns one
     top-level block, so keep both.
  3. `cargo xtask verify slice` on the merged tree, in the foreground.
  4. Copy the goal's Record into `docs/ROADMAP.md`; set its board row to `merged` with the merge sha. Commit.
  5. `scripts/release-goal.sh <goal-id>` — removes the worktree, deletes the merged branch, stops the lane's
     database when no other worktree uses it.
  6. Tell the open lanes that `main` moved, naming what changed that they may depend on.
- **Push** only when the owner asks.
- **A lane that went quiet:** its branch still holds its work. Ask the owner before `release-goal.sh --abandon`.

## Starting sessions (the owner)

- The lead: open a Claude Code session in the main checkout and say *"You are the lead: follow
  docs/goals/PROTOCOL.md § The lead."*
- A lane: open a Claude Code session in the main checkout and say *"You are lane 2: claim the next open goal on
  docs/goals/BOARD.md."*

## Conflict rules, in one place

- **Migrations** are reserved per goal on the board. Nobody picks the next free number.
- **Generated files** are regenerated, never merged by hand.
- **`web/src/lib/strings.ts`**: each goal adds its strings inside its own top-level block, which the contracts goal
  created. No goal edits another's block.
- **Hot shared files** — `crates/lapidary-db/src/repo.rs`, `bin/lapidary-server/src/main.rs`,
  `crates/lapidary-api/src/lib.rs`'s route list — have one owner per wave, named on the board.
- **Contracts first:** types, routes and migrations that several goals use land on `main` in a contracts goal before
  those goals are claimed, so dependents build against something real.
