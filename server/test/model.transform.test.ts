import { describe, it, expect, beforeEach } from 'vitest';
import { getDb } from '../src/db/database.js';
import { createModel, updateModel, getModel } from '../src/services/model.service.js';
import type { PlateTransform } from '../src/services/types.js';

function clean(): void {
  const d = getDb();
  d.prepare('DELETE FROM models').run();
}

describe('model transform persistence', () => {
  beforeEach(() => clean());

  it('defaults transform to null', () => {
    createModel({ id: 't1', name: 'M', creator: 'C', type: 'Miniature', format: 'STL' });
    expect(getModel('t1')!.transform).toBeNull();
  });

  it('round-trips a plate transform through updateModel/getModel', () => {
    createModel({ id: 't2', name: 'M', creator: 'C', type: 'Miniature', format: 'STL' });
    const t: PlateTransform = { position: [1, 2, 3], quaternion: [0, 0, 0.7071, 0.7071], scale: 1 };
    updateModel('t2', { transform: t });
    expect(getModel('t2')!.transform).toEqual(t);
  });

  it('returns transform null (no throw) for a malformed transform_json', () => {
    createModel({ id: 't3', name: 'M', creator: 'C', type: 'Miniature', format: 'STL' });
    getDb().prepare('UPDATE models SET transform_json = ? WHERE id = ?').run('not json{', 't3');
    expect(() => getModel('t3')).not.toThrow();
    expect(getModel('t3')!.transform).toBeNull();
  });
});
