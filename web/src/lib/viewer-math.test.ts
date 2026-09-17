import { expect, test, vi } from 'vitest'
import { AXES, Lru, VIEW_DIR, capPlacement, explodeOffsets, frameBox, hasWebGL, kept, partCentres, sectionPlane, thumbnailFrame, turningFrame, visibleRanges, type Vec3 } from './viewer-math'

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

/** The turntable's model cache: a part looked at again stays, the one nobody returned to goes. */
test('the least recently used model is the one let go, and it is handed back to be freed', () => {
  const freed: string[] = []
  const cache = new Lru<string, string>(2, (model) => freed.push(model))
  cache.set('flange', 'flange mesh')
  cache.set('gear', 'gear mesh')
  expect(cache.get('flange')).toBe('flange mesh')
  cache.set('vee-block', 'vee-block mesh')
  expect(freed).toEqual(['gear mesh'])
  expect(cache.get('gear')).toBeUndefined()
  expect(cache.get('flange')).toBe('flange mesh')
  // Replacing a key frees what it held.
  cache.set('flange', 'flange mesh, reparsed')
  expect(freed).toEqual(['gear mesh', 'flange mesh'])
})

/** An L-shaped bracket's corners, off the origin, as a rung's position buffer would carry them. */
const BRACKET: number[] = [
  [10, 5, 0], [70, 5, 0], [70, 25, 0], [10, 25, 0],
  [10, 5, 6], [70, 5, 6], [70, 25, 6], [10, 25, 6],
  [10, 5, 6], [16, 5, 6], [16, 25, 6], [10, 25, 6],
  [10, 5, 48], [16, 5, 48], [16, 25, 48], [10, 25, 48],
].flat()
const BRACKET_PIVOT: Vec3 = [40, 15, 24]

/** Where a world point lands on screen, relative to a frame, in the frame's own half-heights. */
function onScreen(point: Vec3, frame: { center: Vec3; halfHeight: number }): [number, number] {
  const v = VIEW_DIR
  const r = Math.hypot(v[0], v[1])
  const right: Vec3 = [-v[1] / r, v[0] / r, 0]
  const up: Vec3 = [
    v[1] * right[2] - v[2] * right[1],
    v[2] * right[0] - v[0] * right[2],
    v[0] * right[1] - v[1] * right[0],
  ]
  const d: Vec3 = [point[0] - frame.center[0], point[1] - frame.center[1], point[2] - frame.center[2]]
  const along = (axis: Vec3) => (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]) / frame.halfHeight
  return [along(right), along(up) / Math.hypot(...up)]
}

function corners(positions: number[]): Vec3[] {
  return Array.from({ length: positions.length / 3 }, (_, i) => [positions[3 * i]!, positions[3 * i + 1]!, positions[3 * i + 2]!] as Vec3)
}

test('the thumbnail framing fills 92% of the view with the part, centred, as raster.rs does', () => {
  const frame = thumbnailFrame(BRACKET, BRACKET_PIVOT)
  const points = corners(BRACKET).map((corner) => onScreen(corner, frame))
  const xs = points.map(([x]) => x)
  const ys = points.map(([, y]) => y)
  const spanX = Math.max(...xs) - Math.min(...xs)
  const spanY = Math.max(...ys) - Math.min(...ys)
  // The longer side of the outline is 92% of the view's full height (two half-heights).
  expect(Math.max(spanX, spanY)).toBeCloseTo(2 * 0.92, 6)
  expect((Math.max(...xs) + Math.min(...xs)) / 2).toBeCloseTo(0, 6)
  expect((Math.max(...ys) + Math.min(...ys)) / 2).toBeCloseTo(0, 6)
})

test('the turning framing holds every corner inside the margin at every angle', () => {
  const frame = turningFrame(BRACKET, BRACKET_PIVOT)
  let furthest = 0
  for (let step = 0; step < 360; step++) {
    const angle = (step / 360) * Math.PI * 2
    const [c, s] = [Math.cos(angle), Math.sin(angle)]
    for (const [x, y, z] of corners(BRACKET)) {
      const dx = x - BRACKET_PIVOT[0]
      const dy = y - BRACKET_PIVOT[1]
      const turned: Vec3 = [BRACKET_PIVOT[0] + dx * c - dy * s, BRACKET_PIVOT[1] + dx * s + dy * c, z]
      const [sx, sy] = onScreen(turned, frame)
      expect(Math.abs(sx)).toBeLessThanOrEqual(0.92 + 1e-9)
      expect(Math.abs(sy)).toBeLessThanOrEqual(0.92 + 1e-9)
      furthest = Math.max(furthest, Math.abs(sx), Math.abs(sy))
    }
  }
  // And no looser than it has to be: at some angle some corner reaches the margin.
  expect(furthest).toBeCloseTo(0.92, 3)
})
