import { Mesh, type Object3D } from 'three'
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
import { ThreeMFLoader } from 'three/addons/loaders/3MFLoader.js'
import { unzipSync } from 'three/addons/libs/fflate.module.js'
import { strings } from '../lib/strings'

/**
 * A refusal with a sentence somebody can act on, which is the only kind of message the viewer
 * puts on screen.
 *
 * Its own type because everything else that can go wrong here says something nobody can use:
 * three's loaders throw `THREE.ThreeMFLoader: Cannot find relationship file …` and a failed
 * download throws `Failed to fetch`. `CLAUDE.md` wants an error that says what broke and what to
 * do, and `no-bare-strings` wants it to come from `strings.ts` — so the viewer shows this one's
 * message and maps everything else to `strings.viewer.failed`.
 */
export class Unviewable extends Error {}

/**
 * The original file, read in the browser.
 *
 * Since R1 the viewer draws the ingested bytes rather than a tessellation rung we wrote, so
 * this is where the formats meet three's own loaders. It lives beside `Viewer.tsx` and is
 * imported by nothing else: three.js is in that chunk and must stay there, so a card or a
 * detail page that imported this would pull the whole renderer into the page's first load.
 *
 * **The bytes are never converted.** `?variant=original` returns what was ingested, and this
 * parses it — so what a person turns on screen is their own file and not our idea of it.
 *
 * Nothing here is told the part's dimensions, and that is deliberate: a parser handed the
 * answer can scale its way to it. `fixtures/bbox-mm.json` is the agreement instead —
 * `mesh.test.ts` loads a real file of each format through this function and asserts the box
 * it comes out at is the box `PartDetail.bboxMm` states for it, which is one assertion over
 * axes, units and 3MF transforms together.
 */
export function parseMesh(format: string, bytes: ArrayBuffer): Object3D {
  switch (format.toLowerCase()) {
    case 'stl':
      // STL is millimetres by convention and carries no unit, no transform and no axis
      // declaration: `parse_stl` reads the floats as they are and so does this.
      return new Mesh(new STLLoader().parse(bytes))
    case 'obj':
      // OBJ is text, and unitless in the same way. `parse_obj` triangulates a polygon face
      // by fanning it; so does `OBJLoader`, which is why a quad-walled fixture comes out at
      // the same box from both.
      return new OBJLoader().parse(new TextDecoder().decode(bytes))
    case '3mf': {
      // **The one format where the two sides disagree unless told not to.** A 3MF declares
      // its unit on `<model>`, and `parse_3mf` multiplies by it; three's `ThreeMFLoader`
      // reads the attribute and then never uses it, so a file in centimetres loads ten times
      // too small. The build and component transforms it does apply, so those are left to it.
      const group = new ThreeMFLoader().parse(bytes)
      group.scale.setScalar(unitScale(bytes))
      group.updateMatrixWorld(true)
      return group
    }
    default:
      throw new Unviewable(strings.viewer.unviewable(format))
  }
}

/**
 * Millimetres per unit of a 3MF's own declared unit (`3dmodel.model`'s `<model unit>`).
 *
 * The 3MF core specification's six units, and millimetre for a file that names none, which
 * is what the specification says the default is. `parse_3mf` has the same table; a unit in
 * one and not the other is a part that loads at the wrong size in the viewer while every
 * figure beside it reads correctly, so they are tested against one another through
 * `fixtures/bbox-mm.json` rather than trusted to stay in step.
 */
const MM_PER_UNIT: Record<string, number> = {
  micron: 0.001,
  millimeter: 1,
  centimeter: 10,
  inch: 25.4,
  foot: 304.8,
  meter: 1000,
}

/**
 * The unit a 3MF package declares, read from the model part inside it.
 *
 * `unzipSync` with a filter, so only the model part is inflated rather than the whole
 * archive, and only its first kilobyte is decoded — the attribute is on the root element, so
 * the vertices never need to be turned into a string. `ThreeMFLoader` imports this same
 * fflate module, so nothing new is pulled in for it.
 *
 * ponytail: the model part is inflated twice, once here and once by the loader. Measured at
 * nothing worth avoiding on the fixtures; the upgrade, if a large assembly ever makes it
 * matter, is a loader fork that returns the unit, not a cache here.
 *
 * Anything unreadable falls back to millimetre rather than throwing: the loader is about to
 * read the same bytes and will say so far better than a unit probe can.
 */
function unitScale(bytes: ArrayBuffer): number {
  try {
    const files = unzipSync(new Uint8Array(bytes), { filter: (file) => file.name.endsWith('.model') })
    const model = Object.values(files)[0]
    if (model === undefined) return 1
    const head = new TextDecoder().decode(model.subarray(0, 1024))
    const unit = /<model[^>]*\sunit="([^"]*)"/.exec(head)?.[1]
    return unit === undefined ? 1 : (MM_PER_UNIT[unit] ?? 1)
  } catch {
    return 1
  }
}
