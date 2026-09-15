import { expect, test } from 'vitest'
import { AXES, VIEW_DIR, capPlacement, frameBox, hasWebGL, kept, sectionPlane, visibleRanges } from './viewer-math'

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

