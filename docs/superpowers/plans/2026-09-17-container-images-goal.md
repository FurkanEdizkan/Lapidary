# Goal 8: the images, built and run — the stack, sharing, an upgrade, and the workflow

Written 2026-09-17. **This file is the goal's source of truth.** After any context summary, re-read it together with
`docs/ROADMAP.md` § "Containers (2026-09-17)", which stage 0 opens and every stage after it records into.

## Why

Everything since goal 4 has been checked from native binaries: goal 7's sharing ran as two stacks of `lapidary-server`
processes and `vite preview`, never as containers. `CLAUDE.md` says Lapidary is container-first, and the product ships
as `deploy/compose.yaml` plus `deploy/compose.sharing.yaml`. So the images have not been built from this code, the peer
overlay's new mounts and volumes (goal 7, S3) have never run in a container, migrations `0037`–`0041` have only met fresh
databases, and the workflow that is meant to build the images would build the wrong one. This goal builds them, runs
them the way a person would, and fixes what that finds.

The owner's answers, 2026-09-17:
- **Room on the root disk:** prune unused build cache (`docker builder prune`), never images; stop cleanly if root drops
  under 8 GB.
- **Checks:** the plain stack; sharing between two compose projects; upgrading an old database.
- **The Containers workflow:** fix it, and prove it with one `workflow_dispatch` run.
- **Push** when the goal's last merge lands.

## Facts already checked

As of `d353d70`.

**The machine.**
- Docker 29 with Compose v5.5.1 and BuildKit 0.33; no Podman. 12 CPUs, 15.5 GiB of memory.
- **Docker lives on the root disk** (`/var/lib/docker`, `/dev/nvme0n1p3`, 64 GB), which had **18 GB free**. So does
  `/tmp`, where the harness writes command output: a full root disk loses every tool's output (memory note
  `root-disk-fills-from-docker`). `/mnt/Storage` had 50 GB free.
- `docker system df`: 3 images (16.0 GB, of which `sunny-fw-builder` is 15.2 GB and belongs to another project),
  **21.2 GB of build cache, 6 GB of it reclaimable**, 4 volumes.
- **No Lapidary image exists.** Every base image the build names will be pulled, each by its pinned digest:
  `rust:1.95-trixie@sha256:443dd9a3…` (`deploy/Containerfile` `build`, `occt-test`), `debian:trixie-slim@sha256:abc9cb88…`
  (`occt`, `runtime`), `node:24-trixie@sha256:499ac30d…` and `caddy:2-alpine@sha256:98eb57d8…` (`deploy/web/Containerfile`),
  and `postgres:18@sha256:7341002d…` (`deploy/db/Containerfile`). The locally cached `postgres:18` is a **different**
  digest (`sha256:4ef4dbc9…`, the test database's), so the db build pulls its own.
- `deploy/db/Containerfile` installs `postgresql-18-pgvector` with apt at build time, and its init script creates `vector`
  and `pg_trgm` and checks the `turkish` text search config. So the plain stack answers the open point "checking pgvector
  against `postgres:18`" that ROADMAP holds before Phase 6.

**The images.**
- `deploy/Containerfile` stages: `build` (a release `cargo build -p lapidary-server` with `SERVER_FEATURES`), `occt` (OCCT
  8.0.1 from source; its compile took 766 s in goal 4), `occt-test` (nothing copies from it; compose never builds it),
  `runtime` (uid 10001 `lapidary`, example parts copied in), and the targets `api` and `worker`.
- `deploy/compose.yaml`: `db` (built from `deploy/db`), `api` (`target: api`, 512 MB), `worker` (`target: worker`,
  `SERVER_FEATURES: mock-kernel,occt-kernel`, 2 GB), `web` (node build into Caddy, which proxies `/api/*` to `api:8080`).
  `deploy/compose.sharing.yaml` adds `peer` (`target: api`, 512 MB) with the `lapidary-peer` and `lapidary-peer-staging`
  volumes and the store. `api` and `peer` build the same target with the same arguments.
- **Published ports are literals:** `8080:8080`, `8081:8081`, `3000:8080`, and the overlay's `8082:8082`. Two projects
  on one machine cannot both start as the files are.
- `cargo xtask check-deploy` reads exactly `deploy/compose.yaml`, `deploy/compose.sharing.yaml` and the Containerfile
  (`xtask/src/main.rs`, `check_deploy`).

**Data that is not the goal's.**
- **An old install is on this machine:** volumes `lapidary_lapidary-db` (created 2026-09-05) and `lapidary_lapidary-uploads`
  (2026-09-07), and the repo's `storage/` (155 MB, owned by uid 10001, last written 2026-09-07). They are the owner's.
- **`deploy/compose.yaml`'s defaults point at it:** project name `lapidary`, and `${LAPIDARY_STORAGE_ROOT:-../storage}`.
  `deploy/.env` is local-only and sets no storage root.
- The test database `lapidary-test-db` (port 55432) and the other projects' containers (`trench-bot`) and images stay
  as they are.

**Sharing's harness, from goal 7.** `target/sharing-check/` holds `corpus-1g/` (138 STLs, 1,077,177,442 bytes: a tree of
**symlinks into `/mnt/Storage2/All/STL Files`**, which dangle inside a container unless that path is mounted too),
`measure-s3.sh` and `measure-s4.sh`. Their native results are the numbers to compare against: S3 moved exactly the
prediction across a `kill -9`; S4 waited with 0 bytes, resumed a pause with nothing moved twice, and failed a stopped
share in 1 s naming it.

