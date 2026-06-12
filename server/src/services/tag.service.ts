import { getDb } from '../db/database.js';
import { invalidate } from './model.service.js';

/** Tags with model counts, for the Tags/Groups manager and rail. */
export interface TagInfo {
  name: string;
  count: number;
}

export function listTags(): TagInfo[] {
  const d = getDb();
  return d
    .prepare(
      `SELECT t.name AS name, COUNT(mt.model_id) AS count
       FROM tags t LEFT JOIN model_tags mt ON mt.tag_id = t.id
       GROUP BY t.id ORDER BY count DESC, t.name`,
    )
    .all() as TagInfo[];
}

export function createTag(name: string): void {
  const norm = name.trim().toLowerCase().replace(/\s+/g, '-');
  if (!norm) return;
  getDb().prepare('INSERT OR IGNORE INTO tags (name) VALUES (?)').run(norm);
  invalidate();
}

export function deleteTag(name: string): void {
  getDb().prepare('DELETE FROM tags WHERE name = ?').run(name);
  invalidate();
}
