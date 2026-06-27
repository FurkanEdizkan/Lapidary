import { useEffect, useRef, useState } from 'react';
import type { ModelDetail } from '../api/client';
import { api, useInvalidate } from '../api/client';
import { useThumbnail, isRenderingThumb } from '../lib/thumbs';
import { useUI } from '../store';
import { C, F } from '../theme';

const KINDS = ['printed', 'painted', 'reference'] as const;
type Kind = (typeof KINDS)[number];

/**
 * Static left pane for the detail overlay: a pre-rendered hero (the saved-orientation
 * thumbnail) + a gallery of the model's images, and an INSPECT button that opens the
 * live 3D viewer/editor. No WebGL is mounted here — the detail opens instantly.
 */
export function DetailPreview({ model }: { model: ModelDetail }) {
  const invalidate = useInvalidate();
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [heroId, setHeroId] = useState<'thumb' | number>('thumb');
  const [kind, setKind] = useState<Kind>('printed');

  // When the saved-view thumbnail is regenerated (thumbVersion changes), snap the hero
  // back to it so the user sees the new render/placement.
  useEffect(() => setHeroId('thumb'), [model.thumbVersion]);

  // Server PNG URL (cache-busted by thumbVersion) or proxy data URL or null.
  const thumb = useThumbnail(model);

  const heroImage = typeof heroId === 'number' ? model.images.find((i) => i.id === heroId) : undefined;

  const addPhoto = async (file: File) => {
    const img = await api.uploadImage(model.id, file, kind);
    invalidate(['model']);
    setHeroId(img.id);
  };

  return (
    <div style={{ position: 'relative', background: 'radial-gradient(ellipse at 50% 40%, #222227 0%, #131316 80%)', borderRight: `1px solid ${C.border}`, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
      {/* Hero */}
      <div style={{ position: 'relative', flex: 1, minHeight: 0, display: 'grid', placeItems: 'center', padding: 18 }}>
        {heroImage ? (
          <img src={heroImage.url} alt={heroImage.caption || 'photo'} style={heroImgStyle} />
        ) : thumb ? (
          <img src={thumb} alt={model.name} style={heroImgStyle} />
        ) : isRenderingThumb(model) ? (
          <div style={msgStyle}>rendering preview…</div>
        ) : (
          <div style={msgStyle}>no preview yet</div>
        )}

        <div style={badgeStyle}>{model.format} · {(model.fileSizeBytes / (1024 * 1024) || 0).toFixed(1)} MB</div>

        <button
          onClick={() => useUI.getState().openInspect(model.id)}
          className="hover-cyan"
          title="Open the interactive 3D view & position editor"
          style={{ position: 'absolute', right: 16, bottom: 14, background: 'rgba(18,18,22,0.86)', border: `1px solid ${C.accent}`, color: C.accent, fontFamily: F.mono, fontSize: 10.5, letterSpacing: '0.14em', padding: '7px 14px', borderRadius: 7, cursor: 'pointer' }}
        >
          ⤢ INSPECT 3D
        </button>
      </div>

      {/* Gallery strip */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 14px', borderTop: `1px solid ${C.border}`, overflowX: 'auto', background: 'rgba(10,10,12,0.5)' }}>
        <Tile active={heroId === 'thumb'} onClick={() => setHeroId('thumb')}>
          {thumb
            ? <img src={thumb} alt="saved view" style={tileImg} />
            : <span style={tileLabel}>3D</span>}
          <span style={tileBadge}>saved view</span>
        </Tile>

        {model.images.map((img) => (
          <Tile key={img.id} active={heroId === img.id} onClick={() => setHeroId(img.id)}>
            <img src={img.url} alt={img.caption || img.kind} style={tileImg} />
            <span style={tileBadge}>{img.kind}</span>
          </Tile>
        ))}

        {/* Add photo */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 5, flex: '0 0 auto' }}>
          <div style={{ display: 'flex', gap: 3 }}>
            {KINDS.map((k) => (
              <button
                key={k}
                onClick={() => setKind(k)}
                title={`Add as ${k}`}
                style={{ ...kindPill, ...(kind === k ? { borderColor: C.accent, color: C.accent } : {}) }}
              >
                {k.slice(0, 2).replace(/^./, (c) => c.toUpperCase())}
              </button>
            ))}
          </div>
          <div
            onClick={() => fileRef.current?.click()}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => { e.preventDefault(); const f = e.dataTransfer.files?.[0]; if (f) addPhoto(f); }}
            className="hover-cyan"
            title={`Add a ${kind} photo`}
            style={{ width: 64, height: 64, borderRadius: 8, border: `1.5px dashed ${C.border4}`, display: 'grid', placeItems: 'center', cursor: 'pointer', color: C.textMute2, fontSize: 20, lineHeight: 1 }}
          >
            +
            <input ref={fileRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={(e) => { const f = e.target.files?.[0]; if (f) addPhoto(f); e.target.value = ''; }} />
          </div>
        </div>
      </div>
    </div>
  );
}

function Tile({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <div
      onClick={onClick}
      className="hover-cyan"
      style={{ position: 'relative', flex: '0 0 auto', width: 64, height: 64, borderRadius: 8, overflow: 'hidden', cursor: 'pointer', background: '#0f0f12', border: `1.5px solid ${active ? C.accent : C.border3}`, display: 'grid', placeItems: 'center' }}
    >
      {children}
    </div>
  );
}

const heroImgStyle: React.CSSProperties = { maxWidth: '100%', maxHeight: '100%', objectFit: 'contain', display: 'block' };
const msgStyle: React.CSSProperties = { fontFamily: F.mono, fontSize: 11, letterSpacing: '0.1em', color: '#9b9ba1' };
const badgeStyle: React.CSSProperties = { position: 'absolute', left: 16, top: 14, fontFamily: F.mono, fontSize: 10, letterSpacing: '0.1em', color: C.textMute, background: 'rgba(13,13,15,0.72)', border: `1px solid ${C.border3}`, padding: '3px 8px', borderRadius: 6 };
const tileImg: React.CSSProperties = { width: '100%', height: '100%', objectFit: 'cover', display: 'block' };
const tileLabel: React.CSSProperties = { fontFamily: F.mono, fontSize: 12, color: C.textMute };
const tileBadge: React.CSSProperties = { position: 'absolute', left: 0, right: 0, bottom: 0, fontFamily: F.mono, fontSize: 7.5, letterSpacing: '0.06em', textAlign: 'center', color: C.textDim, background: 'rgba(8,8,10,0.78)', padding: '1.5px 0', textTransform: 'uppercase' };
const kindPill: React.CSSProperties = { width: 18, height: 16, borderRadius: 4, border: `1px solid ${C.border4}`, background: 'transparent', color: C.textMute2, fontFamily: F.mono, fontSize: 9, cursor: 'pointer', padding: 0 };
