import { expect, test } from 'vitest'
import { VIEW_DIR, frameBox, hasWebGL } from './viewer-math'

test('the camera frames a box from the thumbnail direction, far enough to fit it', () => {
  // The 22 mm fixture cylinder, 30 mm long.
  const framing = frameBox([-11, -11, 0], [11, 11, 30], 35)

  expect(framing.target).toEqual([0, 0, 15])
  const offset = framing.position.map((value, axis) => value - (framing.target[axis] ?? 0))
  const distance = Math.hypot(...offset)
  const radius = Math.hypot(22, 22, 30) / 2
  expect(distance).toBeCloseTo(radius / Math.sin((35 * Math.PI) / 360), 9)
  offset.forEach((value, axis) => expect(value / distance).toBeCloseTo(VIEW_DIR[axis] ?? 0, 6))
  expect(framing.near).toBeLessThan(distance - radius)
  expect(framing.far).toBeGreaterThan(distance + radius)
})

test('a degenerate box still frames something', () => {
  const framing = frameBox([4, 4, 4], [4, 4, 4], 35)
  expect(Number.isFinite(framing.position[0])).toBe(true)
  expect(framing.near).toBeGreaterThan(0)
})

test('jsdom cannot draw, so the viewer is never loaded there', () => {
  expect(hasWebGL()).toBe(false)
})
