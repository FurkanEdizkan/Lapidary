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
