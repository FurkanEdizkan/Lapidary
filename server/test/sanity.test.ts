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
