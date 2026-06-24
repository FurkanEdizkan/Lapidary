# Library Ingest — Phase 2 (Thumbnails & Viewer Mesh) Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the indexed library viewable — a background `thumbnail` job handler that, per model, extracts the **largest** mesh from its archive and uses an extended Rust `rust-mesh` to write a **viewer LOD** (3D viewer works) + a **rendered thumbnail PNG** (gallery fills in) + backfills bbox/triangle count. Proven on the Creature Caster folder.

**Architecture:** The `thumbnail` job is already enqueued per model (Phase 1) but unhandled. Add a Node worker handler that picks the largest archive entry, extracts it, shells out to `rust-mesh --lod --thumb --json`, and writes the LOD/thumbnail + updates the model row. The gallery auto-shows `/api/models/:id/thumbnail` and the viewer auto-loads `/api/models/:id/lod` once the `*_path` columns are set — no gallery mesh load.

**Tech Stack:** Rust (extend `rust-mesh`, add the pure-Rust `png` crate), TypeScript ESM worker, `better-sqlite3`, `adm-zip`/`7zip-bin`/`node-unrar-js`, vitest.

## Global Constraints

- **Rust on PATH:** bare `cargo` is NOT on the non-interactive PATH here. Prefix every cargo/rust-mesh build with `export PATH="$HOME/.cargo/bin:$PATH"`. The binary lands at `rust-mesh/target/release/rust-mesh`.
- **Graceful degradation:** if the `rust-mesh` binary is absent (no `MESH_SIDECAR_BIN`, not built), the app still runs and indexes; `thumbnail` jobs fail cleanly (retryable → `failed`) and tiles keep the placeholder. Never crash the worker.
- **Index-in-place:** still no archive copying. Only the small LOD `.stl`, thumbnail `.png`, and the chosen `entry_path` string are stored; the full mesh is extracted on demand for "View full mesh".
- **Reuse, don't reinvent:** `listMeshEntries` (archive.service), `updateModel` allow-list (already permits `thumbnailPath`/`lodPath`/`triangleCount`/`size`), `config.thumbnailsDir`/`lodDir` + `${id}.png`/`${id}.stl` naming, the `meshSidecar` `execFileAsync` + `sidecarAvailable()` graceful-null pattern, the archive-reader machinery (`adm-zip`, `sevenBin.path7za` + `ensureSevenZipExecutable`, `node-unrar-js`).
- ESM `.js` import extensions. Test command: `npm --workspace server test` (sets `DATA_DIR=./.test-data`, `fileParallelism:false`).
- **Thumbnail look:** background `#0d0d0e` (viewer ink-well), mesh base `#bcc0c8`, single key light (matches `DESIGN.md`). Default size 512×512.

---

### Task 1: `rust-mesh` — thumbnail rasterizer (`--thumb` / `--size`)

**Files:** Modify `rust-mesh/src/main.rs`, `rust-mesh/Cargo.toml`; Create `rust-mesh/tests/render.rs`.

**Interfaces produced:** CLI `rust-mesh <mesh> [--lod out.stl] [--thumb out.png] [--size N] [--json]`. `--json` output unchanged (`{"bbox":[x,y,z],"triangles":N}`).

