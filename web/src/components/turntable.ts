import {
  AmbientLight,
  Box3,
  MeshBasicMaterial,
  DirectionalLight,
  Group,
  LinearSRGBColorSpace,
  Mesh,
  OrthographicCamera,
  Scene,
  Vector3,
  WebGLRenderer,
  type Object3D,
} from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js'
import { blobUrl } from '../lib/api'
import type { BlobHash } from '../lib/types'
import { curve, reduced, tokens } from '../lib/motion'
import { LIGHT_DIR, Lru, VIEW_DIR, frameBox, thumbnailFrame, turningFrame, type Vec3, type ViewFrame } from '../lib/viewer-math'
import { SHADOW_OPACITY, contactShadow, disposeModel, placeShadow, rasterLights, rasterMaterial, shadowMaterial } from './studio'

/**
 * The grid's turntable: the card under the pointer turns its part, in the card, in place of the
 * thumbnail.
 *
 * A thumbnail answers "what is this"; turning it answers "what is on the other side", which is the
 * question a person deciding whether a part fits asks next and could only answer by opening it.
 *
 * **One renderer for the page, one canvas moved between wells.** A canvas per card would be a
 * WebGL context per card, and a browser allows a handful. The part's own 3D view keeps its own
 * renderer (`Viewer.tsx`), so the page holds at most two contexts: this one and an open view.
 *
 * It starts exactly where the thumbnail is: `raster.rs`'s framing (`thumbnailFrame`) from the same
 * direction, shaded by its own formula (`rasterLights` in `studio.ts`), at the same 7% inset. The canvas fades in over the
 * picture once its first frame is drawn, so the swap is a part beginning to move and not a picture
 * being replaced, held still for that crossfade (`--duration-base`). Then it steps back over
 * `--duration-slow` to a framing that holds the part at every angle (`turningFrame`), and turns
 * about the vertical, once per `TURN_MS`.
 *
 * Its callers guard what this does not: reduced motion, no WebGL, a touch screen (`Card.tsx`). A
 * lost context turns it off for the page; a hidden tab stops asking for frames on its own, because
 * `requestAnimationFrame` does.
 */

/** One full turn. Slow enough to read the part as it passes, quick enough to see its back. */
const TURN_MS = 8000

/** The card's well insets its render by 7% of its width (`WELL` in `Card.tsx`); the canvas matches. */
const INSET = 0.07

type Parsed = { model: Object3D; min: Vec3; max: Vec3; still: ViewFrame; turning: ViewFrame }

type Stage = {
  renderer: WebGLRenderer
  scene: Scene
  camera: OrthographicCamera
  pivot: Group
  /** Its own material, because the turntable fades it in and the bench does not. */
  shadow: Mesh
}

let stage: Stage | null = null
let lost = false
let current: (() => void) | null = null
const material = rasterMaterial()
const models = new Lru<BlobHash, Parsed>(16, (parsed) => disposeModel(parsed.model))

function build(): Stage {
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  // `rasterMaterial`'s colours are the thumbnail's bytes already; converting them again would lighten them.
  renderer.outputColorSpace = LinearSRGBColorSpace
  const canvas = renderer.domElement
  canvas.setAttribute('aria-hidden', 'true')
  canvas.addEventListener('webglcontextlost', () => {
    lost = true
    current?.()
  })
  const scene = new Scene()
  scene.add(...rasterLights())
  const pivot = new Group()
  scene.add(pivot)
  const camera = new OrthographicCamera(-1, 1, 1, -1, 0.1, 10)
  camera.up.set(0, 0, 1)
  const shadow = contactShadow([0, 0, 0], [1, 1, 0], shadowMaterial())
  scene.add(shadow)
  return { renderer, scene, camera, pivot, shadow }
}

