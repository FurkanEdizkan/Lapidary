import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import type Database from 'better-sqlite3';
import { config } from '../config.js';
import { listMeshEntries, extractEntry } from './archive.service.js';
import { renderAndAnalyze } from './meshSidecar.service.js';
import { getModelPaths, updateModel } from './model.service.js';
import type { JobRow } from './jobs.service.js';

const ARCHIVE_EXTS = new Set(['.zip', '.rar', '.7z']);
const MESH_EXTS = new Set(['.stl', '.3mf', '.obj']);

/**
 * Worker handler for `thumbnail`: extract the largest mesh entry from an archive
 * (or use a loose mesh directly), render a LOD + PNG thumbnail via the Rust sidecar,
 * and persist the results back onto the model row.
 */
export async function thumbnailJob(job: JobRow, _db: Database.Database): Promise<void> {
  const modelId = job.modelId;
  if (!modelId) throw new Error('thumbnail: missing modelId');

  // Idempotency: skip if thumbnail already set.
  const paths = getModelPaths(modelId);
  if (!paths) throw new Error(`thumbnail: model not found: ${modelId}`);
  if (paths.thumbnail) return;

  const srcPath = (job.payload as { path: string } | null)?.path;
  if (!srcPath) throw new Error('thumbnail: missing payload.path');

  const ext = path.extname(srcPath).toLowerCase();

  let meshPath: string;
  let entryPath: string | null;
  let tmp: string | null = null;

  if (ARCHIVE_EXTS.has(ext)) {
    const entries = await listMeshEntries(srcPath);
    if (!entries.length) throw new Error(`No mesh entries in archive: ${srcPath}`);
    const largest = entries.reduce((a, b) => (b.sizeBytes > a.sizeBytes ? b : a));
    tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'lap-thumb-'));
    meshPath = await extractEntry(srcPath, largest.innerPath, tmp);
    entryPath = largest.innerPath;
  } else if (MESH_EXTS.has(ext)) {
    meshPath = srcPath;
    entryPath = null;
  } else {
    throw new Error(`thumbnail: unsupported file type: ${ext}`);
  }

  try {
    const lodOut = path.join(config.lodDir, `${modelId}.stl`);
    const thumbOut = path.join(config.thumbnailsDir, `${modelId}.png`);

    const res = await renderAndAnalyze(meshPath, lodOut, thumbOut, 512);
    if (!res) throw new Error('mesh sidecar unavailable');

    // Build the patch only with defined values — better-sqlite3 rejects undefined bindings.
    const patch: Record<string, unknown> = {
      size: res.bbox,
      triangleCount: res.triangles,
    };
    if (res.lodWritten) patch.lodPath = lodOut;
    if (res.thumbWritten) patch.thumbnailPath = thumbOut;
    if (entryPath != null) patch.entryPath = entryPath;

    updateModel(modelId, patch);
  } finally {
    if (tmp) fs.rmSync(tmp, { recursive: true, force: true });
  }
}
