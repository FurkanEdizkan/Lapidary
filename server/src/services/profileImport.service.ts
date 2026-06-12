/**
 * Parse a slicer profile file into normalized settings rows. Supports:
 *  - PrusaSlicer / OrcaSlicer / SuperSlicer `.ini` (key = value lines)
 *  - Cura `.json` (flat or { settings: { key: { default_value } } } shapes)
 * Only a curated set of human-relevant keys is surfaced; the rest is kept in raw_json.
 */
export interface ParsedProfile {
  rows: { k: string; v: string }[];
  raw: Record<string, string>;
}

// Map of canonical label -> candidate source keys across slicers.
const KEY_MAP: { label: string; keys: string[] }[] = [
  { label: 'Layer height', keys: ['layer_height'] },
  { label: 'First layer height', keys: ['first_layer_height', 'initial_layer_height'] },
  { label: 'Infill', keys: ['fill_density', 'infill_sparse_density'] },
  { label: 'Infill pattern', keys: ['fill_pattern', 'infill_pattern'] },
  { label: 'Perimeters', keys: ['perimeters', 'wall_loops', 'wall_line_count'] },
  { label: 'Nozzle temp', keys: ['temperature', 'nozzle_temperature', 'material_print_temperature'] },
  { label: 'Bed temp', keys: ['bed_temperature', 'material_bed_temperature'] },
  { label: 'Supports', keys: ['support_material', 'support_enable', 'enable_support'] },
  { label: 'Nozzle', keys: ['nozzle_diameter', 'machine_nozzle_size'] },
  { label: 'Material', keys: ['filament_type', 'material'] },
  { label: 'Speed', keys: ['perimeter_speed', 'speed_print', 'speed_wall_0'] },
];

export function parseProfile(filename: string, content: string): ParsedProfile {
  const raw = filename.toLowerCase().endsWith('.json') ? parseJson(content) : parseIni(content);
  const rows: { k: string; v: string }[] = [];
  const used = new Set<string>();
  for (const { label, keys } of KEY_MAP) {
    for (const key of keys) {
      if (raw[key] != null && raw[key] !== '') {
        rows.push({ k: label, v: formatValue(label, raw[key]) });
        used.add(key);
        break;
      }
    }
  }
  if (rows.length === 0) {
    // Nothing matched our curated keys — surface the first handful so the import is visible.
    for (const [k, v] of Object.entries(raw).slice(0, 6)) rows.push({ k, v: String(v) });
  }
  return { rows, raw };
}

function parseIni(content: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#') || trimmed.startsWith(';') || trimmed.startsWith('[')) continue;
    const eq = trimmed.indexOf('=');
    if (eq <= 0) continue;
    out[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
  }
  return out;
}

function parseJson(content: string): Record<string, string> {
  const out: Record<string, string> = {};
  try {
    const obj = JSON.parse(content);
    const settings = obj.settings ?? obj.overrides ?? obj;
    walk(settings, out);
  } catch {
    /* ignore malformed json */
  }
  return out;
}

function walk(node: unknown, out: Record<string, string>): void {
  if (!node || typeof node !== 'object') return;
  for (const [k, v] of Object.entries(node as Record<string, unknown>)) {
    if (v && typeof v === 'object') {
      if ('default_value' in (v as object)) out[k] = String((v as { default_value: unknown }).default_value);
      else if ('value' in (v as object)) out[k] = String((v as { value: unknown }).value);
      else walk(v, out);
    } else if (v != null) {
      out[k] = String(v);
    }
  }
}

function formatValue(label: string, value: string): string {
  if (label === 'Infill' && /^\d+(\.\d+)?$/.test(value)) return `${value} %`;
  if ((label === 'Supports') && /^(0|1|true|false)$/i.test(value)) {
    return /^(1|true)$/i.test(value) ? 'Enabled' : 'None needed';
  }
  return value;
}
