import { expect, test } from 'vitest'
import { VIEW_DIR, frameBox, hasWebGL, visibleRanges } from './viewer-math'

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