async function load(hash: BlobHash): Promise<Parsed> {
  const cached = models.get(hash)
  if (cached !== undefined) return cached
  // The rung the hover already fetched (`createPrefetch` in the grid), so this is a cache read.
  const gltf = await new GLTFLoader().setMeshoptDecoder(MeshoptDecoder).loadAsync(blobUrl(hash))
  gltf.scene.traverse((object) => {
    if (object instanceof Mesh) object.material = material
  })
  const box = new Box3().setFromObject(gltf.scene)
  const min = box.min.toArray() as Vec3
  const max = box.max.toArray() as Vec3
  const pivot: Vec3 = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2]
  // Every vertex in world space, once, for the two framings.
  gltf.scene.updateMatrixWorld(true)
  const positions: number[] = []
  const point = new Vector3()
  gltf.scene.traverse((object) => {
    if (!(object instanceof Mesh)) return
    const attribute = object.geometry.getAttribute('position')
    for (let i = 0; i < attribute.count; i++) {
      point.fromBufferAttribute(attribute, i).applyMatrix4(object.matrixWorld)
      positions.push(point.x, point.y, point.z)
    }
  })
  const parsed: Parsed = {
    model: gltf.scene,
    min,
    max,
    still: thumbnailFrame(positions, pivot),
    turning: turningFrame(positions, pivot),
  }
  models.set(hash, parsed)
  return parsed
}

/** Where the one canvas sits in whatever host has it now: inset like a card's render, or filling the host. */
function place(canvas: HTMLCanvasElement, inset: number) {
  Object.assign(canvas.style, {
    position: 'absolute',
    left: `${inset * 100}%`,
    top: `${inset * 100}%`,
    width: `${(1 - 2 * inset) * 100}%`,
    height: `${(1 - 2 * inset) * 100}%`,
    opacity: '0',
    pointerEvents: 'none',
  })
}

/** Turn the part `hash` in `well` until the returned function is called. */
export function spin(well: HTMLElement, hash: BlobHash): () => void {
  if (lost) return () => {}
  current?.()
  const { renderer, scene, camera, pivot, shadow } = (stage ??= build())
  const canvas = renderer.domElement
  const image = well.querySelector('img')
  const shade = shadow.material as MeshBasicMaterial
  let frame = 0
  let stopped = false

  const stop = () => {
    if (stopped) return
    stopped = true
    cancelAnimationFrame(frame)
    if (current === stop) current = null
    // Both fade on the stylesheet's own opacity transition, the picture back as the canvas goes.
    canvas.style.opacity = '0'
    if (image !== null) image.style.opacity = ''
    setTimeout(() => {
      if (current === null && canvas.parentElement === well) canvas.remove()
    }, 200)
  }
  current = stop

  load(hash)
    .then(async ({ model, min, max, still, turning }) => {
      if (stopped) return
      const side = Math.max(1, Math.round(well.clientWidth * (1 - 2 * INSET)))
      renderer.setSize(side, side, false)
      place(canvas, INSET)
      well.appendChild(canvas)

      // The part turns about its own centre: the pivot sits there and the model is offset back.
      const framing = frameBox(min, max)
      pivot.clear()
      pivot.rotation.set(0, 0, 0)
      pivot.position.set(...framing.target)
      model.position.set(-framing.target[0], -framing.target[1], -framing.target[2])
      pivot.add(model)
      // The thumbnail has no shadow, so the shadow waits for the crossfade and fades in with the step back.
      placeShadow(shadow, min, max)
      shade.opacity = 0
      // `frameBox`'s distance and depth range, which hold the part whole at any angle; the centre and
      // the size on screen come from the two framings, eased from one to the other.
      const distance = Math.hypot(
        framing.position[0] - framing.target[0],
        framing.position[1] - framing.target[1],
        framing.position[2] - framing.target[2],
      )
      camera.near = framing.near
      camera.far = framing.far
      const look = (frame: ViewFrame) => {
        const [x, y, z] = frame.center
        camera.position.set(x + VIEW_DIR[0] * distance, y + VIEW_DIR[1] * distance, z + VIEW_DIR[2] * distance)
        camera.lookAt(x, y, z)
        camera.left = -frame.halfHeight
        camera.right = frame.halfHeight
        camera.top = frame.halfHeight
        camera.bottom = -frame.halfHeight
        camera.updateProjectionMatrix()
      }
      look(still)
      // Compiled off the frame loop, so the first turn does not stall on a shader link.
      await renderer.compileAsync(scene, camera)
      if (stopped) return

      const { base, slow } = tokens()
      const ease = curve()
      const lerp = (a: number, b: number, k: number) => a + (b - a) * k
      const start = performance.now()
      const tick = (now: number) => {
        // A frame's timestamp can sit a little before `start`, which was taken after it was scheduled.
        const elapsed = Math.max(0, now - start)
        if (elapsed <= base) {
          // The crossfade: canvas and picture show the same thing, so the swap itself is unseen.
          look(still)
        } else if (elapsed <= base + slow) {
          // Then the step back, before any turn, so nothing turns out of a frame too tight to hold it.
          const k = ease((elapsed - base) / slow)
          shade.opacity = k * SHADOW_OPACITY
          look({
            center: [
              lerp(still.center[0], turning.center[0], k),
              lerp(still.center[1], turning.center[1], k),
              lerp(still.center[2], turning.center[2], k),
            ],
            halfHeight: lerp(still.halfHeight, turning.halfHeight, k),
          })
        } else {
          look(turning)
          shade.opacity = SHADOW_OPACITY
          pivot.rotation.z = (((elapsed - base - slow) % TURN_MS) / TURN_MS) * Math.PI * 2
        }
        renderer.render(scene, camera)
        if (canvas.style.opacity !== '1') {
          canvas.style.opacity = '1'
          if (image !== null) image.style.opacity = '0'
        }
        frame = requestAnimationFrame(tick)
      }
      frame = requestAnimationFrame(tick)
    })
    .catch(stop)

  return stop
}