**The workflow.** `.github/workflows/containers.yml` runs only on `workflow_dispatch` and `v*` tags, and `gh run list`
has never shown it. It builds `deploy/Containerfile` with **no `--target` and no `SERVER_FEATURES`**, which builds the
file's last stage, `worker`, around a binary with no worker support; `worker_router` refuses to start in that build. A
GitHub `ubuntu-latest` runner has about 14 GB of free disk; goal 4's build took about 4 GB of root here for OCCT and its
base images, before two release Rust builds.

**The upgrade.** `5724553` is the last commit before goal 7 (migrations to `0036`). Goal 7 added `0037_sharing`,
`0038_shares`, `0039_mirror`, `0040_pull` and `0041_consent`.

## Rules for the goal

- **One branch and worktree per stage that changes code**, from `main`, merged `--no-ff` only with `cargo xtask verify
  slice` green and the stage's exit measured and recorded. A stage that needs no code merges nothing and records its
  results. Tests first, seen failing for the right reason; mutation-check any logic a stage adds.
- **Docker, as the owner allowed on 2026-09-17:**
  - building `deploy/`'s images and pulling the pinned base images above is allowed;
  - `docker builder prune -f` (unused cache only, never `-a`, never `docker system prune`, never `image prune`) is allowed
    when root is short, and each run records the space before and after;
  - containers, volumes and images the goal creates carry its project names (`lapidary-check`, `lapidary-share-a`,
    `lapidary-share-b`, `lapidary-upgrade`) and are removed by those names alone;
  - **never** start the `lapidary` project, never pass `-v` to a `down` of a project the goal did not create, and never
    mount the repo's `storage/` or the `lapidary_lapidary-*` volumes, read-write or otherwise.
- **Disk guard.** Before every build, every image run and every measurement: `df --output=avail -BG /` at least 8 GB and
  `/mnt/Storage` at least 20 GB. Short on root: one `docker builder prune -f`, then check again; still short, stop the
  stage cleanly and record where it stood.
- **Every check has its own env file and store** under `target/docker-check/<project>/`, passed with `--env-file` and an
  absolute `LAPIDARY_STORAGE_ROOT`. `deploy/.env` is read, never written. A store directory is made writable for uid 10001
  by the built image itself (`docker run --rm --user 0 --entrypoint chown … -R 10001:10001`), since there is no sudo.
