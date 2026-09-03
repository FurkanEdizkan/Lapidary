# Phase 0a exit-criteria verification

Run from a fresh clone of `rust-rewrite` at commit `8bbe1f8` (`git clone --branch
rust-rewrite . ../lapidary-verify`), the state a new machine would see. `lapidary-test-db`
(a separately-managed Postgres container, not part of the compose stack) supplied
`DATABASE_URL` for the Rust test suite. `cargo-deny` 0.20.2 was installed for this run.

**Not verified, and explicitly out of scope for this task:** whether CI actually passes.
The branch is deliberately unpushed (~40 commits ahead of `origin/rust-rewrite`) and this
task does not push or run `gh`. Criterion 3 below verifies `cargo deny`, the committed
lockfiles, and that every workflow `uses:` is SHA-pinned — it does not verify a GitHub
Actions run.

## Result: 8 of 8 criteria PASS

| # | Criterion | Result |
|---|---|---|
| 1 | Layering check passes; provably fails on injected violation | PASS |
| 2 | Clippy clean, tests green | PASS (42 tests, 0 failed) |
| 3 | Supply chain (`cargo deny`, lockfiles, pinned Actions) | PASS (CI *execution* unverified — no push performed) |
| 4 | Stack serves health-checked page (podman) | PASS |
| 5 | pgvector, pg_trgm, turkish | PASS |
| 6 | Bindings and route-tree staleness checks | PASS |
| 7 | Licence and docs agree | PASS |
| 8 | Docker as well as podman; ignore files | PASS |

Phase 0a is complete by every criterion checkable without pushing to the remote.

---

## 1. Layering check passes, and provably fails on an injected violation

```bash
cargo xtask check-layers
printf '\nlapidary-index.workspace = true\n' >> crates/lapidary-vcs/Cargo.toml
cargo xtask check-layers; echo "expected non-zero: $?"
git checkout crates/lapidary-vcs/Cargo.toml
cargo xtask check-layers
```

Output:

```
layering OK — 14 workspace crates checked
```

After injecting `lapidary-index.workspace = true` into `crates/lapidary-vcs/Cargo.toml`:

```
Layering rule violated (1 problem(s)):

  lapidary-vcs (L2) -> lapidary-index (L2) is forbidden. L2 crates may depend only on L0 and L1. If these two need to share something, move it into lapidary-core.

The rule is in docs/ARCHITECTURE.md: L2 crates may depend on L0 and L1, never on each other or on L3.
Error: layering check failed
EXIT_CODE: 1
```

After `git checkout` reverting the injected edge:

```
Updated 1 path from the index
layering OK — 14 workspace crates checked
```

**PASS** — 14 crates checked clean; the injected `lapidary-vcs -> lapidary-index` edge is
caught and fails with exit code 1; reverting restores a clean pass.

---

## 2. Clippy clean, tests green

```bash
export DATABASE_URL=postgres://postgres:lapidary@localhost:5432/postgres
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

`cargo clippy` output (tail):

```
    Checking lapidary-storage v0.1.0 (/home/dev/All/Develop/lapidary-verify/crates/lapidary-storage)
    Checking lapidary-api v0.1.0 (/home/dev/All/Develop/lapidary-verify/crates/lapidary-api)
    Checking lapidary-server v0.1.0 (/home/dev/All/Develop/lapidary-verify/bin/lapidary-server)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.40s
```

No warnings, no errors, exit 0.

`cargo test --workspace --all-features`: every reported suite was `test result: ok`. 26
test binaries ran (unit tests per crate, doc-tests per crate, plus the DB-backed
integration suite `tests/health.rs` in `lapidary-api`, which exercised the live
`DATABASE_URL` connection — `3 passed` there confirms the DB-dependent path actually ran,
not just the DB-free unit tests). Totals summed across all suites:

```
total passed: 42
total failed: 0
```

Representative suites:

```
     Running tests/health.rs (target/debug/deps/health-87c211dc0ecdeac0)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.60s

     Running unittests src/lib.rs (target/debug/deps/lapidary_core-c51a24be0163a525)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s

     Running unittests src/main.rs (target/debug/deps/xtask-e5e890ac64d061e2)
running 13 tests
test layers::tests::rejects_l2_depending_on_l3 ... ok
test layers::tests::violation_message_names_the_edge_and_the_remedy ... ok
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**PASS** — clippy clean under `-D warnings`; all 42 tests across the workspace (including
the DB-backed health-check integration test) passed, 0 failed.

