/**
 * The sign-in page's one drawing (A19): a low-poly spur gear with a raised hub and a bore, as unique edges projected
 * at a fixed three-quarter view.
 *
 * Pure and deterministic: no randomness, no clock, no network, so two calls give the same picture and a test can say
 * so. The edges come out in the order they should draw: the front tooth profile first (the silhouette a reader
 * recognises), then the teeth's depth and the back profile, then the hub and the bore as rings, and last the
 * triangulated faces that make it a mesh rather than an outline.
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

function project([x, y, z]: Point): { x: number; y: number; depth: number } {
  // Spin about the axle (z), tip about x, turn about y.
  const x1 = x * Math.cos(SPIN) - y * Math.sin(SPIN)
  const y1 = x * Math.sin(SPIN) + y * Math.cos(SPIN)
  const y2 = y1 * Math.cos(TILT) - z * Math.sin(TILT)
  const z2 = y1 * Math.sin(TILT) + z * Math.cos(TILT)
  const x3 = x1 * Math.cos(TURN) + z2 * Math.sin(TURN)
  const z3 = -x1 * Math.sin(TURN) + z2 * Math.cos(TURN)
  const scale = EYE / (EYE - z3)
  // Screen y grows downwards; depth grows away from the reader.
  return { x: x3 * scale, y: -y2 * scale, depth: -z3 }
}

export function lattice(): Lattice {
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

  const projected = points.map(project)
  const xs = projected.map((p) => p.x)
  const ys = projected.map((p) => p.y)
  const depths = projected.map((p) => p.depth)
  const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]
  const [near, far] = [Math.min(...depths), Math.max(...depths)]
  // As wide as SIZE and as tall as the part needs, so a frame can place the drawing by its own bounds.
  const scale = (SIZE - 2 * MARGIN) / (maxX - minX)
  const height = Math.ceil((maxY - minY) * scale + 2 * MARGIN)
  const offsetX = MARGIN
  const offsetY = MARGIN
  const round = (value: number) => Math.round(value * 10) / 10
  const at = (i: number) => {
    const p = projected[i]!
    return `${round(offsetX + (p.x - minX) * scale)} ${round(offsetY + (p.y - minY) * scale)}`
  }

  return {
    viewBox: `0 0 ${SIZE} ${height}`,
    edges: pairs.map(([a, b, inner]) => ({
      d: `M ${at(a)} L ${at(b)}`,
      depth: Math.round((((projected[a]!.depth + projected[b]!.depth) / 2 - near) / (far - near || 1)) * 100) / 100,
      inner,
    })),
  }
}
