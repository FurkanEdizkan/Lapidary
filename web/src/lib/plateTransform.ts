// Pure, GL-free transform math for the build-plate editor. Print-space: Z-up, mm.
// Imports only three's math classes (safe in Node — no WebGL/DOM).
import { Box3, Euler, Matrix4, Quaternion, Vector3 } from 'three';

export interface PlateTransform {
  position: [number, number, number];
  quaternion: [number, number, number, number];
  scale: number;
}

export const IDENTITY_TRANSFORM: PlateTransform = {
  position: [0, 0, 0], quaternion: [0, 0, 0, 1], scale: 1,
};

export function transformToMatrix(t: PlateTransform): Matrix4 {
  return new Matrix4().compose(
    new Vector3(t.position[0], t.position[1], t.position[2]),
    new Quaternion(t.quaternion[0], t.quaternion[1], t.quaternion[2], t.quaternion[3]),
    new Vector3(t.scale, t.scale, t.scale),
  );
}

/** Plate-local minimum Z of a centered mesh box `baseBox` placed under transform `t`. */
export function plateLocalMinZ(baseBox: Box3, t: PlateTransform): number {
  return baseBox.clone().applyMatrix4(transformToMatrix(t)).min.z;
}

/** New transform with Z shifted so the lowest point sits at z = 0. */
export function dropToPlane(baseBox: Box3, t: PlateTransform): PlateTransform {
  const minZ = plateLocalMinZ(baseBox, t);
  return { ...t, position: [t.position[0], t.position[1], t.position[2] - minZ] };
}

/** Default: centered over plate origin in X/Y, seated on the plate. */
export function defaultTransform(baseBox: Box3): PlateTransform {
  return dropToPlane(baseBox, { ...IDENTITY_TRANSFORM, position: [0, 0, 0] });
}

const D2R = Math.PI / 180;
const R2D = 180 / Math.PI;

export function eulerDegToQuat(xDeg: number, yDeg: number, zDeg: number): [number, number, number, number] {
  const q = new Quaternion().setFromEuler(new Euler(xDeg * D2R, yDeg * D2R, zDeg * D2R, 'XYZ'));
  return [q.x, q.y, q.z, q.w];
}

export function quatToEulerDeg(q: [number, number, number, number]): [number, number, number] {
  const e = new Euler().setFromQuaternion(new Quaternion(q[0], q[1], q[2], q[3]), 'XYZ');
  return [e.x * R2D, e.y * R2D, e.z * R2D];
}
