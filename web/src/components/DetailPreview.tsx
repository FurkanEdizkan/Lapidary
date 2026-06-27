import { useEffect, useRef, useState } from 'react';
import type { ModelDetail } from '../api/client';
import { api, useInvalidate } from '../api/client';
import { useThumbnail, isRenderingThumb } from '../lib/thumbs';
import { useUI } from '../store';
import { C, F } from '../theme';

const KINDS = ['printed', 'painted', 'reference'] as const;
type Kind = (typeof KINDS)[number];

const KIND_LABELS: Record<Kind, string> = {
  printed: 'Printed',
  painted: 'Painted',
  reference: 'Reference',
};

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

  const rendering = isRenderingThumb(model);

  return (
    <div style={{ position: 'relative', background: 'radial-gradient(ellipse at 50% 40%, #222227 0%, #131316 80%)', borderRight: `1px solid ${C.border}`, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
      {/*
        Scoped keyframes + focus rings.
        Controls outline on .lp-tile / .lp-kind / .lp-upload via :focus-visible so
        the inline styles can omit outline entirely (inline wins over selectors otherwise).
      */}
      <style>{`
        @keyframes lp-pulse { 0%,100%{opacity:.4} 50%{opacity:.85} }
        .lp-rendering { animation: lp-pulse 2.2s ease-in-out infinite; }
        @media (prefers-reduced-motion: reduce) {
          .lp-rendering { animation: none; opacity: .6; }
        }
        .lp-tile  { outline: none; }
        .lp-kind  { outline: none; }
        .lp-upload { outline: none; }
        .lp-tile:focus-visible  { outline: 2px solid ${C.accent}; outline-offset: 1px; }
        .lp-kind:focus-visible  { outline: 2px solid ${C.accent}; outline-offset: 1px; }
        .lp-upload:focus-visible { outline: 2px solid ${C.accent}; outline-offset: 1px; }
      `}</style>

      {/* ── Hero ─────────────────────────────────────────────────────────── */}
      <div style={{ position: 'relative', flex: 1, minHeight: 0, display: 'grid', placeItems: 'center', padding: 18 }}>
        {heroImage ? (
          <img
            src={heroImage.url}
            alt={heroImage.caption || `${heroImage.kind} photo of ${model.name}`}
            style={heroImgStyle}
          />
        ) : thumb ? (
          <img
            src={thumb}
            alt={`Saved 3D view of ${model.name}`}
            style={heroImgStyle}
          />
        ) : rendering ? (
          /* Background worker is generating the thumbnail server-side */
          <div style={emptyStateWrap}>
            <div className="lp-rendering" aria-hidden style={{ color: C.textMute }}>
              <CubeWireIcon dashed />
            </div>
            <div>
              <div style={emptyStateTitle}>Generating preview</div>
              <div style={emptyStateSub}>
                Thumbnail rendering in progress —{' '}check back in a moment
              </div>
            </div>
          </div>
        ) : (
          /* No mesh ingested or no thumbnail path available yet */
          <div style={emptyStateWrap}>
            <div aria-hidden style={{ color: C.textMute, opacity: 0.45 }}>
              <CubeWireIcon />
            </div>
            <div>
              <div style={emptyStateTitle}>No preview yet</div>
              <div style={emptyStateSub}>
                A preview appears automatically once the mesh is processed
              </div>
            </div>
          </div>
        )}

        {/* Format + file-size badge */}
        <div style={badgeStyle}>
          {model.format} · {(model.fileSizeBytes / (1024 * 1024) || 0).toFixed(1)} MB
        </div>

        {/* Inspect 3D — opens the live viewer + orientation editor */}
        <button
          type="button"
          onClick={() => useUI.getState().openInspect(model.id)}
          className="hover-cyan lp-tile"
          title="Open the interactive 3D view & position editor"
          aria-label="Inspect 3D — open interactive viewer and position editor"
          style={{
            position: 'absolute', right: 16, bottom: 14,
            background: 'rgba(18,18,22,0.86)',
            border: `1px solid ${C.accent}`,
            color: C.accent,
            fontFamily: F.mono, fontSize: 10.5, letterSpacing: '0.14em',
            padding: '7px 14px', borderRadius: 7,
            cursor: 'pointer',
          }}
        >
          ⤢ INSPECT 3D
        </button>
      </div>

      {/* ── Gallery strip ─────────────────────────────────────────────────── */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '10px 14px',
        borderTop: `1px solid ${C.border}`,
        overflowX: 'auto',
        background: 'rgba(10,10,12,0.5)',
      }}>

        {/* Saved-view tile — the rendered thumbnail from the saved orientation */}
        <GalleryTile
          active={heroId === 'thumb'}
          onClick={() => setHeroId('thumb')}
          ariaLabel="View saved 3D orientation"
          title="Saved 3D view — thumbnail rendered from the saved orientation"
        >
          {thumb
            ? <img src={thumb} alt="Saved 3D view" style={tileImg} />
            : <span aria-hidden style={tileLabel}>3D</span>}
          <span style={tileBadge}>Saved view</span>
        </GalleryTile>

        {/* Photo tiles */}
        {model.images.map((img) => (
          <GalleryTile
            key={img.id}
            active={heroId === img.id}
            onClick={() => setHeroId(img.id)}
            ariaLabel={`View ${img.kind} photo${img.caption ? `: ${img.caption}` : ''}`}
            title={img.caption || img.kind}
          >
            <img src={img.url} alt={img.caption || img.kind} style={tileImg} />
            <span style={tileBadge}>
              {KIND_LABELS[img.kind as Kind] ?? img.kind}
            </span>
          </GalleryTile>
        ))}

        {/* Visual separator between existing photos and the upload panel */}
        <div
          aria-hidden
          style={{
            width: 1, height: 48, flex: '0 0 auto',
            background: C.border, margin: '0 2px',
          }}
        />

        {/* Upload panel */}
        <AddPhotoPanel
          kind={kind}
          onKindChange={setKind}
          onFile={addPhoto}
          fileRef={fileRef}
        />
      </div>
    </div>
  );
}

