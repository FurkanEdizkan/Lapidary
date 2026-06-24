import fs from 'node:fs';
import path from 'node:path';
import { nanoid } from 'nanoid';
import type Database from 'better-sqlite3';
import { createModel } from './model.service.js';
import { enqueue, type JobRow } from './jobs.service.js';
import { listMeshEntries, MESH_EXTS } from './archive.service.js';
import { deriveLibraryMeta } from './libraryPath.js';

const ARCHIVE_EXTS = new Set(['.zip', '.rar', '.7z']);

interface IndexPayload {
  path: string;
  root: string;
}

/**
 * Worker handler for `index_archive`: peek one library item, create its model row
 * (pointing at the source in place), and enqueue its thumbnail + image-fetch jobs.
 */
export async function indexArchiveJob(job: JobRow, db: Database.Database): Promise<void> {
  const payload = job.payload as unknown as IndexPayload | null;
  if (!payload?.path || !payload?.root) {
    throw new Error('index_archive: missing payload { path, root }');
  }
  const itemPath = payload.path;

  // Idempotency: skip if a model already references this exact source path.
  const existing = db.prepare('SELECT id FROM models WHERE original_path = ?').get(itemPath) as
    | { id: string }
    | undefined;
  if (existing) return;

  const ext = path.extname(itemPath).toLowerCase();
  let format: string;
  if (ARCHIVE_EXTS.has(ext)) {
    const entries = await listMeshEntries(itemPath);
    if (!entries.length) throw new Error(`No mesh files inside archive: ${itemPath}`);
    format = entries[0].ext.replace('.', '').toUpperCase();
  } else if (MESH_EXTS.has(ext)) {
    format = ext.replace('.', '').toUpperCase();
  } else {
    throw new Error(`Unsupported library file: ${itemPath}`);
  }

  const meta = deriveLibraryMeta(payload.root, itemPath);
  let fileSizeBytes = 0;
  try {
    fileSizeBytes = fs.statSync(itemPath).size;
  } catch {
    /* leave 0 if the file vanished */
  }

  const id = `lib${nanoid(10)}`;
  createModel({
    id,
    name: meta.name,
    creator: meta.creator,
    type: meta.type,
    format,
    fileSizeBytes,
    size: [0, 0, 0], // bbox filled in by the Phase 2 thumbnail job
    originalPath: itemPath, // index-in-place: the archive itself, never a copy
    groups: meta.category && meta.category !== 'Misc' ? [meta.category] : [],
    tags: [],
  });

  enqueue({ kind: 'thumbnail', modelId: id, payload: { path: itemPath } }, db);
  enqueue({ kind: 'image_fetch', modelId: id, payload: { name: meta.name, creator: meta.creator } }, db);
}
