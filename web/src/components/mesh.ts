import { BufferAttribute, BufferGeometry, Mesh } from 'three'
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
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
 * this is where the formats meet a reader. It lives beside `Viewer.tsx` and is imported by
 * nothing else but `mesh.worker.ts`: three.js is in that chunk and must stay there, so a card or
 * a detail page that imported this would pull the whole renderer into the page's first load.
 *
 * **The bytes are never converted.** `?variant=original` returns what was ingested, and this
 * parses it — so what a person turns on screen is their own file and not our idea of it.
 *
 * Nothing here is told the part's dimensions, and that is deliberate: a parser handed the
 * answer can scale its way to it. `fixtures/bbox-mm.json` is the agreement instead —
 * `mesh.test.ts` loads a real file of each format through this function and asserts the box
 * it comes out at is the box `PartDetail.bboxMm` states for it, which is one assertion over
 * axes, units and 3MF transforms together.
 *
 * **Flat triangle positions, in millimetres** (A11), nine floats a triangle: the one shape every
 * format reduces to, and the one a worker can hand back without a copy. See `loadMesh`.
 */
export function meshPositions(format: string, bytes: ArrayBuffer): Float32Array {
  switch (format.toLowerCase()) {
    case 'stl':
      // STL is millimetres by convention and carries no unit, no transform and no axis
      // declaration: `parse_stl` reads the floats as they are and so does this.
      return flat(new STLLoader().parse(bytes))
    case 'obj': {
      // OBJ is text, and unitless in the same way. `parse_obj` triangulates a polygon face
      // by fanning it; so does `OBJLoader`, which is why a quad-walled fixture comes out at
      // the same box from both.
      const group = new OBJLoader().parse(new TextDecoder().decode(bytes))
      group.updateMatrixWorld(true)
      const parts: Float32Array[] = []
      group.traverse((object) => {
        if (object instanceof Mesh)
          parts.push(flat((object.geometry as BufferGeometry).clone().applyMatrix4(object.matrixWorld)))
      })
      return concat(parts)
    }
    case '3mf':
      return read3mf(bytes)
    default:
      throw new Unviewable(strings.viewer.unviewable(format))
  }
}

/** The mesh `positions` draw, its normals from its faces. */
export function meshFrom(positions: Float32Array): Mesh {
  const geometry = new BufferGeometry()
  geometry.setAttribute('position', new BufferAttribute(positions, 3))
  geometry.computeVertexNormals()
  return new Mesh(geometry)
}

/** `meshPositions` then `meshFrom`, on this thread: what the tests and a browser without workers use. */
export function parseMesh(format: string, bytes: ArrayBuffer): Mesh {
  return meshFrom(meshPositions(format, bytes))
}

/**
 * The original file read **off the page's thread** (A11, owner 2026-10-18: a 42 MB Bambu Studio
 * project froze the screen for ten seconds, then three's `ThreeMFLoader` ran out of stack). The bytes
 * go to `mesh.worker.ts` and the positions come back, both moved rather than copied; the page only
 * builds the mesh. One worker a load, ended when it answers or the load is abandoned.
 */
export function loadMesh(format: string, bytes: ArrayBuffer, signal?: AbortSignal): Promise<Mesh> {
  if (typeof Worker === 'undefined') return Promise.resolve().then(() => parseMesh(format, bytes))
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./mesh.worker.ts', import.meta.url), { type: 'module' })
    const end = () => worker.terminate()
    signal?.addEventListener('abort', () => {
      end()
      reject(new DOMException('abandoned', 'AbortError'))
    })
    worker.onmessage = (
      event: MessageEvent<{ positions: Float32Array } | { unviewable: string } | { failed: string }>,
    ) => {
      end()
      const answer = event.data
      if ('positions' in answer) resolve(meshFrom(answer.positions))
      else if ('unviewable' in answer) reject(new Unviewable(answer.unviewable))
      else reject(new Error(answer.failed))
    }
    worker.onerror = (event) => {
      end()
      reject(new Error(event.message))
    }
    worker.postMessage({ format, bytes }, [bytes])
  })
}

/** A geometry's positions as nine floats a triangle, unindexed. */
function flat(geometry: BufferGeometry): Float32Array {
  const plain = geometry.index === null ? geometry : geometry.toNonIndexed()
  return plain.getAttribute('position').array as Float32Array
}

function concat(parts: Float32Array[]): Float32Array {
  if (parts.length === 1) return parts[0]!
  const out = new Float32Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
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

/** The 3D model relationship `_rels/.rels` names the model part with, as `tmf.rs` reads it. */
const MODEL_REL = 'http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel'

/** `tmf.rs`'s `MAX_DEPTH`: deep enough for any real assembly, shallow enough that a cycle ends quickly. */
const MAX_DEPTH = 8

/**
 * The most triangles drawn: about 1.8 GB of positions. Past it the browser would run out of memory
 * rather than say so, so it is said here. ponytail: one ceiling for every machine.
 */
const MAX_TRIANGLES = 50_000_000

type Matrix = number[]
const IDENTITY: Matrix = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]

