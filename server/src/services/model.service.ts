import type Database from 'better-sqlite3';
import { getDb } from '../db/database.js';
import { bumpNamespace } from './cache.service.js';
import type { ModelDTO, ModelDetailDTO, ModelFilter, SettingRow, ImageDTO } from './types.js';

/**
 * Model service — single source of truth for reading/writing models and their
 * related tags, groups, printers, settings and images. One public concern:
 * "models and everything attached to a model".
 */

interface ModelRow {
  id: string;
  name: string;
  creator: string;
  type: string;
  mesh_kind: string | null;
  color: string;
  format: string;
  file_size_bytes: number;
  bbox_x: number;
  bbox_y: number;
  bbox_z: number;
  triangle_count: number;
  created_date: string | null;
  added_date: string;
  original_path: string | null;
  lod_path: string | null;
  thumbnail_path: string | null;
  entry_path: string | null;
  notes: string | null;
}

function rowToDto(d: Database.Database, r: ModelRow): ModelDTO {
  return {
    id: r.id,
    name: r.name,
    creator: r.creator,
    type: r.type,
    meshKind: r.mesh_kind,
    color: r.color,
    format: r.format,
    fileSizeBytes: r.file_size_bytes,
    size: [r.bbox_x, r.bbox_y, r.bbox_z],
    triangleCount: r.triangle_count,
    created: r.created_date,
    added: r.added_date,
    tags: namesFor(d, 'model_tags', 'tags', 'tag_id', r.id),
    groups: namesFor(d, 'model_groups', 'groups', 'group_id', r.id),
    printers: namesFor(d, 'model_printer_types', 'printer_types', 'printer_type_id', r.id),
    hasThumbnail: !!r.thumbnail_path,
    hasLod: !!r.lod_path,
    hasOriginal: !!r.original_path,
  };
}

function namesFor(d: Database.Database, join: string, base: string, fk: string, modelId: string): string[] {
  const rows = d
    .prepare(`SELECT b.name AS name FROM ${join} j JOIN ${base} b ON b.id = j.${fk} WHERE j.model_id = ? ORDER BY b.name`)
    .all(modelId) as { name: string }[];
  return rows.map((x) => x.name);
}

/** List models matching a filter, newest-added first. */
export function listModels(filter: ModelFilter = {}): ModelDTO[] {
  const d = getDb();
  const rows = d.prepare('SELECT * FROM models').all() as ModelRow[];
  let dtos = rows.map((r) => rowToDto(d, r));

  const q = (filter.search || '').trim().toLowerCase();
  if (q) {
    dtos = dtos.filter((m) => {
      const hay = [m.name, m.creator, m.type, m.tags.join(' '), m.groups.join(' ')].join(' ').toLowerCase();
      return hay.includes(q);
    });
  }
  if (filter.tags?.length) {
    dtos = dtos.filter((m) => filter.tags!.every((t) => m.tags.includes(t)));
  }
  if (filter.group) dtos = dtos.filter((m) => m.groups.includes(filter.group!));
  if (filter.creator) dtos = dtos.filter((m) => m.creator === filter.creator);

  dtos.sort((a, b) => (b.added || '').localeCompare(a.added || ''));
  return dtos;
}

export function getModel(id: string): ModelDetailDTO | null {
  const d = getDb();
  const r = d.prepare('SELECT * FROM models WHERE id = ?').get(id) as ModelRow | undefined;
  if (!r) return null;
  const base = rowToDto(d, r);
  const settings = d
    .prepare('SELECT id, k, v, source FROM printer_settings WHERE model_id = ? ORDER BY ord, id')
    .all(id) as SettingRow[];
  const imgs = d
    .prepare('SELECT id, path, caption, kind FROM images WHERE model_id = ? ORDER BY id')
    .all(id) as { id: number; path: string; caption: string | null; kind: string }[];
  const images: ImageDTO[] = imgs.map((i) => ({ id: i.id, url: `/api/images/${i.id}`, caption: i.caption, kind: i.kind }));
  return { ...base, notes: r.notes, settings, images };
}

export interface NewModelInput {
  id: string;
  name: string;
  creator: string;
  type: string;
  meshKind?: string | null;
  color?: string;
  format: string;
  fileSizeBytes?: number;
  size?: [number, number, number];
  created?: string | null;
  originalPath?: string | null;
  lodPath?: string | null;
  thumbnailPath?: string | null;
  tags?: string[];
  groups?: string[];
  printers?: string[];
  settings?: [string, string][];
}

