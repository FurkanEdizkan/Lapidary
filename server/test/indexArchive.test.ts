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
