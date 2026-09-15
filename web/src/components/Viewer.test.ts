import { expect, test } from 'vitest'
import { BufferAttribute, BufferGeometry, Mesh, MeshBasicMaterial } from 'three'
import { visibleRanges } from '../lib/viewer-math'
import { hideParts } from './Viewer'

/**
 * A section's cap is drawn where the stencil passes count an inside. Those passes share the part's mesh, so
 * a part hidden in an assembly has to be left out of them too, or its cut would still be filled.
 */
test('a hidden part is left out of the cap\'s stencil passes as well as the mesh', () => {
  // Two parts of one assembly mesh: 12 and 6 indices, with the cap's two passes sharing its geometry.
  const geometry = new BufferGeometry()
  geometry.setIndex(new BufferAttribute(new Uint16Array(18), 1))
  const material = new MeshBasicMaterial()
  const mesh = new Mesh(geometry, material)
  mesh.userData.parts = [12, 6]
  const passes = [new MeshBasicMaterial(), new MeshBasicMaterial()]
  for (const pass of passes) {
    const stencil = new Mesh(geometry, pass)
    stencil.userData.stencil = true
    mesh.add(stencil)
  }
  const materials = () => mesh.children.map((child) => (child as Mesh).material)

  hideParts(mesh, material, new Set([1]))
  expect(geometry.groups.map(({ start, count }) => ({ start, count }))).toEqual(visibleRanges([12, 6], new Set([1])))
  expect(mesh.material).toEqual([material])
  expect(materials()[0]).toEqual([passes[0]])
  expect(materials()[1]).toEqual([passes[1]])

  hideParts(mesh, material, new Set())
  expect(geometry.groups).toEqual([])
  expect(mesh.material).toBe(material)
  expect(materials()[0]).toBe(passes[0])
  expect(materials()[1]).toBe(passes[1])
})
