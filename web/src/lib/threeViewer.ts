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

export interface ViewerHandle {
  /** Resolves after the mesh has loaded and the first frame has rendered; rejects on load error. */
  ready: Promise<void>;
  destroy(): void;
  reset(): void;
  thumbnail(): string | null;
  boundingBox(): [number, number, number] | null;
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

/**
 * Mount a viewer into `host`; loads `url` as `format`. Returns a handle SYNCHRONOUSLY
 * (the renderer/canvas exist immediately); `handle.ready` resolves once the mesh is
 * loaded and framed.
 */
export function mountViewer(host: HTMLElement, url: string, format: string): ViewerHandle {
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
  const size = new THREE.Vector3();
  let radius = 1;
  let dist = 3;
  let framed = false;

  function applyCamera() {
    const d = dist / state.zoom;
    camera.position.set(
      d * Math.cos(state.rx) * Math.sin(state.ry),
      d * Math.sin(state.rx) + radius * 0.1,
      d * Math.cos(state.rx) * Math.cos(state.ry),
    );
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
  }
  function frame() { applyCamera(); renderer.render(scene, camera); }

  // Pointer interaction (attached immediately; harmless before the mesh frames).
  let drag: { x: number; y: number } | null = null;
  const onDown = (e: PointerEvent) => { drag = { x: e.clientX, y: e.clientY }; canvas.setPointerCapture(e.pointerId); };
  const onMove = (e: PointerEvent) => {
    if (!drag) return;
    state.ry -= (e.clientX - drag.x) * 0.011;
    state.rx = Math.max(-1.45, Math.min(1.45, state.rx + (e.clientY - drag.y) * 0.011));
    drag = { x: e.clientX, y: e.clientY };
    if (framed) frame();
  };
  const onUp = () => { drag = null; };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    state.zoom = Math.max(0.4, Math.min(3.2, state.zoom * (e.deltaY < 0 ? 1.1 : 0.9)));
    if (framed) frame();
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerup', onUp);
  canvas.addEventListener('wheel', onWheel, { passive: false });

  const ready = loadGeometry(url, format).then((object) => {
    // The caller may have torn us down while the mesh was loading (e.g. StrictMode
    // remount or the overlay closing). If so, drop the just-loaded object and stop.
    if (disposed) { disposeObject(object); return; }
    scene.add(object);

    const box = new THREE.Box3().setFromObject(object);
    const center = new THREE.Vector3();
    box.getSize(size);
    box.getCenter(center);
    object.position.sub(center);
    radius = Math.max(size.x, size.y, size.z) / 2 || 1;
    dist = radius / Math.sin((camera.fov * Math.PI) / 360);
    framed = true;
    frame();
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
    thumbnail() {
      if (!framed) return null;
      try { frame(); return canvas.toDataURL('image/png'); } catch { return null; }
    },
    boundingBox() { return framed ? [round(size.x), round(size.y), round(size.z)] : null; },
  };
}

function round(n: number): number { return Math.round(n * 100) / 100; }
