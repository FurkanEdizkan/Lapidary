import { expect, test } from 'vitest'
import { measure, nearestCorner, placeEntities, snap, withoutParts, type Pick } from './measure'
import type { AssemblyNode, AssemblyTree, Entity } from './types'
import type { Vec3 } from './viewer-math'

const IDENTITY: AssemblyNode['transform'] = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
const translate = (x: number, y: number, z: number): AssemblyNode['transform'] => [
  1, 0, 0, x, 0, 1, 0, y, 0, 0, 1, z, 0, 0, 0, 1,
]
const node = (
  name: string,
  prototype: string,
  transform: AssemblyNode['transform'],
  children: AssemblyNode[] = [],
): AssemblyNode => ({ name, prototype, transform, children })

/** What `occt-bridge` wrote for `fixtures/step/cylinder-d22-lp-9010-00.step`, as stored. */
const CYLINDER: Entity[] = [
  { type: 'cylinder', prototype: '0:1:1:1', face: 1, radius: 11, origin: [0, 0, 0], axis: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:1', face: 2, origin: [0, 0, 30], normal: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:1', face: 3, origin: [0, 0, 0], normal: [0, 0, 1] },
  { type: 'circle', prototype: '0:1:1:1', edge: 1, radius: 11, center: [0, 0, 30], normal: [0, 0, 1] },
  { type: 'circle', prototype: '0:1:1:1', edge: 3, radius: 11, center: [0, 0, 0], normal: [0, 0, 1] },
]
const CYLINDER_TREE: AssemblyTree = {
  roots: [node('cylinder-d22-lp-9010-00', '0:1:1:1', IDENTITY)],
  parts: 1,
  prototypes: 1,
}

/** The stop pin of `fixtures/step/fixture-plate-assembly-lp-9000-00.step`, as stored. */
const PIN: Entity[] = [
  { type: 'cylinder', prototype: '0:1:1:12', face: 1, radius: 5, origin: [0, 0, 0], axis: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:12', face: 2, origin: [0, 0, 20], normal: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:12', face: 3, origin: [0, 0, 0], normal: [0, 0, 1] },
]

/** A triangle of the mesh, clicked at its middle, which is where a chord cuts inside a curved surface. */
function facet(normal: Vec3, p: Vec3, q: Vec3, r: Vec3): Pick {
  const point: Vec3 = [(p[0] + q[0] + r[0]) / 3, (p[1] + q[1] + r[1]) / 3, (p[2] + q[2] + r[2]) / 3]
  return { point, normal, corners: [p, q, r] }
}
/** A click on a mesh with no entities, where the triangle does not matter. */
const click = (point: Vec3, normal: Vec3 = [0, 0, 1]): Pick => ({ point, normal, corners: [point, point, point] })
const ring = (radius: number, at: number, z: number, x = 0, y = 0): Vec3 => [
  x + radius * Math.cos(at),
  y + radius * Math.sin(at),
  z,
]
const radial = (at: number): Vec3 => [Math.cos(at), Math.sin(at), 0]

function expectNear(actual: Vec3, expected: Vec3) {
  actual.forEach((value, axis) => expect(value).toBeCloseTo(expected[axis] ?? Number.NaN, 9))
}

test('the fixture cylinder measures 22.000 mm, exact, on its side or at its rim', () => {
  const placed = placeEntities(CYLINDER, CYLINDER_TREE)
  const side = facet(radial(0.3), ring(11, 0.2, 30), ring(11, 0.4, 30), ring(11, 0.2, 0))
  expect(measure('diameter', [side], placed)).toEqual({ value: 22, approximate: false })
  // A cap triangle with a side along the rim: not the cylinder, which it does not face, but the circle.
  const cap = facet([0, 0, 1], ring(11, 0.2, 30), ring(11, 0.4, 30), [0, 0, 30])
  expect(snap(cap, placed, ['cylinder'])).toBeNull()
  expect(measure('diameter', [cap], placed)).toEqual({ value: 22, approximate: false })
})

