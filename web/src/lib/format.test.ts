import { describe, it, expect } from 'vitest';
import { thumbCacheKey } from './format';
import type { PlateTransform } from '../api/client';

const t: PlateTransform = { position: [1, 2, 3], quaternion: [0, 0, 0, 1], scale: 1 };

describe('thumbCacheKey', () => {
  it('is deterministic for equal transforms', () => {
    expect(thumbCacheKey(t)).toBe(thumbCacheKey({ ...t, position: [1, 2, 3] }));
  });

  it('changes when any component changes', () => {
    expect(thumbCacheKey(t)).not.toBe(thumbCacheKey({ ...t, scale: 1.5 }));
    expect(thumbCacheKey(t)).not.toBe(thumbCacheKey({ ...t, position: [1, 2, 4] }));
    expect(thumbCacheKey(t)).not.toBe(thumbCacheKey({ ...t, quaternion: [0, 0, 0.7071, 0.7071] }));
  });

  it('null maps to a stable sentinel distinct from any transform', () => {
    expect(thumbCacheKey(null)).toBe(thumbCacheKey(null));
    expect(thumbCacheKey(null)).not.toBe(thumbCacheKey(t));
  });

  it('produces a URL-safe token', () => {
    expect(thumbCacheKey(t)).toMatch(/^[A-Za-z0-9_-]+$/);
  });
});