- **Two projects on one machine** get host ports from a **ports-only** override file per project under
  `target/docker-check/` (Compose's `!override` on `ports`). It carries no `build`, `target` or `LAPIDARY_ROLE`, so every
  invariant `check-deploy` holds still applies to the images that run. The peer override also adds
  `extra_hosts: ["host.docker.internal:host-gateway"]`, and pairing uses `host.docker.internal:<other project's peer
  port>`: a peer resolves the pasted address inside its own network namespace, where `127.0.0.1` is itself. Two
  machines on a LAN need none of this; record that the override is the one-machine harness, not a deployment change.
- **The corpus** is mounted read-only into A's worker twice: `target/sharing-check/corpus-1g` at `/ingest`, and
  `/mnt/Storage2/All/STL Files` at the same absolute path, so the symlinks resolve. It stays read-only.
- **Long builds** run with the Bash tool in the background, logging to `target/docker-check/`, and are waited on by their
  completion notice rather than polled. Every cargo command runs with `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0` in the
  gates' configuration.
- **Screenshots** with headless Chrome and a throwaway profile (memory note `screenshots-use-headless-chrome`).
- **Time budget: 2.5 hours a stage.** A stage not done by then stops cleanly, recorded, and the goal stops with it.
- **Where a stage would contradict a doc,** take the option that does not and record it; if none exists, stop and record
  the question. No attribution trailers on any commit.
- **Push** once, when the last merge lands (stage 5 needs it for `workflow_dispatch`); a final record after the run is
  pushed too.

## Stages

### 0. Preflight (no branch)

- `main` clean at `d353d70` or later, and `origin/main` equal to it; `lapidary-test-db` up; nothing listening on 3000,
  8080–8082, 13000, 18080–18082.
- Disk guard. Record `docker system df` and `df -h /`. Run `docker builder prune -f` once and record what it freed.
- Record the old install's volumes and `storage/` as they are (`docker volume inspect`, `du -s`, the newest file's
  mtime), to compare against at teardown.
- Open `docs/ROADMAP.md` § "Containers (2026-09-17)" with the preflight's figures.

### 1. Build the images (no branch unless a build fails on our files)

- `docker compose -p lapidary-check -f deploy/compose.yaml -f deploy/compose.sharing.yaml --env-file
  target/docker-check/lapidary-check/check.env build`, one service at a time in this order, with the disk guard between:
  `db`, `web`, `api` (then `peer`, which should come from the same layers), `worker`.
- **Record per image:** wall time, pulled bases, image size (`docker image inspect`), root free before and after, and the
  build cache's size after. OCCT's compile against goal 4's 766 s.
- **Check what each image is:**
  - `api` has no `/opt/occt` and no `occt-bridge`; `worker` has both, and `occt-bridge version` and `selftest` pass in it;
  - each runs `lapidary-server` as uid 10001, and the `worker` binary starts as `LAPIDARY_ROLE=worker` (the build that the
    workflow gets wrong would not).
- A build that fails because of this repository's files (a Containerfile, a lockfile, the web build) is fixed on
  `fix/container-build`, with `check-deploy` still green, and merged before stage 2.

### 2. The plain stack (no branch unless a check needs code)

