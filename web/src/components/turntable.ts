import { Box3, Group, LinearSRGBColorSpace, Mesh, OrthographicCamera, Scene, Vector3, WebGLRenderer, type Object3D } from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js'
import { blobUrl } from '../lib/api'
import type { BlobHash } from '../lib/types'
import { curve, tokens } from '../lib/motion'
import { Lru, VIEW_DIR, frameBox, thumbnailFrame, turningFrame, type Vec3, type ViewFrame } from '../lib/viewer-math'
import { disposeModel, rasterLights, rasterMaterial } from './studio'

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
  Object.assign(canvas.style, {
    position: 'absolute',
    left: `${INSET * 100}%`,
    top: `${INSET * 100}%`,
    width: `${(1 - 2 * INSET) * 100}%`,
    height: `${(1 - 2 * INSET) * 100}%`,
    opacity: '0',
    pointerEvents: 'none',
  })
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
  return { renderer, scene, camera, pivot }
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

/** Turn the part `hash` in `well` until the returned function is called. */
export function spin(well: HTMLElement, hash: BlobHash): () => void {
  if (lost) return () => {}
  current?.()
  const { renderer, scene, camera, pivot } = (stage ??= build())
  const canvas = renderer.domElement
  const image = well.querySelector('img')
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
      well.appendChild(canvas)

      // The part turns about its own centre: the pivot sits there and the model is offset back.
      const framing = frameBox(min, max)
      pivot.clear()
      pivot.rotation.set(0, 0, 0)
      pivot.position.set(...framing.target)
      model.position.set(-framing.target[0], -framing.target[1], -framing.target[2])
      pivot.add(model)
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
