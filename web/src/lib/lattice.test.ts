import { expect, test } from 'vitest'
import { lattice } from './lattice'

/** The two endpoints of an edge's `M x y L x y`. */
function ends(d: string): [string, string] {
  const match = /^M (-?[\d.]+ -?[\d.]+) L (-?[\d.]+ -?[\d.]+)$/.exec(d)
  if (match === null) throw new Error(`not a single line: ${d}`)
  return [match[1]!, match[2]!]
}

test('the gear has the edges a low-poly mesh of it should', () => {
  const { edges } = lattice()
  expect(edges.length).toBeGreaterThanOrEqual(250)
  expect(edges.length).toBeLessThanOrEqual(400)
})

test('no edge is drawn twice, in either direction', () => {
  const keys = lattice().edges.map(({ d }) => ends(d).sort().join('|'))
  expect(new Set(keys).size).toBe(keys.length)
  for (const { d } of lattice().edges) {
    const [a, b] = ends(d)
    expect(a, d).not.toBe(b)
  }
})

test('every point and every depth is inside the drawing', () => {
  const { viewBox, edges } = lattice()
  const [x0, y0, width, height] = viewBox.split(' ').map(Number) as [number, number, number, number]
  for (const { d, depth } of edges) {
    for (const end of ends(d)) {
      const [x, y] = end.split(' ').map(Number) as [number, number]
      expect(x, d).toBeGreaterThanOrEqual(x0)
      expect(x, d).toBeLessThanOrEqual(x0 + width)
      expect(y, d).toBeGreaterThanOrEqual(y0)
      expect(y, d).toBeLessThanOrEqual(y0 + height)
    }
    expect(depth).toBeGreaterThanOrEqual(0)
    expect(depth).toBeLessThanOrEqual(1)
  }
})

test('two calls draw the same gear', () => {
  expect(lattice()).toEqual(lattice())
})
