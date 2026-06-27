import { describe, it, expect } from 'vitest';
import { Box3, Vector3 } from 'three';
import { dropToPlane, defaultTransform, plateLocalMinZ, eulerDegToQuat, quatToEulerDeg, IDENTITY_TRANSFORM } from './plateTransform';

const centeredBox = () => new Box3(new Vector3(-1, -2, -3), new Vector3(1, 2, 3));

describe('plateTransform', () => {
  it('drops the lowest point to z=0', () => {
    const t = dropToPlane(centeredBox(), IDENTITY_TRANSFORM);
    expect(plateLocalMinZ(centeredBox(), t)).toBeCloseTo(0, 6);
    expect(t.position[2]).toBeCloseTo(3, 6); // shifted up by 3 (min was -3)
  });

  it('defaultTransform centers in XY and seats on the plate', () => {
    const t = defaultTransform(centeredBox());
    expect(t.position[0]).toBe(0);
    expect(t.position[1]).toBe(0);
    expect(plateLocalMinZ(centeredBox(), t)).toBeCloseTo(0, 6);
  });

  it('round-trips euler degrees <-> quaternion', () => {
    const q = eulerDegToQuat(90, 0, 0);
    const [x, y, z] = quatToEulerDeg(q);
    expect(x).toBeCloseTo(90, 4);
    expect(y).toBeCloseTo(0, 4);
    expect(z).toBeCloseTo(0, 4);
  });
});
