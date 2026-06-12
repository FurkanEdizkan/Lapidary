import fs from 'node:fs';
import path from 'node:path';
import { nanoid } from 'nanoid';
import { getDb } from '../db/database.js';
import { config } from '../config.js';
import { invalidate } from './model.service.js';
import type { ImageDTO } from './types.js';

/** Printed/painted result photos attached to a model. Stored on disk under DATA_DIR/images. */
export function listImages(modelId: string): ImageDTO[] {
  const rows = getDb()
    .prepare('SELECT id, caption, kind FROM images WHERE model_id = ? ORDER BY id')
    .all(modelId) as { id: number; caption: string | null; kind: string }[];
  return rows.map((r) => ({ id: r.id, url: `/api/images/${r.id}`, caption: r.caption, kind: r.kind }));
}

export function addImage(modelId: string, buffer: Buffer, originalName: string, kind: string, caption?: string): ImageDTO {
  const ext = path.extname(originalName) || '.png';
  const filename = `${modelId}-${nanoid(8)}${ext}`;
  const filePath = path.join(config.imagesDir, filename);
  fs.writeFileSync(filePath, buffer);
  const res = getDb()
    .prepare('INSERT INTO images (model_id, path, caption, kind) VALUES (?, ?, ?, ?)')
    .run(modelId, filePath, caption ?? null, kind);
  invalidate();
  return { id: Number(res.lastInsertRowid), url: `/api/images/${res.lastInsertRowid}`, caption: caption ?? null, kind };
}

export function getImagePath(id: number): string | null {
  const r = getDb().prepare('SELECT path FROM images WHERE id = ?').get(id) as { path: string } | undefined;
  return r?.path ?? null;
}

export function deleteImage(id: number): void {
  const d = getDb();
  const r = d.prepare('SELECT path FROM images WHERE id = ?').get(id) as { path: string } | undefined;
  if (r) {
    try { fs.unlinkSync(r.path); } catch { /* already gone */ }
    d.prepare('DELETE FROM images WHERE id = ?').run(id);
    invalidate();
  }
}
