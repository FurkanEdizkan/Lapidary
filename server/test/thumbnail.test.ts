/**
 * Integration tests for thumbnailJob.
 * Requires the rust-mesh binary at rust-mesh/target/release/rust-mesh.
 * Skipped automatically when the binary is not built (CI without Rust).
 */
import { beforeEach, describe, it, expect } from 'vitest';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { archivesDir, fixturesDir } from './helpers.js';
import { getDb } from '../src/db/database.js';
import { createModel } from '../src/services/model.service.js';
import { enqueue } from '../src/services/jobs.service.js';
import { thumbnailJob } from '../src/services/thumbnail.service.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
const binPath = path.join(repoRoot, 'rust-mesh/target/release/rust-mesh');

const ZIP = path.join(archivesDir, 'cube.zip');
const CUBE_STL = path.join(fixturesDir, 'cube.stl');

function clean(): void {
  const d = getDb();
  for (const t of [
    'model_tags', 'model_groups', 'model_printer_types', 'printer_settings', 'images', 'jobs', 'models', 'tags', 'groups',
  ]) {
    d.prepare(`DELETE FROM ${t}`).run();
  }
}

describe.skipIf(!fs.existsSync(binPath))('thumbnailJob (integration)', () => {
  beforeEach(() => clean());

  it('extracts largest entry, renders LOD+thumbnail, updates model row (archive path)', async () => {
    const d = getDb();
    const id = 'test-model-zip';

    createModel({ id, name: 'Cube', creator: 'test', type: 'model', format: 'STL', originalPath: ZIP });
    const job = enqueue({ kind: 'thumbnail', modelId: id, payload: { path: ZIP } }, d);

    await thumbnailJob({ ...job, modelId: id, payload: { path: ZIP } }, d);

    const row = d.prepare('SELECT thumbnail_path, lod_path, triangle_count, bbox_x, bbox_y, bbox_z, entry_path FROM models WHERE id = ?').get(id) as {
      thumbnail_path: string | null;
      lod_path: string | null;
      triangle_count: number;
      bbox_x: number;
      bbox_y: number;
      bbox_z: number;
      entry_path: string | null;
    } | undefined;

    expect(row).toBeTruthy();
    expect(row!.thumbnail_path).toBeTruthy();
    expect(row!.lod_path).toBeTruthy();
    expect(fs.existsSync(row!.thumbnail_path!)).toBe(true);
    expect(fs.existsSync(row!.lod_path!)).toBe(true);
    expect(row!.triangle_count).toBeGreaterThan(0);
    expect(row!.bbox_x).toBeCloseTo(20, 0);
    expect(row!.bbox_y).toBeCloseTo(20, 0);
    expect(row!.bbox_z).toBeCloseTo(20, 0);
    expect(row!.entry_path).toBe('cube.stl');
  });

  it('is idempotent — second call with thumbnail already set is a no-op', async () => {
    const d = getDb();
    const id = 'test-model-idem';

    createModel({
      id,
      name: 'Cube Idem',
      creator: 'test',
      type: 'model',
      format: 'STL',
      originalPath: ZIP,
      thumbnailPath: '/some/existing/thumb.png',
    });
    const job = enqueue({ kind: 'thumbnail', modelId: id, payload: { path: ZIP } }, d);

    // Should return without error and without changing the row.
    await expect(thumbnailJob({ ...job, modelId: id, payload: { path: ZIP } }, d)).resolves.toBeUndefined();

    const row = d.prepare('SELECT thumbnail_path FROM models WHERE id = ?').get(id) as { thumbnail_path: string } | undefined;
    expect(row!.thumbnail_path).toBe('/some/existing/thumb.png');
  });

  it('handles loose mesh (no archive): entry_path stays null, thumbnail+lod written', async () => {
    const d = getDb();
    const id = 'test-model-stl';

    createModel({ id, name: 'Cube STL', creator: 'test', type: 'model', format: 'STL', originalPath: CUBE_STL });
    const job = enqueue({ kind: 'thumbnail', modelId: id, payload: { path: CUBE_STL } }, d);

    await thumbnailJob({ ...job, modelId: id, payload: { path: CUBE_STL } }, d);

    const row = d.prepare('SELECT thumbnail_path, lod_path, triangle_count, entry_path FROM models WHERE id = ?').get(id) as {
      thumbnail_path: string | null;
      lod_path: string | null;
      triangle_count: number;
      entry_path: string | null;
    } | undefined;

    expect(row!.thumbnail_path).toBeTruthy();
    expect(row!.lod_path).toBeTruthy();
    expect(fs.existsSync(row!.thumbnail_path!)).toBe(true);
    expect(fs.existsSync(row!.lod_path!)).toBe(true);
    expect(row!.triangle_count).toBeGreaterThan(0);
    expect(row!.entry_path).toBeNull();
  });
});
