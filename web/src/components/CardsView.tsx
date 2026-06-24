import type { Model } from '../api/client';
import { useThumbnail, isRenderingThumb } from '../lib/thumbs';
import { sizeShort, fmtDate, fileMB } from '../lib/format';
import { C, F, chip } from '../theme';
import { useUI } from '../store';

function Card({ model }: { model: Model }) {
  const thumb = useThumbnail(model);
  const open = useUI((s) => s.open);
  const toggleTag = useUI((s) => s.toggleTag);
  const activeTags = useUI((s) => s.activeTags);
  const rendering = !thumb && isRenderingThumb(model);
  return (
    <div
      className="card-hover"
      onClick={() => open(model.id)}
      style={{ background: C.surface, border: `1px solid ${C.border}`, borderRadius: 13, overflow: 'hidden', cursor: 'pointer', display: 'flex', flexDirection: 'column' }}
    >
      <div style={{ height: 212, background: 'radial-gradient(circle at 50% 40%, #232328 0%, #18181b 75%)', position: 'relative', display: 'grid', placeItems: 'center' }}>
        {thumb && <div style={{ width: '100%', height: '100%', backgroundImage: `url(${thumb})`, backgroundSize: 'contain', backgroundPosition: 'center', backgroundRepeat: 'no-repeat' }} />}
        {rendering && (
          <div className="thumb-rendering" style={{ borderRadius: 0 }}>
            <span className="thumb-rendering-label">Rendering…</span>
          </div>
        )}
        <div style={{ position: 'absolute', top: 10, left: 10, fontFamily: F.mono, fontSize: 9, letterSpacing: '0.1em', color: C.textMute, background: 'rgba(13,13,15,0.72)', border: '1px solid #2e2e34', padding: '2px 6px', borderRadius: 5 }}>{model.format}</div>
        <div style={{ position: 'absolute', top: 10, right: 10, fontFamily: F.mono, fontSize: 9, letterSpacing: '0.1em', color: C.textMute, background: 'rgba(13,13,15,0.72)', border: '1px solid #2e2e34', padding: '2px 6px', borderRadius: 5 }}>{sizeShort(model)} mm</div>
      </div>
      <div style={{ padding: '13px 15px 15px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        <div>
          <div style={{ fontWeight: 650, fontSize: 15, lineHeight: 1.25 }}>{model.name}</div>
          <div style={{ fontSize: 12, color: C.textMute, marginTop: 2 }}>{model.creator} · {model.type}</div>
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
          {model.tags.map((t) => (
            <button key={t} onClick={(e) => { e.stopPropagation(); toggleTag(t); }} className="hover-cyan" style={chip(activeTags.includes(t))}>{t}</button>
          ))}
        </div>
        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, fontFamily: F.mono, fontSize: 10, color: C.textFaint2, borderTop: `1px solid ${C.border}`, paddingTop: 9 }}>
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.printers.join(' · ') || fileMB(model.fileSizeBytes)}</span>
          <span style={{ flex: '0 0 auto' }}>{fmtDate(model.added)}</span>
        </div>
      </div>
    </div>
  );
}

export function CardsView({ models }: { models: Model[] }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(312px, 1fr))', gap: 18 }}>
      {models.map((m) => <Card key={m.id} model={m} />)}
    </div>
  );
}
