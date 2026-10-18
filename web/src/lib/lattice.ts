/**
 * The sign-in page's one drawing (A19): a low-poly spur gear with a raised hub and a bore, as unique edges projected
 * at a fixed three-quarter view.
 *
 * Pure and deterministic: no randomness, no clock, no network, so two calls give the same picture and a test can say
 * so. The edges come out in the order they should draw: the front tooth profile first (the silhouette a reader
 * recognises), then the teeth's depth and the back profile, then the hub and the bore as rings, and last the
 * triangulated faces that make it a mesh rather than an outline.
 *
 * `latticeAt(turn)` draws the same gear turned about its axle (A26: sign-in keeps it turning). Its frame is fixed for
 * every turn, the bounds of a whole tooth pitch, so a turning gear never shifts or rescales; turning by `PITCH` looks
 * exactly like not turning, which is what lets the loop run without a seam.
 */

/** `depth` is 0 nearest the reader and 1 farthest; `inner` is the face triangulation rather than the outline. */
export type LatticeEdge = { d: string; depth: number; inner: boolean }

export type Lattice = { viewBox: string; edges: LatticeEdge[] }

const SIZE = 600
const MARGIN = 24

const TEETH = 12
const TIP = 1
const ROOT = 0.84
const HUB = 0.4
const BORE = 0.2
/** Half the face width: the front face at +FACE, the back at -FACE. */
const FACE = 0.15
const HUB_HEIGHT = 0.2

/** The view: a small spin about the axle, the face tipped away from the reader, then turned to the right. */
const SPIN = (7 * Math.PI) / 180
const TILT = (-48 * Math.PI) / 180
const TURN = (20 * Math.PI) / 180
/** A weak perspective, distance in gear radii: enough that the back face reads as farther, not a lens. */
const EYE = 7

type Point = [number, number, number]

function polar(angle: number, radius: number, z: number): Point {
  return [radius * Math.cos(angle), radius * Math.sin(angle), z]
}

/** The tooth profile at one face: root, tip, tip, root for each tooth, in angular order. */
function profileAngles(): { angle: number; radius: number }[] {
  const pitch = (2 * Math.PI) / TEETH
  const out: { angle: number; radius: number }[] = []
  for (let k = 0; k < TEETH; k++) {
    const centre = k * pitch
    out.push({ angle: centre - 0.27 * pitch, radius: ROOT })
    out.push({ angle: centre - 0.12 * pitch, radius: TIP })
    out.push({ angle: centre + 0.12 * pitch, radius: TIP })
    out.push({ angle: centre + 0.27 * pitch, radius: ROOT })
  }
  return out
}

/** One tooth pitch: the turn after which the gear looks the same. */
export const PITCH = (2 * Math.PI) / TEETH

function project([x, y, z]: Point, turn = 0): { x: number; y: number; depth: number } {
  // Spin about the axle (z), tip about x, turn about y.
  const spin = SPIN + turn
  const x1 = x * Math.cos(spin) - y * Math.sin(spin)
  const y1 = x * Math.sin(spin) + y * Math.cos(spin)
  const y2 = y1 * Math.cos(TILT) - z * Math.sin(TILT)
  const z2 = y1 * Math.sin(TILT) + z * Math.cos(TILT)
  const x3 = x1 * Math.cos(TURN) + z2 * Math.sin(TURN)
  const z3 = -x1 * Math.sin(TURN) + z2 * Math.cos(TURN)
  const scale = EYE / (EYE - z3)
  // Screen y grows downwards; depth grows away from the reader.
  return { x: x3 * scale, y: -y2 * scale, depth: -z3 }
}

type Model = { points: Point[]; pairs: [number, number, boolean][]; front: number[]; hub: number[]; bore: number[] }

