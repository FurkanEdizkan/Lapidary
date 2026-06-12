import { useRef, useState } from 'react';
import { usePrinterTypes, api, useInvalidate } from '../api/client';
import { useUI } from '../store';
import { C, F, chip } from '../theme';

const MESHES = [
  ['egg', 'Egg / organic'], ['pawn', 'Figure / idol'], ['vase', 'Vase — smooth'], ['facetVase', 'Vase — faceted'],
  ['gear', 'Gear'], ['diceTower', 'Tower'], ['hexTile', 'Hex tile'], ['crystals', 'Crystal cluster'], ['bracket', 'Bracket'], ['tray', 'Tray / box'],
];
const TYPES = ['Miniature', 'Terrain', 'Functional', 'Display', 'Tool'];

export function AddModelModal() {
  const show = useUI((s) => s.showAdd);
  const close = () => useUI.getState().setShowAdd(false);
  const open = useUI((s) => s.open);
  const { data: printers = [] } = usePrinterTypes();
  const invalidate = useInvalidate();
  const fileRef = useRef<HTMLInputElement | null>(null);

  const [file, setFile] = useState<File | null>(null);
  const [form, setForm] = useState({ name: '', creator: '', type: 'Miniature', mesh: 'egg', sx: '', sy: '', sz: '', tags: '', settings: '' });
  const [chosen, setChosen] = useState<string[]>(['Bambu X1C', 'Bambu P1S']);
  const [busy, setBusy] = useState(false);

  if (!show) return null;
  const set = (k: string, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const togglePrinter = (p: string) => setChosen((c) => (c.includes(p) ? c.filter((x) => x !== p) : [...c, p]));

  // File path: upload the real mesh through the asset pipeline (multipart).
  const submit = async () => {
    if (!file) return;
    setBusy(true);
    try {
      const fd = new FormData();
      fd.append('file', file);
      fd.append('name', form.name || file.name.replace(/\.[^.]+$/, ''));
      fd.append('creator', form.creator || 'You');
      fd.append('type', form.type);
      fd.append('tags', form.tags);
      fd.append('printers', chosen.join(','));
      fd.append('settings', form.settings);
      fd.append('sx', form.sx); fd.append('sy', form.sy); fd.append('sz', form.sz);
      const created = await api.uploadModel(fd);
      invalidate();
      close();
      reset();
      if (created?.id) open(created.id);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => { setFile(null); setForm({ name: '', creator: '', type: 'Miniature', mesh: 'egg', sx: '', sy: '', sz: '', tags: '', settings: '' }); setChosen(['Bambu X1C', 'Bambu P1S']); };

  // proxy submit via JSON endpoint
  const submitProxy = async () => {
    setBusy(true);
    try {
      const res = await fetch('/api/models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: form.name || 'Untitled model', creator: form.creator || 'You', type: form.type, meshKind: form.mesh,
          size: [Number(form.sx) || 50, Number(form.sy) || 50, Number(form.sz) || 50],
          tags: form.tags.split(',').map((t) => t.trim().toLowerCase().replace(/\s+/g, '-')).filter(Boolean),
          printers: chosen,
          settings: form.settings.split('\n').map((l) => l.trim()).filter(Boolean).map((line) => {
            const i = line.indexOf(':');
            return i > 0 ? [line.slice(0, i).trim(), line.slice(i + 1).trim()] : ['Note', line];
          }),
        }),
      });
      const created = await res.json();
      invalidate();
      close();
      reset();
      if (created?.id) open(created.id);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div onClick={close} style={modalWrap}>
      <div onClick={(e) => e.stopPropagation()} style={{ ...modalBox, width: 'min(620px, 94vw)' }}>
        <Header title="Add model to library" onClose={close} />

        <div
          onClick={() => fileRef.current?.click()}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => { e.preventDefault(); const f = e.dataTransfer.files?.[0]; if (f) setFile(f); }}
          className="hover-cyan"
          style={{ border: `1.5px dashed ${C.border4}`, borderRadius: 12, padding: 22, textAlign: 'center', color: C.textMute2, fontSize: 12.5, cursor: 'pointer', background: '#1c1c20' }}
        >
          {file ? (
            <>Selected <span style={{ color: C.accent, fontFamily: F.mono }}>{file.name}</span></>
          ) : (
            <>Drop an <span style={{ color: C.accent, fontFamily: F.mono }}>.stl · .3mf · .obj</span> file here, or click to browse</>
          )}
          <div style={{ fontFamily: F.mono, fontSize: 10, marginTop: 6, color: C.textFaint }}>or pick a preview shape below and add without a file</div>
          <input ref={fileRef} type="file" accept=".stl,.3mf,.obj" style={{ display: 'none' }} onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <Field label="Model name"><input value={form.name} onChange={(e) => set('name', e.target.value)} placeholder="e.g. Wyvern Rider" style={inp} /></Field>
          <Field label="Creator"><input value={form.creator} onChange={(e) => set('creator', e.target.value)} placeholder="e.g. Forge & Fathom" style={inp} /></Field>
          <Field label="Type">
            <select value={form.type} onChange={(e) => set('type', e.target.value)} style={inp}>{TYPES.map((t) => <option key={t} value={t}>{t}</option>)}</select>
          </Field>
          <Field label="Preview shape">
            <select value={form.mesh} onChange={(e) => set('mesh', e.target.value)} disabled={!!file} style={{ ...inp, opacity: file ? 0.5 : 1 }}>{MESHES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
          </Field>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <Field label="Width (mm)"><input value={form.sx} onChange={(e) => set('sx', e.target.value)} placeholder="50" style={{ ...inp, fontFamily: F.mono }} /></Field>
          <Field label="Depth (mm)"><input value={form.sy} onChange={(e) => set('sy', e.target.value)} placeholder="50" style={{ ...inp, fontFamily: F.mono }} /></Field>
          <Field label="Height (mm)"><input value={form.sz} onChange={(e) => set('sz', e.target.value)} placeholder="50" style={{ ...inp, fontFamily: F.mono }} /></Field>
        </div>

        <Field label="Tags (comma separated)"><input value={form.tags} onChange={(e) => set('tags', e.target.value)} placeholder="dragon, miniature, warhammer" style={{ ...inp, fontFamily: F.mono }} /></Field>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
          <div style={fieldLabel}>Compatible printers</div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {printers.map((p) => <button key={p} onClick={() => togglePrinter(p)} className="hover-cyan" style={chip(chosen.includes(p))}>{p}</button>)}
          </div>
        </div>

        <Field label="Suggested print settings">
          <textarea value={form.settings} onChange={(e) => set('settings', e.target.value)} rows={4} placeholder={'Layer height: 0.16 mm\nInfill: 15 % gyroid\nSupports: tree (auto)'} style={{ ...inp, fontFamily: F.mono, resize: 'vertical', lineHeight: 1.6 }} />
        </Field>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 2 }}>
          <button onClick={close} className="hover-cyan" style={{ background: 'transparent', border: `1px solid ${C.border4}`, color: C.textDim, borderRadius: 9, padding: '9px 18px', fontSize: 13, fontWeight: 600, cursor: 'pointer' }}>Cancel</button>
          <button disabled={busy} onClick={file ? submit : submitProxy} style={{ background: C.accent, border: `1px solid ${C.accent}`, color: C.onAccent, borderRadius: 9, padding: '9px 20px', fontSize: 13, fontWeight: 700, cursor: 'pointer', opacity: busy ? 0.6 : 1 }}>{busy ? 'Adding…' : 'Add to library'}</button>
        </div>
      </div>
    </div>
  );
}