- [ ] **Step 1 — failing test** `rust-mesh/tests/render.rs`: invoke the built binary (or call a `pub fn render_png(mesh, size) -> Vec<u8>` exposed from a small lib module) on `../fixtures/cube.stl` with `--thumb <tmp>.png --size 64`; assert the file exists, starts with the PNG magic `\x89PNG\r\n\x1a\n`, decodes to 64×64, and is **not uniform** (at least some pixels differ from the background `#0d0d0e` — i.e. the cube was drawn). Also assert `--json` still prints `bbox [20,20,20]`.
- [ ] **Step 2 — run, see it fail** `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --manifest-path rust-mesh/Cargo.toml` → fails (unknown `--thumb`).
- [ ] **Step 3 — implement.** Add `png = "0.17"` to `Cargo.toml` `[dependencies]` (pure-Rust, default features). In `main.rs`: parse `--thumb`/`--size` (reuse `flag_value`). Add a `render(mesh, size) -> Vec<u8>` (RGBA) that: computes `bounds`; sets an orthographic camera framing the bbox with a slight isometric rotation (rotate verts ~30° about Y then ~25° about X); for each triangle, project to screen, rasterize with a depth buffer; shade Lambert `dot(normal, light)` with one key light (e.g. dir `[-0.4,0.6,0.8]` normalized), base color `#bcc0c8`, ambient ~0.15, on background `#0d0d0e`. Encode with the `png` crate (RGBA8) to `--thumb`. Keep `--lod`/`--json` behavior intact.
- [ ] **Step 4 — pass** `cargo test --manifest-path rust-mesh/Cargo.toml` green; smoke `rust-mesh/target/release/rust-mesh fixtures/cube.stl --thumb /tmp/c.png --size 256 --json` writes a visible cube PNG.
- [ ] **Step 5 — commit** `feat(mesh): render thumbnail PNG (--thumb/--size) via software rasterizer`

> Note: `cargo` fetches the `png` crate from crates.io (network needed once). If strictly offline, hand-roll a stored-deflate PNG instead — but `png` is preferred.

---

### Task 2: Build + auto-wire the binary

**Files:** Modify `server/src/config.ts`, root `package.json`, `.env.example`, `README.md`; Test `server/test/config.meshbin.test.ts`.

**Interfaces:** `config.meshSidecarBin` resolves to `MESH_SIDECAR_BIN` env, else an auto-detected built binary path, else `null`.

- [ ] **Step 1 — failing test:** a test that, with `MESH_SIDECAR_BIN` unset, asserts `resolveMeshSidecarBin()` returns the path to `rust-mesh/target/release/rust-mesh` when it exists on disk (it does — Task 1 built it), else null. (Export a small pure `resolveMeshSidecarBin()` from config for testability.)
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** add `resolveMeshSidecarBin()` that returns `process.env.MESH_SIDECAR_BIN` if set, else the first existing candidate among `[<cwd>/rust-mesh/target/release/rust-mesh, <import.meta.dirname>/../../../rust-mesh/target/release/rust-mesh, <import.meta.dirname>/../../../../rust-mesh/target/release/rust-mesh]` (mirror `resolveWebDist`), else null. Use it for `config.meshSidecarBin`. Add root script `"build:mesh": "cargo build --release --manifest-path rust-mesh/Cargo.toml"`; uncomment/annotate `MESH_SIDECAR_BIN` guidance in `.env.example`; add a `README.md` line: run `npm run build:mesh` once (needs Rust; `. \"$HOME/.cargo/env\"`).
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(config): auto-detect built rust-mesh binary + build:mesh script`

---

### Task 3: Migration v3 — `models.entry_path`

**Files:** Modify `server/src/db/database.ts`, `server/src/services/model.service.ts`; Test `server/test/migrate.test.ts` (extend).

- [ ] **Step 1 — failing test:** extend migrate test — after `migrate`, `user_version === 3` and `pragma table_info(models)` includes `entry_path`. Add a v2→v3 upgrade case.
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** add a `version < 3` block: `d.transaction(() => { d.exec("ALTER TABLE models ADD COLUMN entry_path TEXT"); d.pragma('user_version = 3'); })()`. In `model.service.ts`: add `entryPath: 'entry_path'` to the `updateModel` allow-list; add `entry` to `getModelPaths` (select `entry_path` → return `entry`).
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(db): add models.entry_path (migration v3)`

---

### Task 4: `archive.extractEntry`

**Files:** Modify `server/src/services/archive.service.ts`; Test `server/test/archive.extract.test.ts`.

**Interfaces:** `extractEntry(archivePath: string, innerPath: string, destDir: string): Promise<string>` — extracts one entry **flat** to `destDir`, returns the written file path (`join(destDir, basename(innerPath))`).