/** One full sweep of the bench's key light, there and back. Slow enough to read as daylight moving. */
const SWEEP_MS = 12_000

/** How far the bench's key light swings either side of the thumbnail's own direction. */
const SWEEP_DEGREES = 25

/**
 * An empty library's first-run scene: three example parts side by side on the lamp's ground, with
 * the key light slowly swinging across them.
 *
 * It is the first thing a new library shows, so it shows what the product is for — real parts,
 * lit the way every thumbnail in the grid will be — rather than an illustration of an empty box.
 * The models are the worker's own L0 rungs of three example parts (`web/public/first-run/`).
 *
 * It borrows the turntable's renderer, so an empty library holds the same one context a full
 * one does (no card is on screen to turn). It draws only while on screen, one frame under reduced
 * motion, and the caller shows text alone where there is no WebGL. Returns what stops it.
 */
export function bench(host: HTMLElement, urls: readonly string[]): () => void {
  if (lost) return () => {}
  current?.()
  const { renderer } = (stage ??= build())
  const canvas = renderer.domElement
  const scene = new Scene()
  const sun = new DirectionalLight(0xffffff, 0)
  const [ambient, raster] = rasterLights() as [AmbientLight, DirectionalLight]
  sun.intensity = raster.intensity
  scene.add(ambient, sun)
  const camera = new OrthographicCamera(-1, 1, 1, -1, 0.1, 10)
  camera.up.set(0, 0, 1)
  const loaded: Object3D[] = []
  const benchShadow = shadowMaterial()
  const shadows: Mesh[] = []
  let frame = 0
  let stopped = false
  let visible = true

  const stop = () => {
    if (stopped) return
    stopped = true
    cancelAnimationFrame(frame)
    if (current === stop) current = null
    observer.disconnect()
    resize.disconnect()
    for (const model of loaded) disposeModel(model)
    for (const floor of shadows) floor.geometry.dispose()
    benchShadow.dispose()
    if (canvas.parentElement === host) canvas.remove()
  }
  current = stop

  const size = () => {
    renderer.setSize(Math.max(1, host.clientWidth), Math.max(1, host.clientHeight), false)
    return host.clientWidth / Math.max(1, host.clientHeight)
  }
  // The shelf's outline on screen, in world units, measured once the models are placed.
  let outline = { width: 1, height: 1 }
  const fit = () => {
    const aspect = size()
    // Wide enough for the row and tall enough for the tallest part, with a little air: the three are
    // a scene, not a thumbnail cropped to its outline.
    const half = Math.max(outline.height / 2, outline.width / (2 * aspect)) * 1.18
    camera.left = -half * aspect
    camera.right = half * aspect
    camera.top = half
    camera.bottom = -half
    camera.updateProjectionMatrix()
  }
  const draw = (now: number) => {
    const swing = (Math.sin((now / SWEEP_MS) * Math.PI * 2) * SWEEP_DEGREES * Math.PI) / 180
    const [c, s] = [Math.cos(swing), Math.sin(swing)]
    sun.position.set(LIGHT_DIR[0] * c - LIGHT_DIR[1] * s, LIGHT_DIR[0] * s + LIGHT_DIR[1] * c, LIGHT_DIR[2])
    renderer.render(scene, camera)
    canvas.style.opacity = '1'
  }
  const loop = (now: number) => {
    if (visible) draw(now)
    frame = requestAnimationFrame(loop)
  }
  const observer = new IntersectionObserver((entries) => {
    visible = entries.some((entry) => entry.isIntersecting)
  })
  const resize = new ResizeObserver(() => {
    if (stopped || loaded.length === 0) return
    fit()
    draw(performance.now())
  })

  Promise.all(urls.map((url) => new GLTFLoader().setMeshoptDecoder(MeshoptDecoder).loadAsync(url)))
    .then(async (gltfs) => {
      if (stopped) return
      // Side by side along the view's right, each resting on the same floor, a part's width apart.
      const right = new Vector3(-VIEW_DIR[1], VIEW_DIR[0], 0).normalize()
      const shelf = new Group()
      let along = 0
      for (const gltf of gltfs) {
        const model = gltf.scene
        model.traverse((object) => {
          if (object instanceof Mesh) object.material = material
        })
        // Each at the same size, so a 44 mm gear stands beside a 150 mm flange as an equal: the scene
        // is about what parts look like here, not how they compare.
        const raw = new Box3().setFromObject(model)
        const scale = 1 / Math.max(raw.max.x - raw.min.x, raw.max.y - raw.min.y, raw.max.z - raw.min.z, 1e-6)
        model.scale.setScalar(scale)
        const box = new Box3().setFromObject(model)
        const width = box.max.x - box.min.x + (box.max.y - box.min.y)
        const centre = box.getCenter(new Vector3())
        model.position.set(-centre.x, -centre.y, -box.min.z)
        const slot = new Group()
        slot.add(model)
        const footprint = new Box3().setFromObject(model)
        const floor = contactShadow(
          [footprint.min.x, footprint.min.y, footprint.min.z],
          [footprint.max.x, footprint.max.y, footprint.max.z],
          benchShadow,
        )
        slot.add(floor)
        shadows.push(floor)
        slot.position.copy(right.clone().multiplyScalar(along + width / 2))
        along += width * 1.15
        shelf.add(slot)
        loaded.push(model)
      }
      scene.add(shelf)
      shelf.updateMatrixWorld(true)
      // Framed on the parts alone: the shadows are wider than the parts and would loosen the view.
      const box = new Box3()
      for (const model of loaded) box.expandByObject(model)
      const min = box.min.toArray() as Vec3
      const max = box.max.toArray() as Vec3
      const framing = frameBox(min, max)
      const positions: number[] = []
      const point = new Vector3()
      shelf.traverse((object) => {
        if (!(object instanceof Mesh) || shadows.includes(object)) return
        const attribute = object.geometry.getAttribute('position')
        for (let i = 0; i < attribute.count; i++) {
          point.fromBufferAttribute(attribute, i).applyMatrix4(object.matrixWorld)
          positions.push(point.x, point.y, point.z)
        }
      })
      const view = thumbnailFrame(positions, framing.target)
      const up = new Vector3().crossVectors(new Vector3(...VIEW_DIR), right).normalize()
      let [loR, hiR, loU, hiU] = [Infinity, -Infinity, Infinity, -Infinity]
      for (let i = 0; i < positions.length; i += 3) {
        point.set(positions[i]!, positions[i + 1]!, positions[i + 2]!)
        loR = Math.min(loR, point.dot(right))
        hiR = Math.max(hiR, point.dot(right))
        loU = Math.min(loU, point.dot(up))
        hiU = Math.max(hiU, point.dot(up))
      }
      outline = { width: hiR - loR, height: hiU - loU }
      const distance = Math.hypot(...framing.position.map((value, axis) => value - framing.target[axis]!))
      camera.position.set(
        view.center[0] + VIEW_DIR[0] * distance,
        view.center[1] + VIEW_DIR[1] * distance,
        view.center[2] + VIEW_DIR[2] * distance,
      )
      camera.lookAt(...view.center)
      camera.near = framing.near
      camera.far = framing.far

      place(canvas, 0)
      host.appendChild(canvas)
      fit()
      await renderer.compileAsync(scene, camera)
      if (stopped) return
      observer.observe(host)
      resize.observe(host)
      if (reduced()) {
        draw(0)
        return
      }
      frame = requestAnimationFrame(loop)
    })
    .catch(stop)

  return stop
}
