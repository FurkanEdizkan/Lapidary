import type { Model } from '../api/client';
import { useThumbnail, isRenderingThumb } from '../lib/thumbs';
import { sizeShort, tint } from '../lib/format';
import { C, F } from '../theme';
import { useUI } from '../store';

/** Square, image-first grid tile. Hover reveals name/creator/format/size. */
export function ModelTile({ model }: { model: Model }) {
  const thumb = useThumbnail(model);
  const open = useUI((s) => s.open);
  const grad = `radial-gradient(circle at 50% 42%, ${tint(model.color, 0.3)} 0%, #17171a 82%)`;
  const rendering = !thumb && isRenderingThumb(model);

  return (
    <div
      className="tile"
      onClick={() => open(model.id)}
      style={{ position: 'relative', aspectRatio: '1 / 1', borderRadius: 8, overflow: 'hidden', cursor: 'pointer', background: C.surface }}
    >
      <div
        style={{
          position: 'absolute', inset: 0,
          backgroundImage: thumb ? `url(${thumb}), ${grad}` : grad,
          backgroundSize: thumb ? '88%, cover' : 'cover',
          backgroundPosition: 'center', backgroundRepeat: 'no-repeat',
        }}
      />
      {rendering && (
        <div className="thumb-rendering">
          <span className="thumb-rendering-label">Rendering…</span>
        </div>
      )}
      <div
        className="tile-overlay"
        style={{
          position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', justifyContent: 'flex-end',
          padding: 14, background: 'linear-gradient(to top, rgba(8,8,10,0.92) 0%, rgba(8,8,10,0.4) 30%, rgba(8,8,10,0) 58%)',
        }}
      >
        <div style={{ fontWeight: 650, fontSize: 14, lineHeight: 1.25 }}>{model.name}</div>
        <div style={{ fontSize: 11.5, color: C.textDim, marginTop: 3 }}>{model.creator} · {model.type}</div>
        <div style={{ display: 'flex', gap: 6, marginTop: 8, fontFamily: F.mono, fontSize: 9, letterSpacing: '0.08em', color: C.textMute }}>
          <span style={{ background: 'rgba(28,28,32,0.85)', border: '1px solid #36363c', padding: '2px 7px', borderRadius: 5 }}>{model.format}</span>
          <span style={{ background: 'rgba(28,28,32,0.85)', border: '1px solid #36363c', padding: '2px 7px', borderRadius: 5 }}>{sizeShort(model)} MM</span>
        </div>
      </div>
    </div>
  );
}