test('an entity is placed through every transform above its node, where the mesh drew it', () => {
  const tree: AssemblyTree = {
    roots: [
      node('fixture-plate-assembly-lp-9000-00', '0:1:1:1', IDENTITY, [
        node('stop-pin-rack-lp-9300-00', '0:1:1:11', translate(0, 270, 0), [
          node('stop-pin-d10x20-lp-9007-00', '0:1:1:12', translate(0, 0, 0)),
          node('stop-pin-d10x20-lp-9007-00', '0:1:1:12', translate(140, 0, 0)),
        ]),
      ]),
    ],
    parts: 2,
    prototypes: 3,
  }
  const placed = placeEntities(PIN, tree)
  expect(placed).toHaveLength(6)

  const side = facet(radial(0.2), ring(5, 0, 0, 140, 270), ring(5, 0.4, 20, 140, 270), ring(5, 0, 20, 140, 270))
  expect(snap(side, placed, ['cylinder'])).toMatchObject({ origin: [140, 270, 0] })
  expect(measure('diameter', [side], placed)).toEqual({ value: 10, approximate: false })
  // The same triangle where the pin's own coordinates would put it has nothing to snap to.
  const unplaced = facet(radial(0.2), ring(5, 0, 0), ring(5, 0.4, 20), ring(5, 0, 20))
  expect(snap(unplaced, placed, ['cylinder'])).toBeNull()
})

