import type { Approximate, AssemblyNode, AssemblyTree, Entity } from './types'
import type { Vec3 } from './viewer-math'

/**
 * Measurement on the 3D view, kept free of three.js so it can be tested without a GPU.
 *
 * A pick is where a click met the drawn mesh: the point, and the corners and outward normal of the
 * triangle it met. Where that triangle lies on an analytic entity the kernel read — a plane, a
 * cylinder, a cone, a sphere, a torus, a circular edge — a value comes from the entity and is exact, except
 * where it also depends on where the click fell. Anything else comes from
 * the picked points and is approximate. Every value is an `Approximate`, so none reaches the screen
 * without saying which it is.
 */

export type Tool = 'distance' | 'edge' | 'diameter' | 'angle' | 'wall'

export const TOOLS: readonly Tool[] = ['distance', 'edge', 'diameter', 'angle', 'wall']

/**
 * The most picks a tool takes. A diameter picked on a round face or edge has its value at the
 * first, and one fitted through points on a mesh at the third. A wall's second pick is not
 * clicked: the viewer finds it with a ray straight through the wall.
 */
export const PICKS: Record<Tool, number> = { distance: 2, edge: 2, diameter: 3, angle: 2, wall: 2 }

/** Where a click met the mesh, in the part's millimetres. `normal` is the triangle's, facing out. */
export type Pick = { point: Vec3; normal: Vec3; corners: readonly [Vec3, Vec3, Vec3] }

type Transform = AssemblyNode['transform']

/**
 * How far a triangle's corner may be from a surface and still be on it.
 *
 * Corners, not the clicked point: the bridge's mesher puts every node on the B-rep surface, and
 * only a triangle's middle cuts inside it, by up to the 0.1 mm deflection. Tested at its corners,
 * a triangle of a shaft stepping from 10.00 to 10.10 mm snaps to the step it is on; tested at the
 * point, it would snap to whichever radius the chord sags toward, and call that exact. The
 * thousandth covers float32 positions on parts up to a few metres. L0 and L1 are clustered off the
 * surface, so nothing snaps on them, which is why the viewer measures on L2.
 */
const ON_SURFACE_MM = 1e-3

/** The bridge meshes at 0.5 rad of angular deflection, so no facet turns further than that from its surface. */
const FACING = Math.cos(0.5)

/** Planes this near parallel have one distance between them. A drafted face is not one of them. */
const PARALLEL = 1 - 1e-12

/**
 * Every entity where the mesh drew it. The kernel wrote each once, in its prototype's own
 * coordinates; a prototype is drawn wherever a node of the tree names it, through that node's
 * row-major transform and every one above it. A part without a tree is drawn as it was written.
 */
export function placeEntities(entities: readonly Entity[], tree: AssemblyTree | null): readonly Entity[] {
  if (tree === null) return entities
  const byPrototype = new Map<string, Entity[]>()
  for (const entity of entities) {
    const own = byPrototype.get(entity.prototype)
    if (own === undefined) byPrototype.set(entity.prototype, [entity])
    else own.push(entity)
  }
  const placed: Entity[] = []
  const walk = (node: AssemblyNode, above: readonly Transform[]) => {
    const chain = [...above, node.transform]
    for (const entity of byPrototype.get(node.prototype) ?? []) placed.push(place(entity, chain))
    for (const child of node.children) walk(child, chain)
  }
  for (const root of tree.roots) walk(root, [])
  return placed
}

/**
 * `tree` without the placed parts `hidden` names by their depth-first place among its leaves, the order the view
 * counts triangles in, so what is placed from it is only what the view still draws.
 */
export function withoutParts(tree: AssemblyTree, hidden: ReadonlySet<number>): AssemblyTree {
  let leaf = 0
  let removed = 0
  const keep = (nodes: readonly AssemblyNode[]): AssemblyNode[] =>
    nodes.flatMap((node) => {
      if (node.children.length > 0) return [{ ...node, children: keep(node.children) }]
      if (!hidden.has(leaf++)) return [node]
      removed++
      return []
    })
  const roots = keep(tree.roots)
  return { ...tree, roots, parts: tree.parts - removed }
}

/**
 * The entity of one of `kinds` a picked triangle lies on, or `null` when it lies on none.
 *
 * On a plane or a cylinder, every corner is on the surface and the triangle faces the way the
 * surface does. On a circular edge, two corners are on the ring: a side of the triangle runs along
 * the edge. Of several, the one the corners are nearest wins.
 *
 * ponytail: surfaces are matched untrimmed, so a triangle on one face also lies on every face
 * coplanar with it, or coaxial at the same radius. The value is the same from either, so no tool
 * is wrong for it, but nothing can say which face was picked. Per-triangle face ids from the
 * bridge would, when a tool needs to.
 */
