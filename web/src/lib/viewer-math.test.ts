import { expect, test, vi } from 'vitest'
import { AXES, VIEW_DIR, capPlacement, explodeOffsets, frameBox, hasWebGL, kept, partCentres, sectionPlane, visibleRanges, type Vec3 } from './viewer-math'

test('the camera frames a box from the thumbnail direction, without perspective, holding it whole', () => {
  // The 22 mm fixture cylinder, 30 mm long.
  const framing = frameBox([-11, -11, 0], [11, 11, 30])

  expect(framing.target).toEqual([0, 0, 15])
  const offset = framing.position.map((value, axis) => value - (framing.target[axis] ?? 0))
  const distance = Math.hypot(...offset)
  const radius = Math.hypot(22, 22, 30) / 2
  const norm = Math.hypot(...VIEW_DIR)
  offset.forEach((value, axis) => expect(value / distance).toBeCloseTo((VIEW_DIR[axis] ?? 0) / norm, 9))
  expect(framing.halfHeight).toBeGreaterThanOrEqual(radius)
  expect(framing.near).toBeGreaterThan(0)
  expect(framing.near).toBeLessThan(distance - radius)
  expect(framing.far).toBeGreaterThan(distance + radius)
})

test('a degenerate box still frames something', () => {
  const framing = frameBox([4, 4, 4], [4, 4, 4])
  expect(Number.isFinite(framing.position[0])).toBe(true)
  expect(framing.near).toBeGreaterThan(0)
})

test('jsdom cannot draw, so the viewer is never loaded there', () => {
  expect(hasWebGL()).toBe(false)
})

test('asking whether the browser can draw lets the probe context go', async () => {
  const loseContext = vi.fn()
  vi.stubGlobal('WebGL2RenderingContext', class {})
  const canvas = document.createElement('canvas')
  vi.spyOn(canvas, 'getContext').mockReturnValue({
    getExtension: (name: string) => (name === 'WEBGL_lose_context' ? { loseContext } : null),
  } as never)
  vi.spyOn(document, 'createElement').mockReturnValue(canvas)
  vi.resetModules()
  const { hasWebGL: askedAfresh } = await import('./viewer-math')

  expect(askedAfresh()).toBe(true)
  expect(loseContext).toHaveBeenCalledTimes(1)
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

test('hidden parts are left out of the ranges drawn, and the visible ones between merge', () => {
  expect(visibleRanges([2, 3, 4], new Set())).toEqual([{ start: 0, count: 27 }])
  expect(visibleRanges([2, 3, 4], new Set([1]))).toEqual([
    { start: 0, count: 6 },
    { start: 15, count: 12 },
  ])
  expect(visibleRanges([2, 0, 4], new Set([0]))).toEqual([{ start: 6, count: 12 }])
  expect(visibleRanges([2, 3, 4], new Set([0, 1, 2]))).toEqual([])
})

test('a section keeps what is below the cut on its axis, and flipped keeps what is above', () => {
  // The flange's box, 30 mm tall: a cut halfway sits at 15 mm.
  const min = [-40, -40, 0] as const
  const max = [40, 40, 30] as const
  const below = sectionPlane('z', 0.5, false, min, max)
  expect(kept(below, [0, 0, 14])).toBe(true)
  expect(kept(below, [0, 0, 16])).toBe(false)
  const above = sectionPlane('z', 0.5, true, min, max)
  expect(kept(above, [0, 0, 14])).toBe(false)
  expect(kept(above, [0, 0, 16])).toBe(true)
  // A point on the cut itself is kept from either side, so a face lying in the plane still counts.
  expect(kept(below, [0, 0, 15])).toBe(true)
  expect(kept(above, [0, 0, 15])).toBe(true)
  // Each axis cuts across its own extent.
  expect(kept(sectionPlane('x', 0.25, false, min, max), [-21, 0, 0])).toBe(true)
  expect(kept(sectionPlane('x', 0.25, false, min, max), [-19, 0, 0])).toBe(false)
  expect(kept(sectionPlane('y', 1, false, min, max), [0, 40, 0])).toBe(true)
  expect(AXES).toEqual(['x', 'y', 'z'])
})

test('a cap lies on the cut, over the whole box, whichever side is kept', () => {
  // A flange's box: 80 mm across, 16 mm tall, cut halfway up.
  const min: [number, number, number] = [-40, -40, 0]
  const max: [number, number, number] = [40, 40, 16]
  for (const flip of [false, true]) {
    const cap = capPlacement(sectionPlane('z', 0.5, flip, min, max), min, max)
    expect(cap.position).toEqual([0, 0, 8])
    expect(Math.abs(cap.normal[2])).toBe(1)
    expect(cap.size).toBeGreaterThanOrEqual(Math.hypot(80, 80))
  }
  expect(capPlacement(sectionPlane('x', 0.25, false, min, max), min, max).position).toEqual([-20, 0, 8])
})

test('a part’s centre is the middle of the box around the corners its own triangles use', () => {
  // A triangle across x 0 to 2, then one across x 10 to 14 and y 0 to 4, at z 5.
  const positions = [0, 0, 0, 2, 0, 0, 0, 2, 0, 10, 0, 5, 14, 0, 5, 10, 4, 5]
  expect(partCentres(positions, [0, 1, 2, 3, 4, 5], [1, 1])).toEqual([
    [1, 1, 0],
    [12, 2, 5],
  ])
})

test('drawn apart, each part moves out from the centre by as far again as it is, and not at all at none', () => {
  const centres: Vec3[] = [
    [10, 0, 0],
    [0, 0, 0],
    [-5, 5, 20],
  ]
  expect(explodeOffsets(centres, [0, 0, 0], 0).every((offset) => offset.every((v) => v === 0))).toBe(true)
  expect(explodeOffsets(centres, [0, 0, 0], 1)).toEqual([
    [10, 0, 0],
    [0, 0, 0],
    [-5, 5, 20],
  ])
  expect(explodeOffsets(centres, [0, 0, 10], 0.5)[2]).toEqual([-2.5, 2.5, 5])
})
