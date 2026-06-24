import { describe, it, expect, afterEach } from 'vitest';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { archivesDir, fixturesDir } from './helpers.js';
import { extractEntry } from '../src/services/archive.service.js';

const ZIP = path.join(archivesDir, 'cube.zip');
const SZ = path.join(archivesDir, 'cube.7z');
const RAR = process.env.LAPIDARY_TEST_RAR;
const CUBE_STL = path.join(fixturesDir, 'cube.stl');

const tempDirs: string[] = [];

function makeTempDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lap-extract-'));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe('extractEntry — zip', () => {
  it('extracts cube.stl from cube.zip flat into dest dir', async () => {
    const dest = makeTempDir();
    const out = await extractEntry(ZIP, 'cube.stl', dest);
    expect(fs.existsSync(out)).toBe(true);
    expect(out).toBe(path.join(dest, 'cube.stl'));
    const extracted = fs.readFileSync(out);
    const original = fs.readFileSync(CUBE_STL);
    expect(extracted).toEqual(original);
  });

  it('throws when entry is not found in zip', async () => {
    const dest = makeTempDir();
    await expect(extractEntry(ZIP, 'nonexistent.stl', dest)).rejects.toThrow();
  });
});

describe('extractEntry — 7z', () => {
  it('extracts cube.stl from cube.7z flat into dest dir', async () => {
    const dest = makeTempDir();
    const out = await extractEntry(SZ, 'cube.stl', dest);
    expect(fs.existsSync(out)).toBe(true);
    expect(out).toBe(path.join(dest, 'cube.stl'));
    const extracted = fs.readFileSync(out);
    const original = fs.readFileSync(CUBE_STL);
    expect(extracted).toEqual(original);
  });
});

describe.skipIf(!RAR)('extractEntry — rar (guarded by LAPIDARY_TEST_RAR)', () => {
  it('extracts an entry from a real rar and writes a non-empty file', async () => {
    const { listRar } = await import('../src/services/archive.service.js');
    const entries = await listRar(RAR!);
    expect(entries.length).toBeGreaterThan(0);
    const entry = entries[0];
    const dest = makeTempDir();
    const out = await extractEntry(RAR!, entry.innerPath, dest);
    expect(fs.existsSync(out)).toBe(true);
    expect(fs.readFileSync(out).length).toBeGreaterThan(0);
  });
});

describe('extractEntry — unsupported format', () => {
  it('throws on unsupported archive extension', async () => {
    const dest = makeTempDir();
    await expect(extractEntry('/some/file.tar', 'x.stl', dest)).rejects.toThrow(
      /Unsupported archive type/,
    );
  });
});
