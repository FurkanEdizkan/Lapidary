/**
 * The front door's one drawing (A19, A27): a round brilliant, the stone a lapidary cuts, as a faceted wireframe in a
 * three-quarter view with hidden lines, so the facets turned away from the reader are faint.
 *
 * Pure and deterministic: no randomness, no clock, no network, so two calls give the same picture and a test can say
 * so. The edges come out in the order they should draw: the girdle first (the silhouette a reader recognises), then
 * the crown and the table, then the pavilion down to the culet.
 *
 * `frame({ turn, cut, light })` is the stone at any moment of sign-in's loop (A27): `cut` 0 is the rough stone it was
 * cut from, a pebble with the same facets in the wrong places, and 1 the finished brilliant; `turn` is about its axis,
 * and turning by `STEP` (its eightfold symmetry) looks exactly like not turning, which is what lets the loop run
 * without a seam; `light` sweeps the glint. The viewBox is fixed for every frame, so nothing shifts or rescales.
 */

/** One line: how much it faces the reader, 0 behind the stone to 1 on its near side. */
export type LatticeEdge = { d: string; front: number }

/** A facet: `glint` where it mirrors the light into the reader's eye, `cutting` while sign-in's cut is making it. */
export type LatticeFace = { d: string; glint: number; cutting: number }

export type Lattice = { viewBox: string; edges: LatticeEdge[] }

const SIZE = 600
const MARGIN = 24

/** The brilliant's symmetry: eight mains round the crown and the pavilion. The turn after which it looks the same. */
export const STEP = (2 * Math.PI) / 8

/** Proportions of a round brilliant, in girdle radii: a 53% table, a 34.5° crown and a 40.75° pavilion. */
const TABLE = 0.53
const STAR = 0.77
const GIRDLE = 0.03
const CROWN = Math.tan((34.5 * Math.PI) / 180)
const PAVILION = Math.tan((40.75 * Math.PI) / 180)
const LOWER = 0.45

/** The view: the table tipped towards the reader, enough to see into it, with a weak perspective. */
const TILT = (-71 * Math.PI) / 180
const EYE = 7
/**
 * The lights' directions in the view, a studio's three: key from the upper left, fill from the right, one low; `light`
 * turns them about the view's up. A cut stone sparkles from light it returns from inside as well, which a wireframe
 * cannot show, so three outside it stand in for that.
 */
const LIGHTS: Point[] = [normal([-0.45, 0.88, -0.1]), normal([0.7, 0.7, 0.1]), normal([0.2, -0.9, 0.3])]
/** How sharp a glint is: a facet flashes only within a few degrees of mirroring a light. */
const SHINE = 120

type Point = [number, number, number]

function normal([x, y, z]: Point): Point {
  const length = Math.hypot(x, y, z) || 1
  return [x / length, y / length, z / length]
}

/** Deterministic noise in [0, 1) for vertex `i`, channel `s`: the rough stone's lumps, the same every time. */
function noise(i: number, s: number): number {
  let h = Math.imul(i + 1, 0x9e3779b1) ^ Math.imul(s + 7, 0x85ebca6b)
  h = Math.imul(h ^ (h >>> 15), 0x2c1b3c6d)
  h ^= h >>> 12
  return (h >>> 0) / 4294967296
}

/** Newell's normal of a polygon, unnormalised: right-handed round the order its points come in. */
function newell(points: readonly Point[]): Point {
  let [nx, ny, nz] = [0, 0, 0]
  points.forEach((p, i) => {
    const q = points[(i + 1) % points.length]!
    nx += (p[1] - q[1]) * (p[2] + q[2])
    ny += (p[2] - q[2]) * (p[0] + q[0])
    nz += (p[0] - q[0]) * (p[1] + q[1])
  })
  return [nx, ny, nz]
}

type Vertex = { cut: Point; rough: Point; rank: number; order: number }
type Model = { vertices: Vertex[]; faces: number[][]; edges: [number, number, number[]][] }

/** The polished facets come first in `faces`; the 16 after them are the girdle's. */
const POLISHED = 1 + 8 * 7

