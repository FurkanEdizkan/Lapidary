import fs from 'node:fs';
import path from 'node:path';
import { nanoid } from 'nanoid';
import pLimit from 'p-limit';
import { getDb } from '../db/database.js';
import { ingestMesh } from './assetPipeline.service.js';
import { createModel } from './model.service.js';

const SUPPORTED = new Set(['.stl', '.3mf', '.obj']);

/** Recursively collect supported mesh files under a directory. */
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
    if (e.isDirectory()) collect(full, out, depth + 1);
    else if (SUPPORTED.has(path.extname(e.name).toLowerCase())) out.push(full);
  }
  return out;
}

export interface ScanResult {
  scanned: number;
  imported: number;
  skipped: number;
}

/** Scan a folder and import any new meshes through the asset pipeline. */
export async function scanFolder(folderPath: string): Promise<ScanResult> {
  const root = path.resolve(folderPath);
  if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
    throw new Error(`Not a directory: ${root}`);
  }
  const files = collect(root);
  const d = getDb();
  const existingNames = new Set(
    (d.prepare('SELECT name FROM models').all() as { name: string }[]).map((r) => r.name),
  );

  const limit = pLimit(3); // bounded concurrency so a large library cannot exhaust memory
  let imported = 0;
  let skipped = 0;

  await Promise.all(
    files.map((file) =>
      limit(async () => {
        const name = path.basename(file, path.extname(file));
        if (existingNames.has(name)) {
          skipped += 1;
          return;
        }
        existingNames.add(name);
        try {
          const buffer = fs.readFileSync(file);
          const id = `u${nanoid(10)}`;
          const ingest = await ingestMesh(id, path.basename(file), buffer);
          createModel({
            id,
            name,
            creator: 'Imported',
            type: 'Miniature',
            format: ingest.format,
            fileSizeBytes: ingest.fileSizeBytes,
            size: ingest.size ?? [0, 0, 0],
            originalPath: ingest.originalPath,
            lodPath: ingest.lodPath,
            tags: [],
          });
          if (ingest.triangleCount) {
            d.prepare('UPDATE models SET triangle_count = ? WHERE id = ?').run(ingest.triangleCount, id);
          }
          imported += 1;
        } catch {
          skipped += 1;
        }
      }),
    ),
  );

  return { scanned: files.length, imported, skipped };
}
