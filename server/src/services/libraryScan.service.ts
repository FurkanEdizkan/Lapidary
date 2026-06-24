import fs from 'node:fs';
import path from 'node:path';
import type Database from 'better-sqlite3';
import { getDb } from '../db/database.js';
import { enqueue } from './jobs.service.js';

const ARCHIVE_EXTS = new Set(['.zip', '.rar', '.7z']);
const MESH_EXTS = new Set(['.stl', '.3mf', '.obj']);

/** Recursively collect archive + loose-mesh files under a directory. */
function collect(dir: string, out: string[] = [], depth = 0): string[] {
  if (depth > 8) return out;
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      collect(full, out, depth + 1);
    } else {
      const ext = path.extname(e.name).toLowerCase();
      if (ARCHIVE_EXTS.has(ext) || MESH_EXTS.has(ext)) out.push(full);
    }
  }
  return out;
}

export interface ScanResult {
  scanned: number;
  enqueued: number;
  skipped: number;
}

/**
 * Walk a library folder and enqueue one `index_archive` job per archive/mesh found.
 * Index-in-place: nothing is extracted or copied here — each job carries the absolute
 * source path and the scan root. Items already indexed (by `original_path`) are skipped;
 * a duplicate enqueue is harmless because the handler is idempotent.
 */
export function scanFolder(folderPath: string, db: Database.Database = getDb()): ScanResult {
  const root = path.resolve(folderPath);
  if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
    throw new Error(`Not a directory: ${root}`);
  }
  const files = collect(root);
  const indexed = new Set(
    (db.prepare('SELECT original_path FROM models WHERE original_path IS NOT NULL').all() as {
      original_path: string;
    }[]).map((r) => r.original_path),
  );

  const seen = new Set<string>();
  let enqueued = 0;
  let skipped = 0;
  for (const file of files) {
    if (indexed.has(file) || seen.has(file)) {
      skipped += 1;
      continue;
    }
    seen.add(file);
    enqueue({ kind: 'index_archive', payload: { path: file, root } }, db);
    enqueued += 1;
  }
  return { scanned: files.length, enqueued, skipped };
}
