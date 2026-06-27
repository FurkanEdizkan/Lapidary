// Imperative Three.js viewer for real mesh files (STL / 3MF / OBJ / GLTF).
// Owns its renderer/scene/camera and disposes ALL GPU resources on destroy(),
// so paging through many models never leaks WebGL contexts or memory.
//
// IMPORTANT — StrictMode safety: the renderer (and its canvas) are created
// SYNCHRONOUSLY so the caller can store the handle and tear it down in the very
// same tick, even if the geometry is still loading. The renderer also owns its
// OWN canvas (mounted into the host element), so a destroy() — including
// forceContextLoss() — only ever affects that one canvas. React 18 StrictMode
// double-invokes effects in dev (mount → cleanup → mount); with a shared canvas
// + async init this previously force-lost the live context, leaving the viewer
// blank ("THREE.WebGLRenderer: Context Lost"). A fresh canvas per mount fixes it.
import * as THREE from 'three';
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js';
import { ThreeMFLoader } from 'three/examples/jsm/loaders/3MFLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
import { defaultTransform, dropToPlane, type PlateTransform } from './plateTransform';
import { buildMeasurementGrid, makeLabel, type GridHandle, type GridPlate } from './measurementGrid';
import type { ResolvedPlate } from './platePresets';

// Plate sits a hair below z=0 to avoid z-fighting (echoes OrcaSlicer GROUND_Z).
const GROUND_Z = -0.02;

export interface ViewerHandle {
  /** Resolves after the mesh has loaded and the first frame has rendered; rejects on load error. */
  ready: Promise<void>;
  destroy(): void;
  reset(): void;
  thumbnail(opts?: { hidePlate?: boolean }): string | null;
  boundingBox(): [number, number, number] | null;
  // Edit-mode API (Task 6)
  setEditMode(on: boolean): void;
  setGizmoMode(mode: 'translate' | 'rotate'): void;
  getTransform(): PlateTransform;
  setTransform(t?: PlateTransform): void;
  dropToPlane(): void;
  resetTransform(): void;
  onTransformChange(cb: (t: PlateTransform) => void): void;
  /** Update the measurement grid (cell size in mm and/or visibility). */
  setGrid(opts: { cellMm?: number; visible?: boolean }): void;
  /** Set the build plate (a real bed size or 'auto') — rebuilds plate + grid and re-fits. */
  setPlate(plate: ResolvedPlate): void;
  /** Toggle the live bounding-box overlay (W×D×H labels). */
  setBoundingBox(visible: boolean): void;
}

function loadGeometry(url: string, format: string): Promise<THREE.Object3D> {
  const fmt = format.toLowerCase();
  return new Promise((resolve, reject) => {
    if (fmt === 'stl') {
      new STLLoader().load(url, (geo) => resolve(meshFromGeometry(geo)), undefined, reject);
    } else if (fmt === 'obj') {
      new OBJLoader().load(url, (obj) => resolve(applyMaterial(obj)), undefined, reject);
    } else if (fmt === '3mf') {
      new ThreeMFLoader().load(url, (obj) => resolve(applyMaterial(obj)), undefined, reject);
    } else if (fmt === 'glb' || fmt === 'gltf') {
      new GLTFLoader().load(url, (g) => resolve(applyMaterial(g.scene)), undefined, reject);
    } else {
      reject(new Error(`unsupported format ${format}`));
    }
  });
}

const MAT = () => new THREE.MeshStandardMaterial({ color: 0xbcc0c8, roughness: 0.62, metalness: 0.04, flatShading: false });

function meshFromGeometry(geo: THREE.BufferGeometry): THREE.Mesh {
  geo.computeVertexNormals();
  return new THREE.Mesh(geo, MAT());
}
function applyMaterial(obj: THREE.Object3D): THREE.Object3D {
  obj.traverse((c) => {
    if ((c as THREE.Mesh).isMesh) (c as THREE.Mesh).material = MAT();
  });
  return obj;
}

function disposeObject(object: THREE.Object3D): void {
  object.traverse((c) => {
    const m = c as THREE.Mesh;
    if (m.geometry) m.geometry.dispose();
    const mat = m.material as THREE.Material | THREE.Material[] | undefined;
    if (Array.isArray(mat)) mat.forEach((x) => x.dispose());
    else if (mat) mat.dispose();
  });
}

