import { strings } from './strings'
import type { Entity, Pmi, PmiFace } from './types'
import type { Vec3 } from './viewer-math'

/**
 * PMI in the 3D view: each annotation drawn beside the face it names, where the view drew that face.
 *
 * Only on the faces measurement reads as entities — planes, cylinders, cones, spheres and tori. The file's
 * own presentation, its leader lines and text placement, is not read, and a freeform face has no entity to
 * say where it is. So a label sits at a point the entity names: a plane's origin, a point on the rim a
 * cylinder, cone or torus starts from, the top of a sphere. The list beside the view stays the source of
 * truth, and says which annotations the view could not place.
 */

/** One annotation as the list says it, and the faces it names. */
export type Annotation = { text: string; faces: readonly PmiFace[] }

/** Every annotation on one placed face, and where to draw them. */
export type Label = { text: string; at: Vec3 }

/** The file's annotations in the order the list shows them: dimensions, then tolerances, then datums. */
export function annotationsOf(pmi: Pmi): Annotation[] {
  return [
    ...pmi.dimensions.map((d) => ({ text: strings.pmi.dimension(d.type, d.value, d.upper, d.lower), faces: d.faces })),
    ...pmi.tolerances.map((t) => ({ text: strings.pmi.tolerance(t.type, t.value, t.datums), faces: t.faces })),
    ...pmi.datums.map((d) => ({ text: strings.pmi.datum(d.name), faces: d.faces })),
  ]
}

/**
 * The labels to draw for `annotations` over `entities`, and which annotations have no face to be drawn on.
 *
 * `entities` placed (`placeEntities`), so each placed instance of a face gets its own label; unplaced ones
 * answer the same question about which annotations can be drawn. Annotations on one face share one label,
 * a line each, so two do not sit on top of one another.
 */
export function labelsFor(
  annotations: readonly Annotation[],
  entities: readonly Entity[],
): { labels: Label[]; undrawn: ReadonlySet<number> } {
  const grouped = new Map<string, { lines: string[]; at: Vec3 }>()
  const undrawn = new Set<number>()
  annotations.forEach((annotation, index) => {
    let drawn = false
    for (const face of annotation.faces) {
      if (face.face === null) continue
      for (const entity of entities) {
        if (entity.type === 'circle' || entity.prototype !== face.prototype || entity.face !== face.face) continue
        const at = anchor(entity)
        const key = at.map((v) => v.toFixed(6)).join(',')
        const group = grouped.get(key) ?? { lines: [], at }
        if (!group.lines.includes(annotation.text)) group.lines.push(annotation.text)
        grouped.set(key, group)
        drawn = true
      }
    }
    if (!drawn) undrawn.add(index)
  })
  return { labels: [...grouped.values()].map(({ lines, at }) => ({ text: lines.join('\n'), at })), undrawn }
}

/** The point a label for a face is drawn at. */
export function anchor(entity: Exclude<Entity, { type: 'circle' }>): Vec3 {
  switch (entity.type) {
    case 'plane':
      return entity.origin
    case 'cylinder':
      return offset(entity.origin, across(entity.axis), entity.radius)
    case 'cone':
      return offset(entity.origin, across(entity.axis), entity.ref_radius)
    case 'torus':
      return offset(entity.origin, across(entity.axis), entity.major_radius + entity.minor_radius)
    case 'sphere':
      return [entity.center[0], entity.center[1], entity.center[2] + entity.radius]
  }
}

/** A unit vector square to `axis`: across it from Z, or from X when the axis is nearly Z. */
function across(axis: Vec3): Vec3 {
  const helper: Vec3 = Math.abs(axis[2]) < 0.9 ? [0, 0, 1] : [1, 0, 0]
  const v: Vec3 = [axis[1] * helper[2] - axis[2] * helper[1], axis[2] * helper[0] - axis[0] * helper[2], axis[0] * helper[1] - axis[1] * helper[0]]
  const length = Math.hypot(v[0], v[1], v[2])
  return [v[0] / length, v[1] / length, v[2] / length]
}

const offset = (from: Vec3, direction: Vec3, by: number): Vec3 => [
  from[0] + direction[0] * by,
  from[1] + direction[1] * by,
  from[2] + direction[2] * by,
]