function model(): Model {
  const points: Point[] = []
  const add = (point: Point) => points.push(point) - 1

  const profile = profileAngles()
  const front = profile.map(({ angle, radius }) => add(polar(angle, radius, FACE)))
  const back = profile.map(({ angle, radius }) => add(polar(angle, radius, -FACE)))
  // One hub point for every two profile points, at the angle of the middle one it fans to, so the face's triangles
  // never cross however unevenly the profile is spaced.
  const hubAngles = profile.filter((_, index) => index % 2 === 1).map(({ angle }) => angle)
  const hubBase = hubAngles.map((angle) => add(polar(angle, HUB, FACE)))
  const hubTop = hubAngles.map((angle) => add(polar(angle, HUB, FACE + HUB_HEIGHT)))
  const bore = hubAngles.map((angle) => add(polar(angle, BORE, FACE + HUB_HEIGHT)))

  const pairs: [number, number, boolean][] = []
  let inner = false
  const seen = new Set<string>()
  const edge = (a: number, b: number) => {
    const key = a < b ? `${a}:${b}` : `${b}:${a}`
    if (a === b || seen.has(key)) return
    seen.add(key)
    pairs.push([a, b, inner])
  }
  const ring = (indices: readonly number[]) => indices.forEach((a, i) => edge(a, indices[(i + 1) % indices.length]!))
  const hubs = hubAngles.length

  ring(front)
  // The teeth's depth: an axial line at every corner of the profile.
  front.forEach((a, i) => edge(a, back[i]!))
  ring(back)
  ring(hubBase)
  hubBase.forEach((a, i) => {
    if (i % 2 === 0) edge(a, hubTop[i]!)
  })
  ring(hubTop)
  ring(bore)
  // The front face between the profile and the hub, as a fan strip.
  inner = true
  for (let i = 0; i < hubs; i++) {
    const p = front.length
    edge(hubBase[i]!, front[(2 * i) % p]!)
    edge(hubBase[i]!, front[(2 * i + 1) % p]!)
    edge(hubBase[i]!, front[(2 * i + 2) % p]!)
  }
  // The hub's top, between its rim and the bore.
  for (let i = 0; i < hubs; i++) {
    edge(hubTop[i]!, bore[i]!)
    edge(hubTop[i]!, bore[(i + 1) % hubs]!)
  }

  return { points, pairs, front, hub: hubTop, bore }
}

const GEAR = model()

/** Bounds over a whole pitch of turning, so every frame of the loop fits the same viewBox at the same scale. */
const FRAME = (() => {
  const samples = 24
  let [minX, maxX, minY, maxY, near, far] = [Infinity, -Infinity, Infinity, -Infinity, Infinity, -Infinity]
  for (let k = 0; k < samples; k++) {
    for (const point of GEAR.points) {
      const p = project(point, (k / samples) * PITCH)
      ;[minX, maxX, minY, maxY] = [Math.min(minX, p.x), Math.max(maxX, p.x), Math.min(minY, p.y), Math.max(maxY, p.y)]
      ;[near, far] = [Math.min(near, p.depth), Math.max(far, p.depth)]
    }
  }
  // As wide as SIZE and as tall as the part needs, so a frame can place the drawing by its own bounds.
  const scale = (SIZE - 2 * MARGIN) / (maxX - minX)
  return { minX, minY, near, far, scale, height: Math.ceil((maxY - minY) * scale + 2 * MARGIN) }
})()

const round = (value: number) => Math.round(value * 10) / 10

/** The gear turned by `turn` radians about its axle: each edge's line and depth, in `lattice()`'s order. */
export function latticeAt(turn: number): { edges: LatticeEdge[]; front: string; hub: string; bore: string } {
  const projected = GEAR.points.map((point) => project(point, turn))
  const at = (i: number) => {
    const p = projected[i]!
    return `${round(MARGIN + (p.x - FRAME.minX) * FRAME.scale)} ${round(MARGIN + (p.y - FRAME.minY) * FRAME.scale)}`
  }
  const ring = (indices: readonly number[]) => `M ${indices.map(at).join(' L ')} Z`
  return {
    edges: GEAR.pairs.map(([a, b, inner]) => ({
      d: `M ${at(a)} L ${at(b)}`,
      depth:
        Math.round(
          (((projected[a]!.depth + projected[b]!.depth) / 2 - FRAME.near) / (FRAME.far - FRAME.near || 1)) * 100,
        ) / 100,
      inner,
    })),
    front: ring(GEAR.front),
    hub: ring(GEAR.hub),
    bore: ring(GEAR.bore),
  }
}

export function lattice(): Lattice {
  return { viewBox: `0 0 ${SIZE} ${FRAME.height}`, edges: latticeAt(0).edges }
}
