import { expect, test } from 'vitest'
import { Box3 } from 'three'
import { zipSync } from 'three/addons/libs/fflate.module.js'
import { parseMesh } from './mesh'

/**
 * `node:fs`, reached the way `contrast.test.ts` reaches it and for its reasons: the specifier is
 * assembled at run time so TypeScript types the import as `any` rather than looking for
 * declarations this package does not install, and the paths below are relative to Vitest's working
 * directory, `web/`, which is where both `npm --prefix web test` and a local `npx vitest` run from.
 * A run from anywhere else fails loudly with ENOENT naming the path rather than reading nothing.
 */
const fs = (await import(/* @vite-ignore */ ['node', 'fs'].join(':'))) as {
  readFileSync: ((path: string) => Uint8Array) & ((path: string, encoding: 'utf8') => string)
}

/** A file from `fixtures/`, as the browser would be handed it by `?variant=original`. */
function fixture(name: string): ArrayBuffer {
  return Uint8Array.from(fs.readFileSync(`../fixtures/${name}`)).buffer
}

/** What `fixtures/bbox-mm.json` states each file's extent in millimetres is. */
const stated: Record<string, [number, number, number]> = JSON.parse(
  fs.readFileSync('../fixtures/bbox-mm.json', 'utf8'),
).fixtures

/** The box `parseMesh` arrives at, as extents on x, y and z. */
function extents(format: string, bytes: ArrayBuffer): [number, number, number] {
  const box = new Box3().setFromObject(parseMesh(format, bytes))
  return [box.max.x - box.min.x, box.max.y - box.min.y, box.max.z - box.min.z]
}

/**
 * **One assertion per format, over axes, units and transforms together.**
 *
 * The viewer draws the original file now, so there are two parsers for every part: ours, in
 * `lapidary-cad`, whose measurements become `PartDetail.bboxMm`, and three's, in the browser.
 * Nothing makes them agree — no shared code, no compiler, no type. If they drift, a part is
 * drawn at a size or an orientation its own figures contradict, and nothing fails.
 *
 * So `fixtures/bbox-mm.json` states the extent of each real fixture and both sides assert
 * against it: `lapidary-cad/src/measure.rs` for the figures a card shows, this test for what
 * the browser draws. Correcting one side's number now breaks the other side's test.
 *
 * A box and not a vertex count: a count survives an axis swap, a unit error and a dropped
 * build transform, and the box survives none of them.
 */
test('every format loads at the box bbox-mm.json states for it', () => {
  const files: [string, string][] = [
    ['stl', 'bracket-lp-1042-03.stl'],
    ['obj', 'idler-bracket-lp-2210-01.obj'],
    ['3mf', 'planetary-carrier-lp-3480-02.3mf'],
  ]
  for (const [format, name] of files) {
    const want = stated[name]
    expect(want, `${name} is in bbox-mm.json`).toBeDefined()
    const got = extents(format, fixture(name))
    for (const axis of [0, 1, 2]) {
      // 0.01 mm: the fixtures are written to four decimal places and three stores positions
      // as f32, so the two sides agree to well inside a hundredth of a millimetre or not at all.
      expect(Math.abs(got[axis]! - want![axis]!), `${name} axis ${axis}: got ${got}, stated ${want}`).toBeLessThan(
        0.01,
      )
    }
  }
})

/**
 * Text as bytes, re-wrapped in **this** realm's `Uint8Array`.
 *
 * Not `fflate`'s own `strToU8`, and not a bare `TextEncoder().encode`: under jsdom the
 * encoder is jsdom's and hands back a typed array from another realm, which fflate's
 * `instanceof Uint8Array` reads as "not bytes" and files away as a nested directory. The zip
 * then holds one entry per byte and the loader cannot find `_rels/.rels` — which is a confusing
 * half-hour, so it is written down here.
 */
function bytes(text: string): Uint8Array {
  return new Uint8Array(new TextEncoder().encode(text))
}

/**
 * A minimal 3MF package in `unit`, holding a closed 4 × 2.5 × 0.2 box.
 *
 * Built rather than committed, for `images.rs`'s reason: a fixture file would be bytes nobody
 * in this repository could read to check, and the whole point here is that the number in the
 * XML and the number asserted below are a unit conversion apart and visibly so.
 *
 * `_rels/.rels` and `3D/*.model` are the two parts `ThreeMFLoader` looks for, and the same two
 * `tmf.rs`'s own `package` helper writes.
 */
function package3mf(unit: string): ArrayBuffer {
  const corners = [
    [0, 0, 0],
    [4, 0, 0],
    [4, 2.5, 0],
    [0, 2.5, 0],
    [0, 0, 0.2],
    [4, 0, 0.2],
    [4, 2.5, 0.2],
    [0, 2.5, 0.2],
  ]
  const faces = [
    [0, 2, 1],
    [0, 3, 2],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [1, 2, 6],
    [1, 6, 5],
    [2, 3, 7],
    [2, 7, 6],
    [3, 0, 4],
    [3, 4, 7],
  ]
  const model = `<?xml version="1.0" encoding="UTF-8"?>
<model unit="${unit}" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <resources>
  <object id="1" type="model">
   <mesh>
    <vertices>${corners.map(([x, y, z]) => `<vertex x="${x}" y="${y}" z="${z}"/>`).join('')}</vertices>
    <triangles>${faces.map(([a, b, c]) => `<triangle v1="${a}" v2="${b}" v3="${c}"/>`).join('')}</triangles>
   </mesh>
  </object>
 </resources>
 <build><item objectid="1"/></build>
</model>`
  const rels = `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rel0" Target="/3D/shim-plate.model" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>`
  const zip = zipSync({ '_rels/.rels': bytes(rels), '3D/shim-plate.model': bytes(model) })
  return Uint8Array.from(zip).buffer
}

/**
 * **The gap the committed fixtures cannot show.** `planetary-carrier-lp-3480-02.3mf` is in
 * millimetres, so the test above passes whether or not anything reads the unit at all — and
 * `ThreeMFLoader` reads the attribute and then ignores it, which means a shim plate exported
 * from a tool that works in centimetres loads ten times too small while every figure on the
 * page says 40 mm.
 *
 * The same plate in four units, each asserted against the one box in millimetres, so the
 * conversion is pinned in the direction it is applied — a table used upside down turns
 * centimetres into tenths rather than tens, and 4 cm reading as 0.4 mm is just as wrong in a
 * way that looks like "the file was odd".
 */
test('a 3MF is loaded at millimetres whatever unit it declares', () => {
  for (const [unit, mm] of [
    ['millimeter', 1],
    ['centimeter', 10],
    ['inch', 25.4],
    ['meter', 1000],
  ] as [string, number][]) {
    const got = extents('3mf', package3mf(unit))
    for (const [axis, cm] of [4, 2.5, 0.2].entries()) {
      expect(Math.abs(got[axis]! - cm * mm), `${unit} axis ${axis}: got ${got}`).toBeLessThan(0.01)
    }
  }
})

/**
 * A format with no loader says so rather than drawing nothing. `Preview` only offers **view**
 * for the three mesh formats, so this is the path a bookmark or a future format reaches, and
 * "nothing happened" is the one answer that cannot be acted on.
 */
test('a format with no loader is refused in words that name it', () => {
  expect(() => parseMesh('step', new ArrayBuffer(8))).toThrow(/STEP/i)
})