Project `lapidary-check`, its own env and an absolute store under `target/docker-check/lapidary-check/`, the default
ports, `LAPIDARY_INGEST_DIR` = the repo's `fixtures/step` (6 files: 5 STEP, 1 IGES), read-only.
- **Up:** every service healthy; `db` reports `vector`, `pg_trgm` and the `turkish` config; migrations to `0041`.
- **First start:** the 6 example parts appear through the web on 3000 (Caddy's proxy, not the api directly).
- **The real kernel:** a scan of `fixtures/step` into a controlled library through the OCCT worker: all 6 ingested, a STEP
  part's page shows its tree, face and edge counts, and the PMI cylinder's tolerances. Screenshot it. Record the batch's
  time and the worker's peak memory against its 2 GB limit (`docker stats`).
- **Upload and download:** a chunked upload of a real STL through the api, then `variant=original` byte-identical by
  SHA-256; a bundle exported and imported into another library.
- **Restart:** `down` (no `-v`) and `up` again: the same parts, revisions and thumbnails, and no job left failed.
- **Exit:** a table of the above in the ROADMAP record, with any bug a check found fixed and merged.

### 3. Sharing between two projects (branch only if a check needs code)

Projects `lapidary-share-a` (ports 8080–8082, 3000) and `lapidary-share-b` (18080–18082, 13000), each with the sharing
overlay and its ports-only override, own env and store. `lapidary-check` is stopped first (its ports).
- **A** ingests `corpus-1g` into a controlled library (the corpus mounted as above); B has a hobby library to pull into.
- **Exits, the same as goal 7's, now in containers:**
  - pair by pasting device ids and `host.docker.internal` addresses; both online; record the time;
  - A shares `STL Files` asking first; B mirrors it and predicts the bytes; B's pull waits with nothing staged;
  - A grants; B pulls; **`docker kill` B's peer container at about half way** and `docker compose start peer`: bytes
    moved equal the prediction, and the staged files survived in the `lapidary-peer-staging` volume;
  - the parts land under `Shared/Furkan’s workbench (…)/` with licences and the sharer named;
  - A records one new revision; B pulls again: one file moves, no duplicate;
  - B pauses a second pull part-way, A stops sharing, B resumes: refused naming the share, and B's parts stay;
  - `down` and `up` for both projects: each keeps its device id (the `lapidary-peer` volume) and its pairing.
- **Record also:** each peer container's peak memory against its 512 MB limit during the pull, and the time to pull.
- **Exit:** the numbers beside goal 7's native ones.

### 4. Upgrading an old database (branch `fix/upgrade-<what>` only if a migration fails)

Not the owner's old install: a database built by the code before goal 7.
- A worktree at `5724553`. Its own debug `lapidary-server --features mock-kernel` (a new build, on `/mnt/Storage`),
  against a scratch database `lapidary_upgrade` inside `lapidary-test-db` and a store under `target/docker-check/upgrade/`:
  migrate to `0036`, ingest `example/parts` and `fixtures/step`'s meshes into a hobby library and a controlled one, record
  a revision, add tags, sources and a licence, and create a category tree.
- Record the rows per table and each part's source hash.
- Then **the images from stage 1** against that database and store (project `lapidary-upgrade`, its env pointing
  `DATABASE_URL` at `lapidary-test-db` through `host.docker.internal:55432`, with that host added by its override, and
  services started with `--no-deps` so `depends_on` does not bring a `db` up): `0037`–`0041`
  apply, the same rows and hashes are there, every part's page answers 200, a sample downloads byte-identical, and a share
  and a pull work on the upgraded data (with a second stack from stage 3's images).
- A migration that fails on existing rows is fixed with a `#[sqlx::test]` that seeds the old shape and sees it fail
  first, then merged.
- **Exit:** migrations applied and their time, row counts before and after, every part opens.

### 5. The Containers workflow: `fix/containers-workflow`

- Build what compose builds: `api` (`--target api`), `worker` (`--target worker --build-arg
  SERVER_FEATURES=mock-kernel,occt-kernel`), `web`, `db`. Actions stay pinned by SHA.
- **Keep it from happening again:** `check-deploy` learns the workflow — every `docker build` of `deploy/Containerfile`
  names a target, and a `worker` target build passes `SERVER_FEATURES` with both kernel features — tested the way
  `deploy.rs`'s other rules are. Mutation-check the new rule.
- Merge with `verify slice` green; write the stage 0–4 records; **push**; then `gh workflow run containers.yml --ref main`
  and wait for it.
- If the runner runs out of disk building `worker`, record where, and change the dispatch to build `api`, `web` and `db`
  while tags build all four, then run it again. Record the run's URL, its time per image and its result.

### 6. Close (on stage 5's branch before its merge, and one record after the run)

- The ROADMAP record per stage: what was built or fixed, bugs found, mutation counts, measured exits, decisions taken
  without the owner, what is left.
- `deploy/compose.sharing.yaml`'s header, and `deploy/.env.example`'s storage note, corrected where the checks showed them
  wrong or incomplete (for instance how two installations on one machine reach each other, and making a store writable
  without sudo).
- Teardown: `down -v` for `lapidary-check`, `lapidary-share-a`, `lapidary-share-b` and `lapidary-upgrade` only; drop
  `lapidary_upgrade`; the built images stay, tagged, for the owner. The old install's volumes and `storage/` compared
  against stage 0's record: unchanged.
- The memory notes brought up to date.

## Recorded, not built

- Starting the owner's old install (`lapidary_lapidary-db`, `storage/`) on the new images. It would test a real upgrade
  including the storage layout migration, and it is the owner's data: ask first, and do it on copies.
- Images for another architecture, a registry push, image signing.
- Quadlet units and the air-gapped image bundle (Phase 8).

## Done when

The images build from `main`; the plain stack, sharing between two projects, and an upgrade from `5724553` each pass in
containers with their results recorded in `docs/ROADMAP.md` § "Containers (2026-09-17)"; every fix is merged `--no-ff`
with `cargo xtask verify slice` green; the Containers workflow builds the right targets and one `workflow_dispatch` run is
recorded; and `main` is pushed.
