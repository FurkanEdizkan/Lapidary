// A dimensioned build-plate grid (real-world cells, e.g. 10mm) with cm tick labels
// along two edges, so the viewer conveys scale. Lives in plate/print space (Z-up, mm);
// the caller places it on the plate plane. Pure geometry — no DOM beyond an offscreen
// canvas for the label textures.
import * as THREE from 'three';

export interface GridHandle {
  group: THREE.Group;
  dispose(): void;
}

const GRID_MAJOR = 0x2cb4f5; // cyan accent — center lines
const GRID_MINOR = 0x2a2a30; // faint grid
const LABEL_COLOR = '#8c8c95';

/**
 * Build a measurement grid of `cellMm` cells covering ~`plateSizeMm`, at z=`groundZ`.
 * Returns the THREE.Group and a dispose() that frees all geometry/materials/textures.
 */
export function buildMeasurementGrid(cellMm: number, plateSizeMm: number, groundZ: number): GridHandle {
  const group = new THREE.Group();
  const divisions = Math.max(2, Math.round(plateSizeMm / cellMm));
  const sizeMm = divisions * cellMm; // exact cells
  const half = sizeMm / 2;

  const grid = new THREE.GridHelper(sizeMm, divisions, GRID_MAJOR, GRID_MINOR);
  const gmat = grid.material as THREE.Material;
  gmat.opacity = 0.32;
  gmat.transparent = true;
  grid.rotation.x = Math.PI / 2; // GridHelper is XZ (Y-up) by default → rotate into plate's XY
  grid.position.z = groundZ;
  group.add(grid);

  // cm tick labels along the bottom (+X) and left (+Y) edges, measured from the corner.
  const textures: THREE.Texture[] = [];
  const stepMm = cellMm >= 10 ? cellMm : 10; // label at most every 10mm
  const labelH = Math.max(5, cellMm * 0.7);
  const off = cellMm * 0.7;
  for (let v = 0; v <= sizeMm + 1e-3; v += stepMm) {
    const mm = Math.round(v);
    const text = mm % 10 === 0 ? `${mm / 10}` : `${mm}`; // cm when round, else mm
    const bottom = makeLabel(text, labelH, textures);
    bottom.position.set(-half + v, -half - off, groundZ);
    group.add(bottom);
    if (v > 0) {
      const left = makeLabel(text, labelH, textures);
      left.position.set(-half - off, -half + v, groundZ);
      group.add(left);
    }
  }
  // unit marker at the origin corner
  const unit = makeLabel('cm', labelH * 0.9, textures);
  unit.position.set(-half - off, -half - off, groundZ);
  group.add(unit);

  return {
    group,
    dispose() {
      grid.geometry.dispose();
      gmat.dispose();
      group.traverse((o) => {
        const s = o as THREE.Sprite;
        if (s.isSprite) {
          const m = s.material as THREE.SpriteMaterial;
          m.map?.dispose();
          m.dispose();
        }
      });
      textures.forEach((t) => t.dispose());
    },
  };
}

function makeLabel(text: string, heightMm: number, textures: THREE.Texture[]): THREE.Sprite {
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
