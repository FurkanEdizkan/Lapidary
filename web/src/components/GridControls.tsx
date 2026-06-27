import { useViewerSettings, GRID_CELLS } from '../viewerSettings';
import { C, F } from '../theme';

/**
 * Viewport control for the measurement grid: toggle it and pick the cell size (mm).
 * Bound to the persisted viewer settings, so a change here also becomes the default
 * for the next model you open.
 */
export function GridControls() {
  const gridCellMm = useViewerSettings((s) => s.gridCellMm);
  const showGrid = useViewerSettings((s) => s.showGrid);
  const setGridCellMm = useViewerSettings((s) => s.setGridCellMm);
  const toggleGrid = useViewerSettings((s) => s.toggleGrid);

  return (
    <div style={{ position: 'absolute', left: 14, top: 44, display: 'flex', alignItems: 'center', gap: 5, flexWrap: 'wrap', maxWidth: 240 }}>
      <button onClick={toggleGrid} className="hover-cyan" title="Toggle the measurement grid" style={pill(showGrid)}>
        ▦ GRID {showGrid ? 'ON' : 'OFF'}
      </button>
      {showGrid && GRID_CELLS.map((mm) => (
        <button key={mm} onClick={() => setGridCellMm(mm)} className="hover-cyan" title={`${mm} mm grid cells`} style={pill(gridCellMm === mm)}>
          {mm}mm
        </button>
      ))}
    </div>
  );
}

function pill(active: boolean): React.CSSProperties {
  return {
    background: 'rgba(24,24,28,0.85)',
    border: `1px solid ${active ? C.accent : C.border4}`,
    color: active ? C.accent : '#b3b3ba',
    fontFamily: F.mono,
    fontSize: 9.5,
    letterSpacing: '0.1em',
    padding: '4px 8px',
    borderRadius: 6,
    cursor: 'pointer',
  };
}
