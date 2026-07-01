import type { Model } from '../api/client';
import { useThumbnail, isRenderingThumb } from '../lib/thumbs';
import { sizeShort, fmtDate, fileMB } from '../lib/format';
import { C, F } from '../theme';
import { useUI } from '../store';

const COLS = '56px 2.2fr 1.2fr 0.8fr 1fr 1.8fr 0.9fr';

function Row({ model }: { model: Model }) {
  const thumb = useThumbnail(model);
  const open = useUI((s) => s.open);
  const rendering = !thumb && isRenderingThumb(model);
  return (
    <div
      className="list-row"
      onClick={() => open(model.id)}
      style={{ display: 'grid', gridTemplateColumns: COLS, gap: 12, alignItems: 'center', padding: '8px 16px', borderBottom: '1px solid #222226', cursor: 'pointer', background: '#161619' }}
    >
      <div style={{ width: 44, height: 40, borderRadius: 7, background: '#1d1d21', border: '1px solid #2a2a2e', display: 'grid', placeItems: 'center', overflow: 'hidden', position: 'relative' }}>
        {thumb && <div style={{ width: '100%', height: '100%', backgroundImage: `url(${thumb})`, backgroundSize: 'contain', backgroundPosition: 'center', backgroundRepeat: 'no-repeat' }} />}
        {rendering && <div className="thumb-rendering" style={{ borderRadius: 7 }} />}
      </div>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontWeight: 600, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.name}</div>
        <div style={{ fontFamily: F.mono, fontSize: 9.5, color: C.textFaint }}>{model.format} · {fileMB(model.fileSizeBytes)}</div>
      </div>
      <div style={{ fontSize: 12, color: '#b3b3ba', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.creator}</div>
      <div style={{ fontSize: 12, color: '#b3b3ba' }}>{model.type}</div>
      <div style={{ fontFamily: F.mono, fontSize: 10.5, color: C.textMute }}>{sizeShort(model)}</div>
      <div style={{ fontFamily: F.mono, fontSize: 10, color: C.textMute2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.tags.join(', ')}</div>
      <div style={{ fontFamily: F.mono, fontSize: 10, color: C.textMute2, textAlign: 'right' }}>{fmtDate(model.added)}</div>
    </div>
  );
}

export function ListView({ models }: { models: Model[] }) {
  return (
    <div style={{ border: `1px solid ${C.border}`, borderRadius: 12, overflow: 'hidden' }}>
      <div style={{ display: 'grid', gridTemplateColumns: COLS, gap: 12, alignItems: 'center', padding: '9px 16px', background: '#19191c', borderBottom: `1px solid ${C.border}`, fontFamily: F.mono, fontSize: 9.5, letterSpacing: '0.14em', color: C.textFaint }}>
        <span /><span>MODEL</span><span>CREATOR</span><span>TYPE</span><span>SIZE (MM)</span><span>TAGS</span><span style={{ textAlign: 'right' }}>ADDED</span>
      </div>
      {models.map((m) => <Row key={m.id} model={m} />)}
    </div>
  );
}