export function snap(pick: Pick, entities: readonly Entity[], kinds: readonly Entity['type'][]): Entity | null {
  let best: Entity | null = null
  let nearest = ON_SURFACE_MM
  for (const entity of entities) {
    if (!kinds.includes(entity.type)) continue
    const off = offSurface(pick, entity)
    if (off !== null && off <= nearest) {
      best = entity
      nearest = off
    }
  }
  return best
}

/** Of a triangle's corners, the one nearest a click: an edge runs between B-rep vertices, which are corners of L2's triangles. */
export function nearestCorner(point: Vec3, corners: Pick['corners']): Vec3 {
  return corners.reduce((best, corner) => (length(sub(corner, point)) < length(sub(best, point)) ? corner : best))
}

/** A tool's value from its picks so far, or `null` while it needs more of them. */
export function measure(tool: Tool, picks: readonly Pick[], entities: readonly Entity[]): Approximate<number> | null {
  const [a, b, c] = picks
  if (a === undefined) return null
  switch (tool) {
    case 'diameter': {
      // A round face before a ring: a triangle along a face's rim lies on the rim's circle too, and the face is what
      // was clicked. A ring reads only from a triangle on no round face, such as the flat face around a hole.
      const round = snap(a, entities, ['cylinder', 'sphere', 'torus', 'cone']) ?? snap(a, entities, ['circle'])
      if (round?.type === 'cylinder' || round?.type === 'circle' || round?.type === 'sphere') return exact(2 * round.radius)
      // A torus's round face is its tube: the diameter a fillet or an O-ring groove is drawn with.
      if (round?.type === 'torus') return exact(2 * round.minor_radius)
      // A cone has a diameter only at a height, and the height is where the click met the mesh.
      if (round?.type === 'cone') return approximate(2 * coneRadius(round, a.point))
      const fit = b === undefined || c === undefined ? null : circumdiameter(a.point, b.point, c.point)
      return fit === null ? null : approximate(fit)
    }
    case 'angle': {
      // A cone's included angle, as a drawing states a countersink: exact, and from the latest click alone, so a
      // click on a cone reads it whatever was clicked before.
      const cone = snap(picks[picks.length - 1] ?? a, entities, ['cone'])
      if (cone?.type === 'cone') return exact((Math.abs(cone.semi_angle_rad) * 360) / Math.PI)
      if (b === undefined) return null
      const first = snap(a, entities, ['plane'])
      const second = snap(b, entities, ['plane'])
      return first?.type === 'plane' && second?.type === 'plane'
        ? exact(degrees(outward(first.normal, a.normal), outward(second.normal, b.normal)))
        : approximate(degrees(a.normal, b.normal))
    }
    case 'wall': {
      if (b === undefined) return null
      const near = snap(a, entities, ['plane'])
      const far = snap(b, entities, ['plane'])
      return near?.type === 'plane' && far?.type === 'plane' && Math.abs(dot(near.normal, far.normal)) >= PARALLEL
        ? exact(Math.abs(dot(near.normal, sub(far.origin, near.origin))))
        : approximate(length(sub(b.point, a.point)))
    }
    case 'distance':
    case 'edge':
      return b === undefined ? null : approximate(length(sub(b.point, a.point)))
  }
}

