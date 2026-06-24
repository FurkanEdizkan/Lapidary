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
