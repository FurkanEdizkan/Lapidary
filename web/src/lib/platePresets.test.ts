import { describe, it, expect } from 'vitest';
import { resolvePlate, bedIdForPrinter, presetById } from './platePresets';

describe('bedIdForPrinter', () => {
  it('matches common printers by name', () => {
    expect(bedIdForPrinter('Bambu P1S')).toBe('bambu-256');
    expect(bedIdForPrinter('Bambu Lab X1C')).toBe('bambu-256');
    expect(bedIdForPrinter('Prusa MK4')).toBe('prusa-mk');
    expect(bedIdForPrinter('Prusa XL')).toBe('prusa-xl');
    expect(bedIdForPrinter('Creality Ender 3 V2')).toBe('ender3');
    expect(bedIdForPrinter('Voron 2.4')).toBe('voron-350');
  });
  it('returns null for unknown printers', () => {
    expect(bedIdForPrinter('Some Random Printer')).toBeNull();
  });
});

describe('resolvePlate', () => {
  it('follow-printer picks the first matching printer bed', () => {
    expect(resolvePlate('follow-printer', undefined, ['Bambu P1S'])).toEqual({ shape: 'rect', widthMm: 256, depthMm: 256 });
    expect(resolvePlate('follow-printer', undefined, ['Unknown', 'Prusa MK4'])).toEqual({ shape: 'rect', widthMm: 250, depthMm: 210 });
  });
  it('follow-printer falls back to auto with no known printer', () => {
    expect(resolvePlate('follow-printer', undefined, [])).toBe('auto');
    expect(resolvePlate('follow-printer', undefined, ['Mystery Box'])).toBe('auto');
  });
  it('resolves a concrete preset id', () => {
    expect(resolvePlate('ender3', undefined, [])).toEqual({ shape: 'rect', widthMm: 220, depthMm: 220 });
    expect(resolvePlate('delta-250', undefined, [])).toEqual({ shape: 'circle', diameterMm: 250 });
  });
  it('resolves custom dimensions, falling back to auto when invalid', () => {
    expect(resolvePlate('custom', { widthMm: 200, depthMm: 150 }, [])).toEqual({ shape: 'rect', widthMm: 200, depthMm: 150 });
    expect(resolvePlate('custom', undefined, [])).toBe('auto');
    expect(resolvePlate('custom', { widthMm: 0, depthMm: 150 }, [])).toBe('auto');
  });
  it('auto resolves to auto', () => {
    expect(resolvePlate('auto', undefined, ['Bambu P1S'])).toBe('auto');
  });
  it('unknown preset id falls back to auto', () => {
    expect(resolvePlate('nope', undefined, [])).toBe('auto');
  });
  it('presetById finds a preset', () => {
    expect(presetById('bambu-256')?.widthMm).toBe(256);
  });
});