function offSurface({ point, normal, corners }: Pick, entity: Entity): number | null {
  switch (entity.type) {
    case 'plane':
      return facing(entity.normal, normal)
        ? Math.max(...corners.map((corner) => Math.abs(dot(entity.normal, sub(corner, entity.origin)))))
        : null
    case 'cylinder': {
      const radial = fromAxis(point, entity.origin, entity.axis)
      const out = length(radial)
      if (out === 0 || !facing(scale(radial, 1 / out), normal)) return null
      return Math.max(
        ...corners.map((corner) => Math.abs(length(fromAxis(corner, entity.origin, entity.axis)) - entity.radius)),
      )
    }
    case 'circle': {
      const [, second] = corners
        .map((corner) => {
          const v = sub(corner, entity.center)
          const height = dot(v, entity.normal)
          return Math.hypot(length(sub(v, scale(entity.normal, height))) - entity.radius, height)
        })
        .sort((x, y) => x - y)
      return second ?? null
    }
    case 'sphere': {
      const out = sub(point, entity.center)
      const reach = length(out)
      if (reach === 0 || !facing(scale(out, 1 / reach), normal)) return null
      return Math.max(...corners.map((corner) => Math.abs(length(sub(corner, entity.center)) - entity.radius)))
    }
    case 'cone': {
      const radial = fromAxis(point, entity.origin, entity.axis)
      const out = length(radial)
      if (out === 0) return null
      const [cos, sin] = [Math.cos(entity.semi_angle_rad), Math.sin(entity.semi_angle_rad)]
      if (!facing(sub(scale(radial, cos / out), scale(entity.axis, sin)), normal)) return null
      // A corner's gap from the radius at its own height, turned square to the surface.
      return Math.max(
        ...corners.map((corner) => Math.abs((length(fromAxis(corner, entity.origin, entity.axis)) - coneRadius(entity, corner)) * cos)),
      )
    }
    case 'torus': {
      const toPoint = fromTube(point, entity)
      const out = toPoint === null ? 0 : length(toPoint)
      if (toPoint === null || out === 0 || !facing(scale(toPoint, 1 / out), normal)) return null
      return Math.max(
        ...corners.map((corner) => {
          const v = fromTube(corner, entity)
          return v === null ? Number.POSITIVE_INFINITY : Math.abs(length(v) - entity.minor_radius)
        }),
      )
    }
  }
}

/** A cone's radius at the height of `p` along its axis. */
function coneRadius(cone: Extract<Entity, { type: 'cone' }>, p: Vec3): number {
  return cone.ref_radius + dot(sub(p, cone.origin), cone.axis) * Math.tan(cone.semi_angle_rad)
}

/** From the nearest point of a torus's centre circle to `p`, or `null` for a point on its axis, where every point of that circle is as near. */
function fromTube(p: Vec3, torus: Extract<Entity, { type: 'torus' }>): Vec3 | null {
  const radial = fromAxis(p, torus.origin, torus.axis)
  const out = length(radial)
  return out === 0 ? null : sub(sub(p, torus.origin), scale(radial, torus.major_radius / out))
}

function place(entity: Entity, chain: readonly Transform[]): Entity {
  const at = (v: Vec3) => transform(chain, v, 1)
  const along = (v: Vec3) => transform(chain, v, 0)
  switch (entity.type) {
    case 'plane':
      return { ...entity, origin: at(entity.origin), normal: along(entity.normal) }
    case 'circle':
      return { ...entity, center: at(entity.center), normal: along(entity.normal) }
    case 'sphere':
      return { ...entity, center: at(entity.center) }
    case 'cylinder':
    case 'cone':
    case 'torus':
      return { ...entity, origin: at(entity.origin), axis: along(entity.axis) }
  }
}

/** A point (`w` 1) or a direction (`w` 0) through a chain of transforms, the innermost last in the chain and first applied. */
function transform(chain: readonly Transform[], v: Vec3, w: 0 | 1): [number, number, number] {
  return chain.reduceRight<[number, number, number]>(
    (q, m) => [
      m[0] * q[0] + m[1] * q[1] + m[2] * q[2] + m[3] * w,
      m[4] * q[0] + m[5] * q[1] + m[6] * q[2] + m[7] * w,
      m[8] * q[0] + m[9] * q[1] + m[10] * q[2] + m[11] * w,
    ],
    [v[0], v[1], v[2]],
  )
}

/** The diameter of the circle through three points: the product of the triangle's sides over twice its area. `null` for three in a line. */
function circumdiameter(p: Vec3, q: Vec3, r: Vec3): number | null {
  const u = sub(q, p)
  const v = sub(r, p)
  const twiceArea = length(cross(u, v))
  if (twiceArea <= 1e-9 * length(u) * length(v)) return null
  return (length(u) * length(v) * length(sub(r, q))) / twiceArea
}

const exact = (value: number): Approximate<number> => ({ value, approximate: false })
const approximate = (value: number): Approximate<number> => ({ value, approximate: true })
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
const scale = (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s]
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
const length = (a: Vec3) => Math.hypot(a[0], a[1], a[2])
const fromAxis = (p: Vec3, origin: Vec3, axis: Vec3) => {
  const v = sub(p, origin)
  return sub(v, scale(axis, dot(v, axis)))
}
const facing = (surface: Vec3, facet: Vec3) => Math.abs(dot(surface, facet)) >= FACING
/** A B-rep plane's normal need not point out of the solid; the triangle's does, so it decides. */
const outward = (surface: Vec3, facet: Vec3): Vec3 => (dot(surface, facet) < 0 ? scale(surface, -1) : surface)
const degrees = (a: Vec3, b: Vec3) => (Math.acos(Math.min(1, Math.max(-1, dot(a, b)))) * 180) / Math.PI
