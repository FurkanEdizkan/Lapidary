import { useEffect, useRef, useState } from 'react';
import type { ModelDetail } from '../api/client';
import { api, useInvalidate } from '../api/client';
import { getMesh3D, buildProxy } from '../lib/mesh3d';
import { mountViewer, type ViewerHandle } from '../lib/threeViewer';
import { useViewerSettings } from '../viewerSettings';
import { resolvePlate } from '../lib/platePresets';
import { C, F } from '../theme';
import { TransformPanel } from './TransformPanel';
import { ViewerControls } from './ViewerControls';

type Tier = 'lod' | 'original';

/**
 * Viewer for a single model. Real files (hasLod/hasOriginal) render via Three.js,
 * loading the Tier-2 LOD first and the Tier-3 original on demand. Seeded "proxy"
 * models (meshKind only) render with the prototype's flat-shaded 2D renderer.
 */
export function ModelViewer({ model }: { model: ModelDetail }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const hostRef = useRef<HTMLDivElement | null>(null);
  const handleRef = useRef<ViewerHandle | null>(null);
  const isReal = model.hasLod || model.hasOriginal;
  const [tier, setTier] = useState<Tier>(model.hasLod ? 'lod' : 'original');
  const [loading, setLoading] = useState(isReal);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [gizmoMode, setGizmoMode] = useState<'translate' | 'rotate'>('rotate');
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const invalidate = useInvalidate();
  const gridCellMm = useViewerSettings((s) => s.gridCellMm);
  const showGrid = useViewerSettings((s) => s.showGrid);
  const showBoundingBox = useViewerSettings((s) => s.showBoundingBox);
  const platePresetId = useViewerSettings((s) => s.platePresetId);
  const customPlate = useViewerSettings((s) => s.customPlate);
  const printersKey = model.printers.join(',');

  // ---- Real mesh path (Three.js) ----
  useEffect(() => {
    if (!isReal || !hostRef.current) return;
    setLoading(true);
    setError(null);
    const url = tier === 'lod' && model.hasLod ? `/api/models/${model.id}/lod` : `/api/models/${model.id}/original`;
    // mountViewer is synchronous: the handle (and its canvas) exist immediately, so
    // this effect's cleanup can always tear down exactly the renderer it created —
    // even under StrictMode's mount→cleanup→mount double-invoke. `ready` resolves
    // after the mesh frames. A destroy() before that makes the load a no-op.
    const handle = mountViewer(hostRef.current, url, model.format, {
      transform: model.transform ?? undefined,
      gridCellMm, showGrid, showBoundingBox,
      plate: resolvePlate(platePresetId, customPlate, model.printers),
    });
    handleRef.current = handle;
    handle.ready
      .then(() => {
        if (handleRef.current !== handle) return; // superseded by a newer mount
        setLoading(false);
        // The live viewer now mounts only inside Inspect, so refresh the saved-view
        // thumbnail on every successful render (default camera, plate hidden). This keeps
        // the static detail preview current and self-heals legacy blank/stale thumbnails
        // (e.g. captures from the old Context-Lost bug). Invalidate AFTER the POST resolves;
        // the mount-effect deps exclude hasThumbnail, so this refetch doesn't remount.
        const dataUrl = handle.thumbnail({ hidePlate: true });
        if (dataUrl) api.saveThumbnail(model.id, dataUrl).then(() => invalidate(['model', 'models'])).catch(() => undefined);
        const bbox = handle.boundingBox();
        if (bbox && model.size.every((v) => v === 0)) api.patchModel(model.id, { size: bbox }).catch(() => undefined);
      })
      .catch((e) => { if (handleRef.current === handle) { setError(String(e)); setLoading(false); } });
    return () => {
      handle.destroy();
      if (handleRef.current === handle) handleRef.current = null;
    };
  // model.transform is intentionally omitted: the saved transform is applied once via
  // mountViewer(..., { transform }) at mount time. Re-running this effect on every
  // Save would destroy + re-parse the whole mesh unnecessarily — a fresh mount already
  // picks up the latest transform when the model is next opened.
  // gridCellMm/showGrid are applied at mount via opts and live via the effect below;
  // they're intentionally excluded here so a grid change never re-parses the mesh.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model.id, tier, isReal, model.format, model.hasLod]);

  // Apply measurement-grid settings live (no remount) when changed from the viewport.
  useEffect(() => {
    handleRef.current?.setGrid({ cellMm: gridCellMm, visible: showGrid });
  }, [gridCellMm, showGrid]);

  // Apply the build-plate (resolved from the preset + the model's printers) live.
  useEffect(() => {
    handleRef.current?.setPlate(resolvePlate(platePresetId, customPlate, model.printers));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [platePresetId, customPlate.widthMm, customPlate.depthMm, model.id, printersKey]);

  // Toggle the bounding-box overlay live.
  useEffect(() => {
    handleRef.current?.setBoundingBox(showBoundingBox);
  }, [showBoundingBox]);

  // ---- Proxy mesh path (2D canvas renderer) ----
  useEffect(() => {
    if (isReal || !canvasRef.current) return;
    const canvas = canvasRef.current;
    const api3d = getMesh3D();
    const mesh = buildProxy(model.meshKind || 'egg');
    if (!api3d || !mesh) return;
    const view = { rx: -0.38, ry: 0.85, zoom: 1 };
    const draw = () => api3d.render(canvas, mesh, { rx: view.rx, ry: view.ry, zoom: view.zoom, color: model.color });
    draw();
    let drag: { x: number; y: number } | null = null;
    const down = (e: PointerEvent) => { drag = { x: e.clientX, y: e.clientY }; try { canvas.setPointerCapture(e.pointerId); } catch { /* ignore */ } };
    const moveE = (e: PointerEvent) => {
      if (!drag) return;
      view.ry += (e.clientX - drag.x) * 0.011;
      view.rx = Math.max(-1.5, Math.min(1.5, view.rx + (e.clientY - drag.y) * 0.011));
      drag = { x: e.clientX, y: e.clientY };
      draw();
    };
    const up = () => { drag = null; };
    const wheel = (e: WheelEvent) => { e.preventDefault(); view.zoom = Math.max(0.4, Math.min(3.2, view.zoom * (e.deltaY < 0 ? 1.1 : 0.9))); draw(); };
    (canvas as HTMLCanvasElement & { _reset?: () => void })._reset = () => { view.rx = -0.38; view.ry = 0.85; view.zoom = 1; draw(); };
    canvas.addEventListener('pointerdown', down);
    canvas.addEventListener('pointermove', moveE);
    canvas.addEventListener('pointerup', up);
    canvas.addEventListener('wheel', wheel, { passive: false });
    return () => {
      canvas.removeEventListener('pointerdown', down);
      canvas.removeEventListener('pointermove', moveE);
      canvas.removeEventListener('pointerup', up);
      canvas.removeEventListener('wheel', wheel);
    };
  }, [model.id, isReal, model.meshKind, model.color]);

  const reset = () => {
    if (isReal) handleRef.current?.reset();
    else (canvasRef.current as (HTMLCanvasElement & { _reset?: () => void }) | null)?._reset?.();
  };

  const enterEdit = () => { setSaveError(null); setEditing(true); handleRef.current?.setEditMode(true); handleRef.current?.setGizmoMode(gizmoMode); };
  const cancelEdit = () => { setSaveError(null); setEditing(false); handleRef.current?.setEditMode(false); handleRef.current?.setTransform(model.transform ?? undefined); };
  const chooseMode = (m: 'translate' | 'rotate') => { setGizmoMode(m); handleRef.current?.setGizmoMode(m); };
  const save = async () => {
    const h = handleRef.current; if (!h) return;
    setSaving(true);
    setSaveError(null);
    try {
      const t = h.getTransform();
      await api.saveTransform(model.id, t);
      const dataUrl = h.thumbnail({ hidePlate: true });
      if (dataUrl) await api.saveThumbnail(model.id, dataUrl).catch(() => undefined);
      invalidate(['model', 'models']);
      setEditing(false); h.setEditMode(false);
    } catch (e) {
      // Keep the in-editor pose so the user can retry; surface the failure inline.
      setSaveError(String(e));
    } finally { setSaving(false); }
  };

  return (
    <div style={{ position: 'relative', background: 'radial-gradient(ellipse at 50% 44%, #222227 0%, #131316 78%)', borderRight: `1px solid ${C.border}`, display: 'flex', minWidth: 0 }}>
      {isReal ? (
        // Three.js owns its own canvas, mounted into this host. A fresh canvas per
        // mount keeps destroy()/forceContextLoss() scoped to one viewer (StrictMode-safe).
        <div ref={hostRef} style={{ position: 'absolute', inset: 0, display: 'block' }} />
      ) : (
        <canvas ref={canvasRef} width={880} height={680} style={{ width: '100%', height: '100%', objectFit: 'contain', cursor: 'grab', touchAction: 'none', display: 'block', flex: 1 }} />
      )}
      <div style={badge('left', 14)}>{model.format} · {(model.fileSizeBytes / (1024 * 1024) || 0).toFixed(1)} MB</div>
      {isReal && <ViewerControls />}
      <div style={{ position: 'absolute', left: 16, bottom: 14, fontFamily: F.mono, fontSize: 9.5, letterSpacing: '0.14em', color: C.textFaint }}>DRAG TO ROTATE · SCROLL TO ZOOM</div>

      {isReal && model.hasLod && (
        <button
          onClick={() => setTier((t) => (t === 'lod' ? 'original' : 'lod'))}
          className="hover-cyan"
          style={{ position: 'absolute', right: 14, top: 12, ...pill }}
          title="LOD loads instantly; full mesh reads the real file"
        >
          {tier === 'lod' ? 'VIEW FULL MESH' : 'BACK TO LOD'}
        </button>
      )}
      {isReal && !editing && !loading && (
        <button onClick={enterEdit} className="hover-cyan" style={{ position: 'absolute', left: 14, bottom: 40, ...pill }}>EDIT POSITION</button>
      )}
      {isReal && editing && (
        <TransformPanel mode={gizmoMode} onMode={chooseMode} onDrop={() => handleRef.current?.dropToPlane()}
          onReset={() => handleRef.current?.resetTransform()} onSave={save} onCancel={cancelEdit} saving={saving} />
      )}
      {saveError && (
        <div style={{ position: 'absolute', left: 14, bottom: 82, fontFamily: F.mono, fontSize: 9.5, color: '#e05252', letterSpacing: '0.08em', maxWidth: 260 }}>
          SAVE FAILED: {saveError}
        </div>
      )}
      <button onClick={reset} className="hover-cyan" style={{ position: 'absolute', right: 14, bottom: 12, ...pill }}>RESET VIEW</button>

      {loading && <div style={overlayMsg}>loading mesh…</div>}
      {error && <div style={{ ...overlayMsg, color: C.danger }}>could not load mesh</div>}
    </div>
  );
}

const pill: React.CSSProperties = {
  background: 'rgba(24,24,28,0.85)', border: `1px solid ${C.border4}`, color: '#b3b3ba',
  fontFamily: F.mono, fontSize: 9.5, letterSpacing: '0.12em', padding: '5px 10px', borderRadius: 6, cursor: 'pointer',
};
function badge(side: 'left' | 'right', top: number): React.CSSProperties {
  return { position: 'absolute', [side]: 16, top, fontFamily: F.mono, fontSize: 10, letterSpacing: '0.1em', color: C.textMute, background: 'rgba(13,13,15,0.72)', border: `1px solid ${C.border3}`, padding: '3px 8px', borderRadius: 6 } as React.CSSProperties;
}
const overlayMsg: React.CSSProperties = {
  position: 'absolute', top: '50%', left: '50%', transform: 'translate(-50%,-50%)',
  fontFamily: F.mono, fontSize: 11, letterSpacing: '0.1em', color: '#9b9ba1',
};