/** Apply a PlateTransform to an Object3D pivot (position/quaternion/scale). */
function applyTransformToPivot(pivot: THREE.Object3D, t: PlateTransform): void {
  pivot.matrixAutoUpdate = true;
  pivot.position.set(t.position[0], t.position[1], t.position[2]);
  pivot.quaternion.set(t.quaternion[0], t.quaternion[1], t.quaternion[2], t.quaternion[3]);
  pivot.scale.setScalar(t.scale);
  pivot.updateMatrixWorld(true);
}

/**
 * Mount a viewer into `host`; loads `url` as `format`. Returns a handle SYNCHRONOUSLY
 * (the renderer/canvas exist immediately); `handle.ready` resolves once the mesh is
 * loaded and framed.
 */
export function mountViewer(
  host: HTMLElement,
  url: string,
  format: string,
  opts: { transform?: PlateTransform; gridCellMm?: number; showGrid?: boolean; plate?: ResolvedPlate; showBoundingBox?: boolean } = {},
): ViewerHandle {
  let disposed = false;

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  const w = host.clientWidth || 880;
  const h = host.clientHeight || 680;
  renderer.setSize(w, h, false);

  // The renderer owns its canvas; mount it into the host. A fresh canvas per mount
  // makes destroy()/forceContextLoss() affect only this viewer (StrictMode-safe).
  const canvas = renderer.domElement;
  canvas.style.width = '100%';
  canvas.style.height = '100%';
  canvas.style.display = 'block';
  canvas.style.cursor = 'grab';
  canvas.style.touchAction = 'none';
  canvas.style.objectFit = 'contain'; // never stretch the buffer during a resize transient
  host.appendChild(canvas);

  const scene = new THREE.Scene();
  scene.add(new THREE.AmbientLight(0xffffff, 0.55));
  const key = new THREE.DirectionalLight(0xffffff, 1.1);
  key.position.set(-0.45, 0.85, 0.65);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0x88aaff, 0.25);
  fill.position.set(0.6, -0.3, -0.5);
  scene.add(fill);

  const camera = new THREE.PerspectiveCamera(38, w / h, 0.1, 5000);

  const state = { rx: -0.38, ry: 0.85, zoom: 1 };
  const target = new THREE.Vector3(); // orbit center; set by frameCamera() after load
  const size = new THREE.Vector3();   // kept for boundingBox()
  let radius = 1;
  let dist = 3;
  let framed = false;
  let gizmoDragging = false;

  // Plate group: print-space (Z-up) content, rotated into the Y-up scene.
  const plate = new THREE.Group();
  plate.rotation.x = -Math.PI / 2;
  scene.add(plate);

  // Subtle plate plane (rect or circle) at z = GROUND_Z (in plate/print space).
  const planeMat = new THREE.MeshBasicMaterial({ color: 0x1a1a1e, transparent: true, opacity: 0.6, depthWrite: false, side: THREE.DoubleSide });
  const planeMesh: THREE.Mesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), planeMat); // geometry set in applyPlate()
  planeMesh.position.z = GROUND_Z;
  plate.add(planeMesh);

  // Plate state: a real bed (rect/circle) or 'auto' (~1.5× the model footprint).
  let plateState: ResolvedPlate = opts.plate ?? 'auto';
  let footprintMm = 80;        // model footprint, set on load (for 'auto')
  let plateW = 200, plateD = 200; // concrete plate extent (mm), set by applyPlate()
  let gridPlate: GridPlate = { shape: 'rect', widthMm: 200, depthMm: 200 };

  // Measurement grid (cells + cm labels + outline), rebuilt on cell-size/plate change.
  let gridCellMm = opts.gridCellMm ?? 10;
  let gridVisible = opts.showGrid ?? true;
  let labelMm = 6; // grid + bbox label height (mm), set from the model size on load (view-relative)
  let gridHandle: GridHandle | null = null;
  const gridHost = new THREE.Group();
  plate.add(gridHost);
  function rebuildGrid() {
    if (gridHandle) { gridHost.remove(gridHandle.group); gridHandle.dispose(); gridHandle = null; }
    if (gridVisible) {
      gridHandle = buildMeasurementGrid(gridPlate, gridCellMm, GROUND_Z, labelMm);
      gridHost.add(gridHandle.group);
    }
  }

  // Resolve plateState → a concrete plate footprint, build the plane geometry + grid.
  function applyPlate() {
    let gp: GridPlate;
    if (plateState === 'auto') {
      const s = Math.min(256, Math.max(80, Math.ceil(1.5 * footprintMm)));
      gp = { shape: 'rect', widthMm: s, depthMm: s };
    } else if (plateState.shape === 'circle') {
      gp = { shape: 'circle', diameterMm: plateState.diameterMm };
    } else {
      gp = { shape: 'rect', widthMm: plateState.widthMm, depthMm: plateState.depthMm };
    }
    gridPlate = gp;
    plateW = gp.shape === 'rect' ? gp.widthMm : gp.diameterMm;
    plateD = gp.shape === 'rect' ? gp.depthMm : gp.diameterMm;
    planeMesh.geometry.dispose();
    planeMesh.geometry = gp.shape === 'circle'
      ? new THREE.CircleGeometry(gp.diameterMm / 2, 72)
      : new THREE.PlaneGeometry(plateW, plateD);
    rebuildGrid();
  }

  // Pivot holds the saved transform; the mesh is offset so it is centered on the pivot origin.
  const pivot = new THREE.Group();
  plate.add(pivot);

  // Live bounding-box overlay (wireframe + W×D×H labels), in plate space.
  let bboxVisible = opts.showBoundingBox ?? true;
  const bboxGroup = new THREE.Group();
  plate.add(bboxGroup);
  const bboxLineMat = new THREE.LineBasicMaterial({ color: 0x2cb4f5, transparent: true, opacity: 0.55 });
  let bboxTextures: THREE.Texture[] = [];
  function clearBbox() {
    while (bboxGroup.children.length) {
      const c = bboxGroup.children.pop()!;
      const g = (c as THREE.Mesh).geometry as THREE.BufferGeometry | undefined; if (g) g.dispose();
      const m = (c as THREE.Sprite).material as (THREE.Material & { map?: THREE.Texture }) | undefined;
      if (m && (c as THREE.Sprite).isSprite) { m.map?.dispose(); m.dispose(); }
    }
    bboxTextures.forEach((t) => t.dispose());
    bboxTextures = [];
  }
  function rebuildBbox() {
    clearBbox();
    if (!bboxVisible || !framed) return;
    pivot.updateMatrix();
    // Plate-local AABB of the posed model (baseBox is the centered mesh box).
    const box = baseBox.clone().applyMatrix4(pivot.matrix);
    const mn = box.min, mx = box.max;
    const v = (x: number, y: number, z: number) => [x, y, z];
    const c = [
      v(mn.x, mn.y, mn.z), v(mx.x, mn.y, mn.z), v(mx.x, mx.y, mn.z), v(mn.x, mx.y, mn.z),
      v(mn.x, mn.y, mx.z), v(mx.x, mn.y, mx.z), v(mx.x, mx.y, mx.z), v(mn.x, mx.y, mx.z),
    ];
    const E = [[0,1],[1,2],[2,3],[3,0],[4,5],[5,6],[6,7],[7,4],[0,4],[1,5],[2,6],[3,7]];
    const pts: number[] = [];
    for (const [a, b] of E) pts.push(...c[a], ...c[b]);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
    bboxGroup.add(new THREE.LineSegments(geo, bboxLineMat));
    // W×D×H labels (mm)
    const labelH = labelMm;
    const wmm = Math.round(mx.x - mn.x), dmm = Math.round(mx.y - mn.y), hmm = Math.round(mx.z - mn.z);
    const lw = makeLabel(`${wmm} mm`, labelH, bboxTextures); lw.position.set((mn.x + mx.x) / 2, mn.y - labelH, mn.z);
    const ld = makeLabel(`${dmm} mm`, labelH, bboxTextures); ld.position.set(mn.x - labelH, (mn.y + mx.y) / 2, mn.z);
    const lh = makeLabel(`${hmm} mm`, labelH, bboxTextures); lh.position.set(mn.x - labelH, mn.y - labelH, (mn.z + mx.z) / 2);
    bboxGroup.add(lw, ld, lh);
  }

  const baseBox = new THREE.Box3(); // centered mesh box, filled after load

  function applyCamera() {
    const d = dist / state.zoom;
    camera.position.set(
      target.x + d * Math.cos(state.rx) * Math.sin(state.ry),
      target.y + d * Math.sin(state.rx) + radius * 0.1,
      target.z + d * Math.cos(state.rx) * Math.cos(state.ry),
    );
    camera.lookAt(target);
    camera.updateProjectionMatrix();
  }
  function frame() { applyCamera(); renderer.render(scene, camera); }

  // Frame the model's CURRENT world bbox without moving the object.
  // scene.updateMatrixWorld(true) is essential: it bakes plate.rotation.x = -π/2
  // into plate.matrixWorld before setFromObject reads it (pivot.updateMatrixWorld
  // only descends, never updates the parent plate).
  function frameCamera() {
    scene.updateMatrixWorld(true);
    const wbox = new THREE.Box3().setFromObject(pivot);
    const wcenter = new THREE.Vector3();
    const wsize = new THREE.Vector3();
    wbox.getCenter(wcenter);
    wbox.getSize(wsize);
    radius = Math.max(wsize.x, wsize.y, wsize.z) / 2 || 1;
    // Fit the bounding sphere within the TIGHTER of the vertical/horizontal FOV so the
    // model is fully framed (never clipped) at any pane aspect — portrait or landscape.
    const vHalf = (camera.fov * Math.PI) / 360;
    const hHalf = Math.atan(Math.tan(vHalf) * camera.aspect);
    dist = radius / Math.sin(Math.min(vHalf, hHalf));
    target.copy(wcenter);
    frame();
  }

  // Keep the render buffer + camera aspect in lockstep with the host's size, so the
  // model is never rendered stretched. Without this, buffer/camera aspect is frozen
  // at mount while the host box follows the window → non-uniform scaling. The
  // observer's initial callback also corrects any off/fallback mount measurement.
  function onResize() {
    if (disposed) return;
    const nw = host.clientWidth;
    const nh = host.clientHeight;
    if (nw === 0 || nh === 0) return;
    renderer.setSize(nw, nh, false);
    camera.aspect = nw / nh;
    camera.updateProjectionMatrix();
    // Re-fit (not just re-render): corrects the aspect AND re-frames the model to the
    // new pane so it neither stretches nor clips. frameCamera() preserves orbit + zoom.
    if (framed) frameCamera();
  }
  const ro = new ResizeObserver(onResize);
  ro.observe(host);

  // Pointer interaction (attached immediately; harmless before the mesh frames).
  let drag: { x: number; y: number } | null = null;
  const onDown = (e: PointerEvent) => { if (gizmoDragging) return; drag = { x: e.clientX, y: e.clientY }; canvas.setPointerCapture(e.pointerId); };
  const onMove = (e: PointerEvent) => {
    if (gizmoDragging) return;
    if (!drag) return;
    state.ry -= (e.clientX - drag.x) * 0.011;
    state.rx = Math.max(-1.45, Math.min(1.45, state.rx + (e.clientY - drag.y) * 0.011));
    drag = { x: e.clientX, y: e.clientY };
    if (framed) frame();
  };
  const onUp = () => { drag = null; };
  const onWheel = (e: WheelEvent) => {
    if (gizmoDragging) return;
    e.preventDefault();
    state.zoom = Math.max(0.4, Math.min(3.2, state.zoom * (e.deltaY < 0 ? 1.1 : 0.9)));
    if (framed) frame();
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerup', onUp);
  canvas.addEventListener('wheel', onWheel, { passive: false });

  // TransformControls (move + rotate only; scale is Phase B).
  // Cast: three 0.172 TransformControls is not typed as Object3D, so `.visible`
  // doesn't appear in TS types even though it exists at runtime.
  const control = new TransformControls(camera, canvas) as TransformControls & { visible: boolean };
  control.setSpace('world');
  control.setMode('rotate');
  control.enabled = false;
  control.visible = false;
  const controlHelper = control.getHelper();
  controlHelper.visible = false;
  scene.add(controlHelper);
  let onChangeCb: ((t: PlateTransform) => void) | null = null;
  control.addEventListener('change', () => { if (framed) frame(); });
  control.addEventListener('objectChange', () => { rebuildBbox(); if (onChangeCb) onChangeCb(readPivotTransform()); });
  control.addEventListener('dragging-changed', (e) => { gizmoDragging = (e as unknown as { value: boolean }).value; });

  function readPivotTransform(): PlateTransform {
    return {
      position: [pivot.position.x, pivot.position.y, pivot.position.z],
      quaternion: [pivot.quaternion.x, pivot.quaternion.y, pivot.quaternion.z, pivot.quaternion.w],
      scale: pivot.scale.x,
    };
  }

  const ready = loadGeometry(url, format).then((object) => {
    // The caller may have torn us down while the mesh was loading (e.g. StrictMode
    // remount or the overlay closing). If so, drop the just-loaded object and stop.
    if (disposed) { disposeObject(object); return; }

    const box = new THREE.Box3().setFromObject(object);
    const center = new THREE.Vector3();
    box.getCenter(center);
    box.getSize(size); // populate size for boundingBox()
    object.position.sub(center); // center the mesh on the pivot origin
    pivot.add(object);
    baseBox.copy(box).translate(center.clone().negate()); // centered base box (mm)

    // Build the plate (real bed, or 'auto' from the model footprint) + grid.
    footprintMm = Math.max(size.x, size.y, 1);
    labelMm = clamp(Math.max(size.x, size.y, size.z) * 0.06, 2.5, 9); // model/view-relative label size
    applyPlate();

    // Apply the saved transform, or the default (centered + dropped to plate).
    const t = opts.transform ?? defaultTransform(baseBox);
    applyTransformToPivot(pivot, t);

    framed = true;
    frameCamera();
    rebuildBbox();
  });

  return {
    ready,
    destroy() {
      if (disposed) return;
      disposed = true;
      ro.disconnect();
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointermove', onMove);
      canvas.removeEventListener('pointerup', onUp);
      canvas.removeEventListener('wheel', onWheel);
      control.detach();
      control.dispose();
      scene.remove(controlHelper);
      // Explicit plate/grid disposal (scene.traverse below also catches these,
      // but explicit disposal is clearer and safe even if traverse order changes).
      planeMesh.geometry.dispose();
      planeMat.dispose();
      if (gridHandle) gridHandle.dispose(); // frees grid lines + label textures
      clearBbox(); bboxLineMat.dispose();
      scene.traverse((c) => {
        const m = c as THREE.Mesh;
        if (m.geometry) m.geometry.dispose();
        const mat = m.material as THREE.Material | THREE.Material[] | undefined;
        if (Array.isArray(mat)) mat.forEach((x) => x.dispose());
        else if (mat) mat.dispose();
      });
      renderer.dispose();
      renderer.forceContextLoss();
      if (canvas.parentNode) canvas.parentNode.removeChild(canvas);
    },
    reset() { state.rx = -0.38; state.ry = 0.85; state.zoom = 1; if (framed) frame(); },
    thumbnail(o?: { hidePlate?: boolean }) {
      if (!framed) return null;
      const hide = o?.hidePlate ?? false;
      const pv = planeMesh.visible, gv = gridHost.visible, cv = controlHelper.visible, bv = bboxGroup.visible;
      if (hide) { planeMesh.visible = false; gridHost.visible = false; controlHelper.visible = false; bboxGroup.visible = false; }
      try { frame(); return canvas.toDataURL('image/png'); }
      finally { if (hide) { planeMesh.visible = pv; gridHost.visible = gv; controlHelper.visible = cv; bboxGroup.visible = bv; frame(); } }
    },
    boundingBox() { return framed ? [round(size.x), round(size.y), round(size.z)] : null; },
    setEditMode(on: boolean) {
      if (on) control.attach(pivot); else control.detach();
      control.enabled = on; control.visible = on; controlHelper.visible = on;
      planeMesh.visible = true; // plate stays visible; the measurement grid is controlled via setGrid
      if (framed) frame();
    },
    setGrid({ cellMm, visible }: { cellMm?: number; visible?: boolean }) {
      if (cellMm !== undefined) gridCellMm = cellMm;
      if (visible !== undefined) gridVisible = visible;
      rebuildGrid();
      if (framed) frame();
    },
    setPlate(p: ResolvedPlate) {
      plateState = p;
      applyPlate();   // rebuilds plate plane + grid for the new bed
      rebuildBbox();  // label sizing is plate-relative
      if (framed) frameCamera(); // re-fit to the new plate extent
    },
    setBoundingBox(visible: boolean) {
      bboxVisible = visible;
      rebuildBbox();
      if (framed) frame();
    },
    setGizmoMode(mode: 'translate' | 'rotate') { control.setMode(mode); if (framed) frame(); },
    getTransform() { return readPivotTransform(); },
    setTransform(t?: PlateTransform) {
      applyTransformToPivot(pivot, t ?? defaultTransform(baseBox));
      rebuildBbox();
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    dropToPlane() {
      const next = dropToPlane(baseBox, readPivotTransform());
      pivot.position.set(next.position[0], next.position[1], next.position[2]);
      pivot.updateMatrixWorld(true);
      rebuildBbox();
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    resetTransform() {
      const next = defaultTransform(baseBox);
      applyTransformToPivot(pivot, next);
      rebuildBbox();
      if (onChangeCb) onChangeCb(next);
      if (framed) frame();
    },
    onTransformChange(cb: (t: PlateTransform) => void) { onChangeCb = cb; },
  };
}

function round(n: number): number { return Math.round(n * 100) / 100; }
function clamp(v: number, lo: number, hi: number): number { return Math.max(lo, Math.min(hi, v)); }
