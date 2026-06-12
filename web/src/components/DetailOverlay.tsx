import { useEffect, useState } from 'react';
import { useModel, usePins, api, useInvalidate } from '../api/client';
import { useUI } from '../store';
import { C, F, label } from '../theme';
import { ModelViewer } from './ModelViewer';
import { SpecTable } from './SpecTable';
import { PrinterCompat } from './PrinterCompat';
import { SettingsTable } from './SettingsTable';
import { GroupChips } from './GroupChips';
import { PrintedResults } from './PrintedResults';
import { fmtDate } from '../lib/format';

export function DetailOverlay() {
  const selectedId = useUI((s) => s.selectedId);
  const close = () => useUI.getState().open(null);
  const { data: model } = useModel(selectedId);
  const { data: pins = [] } = usePins();
  const invalidate = useInvalidate();
  const [tagDraft, setTagDraft] = useState('');

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  if (!selectedId || !model) return null;
  const creatorPinned = pins.some((p) => p.kind === 'creator' && p.name === model.creator);

  const addTag = async () => {
    const t = tagDraft.trim();
    if (!t) return;
    await api.addTag(model.id, t);
    setTagDraft('');
    invalidate(['model', 'models', 'tags', 'railTags']);
  };
  const removeTag = async (t: string) => { await api.removeTag(model.id, t); invalidate(['model', 'models', 'tags', 'railTags']); };
  const pinCreator = async () => { await api.togglePin('creator', model.creator); invalidate(['pins']); };

  return (
    <div onClick={close} style={{ position: 'fixed', inset: 0, background: 'rgba(6,6,8,0.74)', backdropFilter: 'blur(7px)', zIndex: 50, display: 'grid', placeItems: 'center', padding: 28 }}>
      <div onClick={(e) => e.stopPropagation()} style={{ width: 'min(1180px, 96vw)', height: 'min(790px, 92vh)', background: C.surface3, border: `1px solid ${C.border3}`, borderRadius: 16, overflow: 'hidden', display: 'grid', gridTemplateColumns: '1.05fr 1fr', boxShadow: '0 44px 90px rgba(0,0,0,0.65)' }}>
        <ModelViewer model={model} />

        <div style={{ overflowY: 'auto', padding: '24px 26px 30px', display: 'flex', flexDirection: 'column', gap: 19, minHeight: 0 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 14 }}>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontSize: 21, fontWeight: 750, lineHeight: 1.15, letterSpacing: '-0.01em' }}>{model.name}</div>
              <div style={{ marginTop: 6, fontSize: 12.5, color: C.textMute, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <span>by {model.creator} · added {fmtDate(model.added)}</span>
                <button onClick={pinCreator} className="hover-cyan" style={{ background: 'none', border: `1px solid ${C.border4}`, color: '#b3b3ba', cursor: 'pointer', fontSize: 11, padding: '2px 9px', borderRadius: 99, display: 'inline-flex', alignItems: 'center', gap: 5 }}>{creatorPinned ? '★' : '☆'} creator</button>
              </div>
            </div>
            <button onClick={close} className="hover-cyan" style={{ background: C.surfaceInput, border: `1px solid ${C.border3}`, color: '#b3b3ba', width: 30, height: 30, borderRadius: 8, cursor: 'pointer', fontSize: 13, flex: '0 0 auto', display: 'grid', placeItems: 'center' }}>✕</button>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            <div style={label}>TAGS</div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
              {model.tags.map((t) => (
                <span key={t} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, padding: '4px 10px', borderRadius: 99, fontSize: 11, fontFamily: F.mono, color: C.textDim, background: C.surfaceInput, border: '1px solid #303036' }}>
                  {t}<button onClick={() => removeTag(t)} style={{ background: 'none', border: 'none', color: C.textMute2, cursor: 'pointer', padding: 0, fontSize: 11, lineHeight: 1 }}>✕</button>
                </span>
              ))}
              <input value={tagDraft} onChange={(e) => setTagDraft(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') addTag(); }} placeholder="+ add tag ↵" className="hover-cyan" style={{ background: 'transparent', border: '1px dashed #36363c', borderRadius: 99, color: C.text, fontSize: 11, fontFamily: F.mono, padding: '4px 11px', outline: 'none', width: 110 }} />
            </div>
          </div>

          <SpecTable model={model} />
          <PrinterCompat model={model} />
          <SettingsTable model={model} />
          <GroupChips model={model} />
          <PrintedResults model={model} />
        </div>
      </div>
    </div>
  );
}