interface Object3mf {
  vertices: number[]
  triangles: number[]
  components: [string, Matrix][]
}

/**
 * A 3MF read the way `lapidary-cad/src/tmf.rs`'s `parse_3mf` reads it (A11): the model part named by
 * `_rels/.rels`, every other model part a `p:path` component points into (how Bambu Studio and Orca
 * save every project), build and component transforms, then the unit. **Scanned, not parsed into a
 * DOM**: one pass of a regular expression over the tags it needs, because a project's model parts run
 * to hundreds of megabytes and three's loader built a document of them, then ran out of stack.
 */
function read3mf(bytes: ArrayBuffer): Float32Array {
  const files = unzipSync(new Uint8Array(bytes), {
    filter: (file) => file.name === '_rels/.rels' || file.name.toLowerCase().endsWith('.model'),
  })
  const part = (name: string) => {
    const file = files[name]
    if (file === undefined) throw new Error(`the package has no ${name}`)
    return file
  }
  const text = (name: string) => new TextDecoder().decode(part(name))
  const rel = [...text('_rels/.rels').matchAll(/<(?:\w+:)?Relationship\b([^>]*)>/g)]
    .map((m) => m[1]!)
    .find((tag) => attribute(tag, 'Type') === MODEL_REL)
  const main = rel === undefined ? undefined : attribute(rel, 'Target')?.replace(/^\//, '')
  if (main === undefined) throw new Error('the package declares no 3D model')
  const first = readModel(part(main), '')
  const objects = first.objects
  // Each other model part once, its objects filed under the part's path: every part numbers from 1.
  const parts = new Set<string>()
  for (const object of objects.values()) {
    for (const [key] of object.components) if (key.includes('#')) parts.add(key.slice(0, key.indexOf('#')))
  }
  for (const name of parts) {
    const read = readModel(part(name), name)
    // `tmf.rs`'s ponytail: one unit for the whole package.
    if (read.scale !== first.scale) throw new Error(`${name} uses a different unit from the main model`)
    for (const [key, object] of read.objects) objects.set(key, object)
  }
  let count = 0
  const counting = (id: string, depth: number) => {
    if (depth > MAX_DEPTH) throw new Error('its objects nest too deep, or in a cycle')
    const object = objects.get(id)
    if (object === undefined) throw new Error(`it names object ${id}, which does not exist`)
    count += object.triangles.length / 3
    if (count > MAX_TRIANGLES) throw new Unviewable(strings.viewer.tooBig)
    for (const [child] of object.components) counting(child, depth + 1)
  }
  for (const [id] of first.build) counting(id, 0)
  const out = new Float32Array(count * 9)
  let at = 0
  const scale = first.scale
  const emit = (id: string, m: Matrix) => {
    const { vertices: v, triangles: t, components } = objects.get(id)!
    for (let i = 0; i < t.length; i++) {
      // Transform first in the file's own units, then scale: `tmf.rs`'s order.
      const k = t[i]! * 3
      const x = v[k]!
      const y = v[k + 1]!
      const z = v[k + 2]!
      out[at++] = (x * m[0]! + y * m[3]! + z * m[6]! + m[9]!) * scale
      out[at++] = (x * m[1]! + y * m[4]! + z * m[7]! + m[10]!) * scale
      out[at++] = (x * m[2]! + y * m[5]! + z * m[8]! + m[11]!) * scale
    }
    for (const [child, inner] of components) emit(child, compose(m, inner))
  }
  for (const [id, m] of first.build) emit(id, m)
  return out
}

/**
 * A model part's text, in slices of about `SLICE` bytes that each end just after a `>`, so no tag is
 * ever cut in two. A project's one model part can pass 512 MB of XML, which is longer than a
 * JavaScript string may be.
 */
export function* slices(bytes: Uint8Array, size = SLICE): Generator<string> {
  const decoder = new TextDecoder()
  let from = 0
  while (from < bytes.length) {
    let to = Math.min(from + size, bytes.length)
    if (to < bytes.length) {
      const close = bytes.indexOf(0x3e, to)
      to = close === -1 ? bytes.length : close + 1
    }
    yield decoder.decode(bytes.subarray(from, to))
    from = to
  }
}
const SLICE = 64 * 1024 * 1024

/** One model part's objects, keyed `<part>#<id>` outside the main part, its build and its unit. */
function readModel(bytes: Uint8Array, part: string) {
  const key = (id: string) => (part === '' ? id : `${part}#${id}`)
  const objects = new Map<string, Object3mf>()
  const build: [string, Matrix][] = []
  let scale = 1
  let current: Object3mf | null = null
  for (const xml of slices(bytes)) {
    for (const m of xml.matchAll(/<(\/?)(?:[\w-]+:)?(model|object|vertex|triangle|component|item)\b([^>]*)>/g)) {
      const [, closing, tag, attrs] = m as unknown as [string, string, string, string]
      if (closing === '/') {
        if (tag === 'object') current = null
        continue
      }
      if (tag === 'vertex') {
        if (current !== null) {
          // The hot path, millions of times a project: every writer puts x, y and z in that order.
          const quick = XYZ.exec(attrs)
          if (quick === null) current.vertices.push(number(attrs, 'x'), number(attrs, 'y'), number(attrs, 'z'))
          else current.vertices.push(finite(quick[1]!, 'x'), finite(quick[2]!, 'y'), finite(quick[3]!, 'z'))
        }
      } else if (tag === 'triangle') {
        if (current !== null) {
          const n = current.vertices.length / 3
          const quick = V123.exec(attrs)
          for (const [k, name] of ['v1', 'v2', 'v3'].entries()) {
            const i = quick === null ? number(attrs, name) : Number(quick[k + 1])
            if (!Number.isInteger(i) || i < 0 || i >= n)
              throw new Error(`a triangle names vertex ${i}, which does not exist`)
            current.triangles.push(i)
          }
        }
      } else if (tag === 'object') {
        const id = attribute(attrs, 'id')
        if (id === undefined) throw new Error('an object has no id')
        current = { vertices: [], triangles: [], components: [] }
        objects.set(key(id), current)
      } else if (tag === 'component') {
        const id = attribute(attrs, 'objectid')
        if (id === undefined) throw new Error('a component names no object')
        const path = attribute(attrs, 'path')?.replace(/^\//, '')
        current?.components.push([path === undefined ? key(id) : `${path}#${id}`, matrix(attrs)])
      } else if (tag === 'item') {
        const id = attribute(attrs, 'objectid')
        if (id === undefined) throw new Error('a build item names no object')
        build.push([key(id), matrix(attrs)])
      } else if (tag === 'model') {
        const unit = attribute(attrs, 'unit') ?? 'millimeter'
        const mm = MM_PER_UNIT[unit]
        if (mm === undefined) throw new Error(`its unit ${unit} is not one 3MF defines`)
        scale = mm
      }
    }
  }
  return { objects, build, scale }
}

/** One attribute's value from a tag's text, by its local name (`p:path` is `path`). */
function attribute(attrs: string, name: string): string | undefined {
  let pattern = ATTRIBUTES.get(name)
  if (pattern === undefined) {
    pattern = new RegExp(`(?:^|\\s)(?:[\\w-]+:)?${name}\\s*=\\s*["']([^"']*)["']`)
    ATTRIBUTES.set(name, pattern)
  }
  return pattern.exec(attrs)?.[1]
}
const ATTRIBUTES = new Map<string, RegExp>()

function number(attrs: string, name: string): number {
  return finite(attribute(attrs, name), name)
}

function finite(raw: string | undefined, name: string): number {
  const value = Number(raw)
  if (raw === undefined || raw === '' || !Number.isFinite(value)) throw new Error(`a ${name} is not a number`)
  return value
}

const XYZ = /^\s*x="([^"]*)"\s+y="([^"]*)"\s+z="([^"]*)"/
const V123 = /^\s*v1="([^"]*)"\s+v2="([^"]*)"\s+v3="([^"]*)"/

/** 3MF's `transform`: nine numbers of a row-major 3×3, then a translation. */
function matrix(attrs: string): Matrix {
  const raw = attribute(attrs, 'transform')
  if (raw === undefined) return IDENTITY
  const m = raw.trim().split(/\s+/).map(Number)
  if (m.length !== 12 || !m.every(Number.isFinite)) throw new Error('a transform is not twelve numbers')
  return m
}

/** `outer` applied after `inner`, as `tmf.rs`'s `compose`. */
function compose(outer: Matrix, inner: Matrix): Matrix {
  const out = new Array<number>(12).fill(0)
  for (let row = 0; row < 3; row++) {
    for (let col = 0; col < 3; col++) {
      for (let k = 0; k < 3; k++) out[row * 3 + col]! += inner[row * 3 + k]! * outer[k * 3 + col]!
    }
  }
  for (let col = 0; col < 3; col++) {
    let sum = outer[9 + col]!
    for (let k = 0; k < 3; k++) sum += inner[9 + k]! * outer[k * 3 + col]!
    out[9 + col] = sum
  }
  return out
}
