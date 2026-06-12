// Imperative Three.js viewer for real mesh files (STL / 3MF / OBJ / GLTF).
// Owns its renderer/scene/camera and disposes ALL GPU resources on destroy(),
// so paging through many models never leaks WebGL contexts or memory.
import * as THREE from 'three';
import { STLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js';
import { ThreeMFLoader } from 'three/examples/jsm/loaders/3MFLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

export interface ViewerHandle {
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

/** Mount a viewer onto a canvas; loads `url` as `format`. Resolves with a handle. */
export async function mountViewer(canvas: HTMLCanvasElement, url: string, format: string): Promise<ViewerHandle> {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio = renderer.setPixelRatio.bind(renderer);
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  const w = canvas.clientWidth || 880;
  const h = canvas.clientHeight || 680;
  renderer.setSize(w, h, false);

  const scene = new THREE.Scene();
  scene.add(new THREE.AmbientLight(0xffffff, 0.55));
  const key = new THREE.DirectionalLight(0xffffff, 1.1);
  key.position.set(-0.45, 0.85, 0.65);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0x88aaff, 0.25);
  fill.position.set(0.6, -0.3, -0.5);
  scene.add(fill);

  const camera = new THREE.PerspectiveCamera(38, w / h, 0.1, 5000);

  const object = await loadGeometry(url, format);
  scene.add(object);

  // Center + frame
  const box = new THREE.Box3().setFromObject(object);
  const size = new THREE.Vector3();
  const center = new THREE.Vector3();
  box.getSize(size);
  box.getCenter(center);
  object.position.sub(center);
  const radius = Math.max(size.x, size.y, size.z) / 2 || 1;
  const dist = radius / Math.sin((camera.fov * Math.PI) / 360);

  const state = { rx: -0.38, ry: 0.85, zoom: 1 };
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
  frame();

  // Pointer interaction
  let drag: { x: number; y: number } | null = null;
  const onDown = (e: PointerEvent) => { drag = { x: e.clientX, y: e.clientY }; canvas.setPointerCapture(e.pointerId); };
  const onMove = (e: PointerEvent) => {
    if (!drag) return;
    state.ry -= (e.clientX - drag.x) * 0.011;
    state.rx = Math.max(-1.45, Math.min(1.45, state.rx + (e.clientY - drag.y) * 0.011));
    drag = { x: e.clientX, y: e.clientY };
    frame();
  };
  const onUp = () => { drag = null; };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    state.zoom = Math.max(0.4, Math.min(3.2, state.zoom * (e.deltaY < 0 ? 1.1 : 0.9)));
    frame();
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerup', onUp);
  canvas.addEventListener('wheel', onWheel, { passive: false });

  return {
    destroy() {
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
    },
    reset() { state.rx = -0.38; state.ry = 0.85; state.zoom = 1; frame(); },
    thumbnail() {
      try { frame(); return renderer.domElement.toDataURL('image/png'); } catch { return null; }
    },
    boundingBox() { return [round(size.x), round(size.y), round(size.z)]; },
  };
}

function round(n: number): number { return Math.round(n * 100) / 100; }
