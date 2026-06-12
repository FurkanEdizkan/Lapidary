import type Database from 'better-sqlite3';

/**
 * Sample library, mirrored from the design bundle's `design/data.js`, so a fresh
 * install opens to a populated Lapidary gallery. Seeding runs once (when models is empty).
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

export const SEED_PRINTERS = ['Bambu X1C', 'Bambu P1S', 'Ender 3', 'Elegoo Saturn'];

export const SEED_GROUPS: { name: string; shared: boolean }[] = [
  { name: 'Trench Crusade Army', shared: true },
  { name: 'Battle Station', shared: false },
  { name: 'Home Prints', shared: false },
  { name: 'Display Shelf', shared: true },
];

export const SEED_MODELS: SeedModel[] = [
  { id: 'm01', name: 'Gothic Trench Barricade', creator: 'TrenchWorks', type: 'Terrain', tags: ['trench-crusade', 'terrain', 'wargaming'], groups: ['Trench Crusade Army'], mesh: 'barricade', color: '#b4b8c0', size: [120, 45, 62], format: 'STL', fileMB: 18.4, created: '2025-08-12', added: '2026-01-14', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.16 mm'], ['Infill', '12 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA matte'], ['Note', 'Print planks-down, brim 4 mm']] },
  { id: 'm02', name: 'Votive Idol — Pilgrim', creator: 'TrenchWorks', type: 'Miniature', tags: ['trench-crusade', 'miniature', 'wargaming'], groups: ['Trench Crusade Army'], mesh: 'pawn', color: '#c6c1b6', size: [22, 22, 34], format: 'STL', fileMB: 42.1, created: '2025-09-02', added: '2026-01-14', printers: ['Elegoo Saturn'], settings: [['Process', 'Resin (MSLA)'], ['Layer height', '0.05 mm'], ['Exposure', '2.6 s'], ['Supports', 'Light, 35° tilt'], ['Material', 'ABS-like grey'], ['Note', 'Hollow at 1.8 mm, two drain holes']] },
  { id: 'm03', name: 'Dragon Egg Dice Vault', creator: 'Forge & Fathom', type: 'Display', tags: ['dragon', 'dice', 'display'], groups: ['Display Shelf'], mesh: 'egg', color: '#b9bdc9', size: [68, 68, 96], format: '3MF', fileMB: 31.7, created: '2025-06-20', added: '2025-12-02', printers: ['Bambu X1C', 'Bambu P1S', 'Elegoo Saturn'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.12 mm'], ['Infill', '15 % gyroid'], ['Supports', 'Tree (auto)'], ['Material', 'PLA silk'], ['Note', 'Threaded halves — print upright']] },
  { id: 'm04', name: 'Hex Terrain Tile — Wastes', creator: 'Forge & Fathom', type: 'Terrain', tags: ['terrain', 'warhammer', 'wargaming'], groups: [], mesh: 'hexTile', color: '#aeb2ac', size: [100, 87, 28], format: 'STL', fileMB: 22.9, created: '2025-05-11', added: '2025-11-20', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '10 % grid'], ['Supports', 'None needed'], ['Material', 'PLA basic'], ['Note', 'Magnets press-fit after print']] },
  { id: 'm05', name: 'Bastion Dice Tower', creator: 'Forge & Fathom', type: 'Functional', tags: ['dice', 'warhammer', 'display'], groups: ['Display Shelf'], mesh: 'diceTower', color: '#b6b3bf', size: [95, 140, 168], format: 'STL', fileMB: 54.2, created: '2025-03-30', added: '2025-11-20', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '12 % gyroid'], ['Supports', 'Tree on tray lip only'], ['Material', 'PETG'], ['Note', 'Baffles printed in place']] },
  { id: 'm06', name: 'Herringbone Gear Fidget', creator: 'Greeble Garage', type: 'Tool', tags: ['tools', 'fidget'], groups: [], mesh: 'gear', color: '#bcc2ca', size: [52, 52, 16], format: 'STL', fileMB: 6.8, created: '2025-10-05', added: '2026-02-01', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.16 mm'], ['Infill', '25 % grid'], ['Supports', 'None needed'], ['Material', 'PLA basic'], ['Note', '0.2 mm clearance, no glue']] },
  { id: 'm07', name: 'Cable Comb · 8 slot', creator: 'PrintaTech', type: 'Functional', tags: ['pc-case', 'organizer'], groups: ['Battle Station'], mesh: 'comb', color: '#b0b6bd', size: [84, 20, 24], format: '3MF', fileMB: 2.3, created: '2025-11-18', added: '2026-02-10', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.12 mm'], ['Infill', '40 % grid'], ['Supports', 'None needed'], ['Material', 'PETG black'], ['Note', 'Fits 16 AWG sleeved cable']] },
  { id: 'm08', name: 'PC Case Feet · set of 4', creator: 'PrintaTech', type: 'Functional', tags: ['pc-case'], groups: ['Battle Station'], mesh: 'feet', color: '#aab0b8', size: [40, 40, 18], format: 'STL', fileMB: 3.1, created: '2025-07-22', added: '2026-02-10', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '60 % grid'], ['Supports', 'None needed'], ['Material', 'TPU 95A'], ['Note', 'TPU dampens drive vibration']] },
  { id: 'm09', name: 'GPU Anti-Sag Bracket', creator: 'PrintaTech', type: 'Functional', tags: ['pc-case', 'tools'], groups: ['Battle Station'], mesh: 'bracket', color: '#b5bac2', size: [65, 25, 75], format: 'STL', fileMB: 4.6, created: '2025-09-14', added: '2026-02-10', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '50 % cubic'], ['Supports', 'None needed'], ['Material', 'PETG-CF'], ['Note', 'Print flat on side, 5 walls']] },
  { id: 'm10', name: 'Heavy Duty Wall Hook', creator: 'Greeble Garage', type: 'Tool', tags: ['house', 'tools'], groups: ['Home Prints'], mesh: 'hook', color: '#bdbab2', size: [45, 25, 75], format: 'STL', fileMB: 3.8, created: '2025-04-08', added: '2025-10-30', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '45 % cubic'], ['Supports', 'None needed'], ['Material', 'PETG'], ['Note', 'Print on side for layer strength']] },
  { id: 'm11', name: 'Faceted Planter', creator: 'K. Imai', type: 'Display', tags: ['house', 'display'], groups: ['Home Prints'], mesh: 'facetVase', color: '#c2bdb3', size: [110, 110, 92], format: 'STL', fileMB: 9.4, created: '2025-02-19', added: '2025-10-30', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.6 mm nozzle'], ['Layer height', '0.3 mm'], ['Infill', '8 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA matte'], ['Note', 'Drainage hole optional chunk']] },
  { id: 'm12', name: 'Spiral Bud Vase', creator: 'K. Imai', type: 'Display', tags: ['house', 'display', 'vase-mode'], groups: ['Home Prints'], mesh: 'vase', color: '#c5c0c8', size: [78, 78, 150], format: 'STL', fileMB: 5.2, created: '2025-01-25', added: '2025-10-30', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · vase mode'], ['Layer height', '0.24 mm'], ['Walls', 'Single, 0.8 mm line'], ['Supports', 'None needed'], ['Material', 'PLA silk'], ['Note', 'Spiralize outer contour ON']] },
  { id: 'm13', name: 'Watchtower Objective Marker', creator: 'Bastion Forge', type: 'Terrain', tags: ['warhammer', 'terrain', 'wargaming'], groups: [], mesh: 'rook', color: '#b2b6ae', size: [40, 40, 80], format: 'STL', fileMB: 15.6, created: '2025-06-03', added: '2025-12-18', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3', 'Elegoo Saturn'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.12 mm'], ['Infill', '15 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA basic'], ['Note', 'Also slices clean for resin']] },
  { id: 'm14', name: 'Crystal Objective Markers ×3', creator: 'Bastion Forge', type: 'Miniature', tags: ['warhammer', 'terrain', 'crystal'], groups: [], mesh: 'crystals', color: '#b8bcce', size: [55, 55, 48], format: 'STL', fileMB: 11.2, created: '2025-08-27', added: '2025-12-18', printers: ['Elegoo Saturn', 'Bambu X1C'], settings: [['Process', 'Resin (MSLA)'], ['Layer height', '0.05 mm'], ['Exposure', '2.4 s'], ['Supports', 'Medium, auto'], ['Material', 'Clear resin'], ['Note', 'Tint with ink wash after cure']] },
  { id: 'm15', name: 'Sandbag Emplacement', creator: 'TrenchWorks', type: 'Terrain', tags: ['trench-crusade', 'terrain', 'wargaming'], groups: ['Trench Crusade Army'], mesh: 'sandbags', color: '#c0bbab', size: [110, 60, 38], format: 'STL', fileMB: 26.3, created: '2025-08-12', added: '2026-01-14', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '10 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA basic'], ['Note', 'Fuzzy skin ON for burlap texture']] },
  { id: 'm16', name: 'SD Card Tray', creator: 'VoxelSmith', type: 'Functional', tags: ['organizer', 'tools'], groups: ['Battle Station'], mesh: 'tray', color: '#b3b8bf', size: [105, 70, 25], format: '3MF', fileMB: 4.0, created: '2025-12-01', added: '2026-03-02', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.2 mm'], ['Infill', '15 % grid'], ['Supports', 'None needed'], ['Material', 'PLA basic'], ['Note', 'Holds 6 SD + 8 microSD']] },
  { id: 'm17', name: 'Display Plinth · 50 mm', creator: 'M. Aldous', type: 'Display', tags: ['display', 'miniature', 'warhammer'], groups: ['Display Shelf'], mesh: 'plinth', color: '#bfb9c4', size: [50, 50, 55], format: 'STL', fileMB: 7.7, created: '2025-05-29', added: '2025-12-02', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3', 'Elegoo Saturn'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.12 mm'], ['Infill', '15 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA matte black'], ['Note', '5 mm magnet recess in top']] },
  { id: 'm18', name: 'Filament Clip · universal', creator: 'Greeble Garage', type: 'Tool', tags: ['tools'], groups: [], mesh: 'clip', color: '#b9bfb6', size: [22, 14, 18], format: 'STL', fileMB: 0.9, created: '2025-10-05', added: '2026-02-01', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.16 mm'], ['Infill', '30 % grid'], ['Supports', 'None needed'], ['Material', 'PETG'], ['Note', 'Print 10 at a time, flat side down']] },
  { id: 'm19', name: '32 mm Base Pack — Cracked Earth', creator: 'Bastion Forge', type: 'Miniature', tags: ['warhammer', 'trench-crusade', 'miniature'], groups: ['Trench Crusade Army'], mesh: 'base32', color: '#b1ada4', size: [32, 32, 6], format: 'STL', fileMB: 13.5, created: '2025-07-15', added: '2026-01-14', printers: ['Elegoo Saturn', 'Bambu X1C', 'Bambu P1S'], settings: [['Process', 'Resin (MSLA)'], ['Layer height', '0.05 mm'], ['Exposure', '2.5 s'], ['Supports', 'None — print flat'], ['Material', 'ABS-like grey'], ['Note', 'Texture survives 0.12 FDM too']] },
  { id: 'm20', name: 'Dragonscale Coaster Set', creator: 'K. Imai', type: 'Display', tags: ['dragon', 'house', 'display'], groups: ['Home Prints'], mesh: 'coaster', color: '#c4bfc9', size: [98, 98, 8], format: 'STL', fileMB: 8.1, created: '2025-03-12', added: '2025-10-30', printers: ['Bambu X1C', 'Bambu P1S', 'Ender 3'], settings: [['Process', 'FDM · 0.4 mm nozzle'], ['Layer height', '0.12 mm'], ['Infill', '20 % gyroid'], ['Supports', 'None needed'], ['Material', 'PLA silk'], ['Note', 'Color swap at scale layer']] },
];

export const SEED_PINS: { kind: string; name: string }[] = [
  { kind: 'creator', name: 'TrenchWorks' },
  { kind: 'tag', name: 'dragon' },
];

/** Populate the database from the sample set, once, if it has no models yet. */
export function seedDatabase(d: Database.Database): void {
  const count = d.prepare('SELECT COUNT(*) AS n FROM models').get() as { n: number };
  if (count.n > 0) return;

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
