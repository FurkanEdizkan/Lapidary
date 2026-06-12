// Typed accessor over the prototype's self-contained procedural mesh renderer
// (mesh3d.js sets window.Mesh3D as a side effect). Used to render the seeded
// "proxy" models — those that carry only a mesh_kind and no real mesh file.
import './mesh3dProc.js';

export interface ProcMesh {
  v: number[][];
  f: number[][];
}
export interface RenderOpts {
  rx?: number;
  ry?: number;
  zoom?: number;
  color?: string;
  floor?: boolean;
}
interface Mesh3DApi {
  build(kind: string): ProcMesh;
  render(canvas: HTMLCanvasElement, mesh: ProcMesh, opts: RenderOpts): void;
}

declare global {
  interface Window {
    Mesh3D?: Mesh3DApi;
  }
}

export function getMesh3D(): Mesh3DApi | null {
  return typeof window !== 'undefined' && window.Mesh3D ? window.Mesh3D : null;
}

const cache = new Map<string, ProcMesh>();
export function buildProxy(kind: string): ProcMesh | null {
  const api = getMesh3D();
  if (!api) return null;
  if (!cache.has(kind)) cache.set(kind, api.build(kind));
  return cache.get(kind)!;
}

/** Render a proxy mesh to a fresh canvas and return a PNG data URL (for thumbnails). */
export function proxyThumbnail(kind: string, color: string, size = 560): string | null {
  const api = getMesh3D();
  const mesh = buildProxy(kind);
  if (!api || !mesh) return null;
  const c = document.createElement('canvas');
  c.width = size;
  c.height = size;
  api.render(c, mesh, { ry: 0.85, rx: -0.4, zoom: 0.96, color });
  return c.toDataURL('image/png');
}
