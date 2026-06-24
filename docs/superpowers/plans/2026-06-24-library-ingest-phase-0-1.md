# Library Ingest — Phase 0 (Worker Foundation) + Phase 1 (Archive-Aware Indexing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Point Lapidary at a folder of archived STLs and have a background worker index every `.zip`/`.rar`/`.7z` (and loose mesh) into the gallery with the correct creator, category, and name — proven on the Creature Caster folder.

**Architecture:** Add a second Node process (`server/src/worker.ts`) that polls a new `jobs` table and runs the heavy work, sharing the existing SQLite (WAL) + `DATA_DIR` with the Fastify server. The scan endpoint stops doing work inline and instead *enqueues* one `index_archive` job per library item; the worker peeks each archive, derives metadata from the path, and creates a model row that points at the archive in place (nothing is copied).

**Tech Stack:** TypeScript (ESM, `moduleResolution: Bundler`), Fastify, `better-sqlite3`, `nanoid`; new: `vitest` (tests), `adm-zip` (zip), `node-unrar-js` (rar, pure-WASM), `7zip-bin` (bundled `7za` binary, shelled out for 7z listing).

## Global Constraints

- **Node:** `>=22` (dev box is v24). ESM only; source-to-source imports use **`.js` extensions** to match the existing codebase (e.g. `import { getDb } from '../db/database.js'`).
- **Index in place:** never copy archives or extracted STLs into `DATA_DIR`. `models.original_path` holds the absolute source path on disk.
- **Schema parity:** the 9 existing tables are **untouched**. New schema is added only via a forward migration that bumps `PRAGMA user_version` from 1 to 2.
- **Two-process safety:** `getDb()` sets `journal_mode=WAL`, `foreign_keys=ON`, and `busy_timeout=5000`.
- **Testability seam:** every new service function takes the `better-sqlite3` handle as a trailing parameter defaulting to `getDb()` (e.g. `enqueue(input, db = getDb())`), so unit tests run against an isolated `new Database(':memory:')`. Tests that exercise `createModel` (which uses the `getDb()` singleton internally) run against a throwaway file DB via `DATA_DIR=./.test-data` and clean tables in `beforeEach`. (`createModel` calls `invalidate()` → `bumpNamespace`, which is safe without `initCache()`: with no `REDIS_URL` it takes the synchronous in-process LRU branch and never throws.)
- **Test fixture paths:** resolve via `server/test/helpers.ts` (`fixturesDir`/`archivesDir`), which use `fileURLToPath(import.meta.url)` — do **not** use `import.meta.dirname` (not reliably populated under Vitest).
- **Job kinds:** `'index_archive' | 'thumbnail' | 'image_fetch'`. Only `index_archive` has a handler in this plan; `thumbnail`/`image_fetch` rows are enqueued but left `queued` for later phases (the worker never claims a kind it has no handler for).
- **Mesh extensions:** `.stl`, `.3mf`, `.obj`. **Archive extensions:** `.zip`, `.rar`, `.7z`.
- **Test command (run from repo root):** `npm --workspace server test`.

---

### Task 1: Test harness, dependencies, fixtures helper, and worker scripts

**Files:**
- Modify: `server/package.json`
- Modify: `package.json` (root)
- Create: `server/vitest.config.ts`
- Create: `server/test/helpers.ts`
- Create: `server/test/sanity.test.ts`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: a working `npm --workspace server test`; `fixturesDir`/`archivesDir` (absolute paths, used by every fixture-backed test); the dependencies every later task imports.

- [ ] **Step 1: Set the `scripts`, `dependencies`, and `devDependencies` in `server/package.json`**

```jsonc
  "scripts": {
    "dev": "tsx watch src/index.ts",
    "dev:worker": "tsx watch src/worker.ts",
    "build": "tsc -p tsconfig.json",
    "start": "node dist/index.js",
    "worker": "node dist/worker.js",
    "test": "DATA_DIR=./.test-data vitest run",
    "test:watch": "DATA_DIR=./.test-data vitest"
  },
  "dependencies": {
    "@fastify/multipart": "^9.0.1",
    "@fastify/static": "^9.1.3",
    "7zip-bin": "^5.2.0",
    "adm-zip": "^0.5.16",
    "better-sqlite3": "^11.8.1",
    "fastify": "^5.2.1",
    "ioredis": "^5.4.2",
    "lru-cache": "^11.0.2",
    "nanoid": "^5.0.9",
    "node-unrar-js": "^2.0.2",
    "p-limit": "^6.2.0"
  },
  "devDependencies": {
    "@types/adm-zip": "^0.5.8",
    "@types/better-sqlite3": "^7.6.12",
    "@types/node": "^22.10.5",
    "tsx": "^4.19.2",
    "typescript": "^5.7.3",
    "vitest": "^3.2.0"
  }
```

- [ ] **Step 2: Wire the worker into the root dev script in `package.json`**

```jsonc
  "scripts": {
    "dev": "concurrently -n server,worker,web -c cyan,yellow,magenta \"npm:dev:server\" \"npm:dev:worker\" \"npm:dev:web\"",
    "dev:server": "npm --workspace server run dev",
    "dev:worker": "npm --workspace server run dev:worker",
    "dev:web": "npm --workspace web run dev",
    "build": "npm --workspace server run build && npm --workspace web run build",
    "start": "node server/dist/index.js",
    "start:worker": "node server/dist/worker.js"
  },
```

- [ ] **Step 3: Create `server/vitest.config.ts`**

```ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  // The codebase uses NodeNext-style `.js` import specifiers that point at `.ts`
  // sources; map them so Vitest can resolve them.
  resolve: { extensionAlias: { '.js': ['.ts', '.js'] } },
  test: {
    environment: 'node',
    include: ['test/**/*.test.ts'],
    // Tests that use the getDb() singleton share one file DB; run files
    // sequentially so they don't clobber each other.
    fileParallelism: false,
    // better-sqlite3 is a native addon — let Node load it, don't let Vite transform it.
    server: { deps: { external: ['better-sqlite3'] } },
  },
});
```

- [ ] **Step 4: Create `server/test/helpers.ts`**

```ts
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// fileURLToPath(import.meta.url) is the portable way to get this file's dir under
// both Node and Vitest. server/test -> ../../ is the repo root.
const here = path.dirname(fileURLToPath(import.meta.url));

export const fixturesDir = path.resolve(here, '../../fixtures');
export const archivesDir = path.resolve(fixturesDir, 'archives');
```

- [ ] **Step 5: Create `server/test/sanity.test.ts` (also proves the fixture-path mechanism at Task 1)**

```ts
import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { fixturesDir } from './helpers.js';

describe('test harness', () => {
  it('runs', () => {
    expect(1 + 1).toBe(2);
  });

  it('resolves the fixtures dir via fileURLToPath', () => {
    expect(fs.existsSync(path.join(fixturesDir, 'cube.stl'))).toBe(true);
  });
});
```

- [ ] **Step 6: Ignore the test data dir — append to `.gitignore`**

```
.test-data/
```

- [ ] **Step 7: Install and run**

Run: `npm install`
Then run: `npm --workspace server test`
Expected: vitest runs `sanity.test.ts` → 2 passed.

- [ ] **Step 8: Commit**