// ── Gallery tile ──────────────────────────────────────────────────────────────

function GalleryTile({
  active, onClick, children, ariaLabel, title,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
  ariaLabel?: string;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={ariaLabel}
      aria-pressed={active}
      title={title}
      className="hover-cyan lp-tile"
      style={{
        position: 'relative',
        flex: '0 0 auto',
        width: 64, height: 64,
        borderRadius: 8, overflow: 'hidden',
        cursor: 'pointer',
        background: '#0f0f12',
        border: `1.5px solid ${active ? C.accent : C.border3}`,
        display: 'grid', placeItems: 'center',
        padding: 0,
      }}
    >
      {children}
      {/* Shape-based second cue for active state (non-color indicator) */}
      {active && (
        <span
          aria-hidden
          style={{
            position: 'absolute', top: 3, right: 3,
            width: 13, height: 13, borderRadius: '50%',
            background: C.accent,
            display: 'grid', placeItems: 'center',
            flexShrink: 0,
          }}
        >
          <CheckmarkIcon />
        </span>
      )}
    </button>
  );
}

// ── Add-photo panel ───────────────────────────────────────────────────────────

function AddPhotoPanel({
  kind, onKindChange, onFile, fileRef,
}: {
  kind: Kind;
  onKindChange: (k: Kind) => void;
  onFile: (f: File) => void;
  fileRef: { current: HTMLInputElement | null };
}) {
  const [dragOver, setDragOver] = useState(false);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 5, flex: '0 0 auto', width: 168 }}>
      {/* Section label */}
      <span
        aria-hidden
        style={{
          fontFamily: F.mono, fontSize: 9,
          letterSpacing: '0.15em', color: C.textFaint,
          textTransform: 'uppercase', userSelect: 'none',
        }}
      >
        Upload as
      </span>

      {/* Kind selector — one chip per category, clearly labelled */}
      <div role="group" aria-label="Photo category" style={{ display: 'flex', gap: 3 }}>
        {KINDS.map((k) => {
          const active = k === kind;
          return (
            <button
              key={k}
              type="button"
              className="lp-kind"
              onClick={() => onKindChange(k)}
              aria-pressed={active}
              title={k.charAt(0).toUpperCase() + k.slice(1)}
              style={{
                padding: '3px 7px',
                borderRadius: 5,
                border: `1px solid ${active ? C.accent : C.border4}`,
                /* Active: solid cyan fill matching the chip vocabulary */
                background: active ? C.accent : 'transparent',
                color: active ? C.onAccent : C.textMute2,
                fontFamily: F.mono,
                fontSize: 9,
                letterSpacing: '0.02em',
                cursor: 'pointer',
                lineHeight: 1.5,
                whiteSpace: 'nowrap',
                fontWeight: active ? 600 : 400,
                transition: 'border-color 120ms ease, color 120ms ease, background 120ms ease',
              }}
            >
              {KIND_LABELS[k]}
            </button>
          );
        })}
      </div>

      {/* Drop zone / upload button */}
      <button
        type="button"
        className="hover-cyan lp-upload"
        onClick={() => fileRef.current?.click()}
        onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          const f = e.dataTransfer.files?.[0];
          if (f) onFile(f);
        }}
        aria-label={`Upload a ${kind} photo — click or drag and drop an image`}
        title={`Add a ${kind} photo`}
        style={{
          width: '100%', height: 44,
          borderRadius: 7,
          border: `1.5px dashed ${dragOver ? C.accent : C.border4}`,
          background: dragOver ? 'rgba(44,180,245,0.07)' : 'transparent',
          display: 'flex', flexDirection: 'column',
          alignItems: 'center', justifyContent: 'center',
          gap: 3,
          cursor: 'pointer',
          color: dragOver ? C.accent : C.textMute2,
          padding: 0,
          transition: 'border-color 120ms ease, color 120ms ease, background 120ms ease',
        }}
      >
        <UploadArrowIcon />
        <span style={{ fontFamily: F.mono, fontSize: 8.5, letterSpacing: '0.07em' }}>
          Add photo
        </span>
        <input
          ref={fileRef}
          type="file"
          accept="image/*"
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) onFile(f);
            e.target.value = '';
          }}
        />
      </button>
    </div>
  );
}