- [ ] **Step 1 — failing test:** extract `cube.stl` from `fixtures/archives/cube.zip` and `cube.7z` into a temp dir; assert the returned file exists and its bytes equal `fixtures/cube.stl`. Rar via the `LAPIDARY_TEST_RAR`-guarded pattern.
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** zip → `new AdmZip(archivePath).readFile(entry)` (find the entry by name) → `fs.writeFileSync(out, buf)`. 7z → `await ensureSevenZipExecutable(); execFileP(sevenBin.path7za, ['e', archivePath, '-o'+destDir, innerPath, '-y'], {timeout:120_000, maxBuffer:64*1024*1024})` (flat extract). rar → `createExtractorFromFile({filepath})` then `extractor.extract({files:[innerPath]})`, take the yielded file's `extraction` bytes → write. Return `join(destDir, path.basename(innerPath))`.
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(archive): single-entry flat extraction (extractEntry)`

---

### Task 5: `meshSidecar.renderAndAnalyze`

**Files:** Modify `server/src/services/meshSidecar.service.ts`; Test `server/test/meshSidecar.render.test.ts`.

**Interfaces:** `renderAndAnalyze(inputPath, lodOut, thumbOut, size=512): Promise<{bbox:[number,number,number]; triangles:number; lodWritten:boolean; thumbWritten:boolean} | null>`.

- [ ] **Step 1 — failing test (integration, needs the built binary):** point `MESH_SIDECAR_BIN` at `rust-mesh/target/release/rust-mesh`, call `renderAndAnalyze(fixtures/cube.stl, tmpLod, tmpThumb, 128)`; assert `bbox≈[20,20,20]`, `triangles===12`, both files written, thumb is a PNG. Skip the test (`describe.skipIf(!fs.existsSync(bin))`) if the binary isn't built, so CI without Rust stays green.
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** mirror `analyzeMesh`, args `[inputPath, '--lod', lodOut, '--thumb', thumbOut, '--size', String(size), '--json']`, reuse `sidecarAvailable()` (returns null when unavailable) + `execFileAsync` + timeout 120_000; parse JSON; return existence flags. Keep `analyzeMesh` unchanged.
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(mesh): renderAndAnalyze sidecar entrypoint`

---

### Task 6: `thumbnail` job handler + worker wiring

**Files:** Create `server/src/services/thumbnail.service.ts`; Modify `server/src/worker.ts`; Test `server/test/thumbnail.test.ts`.

**Interfaces:** `thumbnailJob(job: JobRow, db: Database.Database): Promise<void>`.