```bash
git add server/package.json package.json package-lock.json server/vitest.config.ts server/test/helpers.ts server/test/sanity.test.ts .gitignore
git commit -m "chore(server): add vitest harness, archive deps, and worker scripts"
```

---

### Task 2: Incremental migrations + `jobs` table (`user_version` 1 → 2)

**Files:**
- Modify: `server/src/db/database.ts`
- Test: `server/test/migrate.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `export function migrate(d: Database.Database): void` (now exported); a `jobs` table at `user_version = 2`.

- [ ] **Step 1: Write the failing test — `server/test/migrate.test.ts`**

```ts
import { describe, it, expect } from 'vitest';
import Database from 'better-sqlite3';
import { migrate } from '../src/db/database.js';

function tables(db: Database.Database): string[] {
  return (db.prepare("SELECT name FROM sqlite_master WHERE type='table'").all() as { name: string }[])
    .map((r) => r.name);
}

describe('migrate', () => {
  it('creates base schema + jobs and sets user_version to 2', () => {
    const db = new Database(':memory:');
    migrate(db);
    expect(db.pragma('user_version', { simple: true })).toBe(2);
    expect(tables(db)).toEqual(expect.arrayContaining(['models', 'tags', 'jobs']));
  });

  it('upgrades a v1 database to v2 without recreating existing tables', () => {
    const db = new Database(':memory:');
    migrate(db);                 // -> v2
    db.exec('DROP TABLE jobs');  // simulate a pre-jobs v1 database
    db.pragma('user_version = 1');
    migrate(db);                 // should only add jobs, bump to 2
    expect(db.pragma('user_version', { simple: true })).toBe(2);
    expect(tables(db)).toContain('jobs');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- migrate`
Expected: FAIL — `migrate` is not exported / not a function.

- [ ] **Step 3: Refactor `server/src/db/database.ts`**

Replace the whole file with:

```ts
import Database from 'better-sqlite3';
import { config } from '../config.js';
import { seedDatabase } from './seed.js';

let db: Database.Database | null = null;

/** Returns the singleton SQLite connection, creating + migrating it on first call. */
export function getDb(): Database.Database {
  if (db) return db;
  db = new Database(config.dbPath);
  db.pragma('journal_mode = WAL');
  db.pragma('foreign_keys = ON');
  db.pragma('busy_timeout = 5000'); // two processes (server + worker) share this file
  migrate(db);
  seedDatabase(db);
  return db;
}

const BASE_SCHEMA = `
  CREATE TABLE models (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    creator TEXT NOT NULL DEFAULT 'Unknown',
    type TEXT NOT NULL DEFAULT 'Miniature',
    mesh_kind TEXT,
    color TEXT NOT NULL DEFAULT '#bcc0c8',
    format TEXT NOT NULL DEFAULT 'STL',
    file_size_bytes INTEGER NOT NULL DEFAULT 0,
    bbox_x REAL NOT NULL DEFAULT 0,
    bbox_y REAL NOT NULL DEFAULT 0,
    bbox_z REAL NOT NULL DEFAULT 0,
    triangle_count INTEGER NOT NULL DEFAULT 0,
    created_date TEXT,
    added_date TEXT NOT NULL,
    original_path TEXT,
    lod_path TEXT,
    thumbnail_path TEXT,
    notes TEXT
  );

  CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
  );
  CREATE TABLE model_tags (
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (model_id, tag_id)
  );

  CREATE TABLE groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    shared INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE model_groups (
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    PRIMARY KEY (model_id, group_id)
  );

  CREATE TABLE printer_types (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
  );
  CREATE TABLE model_printer_types (
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    printer_type_id INTEGER NOT NULL REFERENCES printer_types(id) ON DELETE CASCADE,
    PRIMARY KEY (model_id, printer_type_id)
  );

  CREATE TABLE printer_settings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    ord INTEGER NOT NULL DEFAULT 0,
    k TEXT NOT NULL,
    v TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    raw_json TEXT,
    profile_path TEXT
  );

  CREATE TABLE images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    caption TEXT,
    kind TEXT NOT NULL DEFAULT 'printed'
  );

  CREATE TABLE pins (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    UNIQUE (kind, name)
  );

  CREATE INDEX idx_model_tags_tag ON model_tags(tag_id);
  CREATE INDEX idx_model_groups_group ON model_groups(group_id);
  CREATE INDEX idx_models_creator ON models(creator);
  CREATE INDEX idx_settings_model ON printer_settings(model_id);
  CREATE INDEX idx_images_model ON images(model_id);
`;

const JOBS_SCHEMA = `
  CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    model_id TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    payload_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );
  CREATE INDEX idx_jobs_status ON jobs(status);
  CREATE INDEX idx_jobs_kind_status ON jobs(kind, status);
`;

/** Idempotent, forward-only schema migration, versioned via PRAGMA user_version. */
export function migrate(d: Database.Database): void {
  let version = d.pragma('user_version', { simple: true }) as number;
  if (version < 1) {
    d.exec(BASE_SCHEMA);
    d.pragma('user_version = 1');
    version = 1;
  }
  if (version < 2) {
    d.exec(JOBS_SCHEMA);
    d.pragma('user_version = 2');
    version = 2;
  }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- migrate`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add server/src/db/database.ts server/test/migrate.test.ts
git commit -m "feat(db): add jobs table via user_version 2 migration"
```

---

### Task 3: `jobs.service` — enqueue / claim / complete / fail

**Files:**
- Create: `server/src/services/jobs.service.ts`
- Test: `server/test/jobs.service.test.ts`

**Interfaces:**
- Consumes: `getDb`, `migrate` (tests), `nanoid`.
- Produces:
  - `type JobKind = 'index_archive' | 'thumbnail' | 'image_fetch'`
  - `type JobStatus = 'queued' | 'running' | 'done' | 'failed'`
  - `interface JobRow { id; modelId: string|null; kind: JobKind; status: JobStatus; attempts: number; error: string|null; payload: Record<string,unknown>|null; createdAt: string; updatedAt: string }`
  - `enqueue(input: { kind: JobKind; modelId?: string|null; payload?: Record<string,unknown>|null }, db?): JobRow`
  - `claimNext(kinds: JobKind[], db?): JobRow | null`
  - `completeJob(id: string, db?): void`
  - `failJob(id: string, error: string, maxAttempts: number, db?): void`
  - `getJob(id: string, db?): JobRow | null`
  - `countByStatus(db?): Record<JobStatus, number>`

- [ ] **Step 1: Write the failing test — `server/test/jobs.service.test.ts`**

```ts
import { describe, it, expect } from 'vitest';
import Database from 'better-sqlite3';
import { migrate } from '../src/db/database.js';
import {
  enqueue, claimNext, completeJob, failJob, getJob, countByStatus,
} from '../src/services/jobs.service.js';

function freshDb(): Database.Database {
  const db = new Database(':memory:');
  migrate(db);
  return db;
}

describe('jobs.service', () => {
  it('enqueues a queued job and round-trips the payload', () => {
    const db = freshDb();
    const job = enqueue({ kind: 'index_archive', payload: { path: '/x.zip', root: '/' } }, db);
    expect(job.status).toBe('queued');
    expect(job.attempts).toBe(0);
    expect(job.payload).toEqual({ path: '/x.zip', root: '/' });
  });

  it('claimNext moves queued -> running and increments attempts', () => {
    const db = freshDb();
    enqueue({ kind: 'index_archive', payload: { path: '/a' } }, db);
    const claimed = claimNext(['index_archive'], db)!;
    expect(claimed.status).toBe('running');
    expect(claimed.attempts).toBe(1);
    expect(claimNext(['index_archive'], db)).toBeNull();
  });

  it('claimNext ignores kinds not requested', () => {
    const db = freshDb();
    enqueue({ kind: 'thumbnail' }, db);
    expect(claimNext(['index_archive'], db)).toBeNull();
  });

  it('completeJob marks a job done', () => {
    const db = freshDb();
    const j = enqueue({ kind: 'index_archive' }, db);
    claimNext(['index_archive'], db);
    completeJob(j.id, db);
    expect(getJob(j.id, db)!.status).toBe('done');
  });

  it('failJob requeues until maxAttempts is reached, then fails', () => {
    const db = freshDb();
    const j = enqueue({ kind: 'index_archive' }, db);
    claimNext(['index_archive'], db);              // attempts = 1
    failJob(j.id, 'boom', 3, db);
    expect(getJob(j.id, db)!.status).toBe('queued');
    claimNext(['index_archive'], db);              // attempts = 2
    failJob(j.id, 'boom', 3, db);
    expect(getJob(j.id, db)!.status).toBe('queued');
    claimNext(['index_archive'], db);              // attempts = 3
    failJob(j.id, 'boom', 3, db);
    expect(getJob(j.id, db)!.status).toBe('failed');
  });

  it('countByStatus tallies queued jobs', () => {
    const db = freshDb();
    enqueue({ kind: 'index_archive' }, db);
    enqueue({ kind: 'index_archive' }, db);
    expect(countByStatus(db).queued).toBe(2);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- jobs.service`
Expected: FAIL — cannot find module `jobs.service`.

- [ ] **Step 3: Implement `server/src/services/jobs.service.ts`**

```ts
import { nanoid } from 'nanoid';
import type Database from 'better-sqlite3';
import { getDb } from '../db/database.js';

export type JobKind = 'index_archive' | 'thumbnail' | 'image_fetch';
export type JobStatus = 'queued' | 'running' | 'done' | 'failed';

export interface JobRow {
  id: string;
  modelId: string | null;
  kind: JobKind;
  status: JobStatus;
  attempts: number;
  error: string | null;
  payload: Record<string, unknown> | null;
  createdAt: string;
  updatedAt: string;
}

interface JobDbRow {
  id: string;
  model_id: string | null;
  kind: string;
  status: string;
  attempts: number;
  error: string | null;
  payload_json: string | null;
  created_at: string;
  updated_at: string;
}

function toRow(r: JobDbRow): JobRow {
  return {
    id: r.id,
    modelId: r.model_id,
    kind: r.kind as JobKind,
    status: r.status as JobStatus,
    attempts: r.attempts,
    error: r.error,
    payload: r.payload_json ? (JSON.parse(r.payload_json) as Record<string, unknown>) : null,
    createdAt: r.created_at,
    updatedAt: r.updated_at,
  };
}

const now = (): string => new Date().toISOString();

export interface EnqueueInput {
  kind: JobKind;
  modelId?: string | null;
  payload?: Record<string, unknown> | null;
}

export function enqueue(input: EnqueueInput, db: Database.Database = getDb()): JobRow {
  const id = `j${nanoid(12)}`;
  const ts = now();
  db.prepare(
    `INSERT INTO jobs (id, model_id, kind, status, attempts, error, payload_json, created_at, updated_at)
     VALUES (?, ?, ?, 'queued', 0, NULL, ?, ?, ?)`,
  ).run(id, input.modelId ?? null, input.kind, input.payload ? JSON.stringify(input.payload) : null, ts, ts);
  return getJob(id, db)!;
}

export function getJob(id: string, db: Database.Database = getDb()): JobRow | null {
  const r = db.prepare('SELECT * FROM jobs WHERE id = ?').get(id) as JobDbRow | undefined;
  return r ? toRow(r) : null;
}

/** Atomically claim the oldest queued job among `kinds`: queued -> running, attempts + 1. */
export function claimNext(kinds: JobKind[], db: Database.Database = getDb()): JobRow | null {
  if (!kinds.length) return null;
  const placeholders = kinds.map(() => '?').join(',');
  const tx = db.transaction((): JobRow | null => {
    const r = db
      .prepare(
        `SELECT id FROM jobs WHERE status = 'queued' AND kind IN (${placeholders})
         ORDER BY created_at, id LIMIT 1`,
      )
      .get(...kinds) as { id: string } | undefined;
    if (!r) return null;
    db.prepare(`UPDATE jobs SET status = 'running', attempts = attempts + 1, updated_at = ? WHERE id = ?`)
      .run(now(), r.id);
    return getJob(r.id, db);
  });
  return tx();
}

export function completeJob(id: string, db: Database.Database = getDb()): void {
  db.prepare(`UPDATE jobs SET status = 'done', error = NULL, updated_at = ? WHERE id = ?`).run(now(), id);
}

/** Requeue if the job still has attempts left, otherwise mark it failed. */
export function failJob(id: string, error: string, maxAttempts: number, db: Database.Database = getDb()): void {
  const job = getJob(id, db);
  if (!job) return;
  const status: JobStatus = job.attempts >= maxAttempts ? 'failed' : 'queued';
  db.prepare(`UPDATE jobs SET status = ?, error = ?, updated_at = ? WHERE id = ?`)
    .run(status, error.slice(0, 2000), now(), id);
}

export function countByStatus(db: Database.Database = getDb()): Record<JobStatus, number> {
  const out: Record<JobStatus, number> = { queued: 0, running: 0, done: 0, failed: 0 };
  const rows = db.prepare(`SELECT status, COUNT(*) AS n FROM jobs GROUP BY status`)
    .all() as { status: JobStatus; n: number }[];
  for (const r of rows) out[r.status] = r.n;
  return out;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- jobs.service`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add server/src/services/jobs.service.ts server/test/jobs.service.test.ts
git commit -m "feat(jobs): add enqueue/claim/complete/fail job queue service"
```

---

### Task 4: `worker.service` — the polling loop

**Files:**
- Create: `server/src/services/worker.service.ts`
- Test: `server/test/worker.service.test.ts`

**Interfaces:**
- Consumes: `claimNext`, `completeJob`, `failJob`, `JobKind`, `JobRow` (Task 3); `getDb`.
- Produces:
  - `const MAX_ATTEMPTS = 3`
  - `type JobHandler = (job: JobRow, db: Database.Database) => Promise<void>`
  - `type HandlerMap = Partial<Record<JobKind, JobHandler>>`
  - `processOnce(handlers: HandlerMap, db?): Promise<boolean>` — claims+runs one job; returns `true` if a job ran.
  - `startWorker(handlers: HandlerMap, intervalMs?): () => void` — starts the poll loop; returns a `stop()` fn.

- [ ] **Step 1: Write the failing test — `server/test/worker.service.test.ts`**

```ts
import { describe, it, expect, vi } from 'vitest';
import Database from 'better-sqlite3';
import { migrate } from '../src/db/database.js';
import { enqueue, getJob } from '../src/services/jobs.service.js';
import { processOnce } from '../src/services/worker.service.js';

function freshDb(): Database.Database {
  const db = new Database(':memory:');
  migrate(db);
  return db;
}

describe('worker.service processOnce', () => {
  it('runs the handler and marks the job done', async () => {
    const db = freshDb();
    const j = enqueue({ kind: 'index_archive', payload: { path: '/a' } }, db);
    const handler = vi.fn().mockResolvedValue(undefined);
    const ran = await processOnce({ index_archive: handler }, db);
    expect(ran).toBe(true);
    expect(handler).toHaveBeenCalledOnce();
    expect(getJob(j.id, db)!.status).toBe('done');
  });

  it('returns false when there is nothing to do', async () => {
    const db = freshDb();
    expect(await processOnce({ index_archive: vi.fn() }, db)).toBe(false);
  });

  it('requeues the job when the handler throws (retryable)', async () => {
    const db = freshDb();
    const j = enqueue({ kind: 'index_archive' }, db);
    const handler = vi.fn().mockRejectedValue(new Error('kaboom'));
    await processOnce({ index_archive: handler }, db);
    const after = getJob(j.id, db)!;
    expect(after.status).toBe('queued');
    expect(after.error).toContain('kaboom');
  });

  it('never claims a kind it has no handler for', async () => {
    const db = freshDb();
    enqueue({ kind: 'thumbnail' }, db);
    expect(await processOnce({ index_archive: vi.fn() }, db)).toBe(false);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- worker.service`
Expected: FAIL — cannot find module `worker.service`.

- [ ] **Step 3: Implement `server/src/services/worker.service.ts`**

```ts
import type Database from 'better-sqlite3';
import { getDb } from '../db/database.js';
import { claimNext, completeJob, failJob, type JobKind, type JobRow } from './jobs.service.js';

export const MAX_ATTEMPTS = 3;

export type JobHandler = (job: JobRow, db: Database.Database) => Promise<void>;
export type HandlerMap = Partial<Record<JobKind, JobHandler>>;

/** Claim and process a single job of a handled kind. Returns true if a job ran. */
export async function processOnce(handlers: HandlerMap, db: Database.Database = getDb()): Promise<boolean> {
  const kinds = Object.keys(handlers) as JobKind[];
  const job = claimNext(kinds, db);
  if (!job) return false;
  const handler = handlers[job.kind]!;
  try {
    await handler(job, db);
    completeJob(job.id, db);
  } catch (e) {
    failJob(job.id, (e as Error).message || String(e), MAX_ATTEMPTS, db);
  }
  return true;
}

/** Start a polling loop that drains the queue each tick. Returns a stop() function. */
export function startWorker(handlers: HandlerMap, intervalMs = 1500): () => void {
  let stopped = false;
  let running = false;
  const tick = async (): Promise<void> => {
    if (stopped || running) return;
    running = true;
    try {
      while (!stopped && (await processOnce(handlers))) {
        /* drain everything available this tick */
      }
    } finally {
      running = false;
    }
  };
  const timer = setInterval(() => { void tick(); }, intervalMs);
  void tick();
  return () => {
    stopped = true;
    clearInterval(timer);
  };
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- worker.service`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add server/src/services/worker.service.ts server/test/worker.service.test.ts
git commit -m "feat(worker): add polling loop with retry-on-throw"
```

---

### Task 5: `libraryPath` — derive creator/category/type/name from a path

**Files:**
- Create: `server/src/services/libraryPath.ts`
- Test: `server/test/libraryPath.test.ts`

**Interfaces:**
- Consumes: `node:path`.
- Produces:
  - `interface LibraryMeta { creator: string; category: string; type: string; name: string }`
  - `deriveLibraryMeta(root: string, fullPath: string): LibraryMeta`

- [ ] **Step 1: Write the failing test — `server/test/libraryPath.test.ts`**

```ts
import { describe, it, expect } from 'vitest';
import { deriveLibraryMeta } from '../src/services/libraryPath.js';

const ROOT = '/lib/Creators';

describe('deriveLibraryMeta', () => {
  it('derives creator, category, type, and name from a 3-level path', () => {
    const m = deriveLibraryMeta(
      ROOT,
      '/lib/Creators/Creature Caster/Miniatures/Creature Caster - Lady of Arcana.zip',
    );
    expect(m).toEqual({
      creator: 'Creature Caster',
      category: 'Miniatures',
      type: 'Miniature',
      name: 'Lady of Arcana',
    });
  });

  it('maps Terrain and Sets categories to their types', () => {
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Terrain/Foo - Wall.7z').type).toBe('Terrain');
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Sets/Foo - Bundle.rar').type).toBe('Set');
  });

  it('falls back to Misc/Miniature when there is no category folder', () => {
    const m = deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Foo - Loose.zip');
    expect(m.category).toBe('Misc');
    expect(m.type).toBe('Miniature');
    expect(m.name).toBe('Loose');
  });

  it('keeps the basename when there is no "<creator> - " prefix', () => {
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Miniatures/Dragon King.zip').name).toBe('Dragon King');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- libraryPath`
Expected: FAIL — cannot find module `libraryPath`.

- [ ] **Step 3: Implement `server/src/services/libraryPath.ts`**

```ts
import path from 'node:path';

export interface LibraryMeta {
  creator: string;
  category: string;
  type: string;
  name: string;
}

const TYPE_BY_CATEGORY: Record<string, string> = {
  miniatures: 'Miniature',
  minis: 'Miniature',
  terrain: 'Terrain',
  sets: 'Set',
  bust: 'Bust',
  busts: 'Bust',
};

/**
 * Derive metadata from a file laid out as <root>/<Creator>/<Category>/<file>.
 * - creator  = first path segment under root
 * - category = second segment when present (Miniatures/Sets/Terrain/…), else 'Misc'
 * - type     = mapped from category, defaulting to 'Miniature'
 * - name     = file basename without extension, with a leading "<creator> - " stripped
 */
export function deriveLibraryMeta(root: string, fullPath: string): LibraryMeta {
  const rel = path.relative(path.resolve(root), path.resolve(fullPath));
  const parts = rel.split(path.sep).filter(Boolean);
  const creator = parts.length >= 1 ? parts[0] : 'Unknown';
  const category = parts.length >= 3 ? parts[1] : 'Misc';
  const type = TYPE_BY_CATEGORY[category.toLowerCase()] ?? 'Miniature';

  const base = path.basename(fullPath, path.extname(fullPath));
  const prefix = `${creator} - `;
  let name = base.toLowerCase().startsWith(prefix.toLowerCase()) ? base.slice(prefix.length) : base;
  name = name.trim() || base;

  return { creator, category, type, name };
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- libraryPath`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add server/src/services/libraryPath.ts server/test/libraryPath.test.ts
git commit -m "feat(scan): derive creator/category/type/name from library path"
```

---

### Task 6: `archive.service` — zip / 7z / rar readers + extension dispatch

**Files:**
- Create: `server/src/services/archive.service.ts`
- Create: `fixtures/archives/cube.zip` (generated)
- Create: `fixtures/archives/cube.7z` (generated)
- Test: `server/test/archive.test.ts`

**Interfaces:**
- Consumes: `adm-zip` (zip), `7zip-bin` + `node:child_process` (7z), `node-unrar-js` (rar), `node:path`.
- Produces:
  - `interface MeshEntry { innerPath: string; ext: string; sizeBytes: number }`
  - `const MESH_EXTS: Set<string>`
  - `type ArchiveReader = (archivePath: string) => Promise<MeshEntry[]>`
  - `listZip(p): Promise<MeshEntry[]>`, `listSevenZip(p): Promise<MeshEntry[]>`, `listRar(p): Promise<MeshEntry[]>`
  - `listMeshEntries(archivePath: string, readers?: Record<string, ArchiveReader>): Promise<MeshEntry[]>`

> **Fixture note (honest constraint):** zip and 7z fixtures are generated deterministically below (the box has `zip` + `7z`). There is **no free RAR writer** on the box (`unrar` only extracts), so we cannot generate a tiny `cube.rar`. The rar reader is therefore covered by the **dispatch test** (deterministic) plus a test that runs the real reader **only** when `LAPIDARY_TEST_RAR` points at a real `.rar`; the Task 10 acceptance exercises the rar path for real on Creature Caster (which has `.rar` archives).

- [ ] **Step 1: Generate the zip + 7z fixtures**

Run from the repo root:

```bash
mkdir -p fixtures/archives
( cd fixtures && zip -j archives/cube.zip cube.stl )
( cd fixtures && 7z a archives/cube.7z cube.stl >/dev/null )
```

Verify: `unzip -l fixtures/archives/cube.zip` and `7z l fixtures/archives/cube.7z` each list a single entry `cube.stl`.

- [ ] **Step 2: Write the failing test — `server/test/archive.test.ts`**

```ts
import { describe, it, expect, vi } from 'vitest';
import path from 'node:path';
import { archivesDir } from './helpers.js';
import {
  listZip, listSevenZip, listMeshEntries, type ArchiveReader,
} from '../src/services/archive.service.js';

const ZIP = path.join(archivesDir, 'cube.zip');
const SZ = path.join(archivesDir, 'cube.7z');
const RAR = process.env.LAPIDARY_TEST_RAR; // e.g. a real Creature Caster .rar

describe('archive readers', () => {
  it('listZip lists the inner cube.stl mesh entry', async () => {
    const entries = await listZip(ZIP);
    expect(entries.map((e) => e.innerPath)).toContain('cube.stl');
    expect(entries.find((e) => e.innerPath === 'cube.stl')!.ext).toBe('.stl');
  });

  it('listSevenZip lists the inner cube.stl mesh entry', async () => {
    const entries = await listSevenZip(SZ);
    expect(entries.map((e) => e.innerPath)).toContain('cube.stl');
    expect(entries.find((e) => e.innerPath === 'cube.stl')!.ext).toBe('.stl');
  });
});

describe('listMeshEntries dispatch', () => {
  it('routes by extension to the matching reader', async () => {
    const rar = vi.fn().mockResolvedValue([{ innerPath: 'x.stl', ext: '.stl', sizeBytes: 1 }]);
    const out = await listMeshEntries('/some/file.rar', { '.rar': rar as ArchiveReader });
    expect(rar).toHaveBeenCalledWith('/some/file.rar');
    expect(out[0].innerPath).toBe('x.stl');
  });

  it('throws on an unsupported archive extension', async () => {
    await expect(listMeshEntries('/x.tar', {})).rejects.toThrow(/Unsupported archive/);
  });

  it('resolves a real .zip through the default dispatch', async () => {
    expect((await listMeshEntries(ZIP)).length).toBeGreaterThan(0);
  });
});

describe.skipIf(!RAR)('archive rar reader (guarded by LAPIDARY_TEST_RAR)', () => {
  it('lists mesh entries from a real .rar', async () => {
    const { listRar } = await import('../src/services/archive.service.js');
    const entries = await listRar(RAR!);
    expect(Array.isArray(entries)).toBe(true);
    expect(entries.every((e) => ['.stl', '.3mf', '.obj'].includes(e.ext))).toBe(true);
  });
});
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm --workspace server test -- archive`
Expected: FAIL — cannot find module `archive.service`.

- [ ] **Step 4: Implement `server/src/services/archive.service.ts`**

```ts
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import AdmZip from 'adm-zip';
import sevenBin from '7zip-bin';
import { createExtractorFromFile } from 'node-unrar-js';

const execFileP = promisify(execFile);

export interface MeshEntry {
  innerPath: string;
  ext: string;
  sizeBytes: number;
}

export const MESH_EXTS = new Set(['.stl', '.3mf', '.obj']);

export type ArchiveReader = (archivePath: string) => Promise<MeshEntry[]>;

/** List supported mesh entries inside a .zip (pure JS, no external binary). */
export async function listZip(archivePath: string): Promise<MeshEntry[]> {
  const zip = new AdmZip(archivePath);
  return zip
    .getEntries()
    .filter((e) => !e.isDirectory)
    .map((e) => ({
      innerPath: e.entryName,
      ext: path.extname(e.entryName).toLowerCase(),
      sizeBytes: e.header.size,
    }))
    .filter((e) => MESH_EXTS.has(e.ext));
}

/**
 * List supported mesh entries inside a .7z by shelling out to the bundled 7za with a
 * technical listing (`-slt`): blocks of "Path = …" / "Size = …" / "Attributes = …"
 * separated by blank lines. Avoids any node-7z stream/type quirks.
 */
export async function listSevenZip(archivePath: string): Promise<MeshEntry[]> {
  const { stdout } = await execFileP(sevenBin.path7za, ['l', '-slt', archivePath], {
    maxBuffer: 64 * 1024 * 1024,
  });
  const entries: MeshEntry[] = [];
  for (const block of stdout.split(/\r?\n\r?\n/)) {
    const pathMatch = block.match(/^Path = (.+)$/m);
    if (!pathMatch) continue;
    const innerPath = pathMatch[1].trim();
    const attrs = (block.match(/^Attributes = (.+)$/m)?.[1] ?? '').trim();
    if (attrs.startsWith('D')) continue; // directory entry
    const ext = path.extname(innerPath).toLowerCase();
    if (!MESH_EXTS.has(ext)) continue;
    const sizeBytes = Number(block.match(/^Size = (\d+)$/m)?.[1] ?? 0);
    entries.push({ innerPath, ext, sizeBytes });
  }
  return entries;
}

/** List supported mesh entries inside a .rar (pure-WASM, no system binary). */
export async function listRar(archivePath: string): Promise<MeshEntry[]> {
  const extractor = await createExtractorFromFile({ filepath: archivePath });
  const list = extractor.getFileList();
  const entries: MeshEntry[] = [];
  for (const h of list.fileHeaders) {
    if (h.flags.directory) continue;
    const ext = path.extname(h.name).toLowerCase();
    if (!MESH_EXTS.has(ext)) continue;
    const sizeBytes = Number((h as { unpSize?: number }).unpSize ?? 0) || 0; // unpacked size
    entries.push({ innerPath: h.name, ext, sizeBytes });
  }
  return entries;
}

const DEFAULT_READERS: Record<string, ArchiveReader> = {
  '.zip': listZip,
  '.7z': listSevenZip,
  '.rar': listRar,
};

/** List mesh entries inside an archive, dispatching on file extension. */
export async function listMeshEntries(
  archivePath: string,
  readers: Record<string, ArchiveReader> = DEFAULT_READERS,
): Promise<MeshEntry[]> {
  const ext = path.extname(archivePath).toLowerCase();
  const reader = readers[ext];
  if (!reader) throw new Error(`Unsupported archive type: ${ext}`);
  return reader(archivePath);
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npm --workspace server test -- archive`
Expected: PASS (zip 1, 7z 1, dispatch 3; rar suite skipped unless `LAPIDARY_TEST_RAR` is set).

- [ ] **Step 6: (Optional but recommended) verify the rar reader against a real archive**

```bash
LAPIDARY_TEST_RAR="/mnt/Storage2/All/STL Files/Creators/Creature Caster/Miniatures/Creature Caster - Guild Assassins.rar" \
  npm --workspace server test -- archive
```
Expected: the guarded rar test now runs and passes (lists `.stl` entries).

- [ ] **Step 7: Commit**

```bash
git add server/src/services/archive.service.ts server/test/archive.test.ts fixtures/archives/cube.zip fixtures/archives/cube.7z
git commit -m "feat(archive): zip/7z/rar mesh-entry readers + extension dispatch"
```

---

### Task 7: `indexArchive` — the `index_archive` job handler

**Files:**
- Create: `server/src/services/indexArchive.service.ts`
- Test: `server/test/indexArchive.test.ts`

**Interfaces:**
- Consumes: `createModel` (`NewModelInput → ModelDetailDTO`, from `model.service.ts`), `enqueue` + `JobRow` (Task 3), `listMeshEntries`/`MESH_EXTS` (Task 6), `deriveLibraryMeta` (Task 5), `nanoid`, `node:fs`, `node:path`.
- Produces: `indexArchiveJob(job: JobRow, db: Database.Database): Promise<void>` — a `JobHandler`.
- Payload shape carried by an `index_archive` job: `{ path: string; root: string }`.

- [ ] **Step 1: Write the failing test — `server/test/indexArchive.test.ts`**

```ts
import { beforeEach, describe, it, expect } from 'vitest';
import path from 'node:path';
import { archivesDir, fixturesDir } from './helpers.js';
import { getDb } from '../src/db/database.js';
import { indexArchiveJob } from '../src/services/indexArchive.service.js';
import { enqueue, countByStatus } from '../src/services/jobs.service.js';

const ZIP = path.join(archivesDir, 'cube.zip');
const ROOT = fixturesDir;

function clean(): void {
  const d = getDb();
  for (const t of ['model_tags', 'model_groups', 'model_printer_types', 'printer_settings', 'images', 'jobs', 'models', 'tags', 'groups']) {
    d.prepare(`DELETE FROM ${t}`).run();
  }
}

describe('indexArchiveJob', () => {
  beforeEach(() => clean());

  it('creates a model pointing at the archive in place and enqueues follow-up jobs', async () => {
    const d = getDb();
    const payload = { path: ZIP, root: ROOT };
    const job = enqueue({ kind: 'index_archive', payload }, d);
    await indexArchiveJob({ ...job, payload }, d);

    const m = d.prepare('SELECT * FROM models').get() as { original_path: string; format: string } | undefined;
    expect(m).toBeTruthy();
    expect(m!.original_path).toBe(ZIP);
    expect(m!.format).toBe('STL');

    // thumbnail + image_fetch were enqueued (the index job itself is still queued here)
    expect(countByStatus(d).queued).toBeGreaterThanOrEqual(2);
  });

  it('is idempotent — a second run does not duplicate the model', async () => {
    const d = getDb();
    const payload = { path: ZIP, root: ROOT };
    const job = enqueue({ kind: 'index_archive', payload }, d);
    await indexArchiveJob({ ...job, payload }, d);
    await indexArchiveJob({ ...job, payload }, d);
    const n = (d.prepare('SELECT COUNT(*) AS n FROM models').get() as { n: number }).n;
    expect(n).toBe(1);
  });

  it('throws when the archive path does not exist / has no mesh', async () => {
    const d = getDb();
    const payload = { path: '/does/not/exist.zip', root: ROOT };
    const job = enqueue({ kind: 'index_archive', payload }, d);
    await expect(indexArchiveJob({ ...job, payload }, d)).rejects.toThrow();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- indexArchive`
Expected: FAIL — cannot find module `indexArchive.service`.

- [ ] **Step 3: Implement `server/src/services/indexArchive.service.ts`**

```ts
import fs from 'node:fs';
import path from 'node:path';
import { nanoid } from 'nanoid';
import type Database from 'better-sqlite3';
import { createModel } from './model.service.js';
import { enqueue, type JobRow } from './jobs.service.js';
import { listMeshEntries, MESH_EXTS } from './archive.service.js';
import { deriveLibraryMeta } from './libraryPath.js';

const ARCHIVE_EXTS = new Set(['.zip', '.rar', '.7z']);

interface IndexPayload {
  path: string;
  root: string;
}

/**
 * Worker handler for `index_archive`: peek one library item, create its model row
 * (pointing at the source in place), and enqueue its thumbnail + image-fetch jobs.
 */
export async function indexArchiveJob(job: JobRow, db: Database.Database): Promise<void> {
  const payload = job.payload as unknown as IndexPayload | null;
  if (!payload?.path || !payload?.root) {
    throw new Error('index_archive: missing payload { path, root }');
  }
  const itemPath = payload.path;

  // Idempotency: skip if a model already references this exact source path.
  const existing = db.prepare('SELECT id FROM models WHERE original_path = ?').get(itemPath) as
    | { id: string }
    | undefined;
  if (existing) return;

  const ext = path.extname(itemPath).toLowerCase();
  let format: string;
  if (ARCHIVE_EXTS.has(ext)) {
    const entries = await listMeshEntries(itemPath);
    if (!entries.length) throw new Error(`No mesh files inside archive: ${itemPath}`);
    format = entries[0].ext.replace('.', '').toUpperCase();
  } else if (MESH_EXTS.has(ext)) {
    format = ext.replace('.', '').toUpperCase();
  } else {
    throw new Error(`Unsupported library file: ${itemPath}`);
  }

  const meta = deriveLibraryMeta(payload.root, itemPath);
  let fileSizeBytes = 0;
  try {
    fileSizeBytes = fs.statSync(itemPath).size;
  } catch {
    /* leave 0 if the file vanished */
  }

  const id = `lib${nanoid(10)}`;
  createModel({
    id,
    name: meta.name,
    creator: meta.creator,
    type: meta.type,
    format,
    fileSizeBytes,
    size: [0, 0, 0], // bbox filled in by the Phase 2 thumbnail job
    originalPath: itemPath, // index-in-place: the archive itself, never a copy
    groups: meta.category && meta.category !== 'Misc' ? [meta.category] : [],
    tags: [],
  });

  enqueue({ kind: 'thumbnail', modelId: id, payload: { path: itemPath } }, db);
  enqueue({ kind: 'image_fetch', modelId: id, payload: { name: meta.name, creator: meta.creator } }, db);
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- indexArchive`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add server/src/services/indexArchive.service.ts server/test/indexArchive.test.ts
git commit -m "feat(ingest): index_archive job handler (peek + create model in place)"
```

---

### Task 8: Rewrite `libraryScan` to enqueue jobs + update the scan route

**Files:**
- Modify: `server/src/services/libraryScan.service.ts` (full rewrite)
- Modify: `server/src/routes/api.ts` (scan route comment + response shape)
- Test: `server/test/libraryScan.test.ts`

**Interfaces:**
- Consumes: `enqueue` (Task 3), `getDb`, `node:fs`, `node:path`.
- Produces:
  - `interface ScanResult { scanned: number; enqueued: number; skipped: number }`
  - `scanFolder(folderPath: string, db?): ScanResult`

- [ ] **Step 1: Write the failing test — `server/test/libraryScan.test.ts`**

```ts
import { beforeEach, describe, it, expect } from 'vitest';
import path from 'node:path';
import { archivesDir } from './helpers.js';
import { getDb } from '../src/db/database.js';
import { scanFolder } from '../src/services/libraryScan.service.js';

const ARCH_DIR = archivesDir;

function clean(): void {
  const d = getDb();
  d.prepare('DELETE FROM jobs').run();
  d.prepare('DELETE FROM models').run();
}

describe('scanFolder', () => {
  beforeEach(() => clean());

  it('enqueues one index_archive job per archive found', () => {
    const res = scanFolder(ARCH_DIR);
    expect(res.enqueued).toBeGreaterThanOrEqual(2); // cube.zip + cube.7z
    const d = getDb();
    const n = (d.prepare("SELECT COUNT(*) AS n FROM jobs WHERE kind = 'index_archive'").get() as { n: number }).n;
    expect(n).toBe(res.enqueued);
  });

  it('skips items already indexed (idempotent)', () => {
    const d = getDb();
    d.prepare(
      "INSERT INTO models (id, name, creator, type, format, added_date, original_path) VALUES ('m1','cube','x','Miniature','STL',date('now'),?)",
    ).run(path.join(ARCH_DIR, 'cube.zip'));
    const res = scanFolder(ARCH_DIR);
    expect(res.skipped).toBeGreaterThanOrEqual(1);
  });

  it('throws on a non-directory target', () => {
    expect(() => scanFolder(path.join(ARCH_DIR, 'cube.zip'))).toThrow(/Not a directory/);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- libraryScan`
Expected: FAIL — the current `scanFolder` imports `ingestMesh`/`createModel` and returns `imported`, so `enqueued` is undefined.

- [ ] **Step 3: Rewrite `server/src/services/libraryScan.service.ts`**

Replace the whole file with:

```ts
import fs from 'node:fs';
import path from 'node:path';
import type Database from 'better-sqlite3';
import { getDb } from '../db/database.js';
import { enqueue } from './jobs.service.js';

const ARCHIVE_EXTS = new Set(['.zip', '.rar', '.7z']);
const MESH_EXTS = new Set(['.stl', '.3mf', '.obj']);

/** Recursively collect archive + loose-mesh files under a directory. */
function collect(dir: string, out: string[] = [], depth = 0): string[] {
  if (depth > 8) return out;
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      collect(full, out, depth + 1);
    } else {
      const ext = path.extname(e.name).toLowerCase();
      if (ARCHIVE_EXTS.has(ext) || MESH_EXTS.has(ext)) out.push(full);
    }
  }
  return out;
}

export interface ScanResult {
  scanned: number;
  enqueued: number;
  skipped: number;
}

/**
 * Walk a library folder and enqueue one `index_archive` job per archive/mesh found.
 * Index-in-place: nothing is extracted or copied here — each job carries the absolute
 * source path and the scan root. Items already indexed (by `original_path`) are skipped;
 * a duplicate enqueue is harmless because the handler is idempotent.
 */
export function scanFolder(folderPath: string, db: Database.Database = getDb()): ScanResult {
  const root = path.resolve(folderPath);
  if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
    throw new Error(`Not a directory: ${root}`);
  }
  const files = collect(root);
  const indexed = new Set(
    (db.prepare('SELECT original_path FROM models WHERE original_path IS NOT NULL').all() as {
      original_path: string;
    }[]).map((r) => r.original_path),
  );

  const seen = new Set<string>();
  let enqueued = 0;
  let skipped = 0;
  for (const file of files) {
    if (indexed.has(file) || seen.has(file)) {
      skipped += 1;
      continue;
    }
    seen.add(file);
    enqueue({ kind: 'index_archive', payload: { path: file, root } }, db);
    enqueued += 1;
  }
  return { scanned: files.length, enqueued, skipped };
}
```

- [ ] **Step 4: Update the scan route block in `server/src/routes/api.ts`**

Replace the `// ---------- scan ----------` block (currently `server/src/routes/api.ts:172-182`) with:

```ts
  // ---------- scan ----------
  // Enqueues an index_archive job per library item; the worker indexes them in the
  // background. Returns { scanned, enqueued, skipped } — poll /api/models to watch rows appear.
  app.post('/api/scan', async (req, reply) => {
    const { folderPath } = req.body as { folderPath?: string };
    const target = folderPath || config.libraryPath;
    if (!target) return reply.code(400).send({ error: 'no folderPath and no LIBRARY_PATH configured' });
    try {
      return scanFolder(target);
    } catch (e) {
      return reply.code(400).send({ error: (e as Error).message });
    }
  });
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npm --workspace server test -- libraryScan`
Expected: PASS (3 tests).

- [ ] **Step 6: Type-check the server (the scan route lost its `imported` field)**

Run: `npm --workspace server run build`
Expected: `tsc` completes with no errors.

- [ ] **Step 7: Commit**

```bash
git add server/src/services/libraryScan.service.ts server/src/routes/api.ts
git commit -m "feat(scan): walk archives + loose meshes and enqueue index jobs"
```

---

### Task 9: Worker process entry + run scripts

**Files:**
- Create: `server/src/worker.ts`
- Test: `server/test/worker.entry.test.ts`

**Interfaces:**
- Consumes: `startWorker`/`HandlerMap` (Task 4), `indexArchiveJob` (Task 7), `getDb`.
- Produces: a runnable `server/src/worker.ts` process entry that registers `index_archive → indexArchiveJob`; an exported `WORKER_HANDLERS` map for the test.

- [ ] **Step 1: Write the failing test — `server/test/worker.entry.test.ts`**

```ts
import { describe, it, expect } from 'vitest';
import { WORKER_HANDLERS } from '../src/worker.js';
import { indexArchiveJob } from '../src/services/indexArchive.service.js';

describe('worker entry handler map', () => {
  it('registers indexArchiveJob for index_archive', () => {
    expect(WORKER_HANDLERS.index_archive).toBe(indexArchiveJob);
  });

  it('does not yet handle thumbnail or image_fetch', () => {
    expect(WORKER_HANDLERS.thumbnail).toBeUndefined();
    expect(WORKER_HANDLERS.image_fetch).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm --workspace server test -- worker.entry`
Expected: FAIL — cannot find module `../src/worker.js`.

- [ ] **Step 3: Implement `server/src/worker.ts`**

```ts
import { getDb } from './db/database.js';
import { startWorker, type HandlerMap } from './services/worker.service.js';
import { indexArchiveJob } from './services/indexArchive.service.js';

/** Job kinds this worker can process. Phase 2/3 add thumbnail + image_fetch. */
export const WORKER_HANDLERS: HandlerMap = {
  index_archive: indexArchiveJob,
};

/** Only start the loop when run as a process, not when imported by a test. */
const isMain = import.meta.url === `file://${process.argv[1]}`;
if (isMain) {
  getDb(); // ensure migrations have run before polling
  const stop = startWorker(WORKER_HANDLERS, 1500);
  // eslint-disable-next-line no-console
  console.log('[worker] started; handling: index_archive');
  for (const sig of ['SIGINT', 'SIGTERM'] as const) {
    process.on(sig, () => {
      stop();
      process.exit(0);
    });
  }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm --workspace server test -- worker.entry`
Expected: PASS (2 tests).

- [ ] **Step 5: Smoke-test the worker process boots**

Run: `npm --workspace server run build && timeout 3 node server/dist/worker.js`
Expected: prints `[worker] started; handling: index_archive` and exits after the timeout with no error.

- [ ] **Step 6: Commit**

```bash
git add server/src/worker.ts server/test/worker.entry.test.ts
git commit -m "feat(worker): process entry wiring index_archive handler"
```

---

### Task 10: End-to-end ingest + real-library acceptance + runbook

**Files:**
- Test: `server/test/ingest.e2e.test.ts`
- Modify: `README.md` (add a "Scan a library" runbook section)

**Interfaces:**
- Consumes: `scanFolder` (Task 8), `processOnce` (Task 4), `WORKER_HANDLERS` (Task 9), `getDb`.
- Produces: a green end-to-end test on the fixtures; a documented runbook; a verified gate on Creature Caster.

- [ ] **Step 1: Write the end-to-end test — `server/test/ingest.e2e.test.ts`**

```ts
import { beforeEach, describe, it, expect } from 'vitest';
import { archivesDir } from './helpers.js';
import { getDb } from '../src/db/database.js';
import { scanFolder } from '../src/services/libraryScan.service.js';
import { processOnce } from '../src/services/worker.service.js';
import { WORKER_HANDLERS } from '../src/worker.js';

function clean(): void {
  const d = getDb();
  d.prepare('DELETE FROM jobs').run();
  d.prepare('DELETE FROM models').run();
}

describe('scan -> worker end to end', () => {
  beforeEach(() => clean());

  it('indexes the fixture archives into model rows', async () => {
    scanFolder(archivesDir);
    // drain only the index_archive jobs the worker can handle
    while (await processOnce(WORKER_HANDLERS)) {
      /* keep draining */
    }
    const d = getDb();
    const models = d.prepare('SELECT original_path, format FROM models').all() as {
      original_path: string;
      format: string;
    }[];
    expect(models.length).toBeGreaterThanOrEqual(2);
    expect(models.every((m) => m.format === 'STL')).toBe(true);
    expect(models.every((m) => m.original_path.startsWith(archivesDir))).toBe(true);
  });
});
```

- [ ] **Step 2: Run the full server test suite**

Run: `npm --workspace server test`
Expected: all suites PASS (sanity, migrate, jobs.service, worker.service, libraryPath, archive, indexArchive, libraryScan, worker.entry, ingest.e2e); the guarded rar test is skipped.

- [ ] **Step 3: Add the runbook to `README.md`**

Add this section near the existing usage docs:

```markdown
## Scan a library (background ingest)

Lapidary indexes archived models (`.zip`/`.rar`/`.7z`) and loose meshes
(`.stl`/`.3mf`/`.obj`) **in place** — nothing is copied out of your library.

1. Start the app and the background worker:
   ```bash
   npm run dev          # runs server + worker + web
   ```
2. Point a scan at a folder (or set `LIBRARY_PATH` and omit `folderPath`):
   ```bash
   curl -X POST localhost:5174/api/scan \
     -H 'content-type: application/json' \
     -d '{"folderPath": "/path/to/Creators/Creature Caster"}'
   # -> { "scanned": N, "enqueued": N, "skipped": 0 }
   ```
3. Watch rows appear (the worker peeks each archive and creates a model that
   points at the source file):
   ```bash
   curl -s localhost:5174/api/models | jq 'length'
   ```

Models are grouped by creator and category derived from the folder layout
(`Creators/<Creator>/<Miniatures|Sets|Terrain>/<item>`). Thumbnails and images
are added by later phases.
```

- [ ] **Step 4: GATE — verify on the real Creature Caster folder**

Start the stack (`npm run dev`) and run:

```bash
curl -X POST localhost:5174/api/scan -H 'content-type: application/json' \
  -d '{"folderPath": "/mnt/Storage2/All/STL Files/Creators/Creature Caster"}'
```

Then poll until the worker drains (`enqueued` jobs become models):

```bash
curl -s localhost:5174/api/models | jq '[.[] | {name, creator, type, format}]'
```

Expected (the gate):
- 7 models appear (the 7 Creature Caster archives), all with `creator: "Creature Caster"`.
- Types reflect the subfolders (Miniatures → `Miniature`, Sets → `Set`, Terrain → `Terrain`).
- Names have the `"Creature Caster - "` prefix stripped (e.g. `Lady of Arcana`).
- Re-running the scan reports `skipped: 7, enqueued: 0` (idempotent), and nothing was written under `/mnt/Storage2` (index-in-place).

- [ ] **Step 5: Commit**

```bash
git add server/test/ingest.e2e.test.ts README.md
git commit -m "test(ingest): end-to-end scan->worker + library runbook"
```

---

## Self-Review (completed during planning)

- **Spec coverage (Phases 0–1):** worker process + jobs table (Tasks 1–4, 9) ✓; index-in-place archive walking incl. `.zip/.rar/.7z` (Tasks 6, 8) ✓; creator/category/name derivation (Task 5) ✓; model rows created pointing at source (Task 7) ✓; `thumbnail`/`image_fetch` jobs enqueued but unhandled, awaiting Phases 2–3 (Task 7) ✓; gate on Creature Caster (Task 10) ✓. Phases 2–5 are intentionally out of scope and get their own plans.
- **Deviation from spec §5 (intentional):** migration `user_version = 2` here adds **only** the `jobs` table. The spec's other v2 additions (`secrets`, `models.source_url/license/creator_url`, `images.source_url/attribution/confidence`) are not needed until Phase 3 and will land in a later migration (`user_version = 3`) in the Phase 3 plan. This keeps Phase 0–1 minimal and avoids unused columns.
- **Toolchain hardening (from review):** fixture paths use `fileURLToPath(import.meta.url)` via `server/test/helpers.ts` (not `import.meta.dirname`), and the path mechanism is asserted at Task 1; 7z listing shells out to the bundled `7za` (`7zip-bin`) instead of `node-7z`, removing default-import/`@types` ambiguity; all three archive readers live in one task (Task 6) to avoid mid-file binding-reassignment edits by fresh-context subagents; `createModel`'s cache invalidation is confirmed safe without `initCache()` (synchronous LRU branch).
- **Placeholder scan:** no `TODO`/`TBD`/"implement later" remain; every code step shows complete code.
- **Type consistency:** `JobRow`, `JobKind`, `JobStatus`, `JobHandler`, `HandlerMap`, `MeshEntry`, `ArchiveReader`, `LibraryMeta`, `ScanResult`, and the `index_archive` payload `{ path, root }` are used identically across Tasks 3–10. `createModel(NewModelInput)` matches the real signature in `server/src/services/model.service.ts`.
- **Known constraint surfaced, not hidden:** no RAR writer on the box → no committed `.rar` fixture; the rar reader is covered by the dispatch test (deterministic) + a guarded real-file test + the Task 10 live gate (Creature Caster has `.rar` files).
```
