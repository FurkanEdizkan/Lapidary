import { describe, it, expect } from 'vitest';
import { deriveLibraryMeta } from '../src/services/libraryPath.js';

const ROOT = '/lib/Creators';

describe('deriveLibraryMeta', () => {
  it('derives creator, category, type, and name from a 3-level path', () => {
    const m = deriveLibraryMeta(
      ROOT,
      '/lib/Creators/Creature Caster/Miniatures/Creature Caster - Lady of Arcana.zip',
    );
    expect(m).toEqual({
      creator: 'Creature Caster',
      category: 'Miniatures',
      type: 'Miniature',
      name: 'Lady of Arcana',
    });
  });

  it('maps Terrain and Sets categories to their types', () => {
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Terrain/Foo - Wall.7z').type).toBe('Terrain');
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Sets/Foo - Bundle.rar').type).toBe('Set');
  });

  it('falls back to Misc/Miniature when there is no category folder', () => {
    const m = deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Foo - Loose.zip');
    expect(m.category).toBe('Misc');
    expect(m.type).toBe('Miniature');
    expect(m.name).toBe('Loose');
  });

  it('keeps the basename when there is no "<creator> - " prefix', () => {
    expect(deriveLibraryMeta(ROOT, '/lib/Creators/Foo/Miniatures/Dragon King.zip').name).toBe('Dragon King');
  });
});
