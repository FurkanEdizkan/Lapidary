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
  opts: { transform?: PlateTransform } = {},
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

  // Subtle plate plane + grid at z = GROUND_Z (in plate/print space).
  const planeMat = new THREE.MeshBasicMaterial({ color: 0x1a1a1e, transparent: true, opacity: 0.6, depthWrite: false, side: THREE.DoubleSide });
  const planeMesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), planeMat); // sized after we know the model
  planeMesh.position.z = GROUND_Z;
  const grid = new THREE.GridHelper(1, 10, 0x2cb4f5, 0x2a2a30);
  (grid.material as THREE.Material).opacity = 0.35;
  (grid.material as THREE.Material).transparent = true;
  grid.rotation.x = Math.PI / 2; // GridHelper is XZ (Y-up) by default; rotate into plate's XY
  grid.position.z = GROUND_Z;
  grid.visible = false; // hidden until edit mode; setEditMode(true) reveals it
  plate.add(planeMesh, grid);

  // Pivot holds the saved transform; the mesh is offset so it is centered on the pivot origin.
  const pivot = new THREE.Group();
  plate.add(pivot);

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
    dist = radius / Math.sin((camera.fov * Math.PI) / 360);
    target.copy(wcenter);
    frame();
  }

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
  control.addEventListener('objectChange', () => { if (onChangeCb) onChangeCb(readPivotTransform()); });
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

    // Size the plate/grid to ~1.5x the model footprint, clamped to a sensible range.
    const footprint = Math.max(size.x, size.y, 1);
    const plateSize = Math.min(256, Math.max(80, Math.ceil(1.5 * footprint)));
    planeMesh.geometry.dispose();
    planeMesh.geometry = new THREE.PlaneGeometry(plateSize, plateSize);
    grid.scale.setScalar(plateSize); // base GridHelper is size 1

    // Apply the saved transform, or the default (centered + dropped to plate).
    const t = opts.transform ?? defaultTransform(baseBox);
    applyTransformToPivot(pivot, t);

    framed = true;
    frameCamera();
  });

  return {
    ready,
    destroy() {
      if (disposed) return;
      disposed = true;
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
      grid.geometry.dispose();
      (grid.material as THREE.Material).dispose();
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
      const pv = planeMesh.visible, gv = grid.visible, cv = controlHelper.visible;
      if (hide) { planeMesh.visible = false; grid.visible = false; controlHelper.visible = false; }
      try { frame(); return canvas.toDataURL('image/png'); }
      finally { if (hide) { planeMesh.visible = pv; grid.visible = gv; controlHelper.visible = cv; frame(); } }
    },
    boundingBox() { return framed ? [round(size.x), round(size.y), round(size.z)] : null; },
    setEditMode(on: boolean) {
      if (on) control.attach(pivot); else control.detach();
      control.enabled = on; control.visible = on; controlHelper.visible = on;
      planeMesh.visible = true; grid.visible = on; // plate always visible; grid only while editing
      if (framed) frame();
    },
    setGizmoMode(mode: 'translate' | 'rotate') { control.setMode(mode); if (framed) frame(); },
    getTransform() { return readPivotTransform(); },
    setTransform(t?: PlateTransform) {
      applyTransformToPivot(pivot, t ?? defaultTransform(baseBox));
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    dropToPlane() {
      const next = dropToPlane(baseBox, readPivotTransform());
      pivot.position.set(next.position[0], next.position[1], next.position[2]);
      pivot.updateMatrixWorld(true);
      if (onChangeCb) onChangeCb(readPivotTransform());
      if (framed) frame();
    },
    resetTransform() {
      const next = defaultTransform(baseBox);
      applyTransformToPivot(pivot, next);
      if (onChangeCb) onChangeCb(next);
      if (framed) frame();
    },
    onTransformChange(cb: (t: PlateTransform) => void) { onChangeCb = cb; },
  };
}

function round(n: number): number { return Math.round(n * 100) / 100; }
