import { useViewerSettings, GRID_CELLS } from '../viewerSettings';
import { PLATE_PRESETS } from '../lib/platePresets';
import { C, F } from '../theme';

/**
 * Viewport controls for the Inspect 3D scene: measurement grid (toggle + cell size),
 * the live bounding box, and the build-plate size. All bound to the persisted viewer
 * settings — a change here also becomes the default for the next model opened.
 */
export function ViewerControls() {
  const gridCellMm = useViewerSettings((s) => s.gridCellMm);
  const showGrid = useViewerSettings((s) => s.showGrid);
  const showBoundingBox = useViewerSettings((s) => s.showBoundingBox);
  const platePresetId = useViewerSettings((s) => s.platePresetId);
  const customPlate = useViewerSettings((s) => s.customPlate);
  const setGridCellMm = useViewerSettings((s) => s.setGridCellMm);
  const toggleGrid = useViewerSettings((s) => s.toggleGrid);
  const toggleBoundingBox = useViewerSettings((s) => s.toggleBoundingBox);
  const setPlatePresetId = useViewerSettings((s) => s.setPlatePresetId);
  const setCustomPlate = useViewerSettings((s) => s.setCustomPlate);

  return (
    <div style={{ position: 'absolute', left: 14, top: 44, display: 'flex', flexDirection: 'column', gap: 6, maxWidth: 250 }}>
      <div style={{ display: 'flex', gap: 5, alignItems: 'center', flexWrap: 'wrap' }}>
        <button onClick={toggleGrid} className="hover-cyan" title="Toggle the measurement grid" style={pill(showGrid)}>▦ GRID</button>
        <button onClick={toggleBoundingBox} className="hover-cyan" title="Toggle the bounding box + size labels" style={pill(showBoundingBox)}>⊞ BOX</button>
      </div>

      {showGrid && (
        <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap' }}>
          {GRID_CELLS.map((mm) => (
            <button key={mm} onClick={() => setGridCellMm(mm)} className="hover-cyan" title={`${mm} mm grid cells`} style={pill(gridCellMm === mm)}>{mm}mm</button>
          ))}
        </div>
      )}

      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <span style={kicker}>PLATE</span>
        <select
          value={platePresetId}
          onChange={(e) => setPlatePresetId(e.target.value)}
          title="Build-plate size"
          style={selectStyle}
        >
          <option value="follow-printer">Follow printer</option>
          <option value="auto">Auto-fit model</option>
          {PLATE_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name} · {p.shape === 'circle' ? `Ø${p.diameterMm}` : `${p.widthMm}×${p.depthMm}`}
            </option>
          ))}
          <option value="custom">Custom…</option>
        </select>
      </div>

      {platePresetId === 'custom' && (
        <div style={{ display: 'flex', gap: 5, alignItems: 'center', fontFamily: F.mono, fontSize: 10, color: C.textMute }}>
          <input type="number" min={20} value={customPlate.widthMm} onChange={(e) => setCustomPlate({ ...customPlate, widthMm: Number(e.target.value) || 0 })} style={numStyle} aria-label="Plate width (mm)" />
          <span>×</span>
          <input type="number" min={20} value={customPlate.depthMm} onChange={(e) => setCustomPlate({ ...customPlate, depthMm: Number(e.target.value) || 0 })} style={numStyle} aria-label="Plate depth (mm)" />
          <span>mm</span>
        </div>
      )}
    </div>
  );
}

function pill(active: boolean): React.CSSProperties {
  return {
    background: 'rgba(24,24,28,0.85)',
    border: `1px solid ${active ? C.accent : C.border4}`,
    color: active ? C.accent : '#b3b3ba',
    fontFamily: F.mono, fontSize: 9.5, letterSpacing: '0.1em',
    padding: '4px 8px', borderRadius: 6, cursor: 'pointer',
  };
}
const kicker: React.CSSProperties = { fontFamily: F.mono, fontSize: 9, letterSpacing: '0.12em', color: C.textMute2 };
const selectStyle: React.CSSProperties = {
  background: 'rgba(24,24,28,0.95)', color: '#cfcfd6', border: `1px solid ${C.border4}`,
  fontFamily: F.mono, fontSize: 9.5, borderRadius: 6, padding: '3px 6px', maxWidth: 180, cursor: 'pointer',
};
const numStyle: React.CSSProperties = {
  background: 'rgba(24,24,28,0.95)', color: '#cfcfd6', border: `1px solid ${C.border4}`,
  fontFamily: F.mono, fontSize: 10, borderRadius: 6, padding: '3px 5px', width: 56,
};
