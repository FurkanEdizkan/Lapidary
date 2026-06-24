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
