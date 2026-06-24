import path from 'node:path';

export interface LibraryMeta {
  creator: string;
  category: string;
  type: string;
  name: string;
}

const TYPE_BY_CATEGORY: Record<string, string> = {
  miniatures: 'Miniature',
  minis: 'Miniature',
  terrain: 'Terrain',
  sets: 'Set',
  bust: 'Bust',
  busts: 'Bust',
};

/**
 * Derive metadata from a file laid out as <root>/<Creator>/<Category>/<file>.
 * - creator  = first path segment under root
 * - category = second segment when present (Miniatures/Sets/Terrain/…), else 'Misc'
 * - type     = mapped from category, defaulting to 'Miniature'
 * - name     = file basename without extension, with a leading "<creator> - " stripped
 */
export function deriveLibraryMeta(root: string, fullPath: string): LibraryMeta {
  const rel = path.relative(path.resolve(root), path.resolve(fullPath));
  const parts = rel.split(path.sep).filter(Boolean);
  const creator = parts.length >= 1 ? parts[0] : 'Unknown';
  const category = parts.length >= 3 ? parts[1] : 'Misc';
  const type = TYPE_BY_CATEGORY[category.toLowerCase()] ?? 'Miniature';

  const base = path.basename(fullPath, path.extname(fullPath));
  const prefix = `${creator} - `;
  let name = base.toLowerCase().startsWith(prefix.toLowerCase()) ? base.slice(prefix.length) : base;
  name = name.trim() || base;

  return { creator, category, type, name };
}
