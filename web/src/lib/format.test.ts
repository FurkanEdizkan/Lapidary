import { describe, it, expect } from 'vitest';
import { thumbUrl } from './format';

describe('thumbUrl', () => {
  it('omits the version param when none is given', () => {
    expect(thumbUrl('abc')).toBe('/api/models/abc/thumbnail');
  });

  it('omits the version param when version is 0 (no thumbnail)', () => {
    expect(thumbUrl('abc', 0)).toBe('/api/models/abc/thumbnail');
  });

  it('appends ?v=<version> so a regenerated thumbnail busts the cache', () => {
    expect(thumbUrl('abc', 1717000000000)).toBe('/api/models/abc/thumbnail?v=1717000000000');
    expect(thumbUrl('abc', 111)).not.toBe(thumbUrl('abc', 222));
  });
});
