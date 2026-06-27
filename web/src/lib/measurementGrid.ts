// A dimensioned build-plate grid (real-world cells, e.g. 10mm) with cm tick labels along
// two edges + a plate outline, so the viewer conveys scale and bed footprint. Lives in
// plate/print space (Z-up, mm). Label size is decoupled from cell size: changing the cell
// size changes the grid DENSITY (not the label text size).
import * as THREE from 'three';

export interface GridHandle {
  group: THREE.Group;
  dispose(): void;
}

/** The plate footprint the grid spans (already resolved to concrete mm). */
export type GridPlate =
  | { shape: 'rect'; widthMm: number; depthMm: number }
  | { shape: 'circle'; diameterMm: number };

const GRID_MINOR = 0x2a2a30; // faint grid lines
const PLATE_OUTLINE = 0x2cb4f5; // cyan accent — the bed boundary
const LABEL_COLOR = '#8c8c95';

export function buildMeasurementGrid(plate: GridPlate, cellMm: number, groundZ: number, labelMm: number): GridHandle {
  const group = new THREE.Group();
  const w = plate.shape === 'rect' ? plate.widthMm : plate.diameterMm;
  const d = plate.shape === 'rect' ? plate.depthMm : plate.diameterMm;
  const cols = Math.max(2, Math.round(w / cellMm));
  const rows = Math.max(2, Math.round(d / cellMm));
  const gw = cols * cellMm; // snapped to whole cells
  const gd = rows * cellMm;
  const hw = gw / 2;
  const hd = gd / 2;
  const textures: THREE.Texture[] = [];

  // Grid lines (every cellMm across the plate), as one LineSegments.
  const pts: number[] = [];
  for (let i = 0; i <= cols; i++) { const x = -hw + i * cellMm; pts.push(x, -hd, groundZ, x, hd, groundZ); }
  for (let j = 0; j <= rows; j++) { const y = -hd + j * cellMm; pts.push(-hw, y, groundZ, hw, y, groundZ); }
  const gridGeo = new THREE.BufferGeometry();
  gridGeo.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
  group.add(new THREE.LineSegments(gridGeo, new THREE.LineBasicMaterial({ color: GRID_MINOR, transparent: true, opacity: 0.35 })));

  // Plate outline — rectangle or circle (the real bed boundary).
  const outlineMat = new THREE.LineBasicMaterial({ color: PLATE_OUTLINE, transparent: true, opacity: 0.5 });
  if (plate.shape === 'circle') {
    const r = plate.diameterMm / 2;
    const N = 72;
    const ring: number[] = [];
    for (let k = 0; k <= N; k++) { const a = (k / N) * Math.PI * 2; ring.push(Math.cos(a) * r, Math.sin(a) * r, groundZ); }
    const ringGeo = new THREE.BufferGeometry();
    ringGeo.setAttribute('position', new THREE.Float32BufferAttribute(ring, 3));
    group.add(new THREE.Line(ringGeo, outlineMat));
  } else {
    const o = [-hw, -hd, groundZ, hw, -hd, groundZ, hw, hd, groundZ, -hw, hd, groundZ, -hw, -hd, groundZ];
    const oGeo = new THREE.BufferGeometry();
    oGeo.setAttribute('position', new THREE.Float32BufferAttribute(o, 3));
    group.add(new THREE.Line(oGeo, outlineMat));
  }

  // cm tick labels at every 10mm, corner-origin (0…W along bottom, 0…D up the left).
  // Label height is model/view-relative (passed in) — NOT cell-relative — so changing the
  // cell size only changes the grid density, never the label text size.
  const labelH = labelMm;
  const off = labelMm;
  for (let v = 0; v <= gw + 1e-3; v += 10) {
    const s = makeLabel(`${Math.round(v) / 10}`, labelH, textures);
    s.position.set(-hw + v, -hd - off, groundZ);
    group.add(s);
  }
  for (let v = 10; v <= gd + 1e-3; v += 10) {
    const s = makeLabel(`${Math.round(v) / 10}`, labelH, textures);
    s.position.set(-hw - off, -hd + v, groundZ);
    group.add(s);
  }
  const unit = makeLabel('cm', labelH * 0.9, textures);
  unit.position.set(-hw - off, -hd - off, groundZ);
  group.add(unit);

  return {
    group,
    dispose() {
      group.traverse((o) => {
        const g = (o as THREE.Mesh).geometry as THREE.BufferGeometry | undefined;
        if (g) g.dispose();
        const mat = (o as THREE.Mesh).material as (THREE.Material & { map?: THREE.Texture }) | undefined;
        if (mat) { mat.map?.dispose(); mat.dispose(); }
      });
      textures.forEach((t) => t.dispose());
    },
  };
}

/** A billboarded text label (sprite) `heightMm` tall in world units; tracks its texture. */
export function makeLabel(text: string, heightMm: number, textures: THREE.Texture[]): THREE.Sprite {
  const c = document.createElement('canvas');
  c.width = 96;
  c.height = 48;
  const ctx = c.getContext('2d')!;
  ctx.clearRect(0, 0, c.width, c.height);
  ctx.font = 'bold 30px "JetBrains Mono", monospace';
  ctx.fillStyle = LABEL_COLOR;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, c.width / 2, c.height / 2);
  const tex = new THREE.CanvasTexture(c);
  tex.minFilter = THREE.LinearFilter;
  tex.colorSpace = THREE.SRGBColorSpace;
  textures.push(tex);
  const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false, depthWrite: false });
  const sp = new THREE.Sprite(mat);
  sp.scale.set(heightMm * (c.width / c.height), heightMm, 1);
  return sp;
}
