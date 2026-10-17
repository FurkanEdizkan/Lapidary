import { expect, test, vi } from 'vitest'
import { VIEW_DIR, frameBox, hasWebGL, between, gridStep, millimetres } from './viewer-math'

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

test('the grid takes the 1, 2 or 5 step that puts about ten squares across the part', () => {
  expect(gridStep(40)).toBe(5)
  expect(gridStep(721.6)).toBe(50)
  expect(gridStep(18.5)).toBe(2)
  expect(gridStep(0.4)).toBeCloseTo(0.05)
  expect(gridStep(3000)).toBe(200)
  expect(gridStep(0)).toBe(1)
})

test('a measurement is the straight line between two picks and its run along each axis', () => {
  const { distance, delta } = between([0, 0, 0], [3, -4, 12])
  expect(distance).toBe(13)
  expect(delta).toEqual([3, -4, 12])
  expect(millimetres(721.619)).toBe('721.6')
  expect(millimetres(18.48)).toBe('18.48')
  expect(millimetres(0.4567)).toBe('0.457')
})
