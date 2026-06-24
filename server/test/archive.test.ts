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
