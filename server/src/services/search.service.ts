import { getDb } from '../db/database.js';
import { listModels } from './model.service.js';

/**
 * Search suggestions for the top-nav dropdown. Mirrors the prototype's logic:
 * with an empty query, return popular tags; with a query, return matching
 * models, creators (with counts), and tag suggestions drawn from matched models.
 */
export interface Suggestions {
  models: { id: string; name: string; creator: string; type: string }[];
  creators: { name: string; count: number }[];
  tags: { name: string; count: number }[];
  tagHeader: string;
}

export function suggest(rawQuery: string): Suggestions {
  const d = getDb();
  const q = (rawQuery || '').trim().toLowerCase();

  const tagCounts = countMap(d, 'SELECT t.name AS name, COUNT(*) AS n FROM model_tags mt JOIN tags t ON t.id = mt.tag_id GROUP BY t.name');
  const creatorCounts = countMap(d, 'SELECT creator AS name, COUNT(*) AS n FROM models GROUP BY creator');

  if (!q) {
    const tags = topByCount(tagCounts).slice(0, 9);
    return { models: [], creators: [], tags, tagHeader: 'POPULAR TAGS' };
  }

  const hits = listModels({ search: q });
  const models = hits.slice(0, 5).map((m) => ({ id: m.id, name: m.name, creator: m.creator, type: m.type }));

  const creatorSet = new Map<string, number>();
  for (const c of creatorCounts.keys()) if (c.toLowerCase().includes(q)) creatorSet.set(c, creatorCounts.get(c)!);
  for (const m of hits) creatorSet.set(m.creator, creatorCounts.get(m.creator) ?? 0);
  const creators = [...creatorSet.entries()].slice(0, 4).map(([name, count]) => ({ name, count }));

  const tagSet = new Map<string, number>();
  for (const t of tagCounts.keys()) if (t.includes(q)) tagSet.set(t, tagCounts.get(t)!);
  for (const m of hits) for (const t of m.tags) tagSet.set(t, tagCounts.get(t) ?? 0);
  const tags = [...tagSet.entries()].slice(0, 8).map(([name, count]) => ({ name, count }));

  return { models, creators, tags, tagHeader: 'TAGS & SUGGESTIONS' };
}

function countMap(d: ReturnType<typeof getDb>, sql: string): Map<string, number> {
  const rows = d.prepare(sql).all() as { name: string; n: number }[];
  const m = new Map<string, number>();
  for (const r of rows) m.set(r.name, r.n);
  return m;
}

function topByCount(m: Map<string, number>): { name: string; count: number }[] {
  return [...m.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([name, count]) => ({ name, count }));
}

/** Popular tags for the channel rail (most-used first, capped). */
export function railTags(limit = 12): { name: string; count: number }[] {
  const d = getDb();
  const counts = countMap(d, 'SELECT t.name AS name, COUNT(*) AS n FROM model_tags mt JOIN tags t ON t.id = mt.tag_id GROUP BY t.name');
  return topByCount(counts).slice(0, limit);
}
