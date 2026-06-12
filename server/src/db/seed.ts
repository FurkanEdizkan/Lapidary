import type Database from 'better-sqlite3';

/**
 * Optional sample-data seeding. Lapidary ships with an EMPTY library — add your
 * own models via "Add model" or "Scan". To pre-populate a fresh install, fill the
 * arrays below; seeding runs once, only when the library has no models yet.
 */
export interface SeedModel {
  id: string;
  name: string;
  creator: string;
  type: string;
  tags: string[];
  groups: string[];
  mesh: string;
  color: string;
  size: [number, number, number];
  format: string;
  fileMB: number;
  created: string;
  added: string;
  printers: string[];
  settings: [string, string][];
}

export const SEED_PRINTERS: string[] = [];

export const SEED_GROUPS: { name: string; shared: boolean }[] = [];

export const SEED_MODELS: SeedModel[] = [];

export const SEED_PINS: { kind: string; name: string }[] = [];

/** Populate the database from the sample set, once, if it has no models yet. */
export function seedDatabase(d: Database.Database): void {
  const count = d.prepare('SELECT COUNT(*) AS n FROM models').get() as { n: number };
  if (count.n > 0) return;
  if (!SEED_MODELS.length && !SEED_GROUPS.length && !SEED_PRINTERS.length && !SEED_PINS.length) return;

  const tagId = upsertName(d, 'tags');
  const groupId = upsertGroup(d);
  const printerId = upsertName(d, 'printer_types');

  const insModel = d.prepare(`
    INSERT INTO models (id, name, creator, type, mesh_kind, color, format, file_size_bytes,
      bbox_x, bbox_y, bbox_z, triangle_count, created_date, added_date)
    VALUES (@id, @name, @creator, @type, @mesh, @color, @format, @fileBytes,
      @bx, @by, @bz, 0, @created, @added)
  `);
  const insModelTag = d.prepare('INSERT OR IGNORE INTO model_tags (model_id, tag_id) VALUES (?, ?)');
  const insModelGroup = d.prepare('INSERT OR IGNORE INTO model_groups (model_id, group_id) VALUES (?, ?)');
  const insModelPrinter = d.prepare('INSERT OR IGNORE INTO model_printer_types (model_id, printer_type_id) VALUES (?, ?)');
  const insSetting = d.prepare('INSERT INTO printer_settings (model_id, ord, k, v, source) VALUES (?, ?, ?, ?, ?)');

  const run = d.transaction(() => {
    for (const g of SEED_GROUPS) groupId(g.name, g.shared);
    for (const p of SEED_PRINTERS) printerId(p);

    for (const m of SEED_MODELS) {
      insModel.run({
        id: m.id, name: m.name, creator: m.creator, type: m.type, mesh: m.mesh,
        color: m.color, format: m.format, fileBytes: Math.round(m.fileMB * 1024 * 1024),
        bx: m.size[0], by: m.size[1], bz: m.size[2], created: m.created, added: m.added,
      });
      for (const t of m.tags) insModelTag.run(m.id, tagId(t));
      for (const g of m.groups) insModelGroup.run(m.id, groupId(g, false));
      for (const p of m.printers) insModelPrinter.run(m.id, printerId(p));
      m.settings.forEach(([k, v], i) => insSetting.run(m.id, i, k, v, 'manual'));
    }

    const insPin = d.prepare('INSERT OR IGNORE INTO pins (kind, name) VALUES (?, ?)');
    for (const p of SEED_PINS) insPin.run(p.kind, p.name);
  });
  run();
}

function upsertName(d: Database.Database, table: 'tags' | 'printer_types') {
  const ins = d.prepare(`INSERT OR IGNORE INTO ${table} (name) VALUES (?)`);
  const sel = d.prepare(`SELECT id FROM ${table} WHERE name = ?`);
  return (name: string): number => {
    ins.run(name);
    return (sel.get(name) as { id: number }).id;
  };
}

function upsertGroup(d: Database.Database) {
  const ins = d.prepare('INSERT OR IGNORE INTO groups (name, shared) VALUES (?, ?)');
  const sel = d.prepare('SELECT id FROM groups WHERE name = ?');
  return (name: string, shared: boolean): number => {
    ins.run(name, shared ? 1 : 0);
    return (sel.get(name) as { id: number }).id;
  };
}
