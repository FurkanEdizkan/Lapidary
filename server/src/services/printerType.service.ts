import { getDb } from '../db/database.js';

/** The set of known printers, used to populate compatibility toggles. */
export function listPrinterTypes(): string[] {
  const rows = getDb().prepare('SELECT name FROM printer_types ORDER BY name').all() as { name: string }[];
  return rows.map((r) => r.name);
}

export function createPrinterType(name: string): void {
  const n = name.trim();
  if (n) getDb().prepare('INSERT OR IGNORE INTO printer_types (name) VALUES (?)').run(n);
}