test("a parent's rotation turns its child's offset, so transforms apply innermost first", () => {
  // A quarter turn about Z, row-major: x' = −y, y' = x.
  const quarter: AssemblyNode['transform'] = [0, -1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
  const tree: AssemblyTree = {
    roots: [node('fixture', 'a', quarter, [node('block', 'b', translate(10, 0, 0))])],
    parts: 1,
    prototypes: 2,
  }
  const [face] = placeEntities([{ type: 'plane', prototype: 'b', face: 1, origin: [0, 0, 0], normal: [1, 0, 0] }], tree)
  if (face?.type !== 'plane') throw new Error('expected the placed plane')
  expectNear(face.origin, [0, 10, 0])
  expectNear(face.normal, [0, 1, 0])
})

test('a triangle snaps to the surface its corners are on, not the one its middle is nearest', () => {
  expect(snap(facet([0, 0, 1], [1, 0, 30], [0, 1, 30], [-1, 0, 30]), CYLINDER, ['plane'])).toMatchObject({ face: 2 })

  const shaft: Entity[] = [
    { type: 'cylinder', prototype: 'p', face: 1, radius: 5, origin: [0, 0, 0], axis: [0, 0, 1] },
    { type: 'cylinder', prototype: 'p', face: 2, radius: 5.05, origin: [0, 0, 0], axis: [0, 0, 1] },
  ]
  // A triangle of the 10.10 mm step, whose middle sags nearer the 10.00 mm radius than its own.
  const step = facet(radial(0.2), ring(5.05, 0, 10), ring(5.05, 0.4, 20), ring(5.05, 0, 20))
  expect(snap(step, shaft, ['cylinder'])).toMatchObject({ face: 2 })
  expect(measure('diameter', [step], shaft)).toEqual({ value: 10.1, approximate: false })
  // In a bore the triangle faces the axis, which is still facing the way the surface does.
  const bore = facet([-Math.cos(0.2), -Math.sin(0.2), 0], ring(5, 0, 10), ring(5, 0, 20), ring(5, 0.4, 20))
  expect(snap(bore, shaft, ['cylinder'])).toMatchObject({ face: 1 })
})

test('angle and wall thickness between planes are exact, whichever way the kernel wrote the normals', () => {
  const plate: Entity[] = [
    { type: 'plane', prototype: 'p', face: 1, origin: [0, 0, 20], normal: [0, 0, 1] },
    { type: 'plane', prototype: 'p', face: 2, origin: [0, 0, 0], normal: [0, 0, 1] },
    { type: 'plane', prototype: 'p', face: 3, origin: [0, 0, 0], normal: [1, 0, 0] },
  ]
  const top = facet([0, 0, 1], [10, 10, 20], [11, 10, 20], [10, 11, 20])
  const bottom = facet([0, 0, -1], [10, 10, 0], [10, 11, 0], [11, 10, 0])
  const side = facet([-1, 0, 0], [0, 10, 10], [0, 11, 10], [0, 10, 11])

  expect(measure('wall', [top, bottom], plate)).toEqual({ value: 20, approximate: false })
  const angle = measure('angle', [top, side], plate)
  expect(angle?.approximate).toBe(false)
  expect(angle?.value).toBeCloseTo(90, 9)
  // Turned by the triangles picked: the kernel wrote the top and bottom normals the same way.
  expect(measure('angle', [top, bottom], plate)?.value).toBeCloseTo(180, 9)
})

test('a value not read from an entity is approximate, and says so', () => {
  const rim = [0, 2, 4].map((at) => click(ring(11, at, 30)))
  expect(measure('diameter', rim.slice(0, 2), [])).toBeNull()
  const fit = measure('diameter', rim, [])
  expect(fit?.approximate).toBe(true)
  expect(fit?.value).toBeCloseTo(22, 9)
  expect(measure('diameter', [0, 1, 2].map((x) => click([x, 0, 0])), [])).toBeNull()

  // Even on a CAD part, two clicks are two points on a mesh.
  expect(measure('distance', [click([0, 0, 30]), click([3, 4, 30])], CYLINDER)).toEqual({
    value: 5,
    approximate: true,
  })
  expect(measure('angle', [click([0, 0, 30]), click([11, 0, 15], [1, 0, 0])], [])).toMatchObject({
    approximate: true,
  })
  // One wall on a plane and the other on none: the thickness is the points', not the planes'.
  expect(measure('wall', [click([0, 0, 30]), click([0, 0, 0.5], [0, 0, -1])], CYLINDER)).toEqual({
    value: 29.5,
    approximate: true,
  })
})

test('an edge is measured between the triangle corners nearest the clicks', () => {
  expect(nearestCorner([0.9, 0.1, 0], [[0, 0, 0], [1, 0, 0], [0, 1, 0]])).toEqual([1, 0, 0])
})

/** A 12 mm ball, a 6 mm tube on a 40 mm ring, and a 90° countersink opening from ⌀10 mm, each as the bridge writes it. */
const BALL: Entity = { type: 'sphere', prototype: '0:1:1:20', face: 1, radius: 6, center: [0, 0, 0] }
const RING: Entity = { type: 'torus', prototype: '0:1:1:21', face: 1, major_radius: 20, minor_radius: 3, origin: [0, 0, 0], axis: [0, 0, 1] }
const SINK: Entity = { type: 'cone', prototype: '0:1:1:22', face: 1, ref_radius: 5, semi_angle_rad: Math.PI / 4, origin: [0, 0, 0], axis: [0, 0, 1] }

const onBall = (theta: number, phi: number): Vec3 => [6 * Math.cos(phi) * Math.cos(theta), 6 * Math.cos(phi) * Math.sin(theta), 6 * Math.sin(phi)]
const onRing = (theta: number, phi: number): Vec3 => [
  (20 + 3 * Math.cos(phi)) * Math.cos(theta),
  (20 + 3 * Math.cos(phi)) * Math.sin(theta),
  3 * Math.sin(phi),
]
const onSink = (theta: number, height: number): Vec3 => ring(5 + height, theta, height)

test('a ball reads its diameter and a torus its tube, exact', () => {
  const ball = facet(scale3(onBall(0.3, 0.25), 1 / 6), onBall(0.2, 0.2), onBall(0.4, 0.2), onBall(0.3, 0.3))
  expect(measure('diameter', [ball], [BALL, RING, SINK])).toEqual({ value: 12, approximate: false })
  // Out of the tube, at 0.1 rad around the ring and 0.5 rad around the tube.
  const tubeNormal: Vec3 = [Math.cos(0.5) * Math.cos(0.1), Math.cos(0.5) * Math.sin(0.1), Math.sin(0.5)]
  const tube = facet(tubeNormal, onRing(0.05, 0.4), onRing(0.15, 0.4), onRing(0.1, 0.6))
  expect(measure('diameter', [tube], [BALL, RING, SINK])).toEqual({ value: 6, approximate: false })
})

test('a countersink reads its included angle exactly, and its diameter at the click only approximately', () => {
  const outward: Vec3 = [Math.cos(0.3) * Math.SQRT1_2, Math.sin(0.3) * Math.SQRT1_2, -Math.SQRT1_2]
  const side = facet(outward, onSink(0.2, 1), onSink(0.4, 1), onSink(0.3, 2))
  expect(measure('angle', [side], [SINK])).toEqual({ value: 90, approximate: false })
  const diameter = measure('diameter', [side], [SINK])
  expect(diameter?.approximate).toBe(true)
  // The click is at the triangle's middle, 4/3 mm up, where the cone is ⌀12.67 mm.
  expect(diameter?.value).toBeCloseTo(2 * (5 + 4 / 3), 1)
})

test('a click on a cone reads its included angle, whatever was clicked before it', () => {
  const outward: Vec3 = [Math.cos(0.3) * Math.SQRT1_2, Math.sin(0.3) * Math.SQRT1_2, -Math.SQRT1_2]
  const side = facet(outward, onSink(0.2, 1), onSink(0.4, 1), onSink(0.3, 2))
  const top = facet([0, 0, 1], [0, 0, 30], [1, 0, 30], [0, 1, 30])
  expect(measure('angle', [top, side], [SINK, ...CYLINDER])).toEqual({ value: 90, approximate: false })
})

/** A ⌀10 mm knob on a neck, meeting it along a ⌀6 mm rim 4 mm below the knob's centre. Every corner below is exact. */
const KNOB: Entity = { type: 'sphere', prototype: '0:1:1:30', face: 1, radius: 5, center: [0, 0, 0] }
const NECK: Entity = { type: 'circle', prototype: '0:1:1:30', edge: 1, radius: 3, center: [0, 0, -4], normal: [0, 0, 1] }

test('a triangle on a ball along its rim reads the ball, and a flat face beside the rim reads the rim', () => {
  // Two corners on the rim and one on the ball above it, so on both exactly: the ball is what was clicked.
  const middle: Vec3 = [7 / 3, 1, -11 / 3]
  const onBoth = facet(scale3(middle, 1 / Math.hypot(...middle)), [3, 0, -4], [0, 3, -4], [4, 0, -3])
  expect(measure('diameter', [onBoth], [KNOB, NECK])).toEqual({ value: 10, approximate: false })
  // A flat shoulder around the rim is no round face, so there the rim reads.
  const shoulder = facet([0, 0, -1], [3, 0, -4], [0, 3, -4], [4, 4, -4])
  expect(measure('diameter', [shoulder], [KNOB, NECK])).toEqual({ value: 6, approximate: false })
})

test('an assembly without its hidden parts places nothing of theirs', () => {
  const plate: AssemblyTree = {
    roots: [
      node('fixture-plate-assembly-lp-9000-00', '0:1:1:10', IDENTITY, [
        node('stop-pin-lp-9004-00 (1)', '0:1:1:12', translate(40, 0, 0)),
        node('stop-pin-lp-9004-00 (2)', '0:1:1:12', translate(80, 0, 0)),
      ]),
    ],
    parts: 2,
    prototypes: 2,
  }
  const second = withoutParts(plate, new Set([0]))
  expect(second.parts).toBe(1)
  const placed = placeEntities(PIN, second)
  expect(placed).toHaveLength(PIN.length)
  expectNear((placed[0] as { origin: Vec3 }).origin, [80, 0, 0])
  expect(placeEntities(PIN, withoutParts(plate, new Set()))).toHaveLength(2 * PIN.length)
})

test('a triangle off a curved surface does not snap to it', () => {
  const off = facet(scale3(onBall(0.3, 0.25), 1 / 6), onBall(0.2, 0.2), onBall(0.4, 0.2), scale3(onBall(0.3, 0.3), 1.01))
  expect(snap(off, [BALL], ['sphere'])).toBeNull()
  expect(measure('angle', [off], [SINK])).toBeNull()
})

function scale3(v: Vec3, s: number): Vec3 {
  return [v[0] * s, v[1] * s, v[2] * s]
}
