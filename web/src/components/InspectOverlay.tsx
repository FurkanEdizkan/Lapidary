import { useEffect } from 'react';
import { useModel } from '../api/client';
import { useUI } from '../store';
import { C } from '../theme';
import { ModelViewer } from './ModelViewer';

/**
 * Full-screen "Inspect" overlay — the only place the live Three.js viewer (and its
 * EDIT POSITION / move-rotate-drop-save tools) is mounted. State-gated by `inspectId`,
 * layered above the static detail overlay (z-index 60 > 50). Opened from DetailPreview.
 */
export function InspectOverlay() {
  const inspectId = useUI((s) => s.inspectId);
  const close = () => useUI.getState().closeInspect();
  // inspectId === the detail's selectedId, so this reuses the cached ['model', id] query.
  const { data: model } = useModel(inspectId);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, []);

  if (!inspectId || !model) return null;

  return (
    <div
      onClick={close}
      style={{ position: 'fixed', inset: 0, background: 'rgba(6,6,8,0.8)', backdropFilter: 'blur(8px)', zIndex: 60, display: 'grid', placeItems: 'center', padding: 20 }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{ position: 'relative', width: 'min(1600px, 98vw)', height: 'min(1000px, 96vh)', background: C.surface3, border: `1px solid ${C.border3}`, borderRadius: 14, overflow: 'hidden', display: 'grid', boxShadow: '0 44px 90px rgba(0,0,0,0.65)' }}
      >
        <ModelViewer model={model} />
        <button
          onClick={close}
          className="hover-cyan"
          style={{ position: 'absolute', right: 14, top: 14, zIndex: 2, background: C.surfaceInput, border: `1px solid ${C.border3}`, color: '#b3b3ba', width: 30, height: 30, borderRadius: 8, cursor: 'pointer', fontSize: 13, display: 'grid', placeItems: 'center' }}
        >
          ✕
        </button>
      </div>
    </div>
  );
}
