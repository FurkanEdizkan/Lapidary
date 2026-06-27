// Real build-plate presets (mm), à la OrcaSlicer's printer beds. Pure data + helpers;
// no schema change — the model's compatible printers are name-only, so we match by name.

export interface PlatePreset {
  id: string;
  name: string;
  shape: 'rect' | 'circle';
  widthMm?: number;
  depthMm?: number;
  diameterMm?: number;
}

/** A concrete plate size the viewer can render, or `'auto'` (fit ~1.5× the model). */
export type ResolvedPlate =
  | { shape: 'rect'; widthMm: number; depthMm: number }
  | { shape: 'circle'; diameterMm: number }
  | 'auto';

export const PLATE_PRESETS: PlatePreset[] = [
  { id: 'bambu-256', name: 'Bambu X1C / P1S / A1', shape: 'rect', widthMm: 256, depthMm: 256 },
  { id: 'bambu-a1mini', name: 'Bambu A1 mini', shape: 'rect', widthMm: 180, depthMm: 180 },
  { id: 'prusa-mk', name: 'Prusa MK4 / MK3S', shape: 'rect', widthMm: 250, depthMm: 210 },
  { id: 'prusa-mini', name: 'Prusa MINI', shape: 'rect', widthMm: 180, depthMm: 180 },
  { id: 'prusa-xl', name: 'Prusa XL', shape: 'rect', widthMm: 360, depthMm: 360 },
  { id: 'ender3', name: 'Creality Ender 3', shape: 'rect', widthMm: 220, depthMm: 220 },
  { id: 'cr10', name: 'Creality CR-10', shape: 'rect', widthMm: 300, depthMm: 300 },
  { id: 'voron-350', name: 'Voron 2.4 / Trident 350', shape: 'rect', widthMm: 350, depthMm: 350 },
  { id: 'anycubic-max', name: 'Anycubic Kobra Max', shape: 'rect', widthMm: 420, depthMm: 420 },
  { id: 'elegoo-resin', name: 'Elegoo Saturn (resin)', shape: 'rect', widthMm: 219, depthMm: 123 },
  { id: 'delta-250', name: 'Delta (Ø250)', shape: 'circle', diameterMm: 250 },
];

// Printer-name → preset-id rules (first match wins; order matters — specific before generic).
const MATCH: { re: RegExp; id: string }[] = [
  { re: /bambu/i, id: 'bambu-256' },
  { re: /prusa.*xl/i, id: 'prusa-xl' },
  { re: /prusa.*mini/i, id: 'prusa-mini' },
  { re: /prusa/i, id: 'prusa-mk' },
  { re: /ender\s*-?\s*3/i, id: 'ender3' },
  { re: /cr-?\s*10/i, id: 'cr10' },
  { re: /voron/i, id: 'voron-350' },
  { re: /(kobra|anycubic).*max/i, id: 'anycubic-max' },
  { re: /(saturn|mars|elegoo|resin|mono\b)/i, id: 'elegoo-resin' },
  { re: /(delta|kossel|flsun)/i, id: 'delta-250' },
];

export function presetById(id: string): PlatePreset | undefined {
  return PLATE_PRESETS.find((p) => p.id === id);
}

/** Map a printer name to a known bed preset id, or null if unrecognised. */
export function bedIdForPrinter(name: string): string | null {
  for (const m of MATCH) if (m.re.test(name)) return m.id;
  return null;
}

function toResolved(p: PlatePreset): ResolvedPlate {
  return p.shape === 'circle'
    ? { shape: 'circle', diameterMm: p.diameterMm! }
    : { shape: 'rect', widthMm: p.widthMm!, depthMm: p.depthMm! };
}

/**
 * Resolve the plate size to render for a model.
 * - `'follow-printer'` → the first of the model's printers with a known bed, else `'auto'`.
 * - `'auto'` → `'auto'` (viewer fits ~1.5× the model).
 * - `'custom'` → the entered W×D (valid > 0), else `'auto'`.
 * - any other id → that preset, else `'auto'`.
 */
export function resolvePlate(
  presetId: string,
  customPlate: { widthMm: number; depthMm: number } | undefined,
  modelPrinters: string[],
): ResolvedPlate {
  if (presetId === 'auto') return 'auto';
  if (presetId === 'custom') {
    if (customPlate && customPlate.widthMm > 0 && customPlate.depthMm > 0) {
      return { shape: 'rect', widthMm: customPlate.widthMm, depthMm: customPlate.depthMm };
    }
    return 'auto';
  }
  if (presetId === 'follow-printer') {
    for (const name of modelPrinters) {
      const id = bedIdForPrinter(name);
      const p = id ? presetById(id) : undefined;
      if (p) return toResolved(p);
    }
    return 'auto';
  }
  const p = presetById(presetId);
  return p ? toResolved(p) : 'auto';
}
