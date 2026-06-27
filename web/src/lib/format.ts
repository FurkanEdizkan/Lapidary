import type { Model } from '../api/client';

export function fmtDate(d: string | null): string {
  if (!d) return '—';
  try {
    return new Date(d + 'T00:00:00').toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
  } catch {
    return d;
  }
}

export function sizeShort(m: Model): string {
  return `${m.size[0]}×${m.size[1]}×${m.size[2]}`;
}
export function sizeLabel(m: Model): string {
  return `${m.size[0]} × ${m.size[1]} × ${m.size[2]} mm`;
}
export function fileMB(bytes: number): string {
  return `${(bytes / (1024 * 1024) || 0).toFixed(1)} MB`;
}

/** Tint a hex color toward black by factor f (0..1), matching the prototype's tileBg. */
export function tint(hex: string, f: number): string {
  try {
    const h = hex.replace('#', '');
    const r = Math.round(parseInt(h.slice(0, 2), 16) * f);
    const g = Math.round(parseInt(h.slice(2, 4), 16) * f);
    const b = Math.round(parseInt(h.slice(4, 6), 16) * f);
    return `rgb(${r},${g},${b})`;
  } catch {
    return '#1f1f23';
  }
}

/**
 * URL of a model's cached thumbnail (server PNG) — only meaningful when hasThumbnail.
 * `version` (the server-provided `thumbVersion`, a file mtime) is appended as `?v=` so
 * the browser re-fetches whenever the PNG is regenerated (the endpoint sends a 1-day
 * Cache-Control, so the byte-identical URL would otherwise show a stale/blank image).
 */
export function thumbUrl(id: string, version?: number): string {
  const base = `/api/models/${id}/thumbnail`;
  return version ? `${base}?v=${version}` : base;
}