---

## 3. Supply chain

```bash
cargo deny check
test -f Cargo.lock && test -f web/package-lock.json && echo "lockfiles committed"
grep -rn "uses:" .github/workflows | grep -v "@[0-9a-f]\{40\}" && echo "FAIL" || echo "actions pinned"
```

`cargo deny check` (exit 0):

```
advisories ok, bans ok, licenses ok, sources ok
```

(9 non-fatal `duplicate` warnings for crates like `block-buffer`/`digest` pulled in at two
semver-major versions via `sqlx` — expected, not a failure; 0 `error`-level lines.)

Lockfiles:

```
lockfiles committed
```

Actions pinning:

```
actions pinned
```

(no `uses:` line in `.github/workflows` lacking a 40-hex-character commit SHA.)

**PASS** — `cargo deny check` is fully green in the clone; both lockfiles are committed;
every workflow `uses:` is SHA-pinned. Whether the workflows actually execute
successfully on GitHub was **not verified** — that requires a push, which this task was
explicitly told not to do.

---

## 4. Stack serves a health-checked page

```bash
cp deploy/.env.example deploy/.env && sed -i 's/change-me-before-first-run/localdev/' deploy/.env
podman compose --env-file deploy/.env -f deploy/compose.yaml up -d --build
curl -fsS http://localhost:3000/api/healthz
curl -fsS http://localhost:8080/api/healthz
```

Build completed (Rust release build ~3 min, `npm ci` + `vite build` for the web image,
then container start). Final container states:

```
lapidary-db-1      Up (healthy)
lapidary-worker-1  Up
lapidary-api-1     Up   0.0.0.0:8080->8080/tcp
lapidary-web-1     Up   0.0.0.0:3000->8080/tcp
```

```
=== curl :8080 ===
{"status":"ok","database":{"major":18,"reachable":true}}
=== curl :3000 ===
{"status":"ok","database":{"major":18,"reachable":true}}
```

**PASS** — both the direct API port (8080) and the Caddy-proxied port (3000, via
`/api/*`) return the healthy JSON payload.

---

## 5. pgvector and turkish

```bash
podman compose --env-file deploy/.env -f deploy/compose.yaml exec db psql -U lapidary -d lapidary \
  -c "SELECT extname, extversion FROM pg_extension WHERE extname IN ('vector','pg_trgm');" \
  -c "SELECT cfgname FROM pg_ts_config WHERE cfgname='turkish';"
```

```
 extname | extversion 
---------+------------
 vector  | 0.8.6
 pg_trgm | 1.6
(2 rows)

 cfgname 
---------
 turkish
(1 row)
```

**PASS** — `vector` 0.8.6, `pg_trgm` 1.6, and the `turkish` text-search config are all
present in the freshly built and started database.

---

## 6. Bindings track the Rust types, and the route tree tracks the routes

```bash
cargo xtask export-bindings && git diff --exit-code web/src/bindings && echo "bindings current"
(cd web && npm ci && npm run build) && git diff --exit-code web/src/routeTree.gen.ts && echo "route tree current"
```

Bindings:

```
running 7 tests
test ids::export_bindings_blobhash ... ok
test ids::export_bindings_libraryid ... ok
test approximate::export_bindings_approximate ... ok
test ids::export_bindings_partid ... ok
test ids::export_bindings_revisionid ... ok
test part::export_bindings_librarymode ... ok
test part::export_bindings_partsummary ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.36s

bindings written to /home/dev/All/Develop/lapidary-verify/web/src/bindings
```

`git diff --exit-code web/src/bindings` exited 0 (no diff) → `bindings current`.

Web build:

```
npm ci: added 183 packages, and audited 184 packages in 9s; found 0 vulnerabilities

> lapidary-web@0.1.0 build
> tsc --noEmit && vite build

vite v8.2.2 building client environment for production...
✓ 151 modules transformed.
dist/index.html                   0.40 kB │ gzip:  0.27 kB
dist/assets/index-B4o3zffG.css    6.31 kB │ gzip:  2.09 kB
dist/assets/index-CLsjSciH.js   300.60 kB │ gzip: 95.65 kB
✓ built in 234ms
```

`git diff --exit-code web/src/routeTree.gen.ts` exited 0 (no diff) → route tree current.