function model(): Model {
  const vertices: Vertex[] = []
  /** `rank` orders the drawing (girdle first); `order` is when the cut reaches the vertex, 0 first and 1 last. */
  const ring = (count: number, offset: number, radius: number, z: number, rank: number, order: number) =>
    Array.from({ length: count }, (_, k) => {
      const angle = ((k + offset) / count) * 2 * Math.PI
      const i = vertices.length
      const cut: Point = [radius * Math.cos(angle), radius * Math.sin(angle), z]
      // The pebble it was cut from: a little larger everywhere (cutting takes stone away), lumpy, its top domed and
      // its point blunt, each vertex knocked a little round the axis.
      const swell = 1.1 + 0.16 * noise(i, 0)
      const lump = angle + 0.22 * (noise(i, 1) - 0.5)
      const r = radius === 0 ? 0.1 : Math.max(radius, 0.42) * swell
      const rz = z > 0 ? z * 1.35 + 0.08 : z * 0.82 - 0.06
      const rough: Point = [r * Math.cos(lump), r * Math.sin(lump), rz + 0.14 * (noise(i, 2) - 0.5)]
      vertices.push({ cut, rough, rank, order: Math.min(1, order + (k / count) * 0.25) })
      return i
    })
  const table = ring(8, 0, TABLE, GIRDLE + (1 - TABLE) * CROWN, 3, 0.45)
  const star = ring(8, 0.5, STAR, GIRDLE + (1 - STAR) * CROWN, 2, 0.3)
  const upper = ring(16, 0, 1, GIRDLE, 0, 0)
  const lower = ring(16, 0, 1, -GIRDLE, 1, 0.1)
  const mains = ring(8, 0, LOWER, -GIRDLE - (1 - LOWER) * PAVILION, 4, 0.55)
  const [culet] = ring(1, 0, 0, -GIRDLE - PAVILION, 5, 0.75)

  const at = (indices: readonly number[], k: number) => indices[((k % indices.length) + indices.length) % indices.length]!
  const faces: number[][] = [table]
  for (let k = 0; k < 8; k++) {
    faces.push([at(table, k), at(star, k), at(table, k + 1)])
    faces.push([at(star, k), at(upper, 2 * k + 2), at(star, k + 1), at(table, k + 1)])
    faces.push([at(star, k), at(upper, 2 * k + 1), at(upper, 2 * k + 2)])
    faces.push([at(star, k), at(upper, 2 * k), at(upper, 2 * k + 1)])
    faces.push([at(lower, 2 * k + 1), at(lower, 2 * k), at(mains, k)])
    faces.push([at(lower, 2 * k + 2), at(lower, 2 * k + 1), at(mains, k + 1)])
    faces.push([at(mains, k), at(lower, 2 * k + 1), at(mains, k + 1), culet!])
  }
  // The girdle last: it is ground matte, so it never glints (see `frame`).
  for (let k = 0; k < 16; k++) faces.push([at(upper, k + 1), at(upper, k), at(lower, k), at(lower, k + 1)])

  // Wound anticlockwise seen from outside, whatever order they were listed in: the stone is convex, so outward is away
  // from a point inside it.
  for (const face of faces) {
    const points = face.map((a) => vertices[a]!.cut)
    const [nx, ny, nz] = newell(points)
    const [cx, cy, cz] = points.reduce(([x, y, z], p) => [x + p[0], y + p[1], z + p[2]], [0, 0, 0])
    if (nx * cx + ny * cy + nz * (cz + 0.3 * points.length) < 0) face.reverse()
  }

  const shared = new Map<string, [number, number, number[]]>()
  faces.forEach((face, f) =>
    face.forEach((a, i) => {
      const b = face[(i + 1) % face.length]!
      const key = a < b ? `${a}:${b}` : `${b}:${a}`
      const edge = shared.get(key) ?? [Math.min(a, b), Math.max(a, b), []]
      edge[2].push(f)
      shared.set(key, edge)
    }),
  )
  const rank = ([a, b]: [number, number, number[]]) => vertices[a]!.rank + vertices[b]!.rank
  const edges = [...shared.values()].sort((x, y) => rank(x) - rank(y) || x[0] - y[0] || x[1] - y[1])
  return { vertices, faces, edges }
}

const STONE = model()

const ease = (t: number) => t * t * (3 - 2 * t)
const clamp = (t: number) => Math.min(1, Math.max(0, t))

/** How far the cut has reached vertex `v`: each moves from rough to cut over a third of the run, in turn. */
function reached(v: Vertex, cut: number): number {
  return ease(clamp((cut - 0.7 * v.order) / 0.3))
}