export function createModel(input: NewModelInput): ModelDetailDTO {
  const d = getDb();
  const today = new Date().toISOString().slice(0, 10);
  d.prepare(
    `INSERT INTO models (id, name, creator, type, mesh_kind, color, format, file_size_bytes,
       bbox_x, bbox_y, bbox_z, created_date, added_date, original_path, lod_path, thumbnail_path)
     VALUES (@id, @name, @creator, @type, @meshKind, @color, @format, @fileSizeBytes,
       @bx, @by, @bz, @created, @added, @originalPath, @lodPath, @thumbnailPath)`,
  ).run({
    id: input.id,
    name: input.name,
    creator: input.creator,
    type: input.type,
    meshKind: input.meshKind ?? null,
    color: input.color ?? '#bcc0c8',
    format: input.format,
    fileSizeBytes: input.fileSizeBytes ?? 0,
    bx: input.size?.[0] ?? 0,
    by: input.size?.[1] ?? 0,
    bz: input.size?.[2] ?? 0,
    created: input.created ?? today,
    added: today,
    originalPath: input.originalPath ?? null,
    lodPath: input.lodPath ?? null,
    thumbnailPath: input.thumbnailPath ?? null,
  });

  for (const t of input.tags ?? []) attachTag(input.id, t);
  for (const g of input.groups ?? []) attachGroup(input.id, g);
  for (const p of input.printers ?? []) attachPrinter(input.id, p);
  (input.settings ?? []).forEach(([k, v], i) =>
    d.prepare('INSERT INTO printer_settings (model_id, ord, k, v, source) VALUES (?, ?, ?, ?, ?)').run(input.id, i, k, v, 'manual'),
  );

  invalidate();
  return getModel(input.id)!;
}

export function updateModel(id: string, patch: Partial<Record<string, unknown>>): ModelDetailDTO | null {
  const d = getDb();
  const allowed: Record<string, string> = {
    name: 'name', creator: 'creator', type: 'type', notes: 'notes', color: 'color',
    triangleCount: 'triangle_count', thumbnailPath: 'thumbnail_path', lodPath: 'lod_path',
    entryPath: 'entry_path',
  };
  const sets: string[] = [];
  const vals: unknown[] = [];
  for (const [k, col] of Object.entries(allowed)) {
    if (k in patch) { sets.push(`${col} = ?`); vals.push(patch[k]); }
  }
  if ('size' in patch && Array.isArray(patch.size)) {
    sets.push('bbox_x = ?', 'bbox_y = ?', 'bbox_z = ?');
    vals.push(patch.size[0], patch.size[1], patch.size[2]);
  }
  if (sets.length) {
    vals.push(id);
    d.prepare(`UPDATE models SET ${sets.join(', ')} WHERE id = ?`).run(...vals);
  }
  invalidate();
  return getModel(id);
}

export function deleteModel(id: string): boolean {
  const d = getDb();
  const res = d.prepare('DELETE FROM models WHERE id = ?').run(id);
  invalidate();
  return res.changes > 0;
}

// ---- attachment helpers (also used by routes for tag/group/printer toggles) ----

function idForName(d: Database.Database, table: string, name: string): number {
  d.prepare(`INSERT OR IGNORE INTO ${table} (name) VALUES (?)`).run(name);
  return (d.prepare(`SELECT id FROM ${table} WHERE name = ?`).get(name) as { id: number }).id;
}

export function attachTag(modelId: string, name: string): void {
  const d = getDb();
  const tagId = idForName(d, 'tags', name.trim().toLowerCase().replace(/\s+/g, '-'));
  d.prepare('INSERT OR IGNORE INTO model_tags (model_id, tag_id) VALUES (?, ?)').run(modelId, tagId);
  invalidate();
}

export function detachTag(modelId: string, name: string): void {
  const d = getDb();
  d.prepare('DELETE FROM model_tags WHERE model_id = ? AND tag_id = (SELECT id FROM tags WHERE name = ?)').run(modelId, name);
  invalidate();
}

export function attachGroup(modelId: string, name: string): void {
  const d = getDb();
  d.prepare('INSERT OR IGNORE INTO groups (name, shared) VALUES (?, 0)').run(name);
  const gid = (d.prepare('SELECT id FROM groups WHERE name = ?').get(name) as { id: number }).id;
  d.prepare('INSERT OR IGNORE INTO model_groups (model_id, group_id) VALUES (?, ?)').run(modelId, gid);
  invalidate();
}

export function detachGroup(modelId: string, name: string): void {
  const d = getDb();
  d.prepare('DELETE FROM model_groups WHERE model_id = ? AND group_id = (SELECT id FROM groups WHERE name = ?)').run(modelId, name);
  invalidate();
}

export function attachPrinter(modelId: string, name: string): void {
  const d = getDb();
  const pid = idForName(d, 'printer_types', name);
  d.prepare('INSERT OR IGNORE INTO model_printer_types (model_id, printer_type_id) VALUES (?, ?)').run(modelId, pid);
  invalidate();
}

export function detachPrinter(modelId: string, name: string): void {
  const d = getDb();
  d.prepare('DELETE FROM model_printer_types WHERE model_id = ? AND printer_type_id = (SELECT id FROM printer_types WHERE name = ?)').run(modelId, name);
  invalidate();
}

export function getModelPaths(id: string): { original: string | null; lod: string | null; thumbnail: string | null; entry: string | null; format: string } | null {
  const d = getDb();
  const r = d.prepare('SELECT original_path, lod_path, thumbnail_path, entry_path, format FROM models WHERE id = ?').get(id) as
    | { original_path: string | null; lod_path: string | null; thumbnail_path: string | null; entry_path: string | null; format: string }
    | undefined;
  if (!r) return null;
  return { original: r.original_path, lod: r.lod_path, thumbnail: r.thumbnail_path, entry: r.entry_path, format: r.format };
}

/** Bump cache namespaces affected by any model write. */
export function invalidate(): void {
  void bumpNamespace('models');
  void bumpNamespace('suggest');
}