**PASS** — both staleness checks confirm the generated files match the checked-in copies
in a clean clone (`npm ci`, not `npm install`, so no local `node_modules` interference).

---

## 7. Licence and docs agree

```bash
head -3 LICENSE
grep -rn "MIGRATION" README.md docs/README.md && echo "FAIL: MIGRATION still referenced" || echo "OK: no MIGRATION references"
grep -rn -i "licence has not been decided\|not yet decided\|all rights are reserved\|Licensing — decision required\|Licensing conflict" \
  README.md CONTRIBUTING.md docs/ARCHITECTURE.md docs/ROADMAP.md \
  && echo "FAIL: docs still record the licence as undecided" || echo "OK: licence recorded as AGPL-3.0-only"
```

```
=== LICENSE head ===
                    GNU AFFERO GENERAL PUBLIC LICENSE
                       Version 3, 19 November 2007

=== MIGRATION references ===
OK: no MIGRATION references

=== licence-undecided phrases ===
OK: licence recorded as AGPL-3.0-only
```

**PASS** — `LICENSE` is AGPL-3.0-only; no tracked doc (`README.md`, `docs/README.md`)
still references the withdrawn `MIGRATION.md`; no doc still records the licence as
undecided. (Note: an untracked `MIGRATION.md` and `lapidary-docs.zip` exist in the
working repo the clone was cut from — neither is committed, so neither reaches the
clone or affects this criterion.)

---

## 8. Docker as well as podman — and both ignore files present, or the context is the whole repo

```bash
cmp .containerignore .dockerignore && cmp deploy/.containerignore deploy/.dockerignore
podman compose --env-file deploy/.env -f deploy/compose.yaml down -v
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build && curl -fsS http://localhost:8080/api/healthz
docker compose --env-file deploy/.env -f deploy/compose.yaml down -v
```

`cmp` (both pairs identical, exit 0, no output):

```
$ cmp .containerignore .dockerignore; echo exit:$?
exit:0
$ cmp deploy/.containerignore deploy/.dockerignore; echo exit:$?
exit:0
```

Podman teardown:

```
 Container lapidary-worker-1 Removed 
 Container lapidary-web-1 Removed 
 Container lapidary-api-1 Removed 
 Container lapidary-db-1 Removed 
 Volume lapidary_lapidary-db Removed 
 Network lapidary_default Removed 
```

Docker build/up (fresh build — Docker has its own separate image/layer store from
Podman, so nothing was reused across engines) + healthz + teardown:

```
#36 [api build 7/7] RUN cargo build --release --locked -p lapidary-server
...
#45 [api] exporting to image
#45 naming to docker.io/library/lapidary-api:latest 0.1s done
#45 unpacking to docker.io/library/lapidary-api:latest 0.2s done
#45 DONE 2.8s
 Image lapidary-db Built
 Image lapidary-api Built
 Image lapidary-web Built
 Image lapidary-worker Built
 Container lapidary-db-1 Started
 Container lapidary-db-1  Up 13 seconds (healthy)
 Container lapidary-api-1  Up 1 second   0.0.0.0:8080->8080/tcp
 Container lapidary-worker-1  Up 1 second
EXIT: 0
```

```
$ curl -fsS http://localhost:8080/api/healthz
{"status":"ok","database":{"major":18,"reachable":true}}
```

Teardown:

```
 Container lapidary-web-1 Removed
 Container lapidary-api-1 Removed
 Container lapidary-worker-1 Removed
 Container lapidary-db-1 Removed
 Volume lapidary_lapidary-db Removed
 Network lapidary_default Removed
```

Post-teardown state: `docker ps -a` empty; `podman ps -a` shows only the pre-existing,
untouched `lapidary-test-db`.

**Scope note:** Podman here is rootless, and its `podman compose` subcommand delegates to
the `docker-compose` CLI plugin (visible in every podman-compose invocation's banner:
`Executing external compose provider "/usr/libexec/docker/cli-plugins/docker-compose"`).
So this criterion exercises two container **engines** (podman, docker) but a single
compose **implementation** underneath both — not two independent compose stacks.

**PASS** — ignore-file pairs are byte-identical; the stack builds and serves a healthy
`/api/healthz` under both podman and docker; both stacks torn down with `down -v`
leaving no containers, volumes, or networks behind.
