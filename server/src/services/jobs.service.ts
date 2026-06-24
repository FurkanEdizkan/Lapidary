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