export const modalWrap: React.CSSProperties = { position: 'fixed', inset: 0, background: 'rgba(6,6,8,0.74)', backdropFilter: 'blur(7px)', zIndex: 60, display: 'grid', placeItems: 'center', padding: 28 };
export const modalBox: React.CSSProperties = { maxHeight: '92vh', overflowY: 'auto', background: C.surface3, border: `1px solid ${C.border3}`, borderRadius: 16, padding: '26px 28px 28px', display: 'flex', flexDirection: 'column', gap: 18, boxShadow: '0 44px 90px rgba(0,0,0,0.65)' };
const inp: React.CSSProperties = { background: C.surface2, border: `1px solid ${C.border2}`, borderRadius: 8, color: C.text, fontSize: 13, padding: '9px 11px', outline: 'none', width: '100%' };
const fieldLabel: React.CSSProperties = { fontSize: 11, color: C.textMute, fontWeight: 600 };

export function Header({ title, onClose }: { title: string; onClose: () => void }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
      <div style={{ fontSize: 19, fontWeight: 750 }}>{title}</div>
      <button onClick={onClose} className="hover-cyan" style={{ background: C.surfaceInput, border: `1px solid ${C.border3}`, color: '#b3b3ba', width: 30, height: 30, borderRadius: 8, cursor: 'pointer', fontSize: 13, display: 'grid', placeItems: 'center' }}>✕</button>
    </div>
  );
}
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label style={{ display: 'flex', flexDirection: 'column', gap: 5, ...fieldLabel }}>{label}{children}</label>;
}
