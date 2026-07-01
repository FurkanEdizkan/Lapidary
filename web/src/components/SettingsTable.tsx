import { useRef, useState } from 'react';
import type { ModelDetail } from '../api/client';
import { api, useInvalidate } from '../api/client';
import { C, F, label } from '../theme';

/** Suggested print settings: editable rows + slicer-profile import. */
export function SettingsTable({ model }: { model: ModelDetail }) {
  const invalidate = useInvalidate();
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [k, setK] = useState('');
  const [v, setV] = useState('');
  const [imported, setImported] = useState<string | null>(null);

  const add = async () => {
    if (!k.trim()) return;
    await api.addSetting(model.id, k.trim(), v.trim());
    setK(''); setV('');
    invalidate(['model']);
  };
  const del = async (id: number) => { await api.deleteSetting(id); invalidate(['model']); };
  const onImport = async (file: File) => {
    const res = await api.importProfile(model.id, file);
    setImported(res.imported);
    invalidate(['model']);
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div style={label}>SUGGESTED PRINT SETTINGS</div>
        <button onClick={() => fileRef.current?.click()} className="hover-cyan" style={{ background: 'transparent', border: `1px solid ${C.border4}`, color: C.textDim, borderRadius: 7, padding: '5px 11px', fontSize: 11, fontWeight: 600, cursor: 'pointer', fontFamily: F.mono }}>⇪ Import from slicer preset</button>
        <input ref={fileRef} type="file" accept=".ini,.json,.cfg" style={{ display: 'none' }} onChange={(e) => { const f = e.target.files?.[0]; if (f) onImport(f); e.target.value = ''; }} />
      </div>
      {imported && <div style={{ fontFamily: F.mono, fontSize: 10, color: C.success }}>✓ imported {imported}</div>}
      <div style={{ background: '#1c1c20', border: '1px solid #2a2a2e', borderRadius: 10, padding: '4px 14px' }}>
        {model.settings.map((r) => (
          <div key={r.id} style={{ display: 'grid', gridTemplateColumns: '140px 1fr 18px', gap: 10, padding: '7px 0', borderBottom: '1px solid #242428', fontSize: 12, alignItems: 'center' }}>
            <span style={{ color: '#8b8b93' }}>{r.k}</span>
            <span style={{ color: '#e0e0e5', fontFamily: F.mono, fontSize: 11 }}>{r.v}</span>
            <button onClick={() => del(r.id)} title="Remove" style={{ background: 'none', border: 'none', color: C.textFaint, cursor: 'pointer', fontSize: 11 }} className="hover-cyan">✕</button>
          </div>
        ))}
        {model.settings.length === 0 && <div style={{ padding: '8px 0', fontSize: 11.5, color: C.textFaint }}>No settings yet — add one or import a preset.</div>}
        <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr auto', gap: 8, padding: '8px 0 4px', alignItems: 'center' }}>
          <input value={k} onChange={(e) => setK(e.target.value)} placeholder="Setting" style={miniInput} />
          <input value={v} onChange={(e) => setV(e.target.value)} placeholder="Value" onKeyDown={(e) => { if (e.key === 'Enter') add(); }} style={{ ...miniInput, fontFamily: F.mono }} />
          <button onClick={add} style={{ background: C.accent, border: 'none', color: C.onAccent, borderRadius: 6, padding: '6px 12px', fontSize: 11, fontWeight: 700, cursor: 'pointer' }}>Add</button>
        </div>
      </div>
    </div>
  );
}

const miniInput: React.CSSProperties = {
  background: C.surface2, border: `1px solid ${C.border2}`, borderRadius: 6, color: C.text, fontSize: 11, padding: '6px 9px', outline: 'none',
};
