import { getDb } from '../db/database.js';
import { invalidate } from './model.service.js';

/** Groups are personal/shared folders of models. */
export interface GroupInfo {
  name: string;
  shared: boolean;
  count: number;
}

export function listGroups(): GroupInfo[] {
  const d = getDb();
  const rows = d
    .prepare(
      `SELECT g.name AS name, g.shared AS shared, COUNT(mg.model_id) AS count
       FROM groups g LEFT JOIN model_groups mg ON mg.group_id = g.id
       GROUP BY g.id ORDER BY g.name`,
    )
    .all() as { name: string; shared: number; count: number }[];
  return rows.map((r) => ({ name: r.name, shared: !!r.shared, count: r.count }));
}

export function createGroup(name: string): void {
  const n = name.trim();
  if (!n) return;
  getDb().prepare('INSERT OR IGNORE INTO groups (name, shared) VALUES (?, 0)').run(n);
  invalidate();
}

export function setGroupShared(name: string, shared: boolean): void {
  getDb().prepare('UPDATE groups SET shared = ? WHERE name = ?').run(shared ? 1 : 0, name);
  invalidate();
}

export function deleteGroup(name: string): void {
  getDb().prepare('DELETE FROM groups WHERE name = ?').run(name);
  invalidate();
}