- [ ] **Step 1 — failing test (singleton `.test-data` DB, skipIf no binary):** insert/create a model whose `original_path = fixtures/archives/cube.zip`; run `thumbnailJob({payload:{path: zip}})`; assert `thumbnail_path` + `lod_path` set, those files exist, `triangle_count>0`, `bbox≈[20,20,20]`, `entry_path === 'cube.stl'`. Add an idempotency test (second run no-op) and a graceful test: with the sidecar unavailable, `thumbnailJob` rejects (retryable) and writes nothing.
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** `thumbnailJob`: load the model (`getModelPaths`/row); if `thumbnail_path` already set → return (idempotent). Read `payload.path`; if its ext ∈ `{.zip,.rar,.7z}` → `listMeshEntries` → pick `entries.reduce(largest by sizeBytes)` → `extractEntry` into `fs.mkdtempSync(path.join(os.tmpdir(),'lap-'))`; else use the path directly (entry = null). `renderAndAnalyze(mesh, join(lodDir,id+'.stl'), join(thumbnailsDir,id+'.png'), 512)`; if null → throw `Error('mesh sidecar unavailable')`. `updateModel(id, { lodPath, thumbnailPath, size: bbox, triangleCount, entryPath: chosenInner })`. `finally` rm the temp dir. Register `thumbnail: thumbnailJob` in `WORKER_HANDLERS` (and the startup log).
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(thumbnail): worker handler renders LOD+thumbnail per model`

---

### Task 7: Archive-aware `/api/models/:id/original` (View full mesh)

**Files:** Modify `server/src/routes/api.ts`; Test `server/test/original.route.test.ts` (or extend an existing route test).

- [ ] **Step 1 — failing test:** build a Fastify instance (or call the handler) for a model with `original_path=cube.zip`, `entry_path=cube.stl`; GET `/original` returns the STL bytes (equal to `fixtures/cube.stl`), not the zip. A model with a non-archive original still returns the decompressed original.
- [ ] **Step 2 — run, fail.**
- [ ] **Step 3 — implement:** in `/original`, after `getModelPaths`, if `path.extname(paths.original)` ∈ archive exts and `paths.entry` set → `extractEntry(paths.original, paths.entry, mkdtemp)` → read bytes → send with `X-Model-Format`, clean temp; else the existing `decompress` path. 404 if entry missing.
- [ ] **Step 4 — pass.**
- [ ] **Step 5 — commit** `feat(api): serve extracted mesh from archive on /original`

---

### Task 8: Gallery in-progress skeleton (web)

**Files:** Modify `web/src/lib/thumbs.ts`, `web/src/components/ModelTile.tsx` (+ `CardsView.tsx`/`ListView.tsx` as needed).

- [ ] **Step 1 — implement:** when a real model is still rendering (`model.hasOriginal && !model.hasThumbnail && !model.meshKind`), render a subtle pulsing "rendering…" shimmer over the gradient placeholder (use `DESIGN.md` tokens — `--surface-2`/`--surface-3`, ~160ms, respect `prefers-reduced-motion`). The gallery already auto-swaps to `/thumbnail` once `hasThumbnail` flips. No data changes.
- [ ] **Step 2 — verify** `npm --workspace web run build` (tsc) clean; eyeball in the gate.
- [ ] **Step 3 — commit** `feat(web): rendering skeleton for tiles awaiting thumbnails`

---

### Task 9: Acceptance gate — Creature Caster

- [ ] **Step 1:** `export PATH="$HOME/.cargo/bin:$PATH"; npm run build:mesh`; then `npm --workspace server test` (all green incl. the render/thumbnail suites; rar+skipIf suites skipped appropriately); `npm --workspace server run build` + `npm --workspace web run build` clean.
- [ ] **Step 2 — gate:** fresh `DATA_DIR`, start server + worker, `POST /api/scan {folderPath:".../Creature Caster"}`, drain. Assert via `/api/models | jq`: all 7 have `hasThumbnail:true`, `hasLod:true`, non-zero `size` + `triangleCount`. In the browser: all 7 tiles show a **rendered thumbnail**; opening each shows the **mesh in the 3D viewer**; "View full mesh" loads the full mesh. Idempotent re-scan; nothing written under `/mnt/Storage2`.
- [ ] **Step 3 — runbook:** update `README.md` "Scan a library" to note thumbnails/viewer now populate in the background.
- [ ] **Step 4 — commit** `test(thumbnail): Creature Caster gate — thumbnails + 3D viewer`

---

## Self-Review
- Spec coverage: rasterizer (T1), build/wire (T2), entry_path (T3), extraction (T4), sidecar render (T5), handler+wiring (T6), View-full-mesh (T7), web skeleton (T8), real gate (T9). ✓
- Ordering: T3 (entry_path column) precedes T6 (handler sets entryPath) and T7 (route reads entry). ✓
- Degradation: skipIf-no-binary on render/thumbnail tests keeps CI green without Rust; worker throws-but-doesn't-crash when the sidecar is missing. ✓
- Reuse: `listMeshEntries`, `updateModel` allow-list, `getModelPaths`, `sidecarAvailable`, archive readers, dir/naming conventions — all reused, not reinvented. ✓
