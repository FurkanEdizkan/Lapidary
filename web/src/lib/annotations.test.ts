import { expect, test } from 'vitest'
import { anchor, annotationsOf, labelsFor } from './annotations'
import { placeEntities } from './measure'
import { strings } from './strings'
import type { AssemblyTree, Entity, Pmi } from './types'

/** `cylinder-d22-pmi-lp-9012-00.step`'s PMI and entities, as stored. */
const PMI: Pmi = {
  dimensions: [{ type: 'diameter', value: 22, upper: 0.05, lower: 0, faces: [{ prototype: '0:1:1:1', face: 1 }] }],
  tolerances: [
    { type: 'flatness', value: 0.02, datums: [], faces: [{ prototype: '0:1:1:1', face: 2 }] },
    { type: 'perpendicularity', value: 0.05, datums: ['A'], faces: [{ prototype: '0:1:1:1', face: 1 }] },
    { type: 'profile_of_surface', value: 0.1, datums: [], faces: [{ prototype: '0:1:1:1', face: 9 }] },
  ],
  datums: [{ name: 'A', faces: [{ prototype: '0:1:1:1', face: 3 }] }],
}
const ENTITIES: Entity[] = [
  { type: 'cylinder', prototype: '0:1:1:1', face: 1, radius: 11, origin: [0, 0, 0], axis: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:1', face: 2, origin: [0, 0, 30], normal: [0, 0, 1] },
  { type: 'plane', prototype: '0:1:1:1', face: 3, origin: [0, 0, 0], normal: [0, 0, -1] },
  { type: 'circle', prototype: '0:1:1:1', edge: 1, radius: 11, center: [0, 0, 30], normal: [0, 0, 1] },
]

test('each annotation on a face the view draws gets a label there, two on one face sharing it', () => {
  const { labels, undrawn } = labelsFor(annotationsOf(PMI), ENTITIES)
  expect(labels).toEqual([
    {
      text: `${strings.pmi.dimension('diameter', 22, 0.05, 0)}\n${strings.pmi.tolerance('perpendicularity', 0.05, ['A'])}`,
      at: [0, 11, 0],
    },
    { text: strings.pmi.tolerance('flatness', 0.02, []), at: [0, 0, 30] },
    { text: strings.pmi.datum('A'), at: [0, 0, 0] },
  ])
  // The profile tolerance names face 9, which has no entity: a freeform face, listed and not drawn.
  expect([...undrawn]).toEqual([3])
})

test('an annotation on the whole part is not drawn', () => {
  const whole: Pmi = { dimensions: [], tolerances: [], datums: [{ name: 'B', faces: [{ prototype: '0:1:1:1', face: null }] }] }
  expect(labelsFor(annotationsOf(whole), ENTITIES)).toEqual({ labels: [], undrawn: new Set([0]) })
})

test('a label goes where its placed instance was drawn, once for each', () => {
  const tree: AssemblyTree = {
    roots: [
      { name: 'a', prototype: '0:1:1:1', transform: [1, 0, 0, 100, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1], children: [] },
      { name: 'b', prototype: '0:1:1:1', transform: [1, 0, 0, 0, 0, 1, 0, 50, 0, 0, 1, 0, 0, 0, 0, 1], children: [] },
    ],
    parts: 2,
    prototypes: 1,
  }
  const flatness: Pmi = { dimensions: [], tolerances: [PMI.tolerances[0]!], datums: [] }
  expect(labelsFor(annotationsOf(flatness), placeEntities(ENTITIES, tree)).labels.map((label) => label.at)).toEqual([
    [100, 0, 30],
    [0, 50, 30],
  ])
})

test('a label sits on a cone, sphere or torus at a point the entity names', () => {
  expect(anchor({ type: 'sphere', prototype: 'p', face: 1, radius: 10, center: [0, 0, 38] })).toEqual([0, 0, 48])
  expect(anchor({ type: 'cone', prototype: 'p', face: 2, ref_radius: 6, semi_angle_rad: Math.PI / 4, origin: [0, 0, 0], axis: [0, 0, 1] })).toEqual([0, 6, 0])
  const torus = anchor({ type: 'torus', prototype: 'p', face: 3, major_radius: 6, minor_radius: 1.5, origin: [0, 0, 15], axis: [1, 0, 0] })
  expect(Math.hypot(torus[1], torus[2] - 15)).toBeCloseTo(7.5, 9)
  expect(torus[0]).toBeCloseTo(0, 9)
})
