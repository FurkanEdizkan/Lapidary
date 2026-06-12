import { getDb } from '../db/database.js';
import { invalidate } from './model.service.js';
import type { SettingRow } from './types.js';

/** Suggested print settings: ordered key/value rows, manual or imported from a profile. */
export function listSettings(modelId: string): SettingRow[] {
  return getDb()
    .prepare('SELECT id, k, v, source FROM printer_settings WHERE model_id = ? ORDER BY ord, id')
    .all(modelId) as SettingRow[];
}

export function addSetting(modelId: string, k: string, v: string, source = 'manual'): void {
  const d = getDb();
  const max = d.prepare('SELECT COALESCE(MAX(ord), -1) AS m FROM printer_settings WHERE model_id = ?').get(modelId) as { m: number };
  d.prepare('INSERT INTO printer_settings (model_id, ord, k, v, source) VALUES (?, ?, ?, ?, ?)').run(modelId, max.m + 1, k, v, source);
  invalidate();
}

export function updateSetting(id: number, k: string, v: string): void {
  getDb().prepare('UPDATE printer_settings SET k = ?, v = ? WHERE id = ?').run(k, v, id);
  invalidate();
}

export function deleteSetting(id: number): void {
  getDb().prepare('DELETE FROM printer_settings WHERE id = ?').run(id);
  invalidate();
}

/** Replace a model's settings wholesale (used after a profile import). */
export function replaceSettings(modelId: string, rows: { k: string; v: string }[], source: string, profilePath?: string): void {
  const d = getDb();
  const tx = d.transaction(() => {
    d.prepare('DELETE FROM printer_settings WHERE model_id = ? AND source = ?').run(modelId, source);
    const max = d.prepare('SELECT COALESCE(MAX(ord), -1) AS m FROM printer_settings WHERE model_id = ?').get(modelId) as { m: number };
    rows.forEach((r, i) =>
      d.prepare('INSERT INTO printer_settings (model_id, ord, k, v, source, profile_path) VALUES (?, ?, ?, ?, ?, ?)').run(
        modelId, max.m + 1 + i, r.k, r.v, source, profilePath ?? null,
      ),
    );
  });
  tx();
  invalidate();
}