/** Turn about the stone's axis (z), then tip it towards the reader about x. Screen y grows upwards here. */
function view([x, y, z]: Point, turn: number): Point {
  const x1 = x * Math.cos(turn) - y * Math.sin(turn)
  const y1 = x * Math.sin(turn) + y * Math.cos(turn)
  return [x1, y1 * Math.cos(TILT) - z * Math.sin(TILT), y1 * Math.sin(TILT) + z * Math.cos(TILT)]
}

function place(turn: number, cut: number): Point[] {
  return STONE.vertices.map((v) => {
    const t = reached(v, cut)
    const [a, b] = [v.rough, v.cut]
    return view([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t], turn)
  })
}

/** Bounds over a whole step of turning, rough and cut, so every frame of the loop fits one viewBox at one scale. */
const BOX = (() => {
  let [minX, maxX, minY, maxY] = [Infinity, -Infinity, Infinity, -Infinity]
  for (const cut of [0, 0.25, 0.5, 0.75, 1]) {
    for (let k = 0; k < 16; k++) {
      for (const [x, y, z] of place((k / 16) * STEP, cut)) {
        const scale = EYE / (EYE - z)
        ;[minX, maxX] = [Math.min(minX, x * scale), Math.max(maxX, x * scale)]
        ;[minY, maxY] = [Math.min(minY, -y * scale), Math.max(maxY, -y * scale)]
      }
    }
  }
  const scale = (SIZE - 2 * MARGIN) / (maxX - minX)
  return { minX, minY, scale, height: Math.ceil((maxY - minY) * scale + 2 * MARGIN) }
})()

export const VIEWBOX = `0 0 ${SIZE} ${BOX.height}`

const round = (value: number) => Math.round(value * 10) / 10
const round2 = (value: number) => Math.round(value * 100) / 100

/** The stone at one moment: every line in `lattice()`'s order with how much it faces the reader, and every facet. */
export function frame({ turn = 0, cut = 1, light = 0 }: { turn?: number; cut?: number; light?: number } = {}): {
  edges: LatticeEdge[]
  faces: LatticeFace[]
} {
  const points = place(turn, cut)
  const screen = points.map(([x, y, z]) => {
    const scale = EYE / (EYE - z)
    return `${round(MARGIN + (x * scale - BOX.minX) * BOX.scale)} ${round(MARGIN + (-y * scale - BOX.minY) * BOX.scale)}`
  })
  // Halfway between each light and the reader's eye: a facet facing that way mirrors the light into it.
  const halves = LIGHTS.map(([x, y, z]) =>
    normal([x * Math.cos(light) + z * Math.sin(light), y, z * Math.cos(light) - x * Math.sin(light) + 1]),
  )
  const facets = STONE.faces.map((face, f) => {
    // Outward, because `model()` wound every face anticlockwise seen from outside.
    const n = normal(newell(face.map((a) => points[a]!)))
    const mirror = Math.max(0, ...halves.map((h) => n[0] * h[0] + n[1] * h[1] + n[2] * h[2]))
    const moving = face.reduce((sum, a) => sum + reached(STONE.vertices[a]!, cut), 0) / face.length
    return {
      facing: n[2],
      d: `M ${face.map((a) => screen[a]).join(' L ')} Z`,
      glint: n[2] > 0 && f < POLISHED ? round2(mirror ** SHINE) : 0,
      cutting: round2(Math.sin(Math.PI * moving)),
    }
  })
  return {
    edges: STONE.edges.map(([a, b, adjacent]) => ({
      d: `M ${screen[a]} L ${screen[b]}`,
      front: round2(clamp((Math.max(...adjacent.map((f) => facets[f]!.facing)) + 0.05) / 0.2)),
    })),
    faces: facets.map(({ d, glint, cutting }) => ({ d, glint, cutting })),
  }
}

/** The finished brilliant at rest: the settled drawing, and the still stand-in for a missing picture. */
export function lattice(): Lattice {
  return { viewBox: VIEWBOX, edges: frame().edges }
}

/** The rough stone sign-in draws before it cuts it. */
export function rough(): Lattice {
  return { viewBox: VIEWBOX, edges: frame({ cut: 0 }).edges }
}
