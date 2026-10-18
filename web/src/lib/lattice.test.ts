import { expect, test } from 'vitest'
import { STEP, frame, lattice, rough } from './lattice'

/** The two endpoints of an edge's `M x y L x y`. */
function ends(d: string): [string, string] {
  const match = /^M (-?[\d.]+ -?[\d.]+) L (-?[\d.]+ -?[\d.]+)$/.exec(d)
  if (match === null) throw new Error(`not a single line: ${d}`)
  return [match[1]!, match[2]!]
}

test('the brilliant has the facets a round brilliant has, and the lines between them', () => {
  // Table, 8 stars, 8 bezels, 16 upper girdles, 16 girdle facets, 16 lower girdles, 8 pavilion mains.
  expect(frame().faces).toHaveLength(73)
  expect(lattice().edges.length).toBe(128)
})

test('no edge is drawn twice, in either direction', () => {
  const keys = lattice().edges.map(({ d }) => ends(d).sort().join('|'))
  expect(new Set(keys).size).toBe(keys.length)
  for (const { d } of lattice().edges) {
    const [a, b] = ends(d)
    expect(a, d).not.toBe(b)
  }
})

test('every point of every frame, rough, cutting or turned, is inside the one drawing', () => {
  const [x0, y0, width, height] = lattice().viewBox.split(' ').map(Number) as [number, number, number, number]
  for (const cut of [0, 0.3, 0.6, 1]) {
    for (const turn of [0, 0.3, STEP / 2, STEP]) {
      const { edges, faces } = frame({ turn, cut, light: 0.6 })
      expect(edges).toHaveLength(lattice().edges.length)
      for (const { d, front } of edges) {
        for (const end of ends(d)) {
          const [x, y] = end.split(' ').map(Number) as [number, number]
          expect(x, d).toBeGreaterThanOrEqual(x0)
          expect(x, d).toBeLessThanOrEqual(x0 + width)
          expect(y, d).toBeGreaterThanOrEqual(y0)
          expect(y, d).toBeLessThanOrEqual(y0 + height)
        }
        expect(front).toBeGreaterThanOrEqual(0)
        expect(front).toBeLessThanOrEqual(1)
      }
      for (const { glint, cutting } of faces) {
        expect(glint).toBeGreaterThanOrEqual(0)
        expect(glint).toBeLessThanOrEqual(1)
        expect(cutting).toBeGreaterThanOrEqual(0)
      }
    }
  }
})

test('two calls draw the same stone, and the rough stone is not the cut one', () => {
  expect(lattice()).toEqual(lattice())
  expect(rough()).toEqual(rough())
  expect(rough().edges).not.toEqual(lattice().edges)
})

test('nothing is being cut before the cut starts or after it ends', () => {
  for (const cut of [0, 1]) expect(frame({ cut }).faces.every(({ cutting }) => cutting === 0)).toBe(true)
  expect(frame({ cut: 0.5 }).faces.some(({ cutting }) => cutting > 0.5)).toBe(true)
})

test('the loop has no seam: turned by one step, every line lands on a line of the unturned stone, as faint', () => {
  const near = (a: string, b: string) =>
    a.split(' ').every((value, i) => Math.abs(Number(value) - Number(b.split(' ')[i])) <= 0.2)
  const unturned = lattice().edges.map(({ d, front }) => ({ ends: ends(d), front }))
  for (const { d, front } of frame({ turn: STEP }).edges) {
    const [a, b] = ends(d)
    const match = unturned.find(({ ends: [c, e] }) => (near(a, c) && near(b, e)) || (near(a, e) && near(b, c)))
    expect(match, d).toBeDefined()
    expect(Math.abs(match!.front - front), d).toBeLessThanOrEqual(0.02)
  }
})

test('the lights catch some facets on the near side at every moment of a turn', () => {
  for (let k = 0; k < 8; k++) {
    const progress = k / 8
    const { faces } = frame({ turn: progress * STEP, light: 0.6 * Math.sin(2 * Math.PI * progress) })
    expect(faces.some(({ glint }) => glint > 0.2)).toBe(true)
  }
})
