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
