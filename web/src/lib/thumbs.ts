import { useEffect, useState } from 'react';
import type { Model } from '../api/client';
import { api } from '../api/client';
import { proxyThumbnail } from './mesh3d';
import { thumbUrl } from './format';

// In-memory cache of generated proxy thumbnails, keyed by meshKind+color, so the
// gallery generates each shape at most once per session.
const memo = new Map<string, string>();
const posted = new Set<string>();

function generate(model: Model): string | null {
  const key = `${model.meshKind}:${model.color}`;
  if (memo.has(key)) return memo.get(key)!;
  const url = proxyThumbnail(model.meshKind || 'egg', model.color);
  if (url) {
    memo.set(key, url);
    // Persist once so reloads / other clients get a server-cached PNG (Tier 1).
    if (!posted.has(model.id)) {
      posted.add(model.id);
      api.saveThumbnail(model.id, url).catch(() => undefined);
    }
  }
  return url;
}

/** Resolve a tile's image source: server PNG if cached, else a client proxy render. */
export function useThumbnail(model: Model): string | null {
  const [src, setSrc] = useState<string | null>(model.hasThumbnail ? thumbUrl(model.id) : null);
  useEffect(() => {
    if (model.hasThumbnail) {
      setSrc(thumbUrl(model.id));
      return;
    }
    if (model.meshKind) {
      // defer to next tick so first paint isn't blocked by canvas work
      const t = setTimeout(() => setSrc(generate(model)), 0);
      return () => clearTimeout(t);
    }
    setSrc(null);
  }, [model.id, model.hasThumbnail, model.meshKind, model.color]);
  return src;
}
