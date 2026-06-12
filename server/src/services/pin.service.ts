import { getDb } from '../db/database.js';

/** Pinned favorites (creators, tags, or groups) shown in the PinnedGroupsBar. */
export interface Pin {
  kind: 'creator' | 'tag' | 'group';
  name: string;
}

export function listPins(): Pin[] {
  return getDb().prepare('SELECT kind, name FROM pins ORDER BY id').all() as Pin[];
}

export function addPin(kind: string, name: string): void {
  getDb().prepare('INSERT OR IGNORE INTO pins (kind, name) VALUES (?, ?)').run(kind, name);
}

export function removePin(kind: string, name: string): void {
  getDb().prepare('DELETE FROM pins WHERE kind = ? AND name = ?').run(kind, name);
}

export function togglePin(kind: string, name: string): boolean {
  const d = getDb();
  const existing = d.prepare('SELECT id FROM pins WHERE kind = ? AND name = ?').get(kind, name);
  if (existing) {
    d.prepare('DELETE FROM pins WHERE kind = ? AND name = ?').run(kind, name);
    return false;
  }
  d.prepare('INSERT INTO pins (kind, name) VALUES (?, ?)').run(kind, name);
  return true;
}
