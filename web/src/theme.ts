/** Design tokens lifted verbatim from `design/Manifold Print Library.dc.html`. */
export const C = {
  page: '#121214',
  nav: '#0d0d0e',
  surface: '#1a1a1d',
  surface2: '#1d1d20',
  surface3: '#18181b',
  surfaceInput: '#212125',
  border: '#26262a',
  border2: '#2c2c30',
  border3: '#2e2e34',
  border4: '#34343a',
  borderHover: '#4a4a52',
  text: '#eaeaec',
  textDim: '#c9c9cf',
  textMute: '#9b9ba1',
  textMute2: '#82828a',
  textFaint: '#6c6c73',
  textFaint2: '#75757d',
  accent: '#2cb4f5',
  accentHover: '#4cc3ff',
  onAccent: '#06151d',
  danger: '#e07a7a',
  success: '#6fd394',
} as const;

export const F = {
  sans: "'Archivo', sans-serif",
  mono: "'JetBrains Mono', monospace",
} as const;

/** Pill chip style (active = cyan fill). Matches `tagChip`/`barChip`/`railChip`. */
export function chip(active: boolean, variant: 'tag' | 'bar' | 'rail' = 'tag'): React.CSSProperties {
  if (variant === 'rail') {
    return {
      flex: '0 0 auto', padding: '7px 16px', borderRadius: 99, fontSize: 12.5, fontWeight: 600,
      cursor: 'pointer', lineHeight: 1.4,
      color: active ? C.onAccent : C.textDim, background: active ? C.accent : C.surface2,
      border: `1px solid ${active ? C.accent : C.border2}`,
    };
  }
  if (variant === 'bar') {
    return {
      display: 'inline-flex', alignItems: 'center', gap: 6, maxWidth: 240,
      padding: '5px 12px', borderRadius: 99, fontSize: 12, fontWeight: 600, lineHeight: 1.5,
      color: active ? C.onAccent : C.textDim, background: active ? C.accent : C.surface2,
      border: `1px solid ${active ? C.accent : C.border2}`,
    };
  }
  return {
    display: 'inline-flex', alignItems: 'center', gap: 5, padding: '3px 10px', borderRadius: 99,
    fontSize: 11, cursor: 'pointer', fontFamily: F.mono, lineHeight: 1.6,
    color: active ? C.onAccent : '#b3b3ba', background: active ? C.accent : C.surfaceInput,
    border: `1px solid ${active ? C.accent : '#303036'}`,
  };
}

export const label: React.CSSProperties = {
  fontFamily: F.mono, fontSize: 9.5, letterSpacing: '0.2em', color: C.textFaint,
};
