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
 * Derive {creator, category, type, name} from a library file path.
 *
 * The library is laid out as <…>/<Creator>/<Category>/<file>, where Category is one of a
 * known vocabulary (Miniatures/Sets/Terrain/…). A scan can be pointed either at the Creators
 * root OR at a single creator's folder, so we do NOT derive position relative to `root`.
 * Instead we anchor on the Category folder: searching the absolute path's directory segments
 * from the file upward, the first segment whose name is a known category is the category, and
 * the segment directly above it is the creator. If no known category folder is present, we
 * fall back to the file's immediate parent folder as the creator with category 'Misc'.
 *
 * `root` is accepted for API stability but no longer affects the result.
 */
export function deriveLibraryMeta(root: string, fullPath: string): LibraryMeta {
  const abs = path.resolve(fullPath);
  const segs = path.dirname(abs).split(path.sep).filter(Boolean);
  const base = path.basename(abs, path.extname(abs));

  let creator = 'Unknown';
  let category = 'Misc';
  for (let i = segs.length - 1; i >= 1; i--) {
    if (Object.hasOwn(TYPE_BY_CATEGORY, segs[i].toLowerCase())) {
      category = segs[i];
      creator = segs[i - 1];
      break;
    }
  }
  if (category === 'Misc' && segs.length >= 1) {
    creator = segs[segs.length - 1];
  }

  const type = TYPE_BY_CATEGORY[category.toLowerCase()] ?? 'Miniature';

  const prefix = `${creator} - `;
  let name = base.toLowerCase().startsWith(prefix.toLowerCase()) ? base.slice(prefix.length) : base;
  name = name.trim() || base;

  return { creator, category, type, name };
}