// ── Icons ─────────────────────────────────────────────────────────────────────

function CubeWireIcon({ dashed = false }: { dashed?: boolean }) {
  return (
    <svg
      width="44" height="44" viewBox="0 0 44 44"
      fill="none" xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      {dashed && (
        <rect
          x="1" y="1" width="42" height="42" rx="7"
          stroke="currentColor" strokeWidth="1.5" strokeDasharray="4 3"
        />
      )}
      {/* Cube wireframe (isometric hex outline) */}
      <polygon
        points="22,8 32,14 32,30 22,36 12,30 12,14"
        stroke="currentColor" strokeWidth="1.4"
        fill="none" strokeLinejoin="round"
      />
      {/* Inner axis lines */}
      <line x1="22" y1="8"  x2="22" y2="22" stroke="currentColor" strokeWidth="0.9" opacity="0.38" />
      <line x1="12" y1="14" x2="22" y2="22" stroke="currentColor" strokeWidth="0.9" opacity="0.38" />
      <line x1="32" y1="14" x2="22" y2="22" stroke="currentColor" strokeWidth="0.9" opacity="0.38" />
    </svg>
  );
}

function UploadArrowIcon() {
  return (
    <svg
      width="14" height="14" viewBox="0 0 14 14"
      fill="none" xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path
        d="M7 1.5 L7 9.5"
        stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"
      />
      <path
        d="M4 4.5 L7 1.5 L10 4.5"
        stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"
      />
      <path
        d="M2.5 11 Q2.5 12.5 4 12.5 L10 12.5 Q11.5 12.5 11.5 11"
        stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"
      />
    </svg>
  );
}

function CheckmarkIcon() {
  return (
    <svg
      width="7" height="7" viewBox="0 0 7 7"
      fill="none" xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path
        d="M1.5 3.5 L2.8 5 L5.5 2"
        stroke={C.onAccent}
        strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"
      />
    </svg>
  );
}

// ── Shared styles ─────────────────────────────────────────────────────────────

const heroImgStyle: React.CSSProperties = {
  maxWidth: '100%', maxHeight: '100%',
  objectFit: 'contain', display: 'block',
};

const emptyStateWrap: React.CSSProperties = {
  display: 'flex', flexDirection: 'column',
  alignItems: 'center', gap: 12,
  textAlign: 'center', maxWidth: 200,
};

const emptyStateTitle: React.CSSProperties = {
  fontFamily: F.sans, fontSize: 13, fontWeight: 600,
  color: C.textDim, marginBottom: 4,
};

const emptyStateSub: React.CSSProperties = {
  fontFamily: F.mono, fontSize: 10,
  letterSpacing: '0.06em', color: C.textFaint,
  lineHeight: 1.6,
};

const badgeStyle: React.CSSProperties = {
  position: 'absolute', left: 16, top: 14,
  fontFamily: F.mono, fontSize: 10, letterSpacing: '0.1em', color: C.textMute,
  background: 'rgba(13,13,15,0.72)',
  border: `1px solid ${C.border3}`,
  padding: '3px 8px', borderRadius: 6,
};

const tileImg: React.CSSProperties = {
  width: '100%', height: '100%',
  objectFit: 'cover', display: 'block',
};

const tileLabel: React.CSSProperties = {
  fontFamily: F.mono, fontSize: 12, color: C.textMute,
};

const tileBadge: React.CSSProperties = {
  position: 'absolute', left: 0, right: 0, bottom: 0,
  fontFamily: F.mono, fontSize: 8, letterSpacing: '0.05em',
  textAlign: 'center', color: C.textDim,
  background: 'rgba(8,8,10,0.82)',
  padding: '2px 0',
  textTransform: 'uppercase',
};
