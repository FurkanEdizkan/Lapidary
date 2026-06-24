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
